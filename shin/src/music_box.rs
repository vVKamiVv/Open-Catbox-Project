//! The Music Box (`shin::BGMMode`, ctor `0x710002fd40`, update `0x71000306d0`, render `0x71000309b0`, keys
//! `0x71000315a0`): the scenario's MusicBoxInfo tracks in two columns over `/bgmmode.txa`, locked ones as "? ? ?". A
//! plays a track (looping, or once for the tracks marked so), Y stops it, B returns to the title.
//!
//! shortcut: the console's keep-awake request while music plays (setting v18, `0x71000fc3b0`) has no PC equivalent
//! here, and the list cannot be dragged or flung with the pointer.

use std::sync::Arc;

use enum_map::EnumMap;
use glam::Mat4;
use shin_audio::AudioManager;
use shin_core::{
    format::{save::SaveData, scenario::info::BgmId},
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tween, Tweener},
    vm::command::types::{AudioWaitStatus, Volume},
};
use shin_input::{ActionState, Pointer};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, resize::CanvasSize,
};
use tracing::warn;

use crate::{
    adv::{
        hud::{OUTLINE, text_layout},
        menu::MenuAssets,
    },
    app::AppAction,
    asset::{system::AssetServer, texture_archive::NamedTextures},
    audio::{BgmPlayer, play_sys_se},
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    ui::{self, MenuInput},
    update::UpdateContext,
};

const ROW: f32 = 92.0;
const ITEM_H: f32 = 88.0;
const VIEW: f32 = 730.0;
const LIST_Y: f32 = 188.0;
const CLIP: [f32; 4] = [136.0, 183.0, 1784.0, 923.0];
/// The completion text's outline colour (`0x710015aa4c`)
const BORDER: FloatColor4 = FloatColor4::from_rgba(0.1843, 0.0980, 0.0784, 1.0);

struct Item {
    bgm_id: BgmId,
    /// The row of the `title*` textures: 0 ("? ? ?") while locked
    name: u16,
    unlocked: bool,
    once: bool,
    highlight: Tweener,
    flash: Tweener,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Active,
    /// The close button flashes, then the title
    Closing,
    Done,
}

pub struct MusicBox {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    textures: Arc<NamedTextures>,
    bgm: BgmPlayer,
    pub global: SaveData,
    items: Vec<Item>,
    height: f32,
    cursor: usize,
    playing: Option<usize>,
    scroll: Tweener,
    close_flash: Tweener,
    completion: TextBlock,
    state: State,
    target_size: (u32, u32),
}

impl MusicBox {
    pub fn new(
        pre_render: &crate::render::PreRenderContext,
        asset_server: &AssetServer,
        audio: Arc<AudioManager>,
        assets: MenuAssets,
        global: SaveData,
    ) -> anyhow::Result<Self> {
        let textures = asset_server.load_sync::<NamedTextures>("/bgmmode.txa")?;
        let items = assets
            .scenario
            .info_tables()
            .music_box_info
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let unlocked = global
                    .save_vectors
                    .unlocked_bgms
                    .contains(entry.bgm_id.into_i32() as usize);
                Item {
                    bgm_id: entry.bgm_id,
                    name: if unlocked { entry.name_index } else { 0 },
                    unlocked,
                    once: entry.once_flag != 0,
                    highlight: Tweener::new((i == 0) as i32 as f32),
                    flash: Tweener::new(0.0),
                }
            })
            .collect::<Vec<_>>();
        let n = items.len();
        let height = n.div_ceil(2) as f32 * ROW - 4.0;

        // "Unlocked:12.3％": the English patch's label loses its space and a stray byte (`0x710015e9cf`)
        let unlocked = items.iter().filter(|item| item.unlocked).count();
        let rate = unlocked * 1000 / n.max(1);
        let rate = if rate < 1000 {
            format!("{}.{}", rate / 10, rate % 10)
        } else {
            "100".to_string()
        };
        let layout = LayoutParams {
            always_leave_space_for_rubi: false,
            ..text_layout(640.0, 40.0)
        };
        let completion = TextBlock::new(
            pre_render,
            &assets.system_font,
            &format!("Unlocked:{rate}％"),
            layout,
            OUTLINE,
        );

        Ok(Self {
            assets,
            audio: audio.clone(),
            textures,
            bgm: BgmPlayer::new(audio),
            global,
            items,
            height,
            cursor: 0,
            playing: None,
            scroll: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            completion,
            state: State::Active,
            target_size: (1920, 1080),
        })
    }

    /// Done: the title should open with its cursor on the Music Box
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    pub fn shutdown(&mut self, frames: f32) {
        self.bgm
            .stop(Tween::linear(Ticks::from_f32(frames.max(4.0))));
    }

    fn sound(&self, assets: &AssetServer, name: &str) {
        play_sys_se(&self.audio, assets, name, Volume::default());
    }

    fn max_scroll(&self) -> f32 {
        (self.height - VIEW).max(0.0)
    }

    /// `0x7100031a50`
    fn set_cursor(&mut self, to: usize) {
        minigame::tween_now(&mut self.items[self.cursor].highlight, 0.0, 0.2);
        self.cursor = to;
        minigame::tween_now(&mut self.items[to].highlight, 1.0, 0.25);
        let y = ROW * (to / 2) as f32;
        let scroll = self.scroll.target_value();
        let target = if y < scroll {
            y
        } else if y - scroll + ITEM_H > VIEW {
            (y + ITEM_H - VIEW).min(self.height)
        } else {
            return;
        };
        ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
    }

    /// `0x7100031960`: the item flashes; an unlocked track starts (looping unless marked "once")
    fn play(&mut self, asset_server: &AssetServer) {
        let item = &mut self.items[self.cursor];
        item.flash = Tweener::new(0.0);
        minigame::flash(&mut item.flash);
        if !item.unlocked {
            self.sound(asset_server, "error");
            return;
        }
        let (bgm_id, once) = (item.bgm_id, item.once);
        let info = self.assets.scenario.info_tables().bgm_info(bgm_id);
        match asset_server.load_sync(info.path()) {
            Ok(track) => {
                self.bgm.play(
                    bgm_id,
                    track,
                    info.display_name.as_str(),
                    !once,
                    Volume(1.0),
                    Tween::linear(Ticks::ZERO),
                );
                crate::adv::unlocks::bgm_started(
                    &mut self.global.save_vectors,
                    &self.assets.scenario,
                    bgm_id,
                );
                self.playing = Some(self.cursor);
            }
            Err(e) => warn!("Could not load the track: {e:?}"),
        }
    }

    fn handle_keys(&mut self, context: &UpdateContext, input: &MenuInput) {
        let assets = context.asset_server;
        let n = self.items.len();
        if input.cancel {
            // `0x7100031800`
            self.sound(assets, "cancel");
            self.close_flash = Tweener::new(0.0);
            minigame::flash(&mut self.close_flash);
            self.state = State::Closing;
            return;
        }
        if input.confirm {
            self.play(assets);
            return;
        }
        if input.y {
            if self.playing.take().is_some() {
                self.bgm.stop(Tween::linear(Ticks::from_f32(30.0)));
            }
            return;
        }
        let h: i32 = if input.left {
            -1
        } else if input.right {
            1
        } else {
            0
        };
        let v: i32 = if input.up {
            -1
        } else if input.down {
            1
        } else if input.l {
            -8
        } else if input.r {
            8
        } else {
            0
        };
        if h != 0 || v != 0 {
            let rows = n.div_ceil(2) as i32;
            let row = (self.cursor as i32 / 2 + v).clamp(0, rows - 1);
            let column = (self.cursor as i32 % 2 + h).clamp(0, 1);
            let to = ((2 * row + column) as usize).min(n - 1);
            if to != self.cursor {
                self.sound(assets, "move");
                self.set_cursor(to);
            }
            return;
        }
        if input.scroll != 0.0 {
            let scroll = (self.scroll.value() + input.scroll * 32.0).clamp(0.0, self.max_scroll());
            self.scroll = Tweener::new(scroll);
            // the cursor follows by rows into view (`0x71000308a0`)
            let mut to = self.cursor as i32;
            while to >= 0 && (to as usize) < n {
                let y = ROW * (to / 2) as f32;
                if y < scroll {
                    to += 2;
                } else if y + ITEM_H > scroll + VIEW {
                    to -= 2;
                } else {
                    break;
                }
            }
            if to >= 0 && (to as usize) < n && to as usize != self.cursor {
                minigame::tween_now(&mut self.items[self.cursor].highlight, 0.0, 0.2);
                self.cursor = to as usize;
                minigame::tween_now(&mut self.items[self.cursor].highlight, 1.0, 0.25);
            }
        }
    }

    pub fn update(
        &mut self,
        context: &mut UpdateContext,
        state: &EnumMap<AppAction, ActionState>,
        pointer: Pointer,
    ) {
        let frames = context.delta_ticks.as_f32();
        let settings = self.global.settings;
        self.bgm.set_gain(settings.v0_bgmvol, 1.0);
        crate::audio::sys_se::set_setting_volume(settings.v3_sysvol);
        let size = context
            .pre_render
            .resize_source
            .handle::<CanvasSize>()
            .get();
        self.target_size = (size.width, size.height);

        match self.state {
            State::Active => {
                if self.scroll.is_idle() {
                    self.handle_keys(context, &MenuInput::new(state, pointer));
                }
            }
            State::Closing => {
                if self.close_flash.is_idle() {
                    self.state = State::Done;
                }
            }
            State::Done => {}
        }

        // a track played once ends by itself
        if self.playing.is_some()
            && !self
                .bgm
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        {
            self.playing = None;
        }
        ui::advance(&mut self.scroll, frames);
        ui::advance(&mut self.close_flash, frames);
        for item in &mut self.items {
            ui::advance(&mut item.highlight, frames);
            ui::advance(&mut item.flash, frames);
        }
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let projection = ui::projection();
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
        Sprite::new(t("bg"), [0.0, 0.0, 1920.0, 1080.0]).draw(pass, opaque, projection);

        let scroll = self.scroll.value();
        ui::clip(pass, Some(CLIP), self.target_size);
        for (i, item) in self.items.iter().enumerate() {
            let y = LIST_Y + ROW * (i / 2) as f32 - scroll;
            if !(-ITEM_H..1080.0).contains(&y) {
                continue;
            }
            let x = if i % 2 == 0 { 150.0 } else { 970.0 };
            let (glow, flash) = (item.highlight.value() * 0.5, item.flash.value());
            let plate = if self.playing == Some(i) {
                "menu_on"
            } else {
                "menu_off"
            };
            Sprite::new(t(plate), [x, y, x + 800.0, y + ITEM_H])
                .glow(glow)
                .flash(flash)
                .draw(pass, builder, projection);
            let texture = format!("title{}", item.name / 32);
            let v = (item.name % 32) as f32 * 64.0 / 2048.0;
            Sprite::new(t(&texture), [x + 40.0, y + 14.0, x + 760.0, y + 78.0])
                .uv([0.0, v, 1.0, v + 0.03125])
                .glow(glow)
                .flash(flash)
                .draw(pass, builder, projection);
        }
        ui::clip(pass, None, self.target_size);

        let bg = t("bg");
        let clear = FloatColor4::WHITE.with_alpha(0.0);
        ui::gradient_sprite(
            pass,
            bg,
            projection,
            [136.0, 183.0, 1784.0, 188.0],
            [0.070833, 0.169444, 0.929167, 0.174074],
            FloatColor4::WHITE,
            clear,
        );
        ui::gradient_sprite(
            pass,
            bg,
            projection,
            [136.0, 918.0, 1784.0, 923.0],
            [0.070833, 0.85, 0.929167, 0.85463],
            clear,
            FloatColor4::WHITE,
        );
        ui::draw_scroll_bar(
            pass,
            self.assets.system_textures.get("scroll"),
            projection,
            (1788.0, LIST_Y),
            VIEW,
            scroll,
            self.height,
        );
        Sprite::new(t("close"), [1764.0, 32.0, 1848.0, 116.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);

        // the key guide, right to left (`0x7100031230`, `0x710016691c`)
        let grey = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);
        let white = FloatColor4::WHITE;
        let stop = if self.playing.is_some() { white } else { grey };
        let play = if self.items[self.cursor].unlocked {
            white
        } else {
            grey
        };
        let mut x = self.draw_help(pass, 1872.0, &[4], 48.0, 72.0, white);
        x = self.draw_help(pass, x, &[6], 144.0, 74.0, stop);
        x = self.draw_help(pass, x, &[3], 96.0, 76.0, play);
        self.draw_help(pass, x, &[7, 0], 0.0, 206.0, white);

        ui::draw_text(
            pass,
            &self.completion,
            projection * Mat4::from_translation(glam::vec3(48.0, 1012.0, 0.0)),
            FloatColor4::WHITE,
            BORDER,
        );
    }

    fn draw_help(
        &self,
        pass: &mut RenderPass,
        right: f32,
        keys: &[u8],
        row: f32,
        width: f32,
        color: FloatColor4,
    ) -> f32 {
        let projection = ui::projection();
        let y = 1012.0;
        let v = row / 192.0;
        let mut label = Sprite::new(
            self.textures.get("help_bgmmode"),
            [right - width, y, right, y + 48.0],
        )
        .uv([0.0, v, width / 320.0, v + 0.25]);
        label.color = color;
        label.draw(pass, ui::builder(), projection);
        let mut x = right - width + 2.0;
        for &key in keys {
            x -= 48.0;
            let (u, v) = ((key & 7) as f32 * 0.125, (key >> 3) as f32 * 0.5);
            let mut icon = Sprite::new(self.textures.get("key"), [x, y, x + 48.0, y + 48.0]).uv([
                u,
                v,
                u + 0.125,
                v + 0.5,
            ]);
            icon.color = color;
            icon.draw(pass, ui::builder(), projection);
        }
        x - 16.0
    }
}
