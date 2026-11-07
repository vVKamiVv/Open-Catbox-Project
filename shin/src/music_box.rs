//! The Music Box (`shin::BGMMode`, ctor `0x710002fd40`, update `0x71000306d0`, render `0x71000309b0`, keys
//! `0x71000315a0`): the scenario's MusicBoxInfo tracks in two columns over `/bgmmode.txa`, locked ones as "? ? ?". A
//! plays a track (looping, or once for the tracks marked so), Y stops it, B returns to the title.
//!
//! shortcut: the console's keep-awake request while music plays (setting v18, `0x71000fc3b0`) has no PC equivalent
//! here.

use std::sync::Arc;

use enum_map::EnumMap;
use glam::{Mat4, Vec2};
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
    export::{self, Exporter},
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    ui::{
        self, MenuInput,
        gesture::{Coast, Gesture, GestureRecognizer},
    },
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

/// What a touch went down on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    Close,
    /// The list, on an item or between them
    List(Option<usize>),
}

pub struct MusicBox {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    textures: Arc<NamedTextures>,
    pub bgm: BgmPlayer,
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
    /// The list's touch input (`GestureRecognizer`, listener `0x7100031d40` and its neighbours)
    gestures: GestureRecognizer,
    coast: Coast,
    touch: Option<Touch>,
    /// Port-only: the selected track's Save
    export: Exporter,
}

impl MusicBox {
    pub fn new(
        pre_render: &crate::render::PreRenderContext,
        asset_server: &AssetServer,
        audio: Arc<AudioManager>,
        assets: MenuAssets,
        global: SaveData,
        bgm: BgmPlayer,
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
        let export = Exporter::new(assets.system_font.clone());

        Ok(Self {
            assets,
            audio: audio.clone(),
            textures,
            bgm,
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
            gestures: GestureRecognizer::new(false),
            coast: Coast::default(),
            touch: None,
            export,
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

    fn handle_keys(&mut self, context: &UpdateContext, input: &MenuInput, save: bool) {
        let assets = context.asset_server;
        let n = self.items.len();
        if input.cancel {
            self.sound(assets, "cancel");
            self.close();
            return;
        }
        if input.confirm {
            self.play(assets);
            return;
        }
        if save {
            self.save_track(assets);
            return;
        }
        if input.y {
            self.stop();
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
            let frames = context.delta_ticks.as_f32();
            let scroll =
                (self.scroll.value() + input.scroll * 32.0 * frames).clamp(0.0, self.max_scroll());
            self.scroll = Tweener::new(scroll);
            self.follow_cursor(scroll);
        }
    }

    /// Port-only: the selected track as a WAV, `<number> <title>.wav`
    fn save_track(&mut self, assets: &AssetServer) {
        let item = &self.items[self.cursor];
        if !item.unlocked {
            self.sound(assets, "error");
            return;
        }
        let info = self.assets.scenario.info_tables().bgm_info(item.bgm_id);
        let title = info.display_name.as_str().to_string();
        let data = assets.open(info.path());
        let file = export::folder(dirs_next::audio_dir()).join(format!(
            "{:02} {}.wav",
            self.cursor + 1,
            export::file_name(&title)
        ));
        self.export
            .start(move || export::save_track(&data?, &title, &file));
    }

    /// `0x71000308a0`: the cursor follows by rows into the view
    fn follow_cursor(&mut self, scroll: f32) {
        let n = self.items.len();
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

    /// `0x7100031800`: the close button flashes, then the title
    fn close(&mut self) {
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.state = State::Closing;
    }

    /// The playing track fades out over 30 frames
    fn stop(&mut self) {
        if self.playing.take().is_some() {
            self.bgm.stop(Tween::linear(Ticks::from_f32(30.0)));
        }
    }

    /// `0x7100031bb0`: the item under a point of the list, `Some(None)` between items, `None` outside the list. The
    /// two columns are 800 wide and 20 apart from the list's x (`+0xd4`, lost in the decompile: 150 as the port's
    /// render); a row's bottom 3 px are a gap.
    fn item_at(&self, at: Vec2) -> Option<Option<usize>> {
        if !minigame::contains([136.0, LIST_Y, 1784.0, 918.0], at) {
            return None;
        }
        let column = if (150.0..950.0).contains(&at.x) {
            0
        } else if (970.0..1770.0).contains(&at.x) {
            1
        } else {
            return Some(None);
        };
        let y = (self.scroll.value() - LIST_Y + at.y) as i32;
        if y % 92 > 88 {
            return Some(None);
        }
        let index = (y / 92) as usize * 2 + column;
        Some((index < self.items.len()).then_some(index))
    }

    /// The list's gesture listener (down `0x7100031d40`, press `0x7100031f20`, tap `0x7100032080`, drag
    /// `0x71000321e0`, flick `0x71000322e0`): a press raises the item and moves the cursor to it, a tap plays it or,
    /// when it is the playing track, stops it, and drags and flicks scroll the list with the cursor following
    fn handle_pointer(&mut self, assets: &AssetServer, pointer: Pointer, frames: f32) {
        for gesture in self.gestures.update(pointer, frames) {
            match gesture {
                Gesture::Down(at) => {
                    self.touch = if minigame::contains([1764.0, 32.0, 1848.0, 116.0], at) {
                        Some(Touch::Close)
                    } else if (150.0..992.0).contains(&at.y) {
                        self.item_at(at).map(Touch::List)
                    } else {
                        None
                    };
                    match self.touch {
                        None => self.gestures.cancel(),
                        // touching a coasting list stops it, and that touch selects nothing
                        Some(Touch::List(_)) if self.coast.is_moving() => {
                            self.coast.stop();
                            self.touch = Some(Touch::List(None));
                        }
                        _ => {}
                    }
                }
                Gesture::Press => match self.touch {
                    Some(Touch::Close) => minigame::raise(&mut self.close_flash),
                    Some(Touch::List(Some(index))) => {
                        minigame::raise(&mut self.items[index].flash);
                        self.set_cursor(index);
                    }
                    _ => {}
                },
                // the flash and the close restart the raised highlight
                Gesture::Tap(_) => match self.touch.take() {
                    Some(Touch::Close) => {
                        self.sound(assets, "decide");
                        self.close();
                    }
                    Some(Touch::List(Some(index))) => {
                        self.set_cursor(index);
                        if self.playing == Some(index) {
                            minigame::lower(&mut self.items[index].flash);
                            self.stop();
                        } else {
                            self.play(assets);
                        }
                    }
                    _ => {}
                },
                Gesture::Drag(from, to) => {
                    self.lower_touch();
                    if let Some(Touch::List(_)) = self.touch {
                        let scroll =
                            (self.scroll.value() - (to.y - from.y)).clamp(0.0, self.max_scroll());
                        self.follow_cursor(scroll);
                        self.scroll.fast_forward_to(scroll);
                    }
                }
                Gesture::Flick(velocity) => {
                    if let Some(Touch::List(_)) = self.touch {
                        self.coast.start(velocity.y);
                    }
                }
                Gesture::LongPress => {}
                Gesture::End => {
                    self.lower_touch();
                    self.touch = None;
                }
            }
        }
    }

    /// A drag or the touch's end drops the press's highlight; the touch then selects nothing
    fn lower_touch(&mut self) {
        match self.touch {
            Some(Touch::Close) => minigame::lower(&mut self.close_flash),
            Some(Touch::List(Some(index))) => {
                minigame::lower(&mut self.items[index].flash);
                self.touch = Some(Touch::List(None));
            }
            _ => {}
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
        let (pointer, clicked) = self.export.pointer(pointer);
        let save = clicked || Exporter::pressed(state);

        match self.state {
            State::Active => {
                self.handle_pointer(context.asset_server, pointer, frames);
                // `0x71000306d0`: a flick coasts the list, the cursor following
                if self.coast.is_moving() {
                    let scroll =
                        self.coast
                            .update(self.scroll.value(), 0.0, self.max_scroll(), frames);
                    self.follow_cursor(scroll);
                    self.scroll.fast_forward_to(scroll);
                }
                // touches and the list moving hold the buttons off (`0x710006b860`)
                if !self.gestures.is_active()
                    && !self.coast.is_moving()
                    && self.scroll.is_idle()
                    && self.state == State::Active
                {
                    self.handle_keys(context, &MenuInput::new(state, pointer), save);
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
        self.export.update(
            context.pre_render,
            &self.audio,
            context.asset_server,
            frames,
        );
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
        x = self.draw_help(pass, x, &[7, 0], 0.0, 206.0, white);
        // port-only
        self.export.draw_entry(pass, t("key"), x, 1012.0, play);

        ui::draw_text(
            pass,
            &self.completion,
            projection * Mat4::from_translation(glam::vec3(48.0, 1012.0, 0.0)),
            FloatColor4::WHITE,
            BORDER,
        );
        self.export.draw_notice(pass);
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
            let keys = self.textures.get("key");
            let rect = [x, y, x + 48.0, y + 48.0];
            ui::prompts::draw(
                pass,
                ui::builder(),
                projection,
                keys,
                key as u8,
                rect,
                color,
            );
        }
        x - 16.0
    }
}
