//! The title screen (`shin::Title`, ctor `0x710010b130`, update `0x710010c9b0`, render `0x710010cc30`, keys
//! `0x710010e1a0`): a background that follows the story's progress, the logo, and the menu items with their
//! description panels, all from `/title.txa`. Load and Config open over it; Continue starts the auto slot.
//!
//! shortcut: Characters, Tips, the galleries and the manual have no screens in the port yet (choosing them only
//! flashes), and the BGM does not carry over from the adventure screen (the original's player is shared by every
//! scene).

use std::sync::Arc;

use enum_map::EnumMap;
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::{GameDataEntry, SaveData},
        scenario::{info::BgmId, instruction_elements::FromNumber},
    },
    primitives::color::{FloatColor4, UnormColor},
    time::{Easing, Ticks, Tween, Tweener},
    vm::command::types::{LayerProperty, Volume},
};
use shin_input::{ActionState, Pointer};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, render_texture::RenderTexture,
};
use tracing::{info, warn};

use crate::{
    adv::menu::{
        AdvRequest, MenuAssets, Task, TaskCx,
        config_menu::ConfigMenu,
        save_menu::{Mode, SaveMenu},
    },
    app::AppAction,
    asset::{picture::Picture, system::AssetServer, texture_archive::NamedTextures},
    audio::{BgmPlayer, play_sys_se},
    layer::{
        DrawableLayer as _, Layer as _, render_layer_without_bg,
        render_params::TransformParams,
        user::{
            PictureLayer,
            minigame::{self, Sprite},
        },
    },
    ui::{self, MenuInput},
    update::{AdvUpdatable as _, AdvUpdateContext, UpdateContext},
};

/// The item types, which are also their rows in `menu` and their descriptions (`0x7100223ea0`)
const START: u8 = 0;
const CONTINUE: u8 = 1;
const LOAD: u8 = 2;
const CHARACTERS: u8 = 3;
const TIPS: u8 = 4;
const CONFIG: u8 = 5;
const PICTURE_BOX: u8 = 6;
const MUSIC_BOX: u8 = 7;
const MOVIE_BOX: u8 = 8;
const MANUAL: u8 = 9;
const CHARACTER_BOX: u8 = 10;

const DESCRIPTIONS: [&str; 11] = [
    "text_start",
    "text_continue",
    "text_load",
    "text_chars",
    "text_tips",
    "text_config",
    "text_cg",
    "text_bgm",
    "text_movie",
    "text_manual",
    "text_bup",
];

/// The background, BGM and its volume by progress (`0x7100187796`): `/title/bg%02d.pic`, the BGM id, volume %
const BACKGROUNDS: [(u8, i32, f32); 14] = [
    (1, 0, 75.0),
    (2, 0, 75.0),
    (3, 0, 75.0),
    (4, 0, 75.0),
    (6, 0, 75.0),
    (7, 1, 75.0),
    (8, 0, 75.0),
    (9, 0, 75.0),
    (10, 0, 75.0),
    (11, 0, 75.0),
    (13, 2, 100.0),
    (14, 3, 100.0),
    (15, 4, 100.0),
    (16, 8, 100.0),
];

/// The background's row for a progress value (the ctor's ladder)
fn background_index(progress: i32) -> usize {
    match progress {
        ..3 => 0,
        ..6 => 1,
        ..9 => 2,
        ..12 => 3,
        ..15 => 4,
        ..18 => 5,
        18 => 6,
        ..21 => 7,
        21 => 8,
        ..24 => 9,
        ..27 => 10,
        ..29 => 11,
        ..=30 => 12,
        _ => 13,
    }
}

/// The rows' colour ramp (`*0x7100228498`): dark brown to red
const BROWN: FloatColor4 = FloatColor4::from_rgba(0.2157, 0.1059, 0.1059, 1.0);
const RED: FloatColor4 = FloatColor4::from_rgba(1.0, 0.1608, 0.1608, 1.0);

const ITEM_X: f32 = 1184.0;
const ITEM_H: f32 = 82.0;

struct Item {
    ty: u8,
    alpha: Tweener,
    highlight: Tweener,
    x: Tweener,
    y: Tweener,
    flash: Tweener,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// The intro timer runs (`Title::Start` waits for it)
    Intro,
    /// The items slide in (`StartMenu`'s `$_0` waits for them)
    ItemsIn,
    Active,
    /// An item flashes before its action (`MenuItemOut`)
    Deciding(u8),
    /// Continue: everything dims and the music fades, then the auto slot starts
    Continuing,
    Done,
}

/// Where the title leaves for
pub enum TitleExit {
    /// Start: the scenario select
    ScenarioSelect,
    /// `CreateCGMode`
    PictureBox,
    /// `CreateBGMMode`
    MusicBox,
    /// `CreateMovieMode`
    MovieBox,
    /// Continue or a bookmark from Load
    Load(GameDataEntry),
}

pub struct Title {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    textures: Arc<NamedTextures>,
    background: PictureLayer,
    /// The background rendered each frame (`+0x2c8`), drawn under everything and over fading items
    background_texture: Option<RenderTexture>,
    bgm: BgmPlayer,
    pub global: SaveData,
    items: Vec<Item>,
    cursor: usize,
    logo_base: f32,
    logo_x: Tweener,
    logo_alpha: Tweener,
    intro: Tweener,
    dim: Tweener,
    description: Option<&'static str>,
    description_alpha: Tweener,
    /// Came back from another screen (`type >= 0`): everything shows at once
    returning: bool,
    state: State,
    /// Load or Config over the title
    child: Option<Box<dyn Task>>,
    requests: Vec<AdvRequest>,
    exit: Option<TitleExit>,
    write_requested: bool,
    pointer_held: bool,
    pressed: Option<usize>,
}

impl Title {
    /// `CreateTitle(cursor_type)`: -1 as after the boot (and the adventure screen's "Title"), or the item to put the
    /// cursor on when coming back
    pub fn new(
        asset_server: &Arc<AssetServer>,
        audio: Arc<AudioManager>,
        assets: MenuAssets,
        global: SaveData,
        cursor_type: i32,
    ) -> anyhow::Result<Self> {
        let textures = asset_server.load_sync::<NamedTextures>("/title.txa")?;
        let progress = global.persist_data.get(0).clamp(0, 255);
        let (picture, bgm_id, volume) = BACKGROUNDS[background_index(progress)];
        let picture_path = format!("/title/bg{picture:02}.pic");
        let picture = asset_server.load_sync::<Picture>(&picture_path)?;
        let mut background = PictureLayer::new(picture, Some(picture_path));

        // which items there are (the ctor)
        let persist = |i: usize| global.persist_data.get(i);
        let mut mask = 0u32;
        for k in 1..=8 {
            if persist(k) > 2 {
                mask |= 1 << (k - 1);
            }
        }
        for k in 0..8 {
            if persist(17 + k) > 2 {
                mask |= 1 << (8 + k);
            }
        }
        let mut types = vec![START];
        if global.auto_save_slot.is_some() {
            types.push(CONTINUE);
        }
        types.extend([LOAD, CONFIG]);
        if mask & 0xff00 != 0 {
            types.push(CHARACTERS);
        }
        if mask & 0x7f00 != 0 {
            types.push(TIPS);
        }
        if progress > 0 {
            types.extend([PICTURE_BOX, MUSIC_BOX, MOVIE_BOX]);
            if progress > 30 {
                types.push(CHARACTER_BOX);
            }
        }
        types.push(MANUAL);

        // the list ends at y 1032 at most; a long list moves the logo to the left
        let end = 450.0 + ITEM_H * types.len() as f32;
        let (top, logo_base) = if end > 1032.0 {
            (450.0 - (end - 1032.0), 96.0)
        } else {
            (450.0, 840.0)
        };
        let items = types
            .iter()
            .enumerate()
            .map(|(i, &ty)| Item {
                ty,
                alpha: Tweener::new(0.0),
                highlight: Tweener::new(0.0),
                x: Tweener::new(ITEM_X),
                y: Tweener::new(top + ITEM_H * i as f32),
                flash: Tweener::new(0.0),
            })
            .collect::<Vec<_>>();

        let returning = cursor_type >= 0;
        let mut intro = Tweener::new(0.0);
        let mut logo_x = Tweener::new(0.0);
        let mut logo_alpha = Tweener::new(1.0);
        let cursor_type = if returning {
            cursor_type as u8
        } else {
            // `0x710010de70`: from the boot, on Continue if there is one; the logo slides and fades in from frame 90
            // and the items follow at frame 120
            intro = Tweener::new(-200.0);
            ui::tween(&mut intro, 0.0, 90.0, Easing::SineOut);
            ui::hold(&mut intro, 30.0);
            logo_x = Tweener::new(-100.0);
            ui::hold(&mut logo_x, 90.0);
            ui::tween(&mut logo_x, 0.0, 60.0, Easing::SineOut);
            logo_alpha = Tweener::new(0.0);
            ui::hold(&mut logo_alpha, 90.0);
            ui::tween(&mut logo_alpha, 1.0, 60.0, Easing::Linear);
            // the background's blur clears over the first 90 frames (`Title::First`)
            let blur = background
                .properties_mut()
                .property_tweener_mut(LayerProperty::BlurRadius);
            blur.fast_forward_to(2000.0);
            blur.enqueue(0.0, Tween::linear(Ticks::from_f32(90.0)));
            if types.contains(&CONTINUE) {
                CONTINUE
            } else {
                START
            }
        };
        let cursor = types.iter().position(|&ty| ty == cursor_type).unwrap_or(0);

        let mut bgm = BgmPlayer::new(audio.clone());
        let bgm_id = BgmId::from_number(bgm_id);
        let info = assets.scenario.info_tables().bgm_info(bgm_id);
        match asset_server.load_sync(info.path()) {
            Ok(track) => {
                bgm.play(
                    bgm_id,
                    track,
                    info.display_name.as_str(),
                    true,
                    Volume(volume * 0.01),
                    Tween::linear(Ticks::from_f32(60.0)),
                );
            }
            Err(e) => warn!("Could not load the title's BGM: {e:?}"),
        }

        let mut title = Self {
            assets,
            audio,
            textures,
            background,
            background_texture: None,
            bgm,
            global,
            items,
            cursor,
            logo_base,
            logo_x,
            logo_alpha,
            intro,
            dim: Tweener::new(0.0),
            description: None,
            description_alpha: Tweener::new(0.0),
            returning,
            state: State::Intro,
            child: None,
            requests: Vec::new(),
            exit: None,
            write_requested: false,
            pointer_held: false,
            pressed: None,
        };
        if returning {
            title.show_items_now();
        }
        info!("Title: progress {progress}, {} items", title.items.len());
        Ok(title)
    }

    /// Where the player chose to go, once
    pub fn take_exit(&mut self) -> Option<TitleExit> {
        self.exit.take()
    }

    pub fn take_write_request(&mut self) -> bool {
        std::mem::take(&mut self.write_requested)
    }

    /// The music fades out over `frames` frames as the title is left
    pub fn shutdown(&mut self, frames: f32) {
        self.bgm
            .stop(Tween::linear(Ticks::from_f32(frames.max(4.0))));
    }

    /// `0x710010d2e0`
    fn show_items_now(&mut self) {
        for (i, item) in self.items.iter_mut().enumerate() {
            item.highlight = Tweener::new((i == self.cursor) as i32 as f32);
            item.alpha = Tweener::new(1.0);
            item.x = Tweener::new(ITEM_X);
            item.flash = Tweener::new(0.0);
        }
        self.description = Some(DESCRIPTIONS[self.items[self.cursor].ty as usize]);
        self.description_alpha = Tweener::new(1.0);
    }

    /// `MenuItemIn(0)` (`0x710010d670`): the items slide in from the right 4 frames apart, then the description
    fn items_in(&mut self) {
        for (k, item) in self.items.iter_mut().enumerate() {
            let delay = 4.0 * k as f32;
            item.alpha = Tweener::new(0.0);
            ui::hold(&mut item.alpha, delay);
            ui::tween(&mut item.alpha, 1.0, 10.0, Easing::Linear);
            item.x = Tweener::new(ITEM_X + 100.0);
            ui::hold(&mut item.x, delay);
            ui::tween(&mut item.x, ITEM_X, 10.0, Easing::SineOut);
            item.highlight = Tweener::new((k == self.cursor) as i32 as f32);
            item.flash = Tweener::new(0.0);
        }
        self.description = Some(DESCRIPTIONS[self.items[self.cursor].ty as usize]);
        ui::hold(
            &mut self.description_alpha,
            4.0 * (self.items.len() - 1) as f32,
        );
        ui::tween(&mut self.description_alpha, 1.0, 30.0, Easing::Linear);
    }

    fn items_moving(&self) -> bool {
        self.items
            .iter()
            .any(|item| !item.alpha.is_idle() || !item.y.is_idle())
    }

    fn sound(&self, context: &UpdateContext, name: &str) {
        play_sys_se(&self.audio, context.asset_server, name, Volume::default());
    }

    fn move_cursor(&mut self, context: &UpdateContext, to: usize) {
        if to == self.cursor {
            return;
        }
        self.sound(context, "move");
        minigame::tween_now(&mut self.items[self.cursor].highlight, 0.0, 0.1);
        self.cursor = to;
        minigame::tween_now(&mut self.items[to].highlight, 1.0, 0.25);
        self.description = Some(DESCRIPTIONS[self.items[to].ty as usize]);
    }

    /// `0x710010da10`: "bell" for Start, else "decide"; the item flashes (Start holds 30 frames more), then acts
    fn decide(&mut self, context: &UpdateContext) {
        let ty = self.items[self.cursor].ty;
        self.sound(context, if ty == START { "bell" } else { "decide" });
        let flash = &mut self.items[self.cursor].flash;
        *flash = Tweener::new(0.0);
        minigame::flash(flash);
        if ty == START {
            ui::hold(flash, 30.0);
        }
        self.state = State::Deciding(ty);
    }

    /// `0x710010e310`
    fn dispatch(&mut self, context: &mut UpdateContext, ty: u8) {
        self.state = State::Active;
        match ty {
            START => {
                self.exit = Some(TitleExit::ScenarioSelect);
                self.state = State::Done;
            }
            CONTINUE => {
                self.bgm.stop(Tween::linear(Ticks::from_f32(90.0)));
                ui::tween(&mut self.dim, 1.0, 90.0, Easing::Linear);
                self.state = State::Continuing;
            }
            LOAD | CONFIG => {
                minigame::tween_now(&mut self.dim, 0.5, 0.1);
                let assets = self.assets.clone();
                let cx = self.task_cx(context, true, MenuInput::default());
                let child: Option<Box<dyn Task>> = if ty == LOAD {
                    SaveMenu::new(&cx, &assets, Mode::Load).map(|m| Box::new(m) as Box<dyn Task>)
                } else {
                    ConfigMenu::new(&cx, &assets).map(|m| Box::new(m) as Box<dyn Task>)
                };
                self.child = child;
            }
            PICTURE_BOX => {
                self.exit = Some(TitleExit::PictureBox);
                self.state = State::Done;
            }
            MUSIC_BOX => {
                self.exit = Some(TitleExit::MusicBox);
                self.state = State::Done;
            }
            MOVIE_BOX => {
                self.exit = Some(TitleExit::MovieBox);
                self.state = State::Done;
            }
            _ => warn!("Title item {ty} has no screen in the port yet"),
        }
    }

    fn task_cx<'a, 'i, 'p, 'd, 'e>(
        &'a mut self,
        context: &'a mut UpdateContext<'i, '_, 'p, 'd, 'e>,
        focused: bool,
        input: MenuInput,
    ) -> TaskCx<'a, 'i, 'p, 'd, 'e> {
        TaskCx {
            frames: context.delta_ticks.as_f32(),
            focused,
            input,
            pre_render: &mut *context.pre_render,
            asset_server: context.asset_server,
            audio: &self.audio,
            assets: context.asset_server,
            global: &mut self.global,
            requests: &mut self.requests,
        }
    }

    fn item_at(&self, point: glam::Vec2) -> Option<usize> {
        self.items.iter().position(|item| {
            let (x, y) = (item.x.value(), item.y.value());
            minigame::contains([x, y, x + 704.0, y + 88.0], point)
        })
    }

    fn handle_input(&mut self, context: &mut UpdateContext, input: &MenuInput) {
        // the pointer: a press makes the item the cursor and flashes it, a release on it decides
        let pointer = input.pointer;
        let went_down = pointer.held && !self.pointer_held;
        let went_up = !pointer.held && self.pointer_held;
        self.pointer_held = pointer.held;
        let hit = pointer.position.and_then(|p| self.item_at(p));
        if went_down {
            if let Some(i) = hit {
                self.pressed = Some(i);
                self.move_cursor(context, i);
                minigame::raise(&mut self.items[i].flash);
            }
            return;
        }
        if went_up {
            if let Some(i) = self.pressed.take() {
                if hit == Some(i) {
                    self.decide(context);
                } else {
                    minigame::lower(&mut self.items[i].flash);
                }
            }
            return;
        }
        if self.pressed.is_some() {
            return;
        }

        let n = self.items.len();
        if input.confirm {
            self.decide(context);
        } else if input.down {
            self.move_cursor(context, (self.cursor + 1) % n);
        } else if input.up {
            self.move_cursor(context, (self.cursor + n - 1) % n);
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
        crate::audio::sys_se::set_movie_setting_volumes(
            settings.v0_bgmvol,
            settings.v1_sfxvol,
            settings.v2_voicevol,
        );

        let input = MenuInput::new(state, pointer);
        if let Some(mut child) = self.child.take() {
            let running = child.update(&mut self.task_cx(context, true, input));
            if running {
                self.child = Some(child);
            } else {
                // the screen's callback with 1: the dim lifts
                minigame::tween_now(&mut self.dim, 0.0, 0.1);
            }
            for request in std::mem::take(&mut self.requests) {
                match request {
                    AdvRequest::WriteSave => self.write_requested = true,
                    AdvRequest::LoadSlot(slot) => {
                        if let Some(data) = &self.global.manual_save_slots[slot] {
                            self.global.save_menu_position = slot as u8;
                            self.exit = Some(TitleExit::Load(data.entry.clone()));
                            self.state = State::Done;
                        }
                    }
                    other => warn!("The title cannot do {other:?}"),
                }
            }
        } else {
            match self.state {
                State::Intro => {
                    if self.intro.is_idle() {
                        if !self.returning {
                            self.items_in();
                        }
                        self.state = State::ItemsIn;
                    }
                }
                State::ItemsIn => {
                    if !self.items_moving() {
                        self.state = State::Active;
                    }
                }
                State::Active => self.handle_input(context, &input),
                State::Deciding(ty) => {
                    if self.items[self.cursor].flash.is_idle() {
                        self.dispatch(context, ty);
                    }
                }
                State::Continuing => {
                    if self.dim.is_idle() {
                        if let Some(data) = &self.global.auto_save_slot {
                            self.exit = Some(TitleExit::Load(data.entry.clone()));
                        }
                        self.state = State::Done;
                    }
                }
                State::Done => {}
            }
        }

        for tweener in [
            &mut self.dim,
            &mut self.intro,
            &mut self.logo_x,
            &mut self.logo_alpha,
            &mut self.description_alpha,
        ] {
            ui::advance(tweener, frames);
        }
        for item in &mut self.items {
            for tweener in [
                &mut item.alpha,
                &mut item.highlight,
                &mut item.x,
                &mut item.y,
                &mut item.flash,
            ] {
                ui::advance(tweener, frames);
            }
        }

        // the background is rendered into its texture every frame
        let update = AdvUpdateContext {
            frame_id: context.frame_id,
            delta_ticks: context.delta_ticks,
            asset_server: context.asset_server,
            device: context.pre_render.device,
            queue: context.pre_render.queue,
            are_animations_allowed: true,
        };
        self.background.update(&update);
        let transform = TransformParams::default();
        let ctx = &mut *context.pre_render;
        self.background.pre_render(ctx, &transform);
        let texture = self
            .background_texture
            .get_or_insert_with(|| ctx.new_render_texture("title/background".to_string()));
        let depth_stencil = ctx.depth_stencil;
        let mut pass = ctx.begin_pass(
            texture.as_texture_target(),
            Some(depth_stencil),
            "title/background",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        render_layer_without_bg(&mut pass, &transform, &self.background, 0);
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let projection = ui::projection();
        let builder = ui::builder();
        let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
        let t = |name: &str| self.textures.get(name);
        let Some(background) = &self.background_texture else {
            return;
        };
        let full = [0.0, 0.0, 1920.0, 1080.0];
        ui::draw_texture(
            pass,
            background.as_texture_source(),
            projection,
            full,
            [0.0, 0.0, 1.0, 1.0],
            FloatColor4::WHITE,
            opaque,
        );

        let logo = self.logo_x.value() + self.logo_base;
        Sprite::new(t("logo"), [logo, 64.0, logo + 1014.0, 424.0])
            .alpha(self.logo_alpha.value())
            .draw(pass, builder, projection);
        Sprite::new(t("copyright"), [0.0, 952.0, 672.0, 1080.0]).draw(pass, builder, projection);
        if let Some(description) = self.description {
            Sprite::new(t(description), [96.0, 448.0, 976.0, 948.0])
                .alpha(self.description_alpha.value())
                .draw(pass, builder, projection);
        }

        // `0x710010cf90`
        for (i, item) in self.items.iter().enumerate() {
            let (x, y, a) = (item.x.value(), item.y.value(), item.alpha.value());
            // the plate's rows are read with the menu's height (902), as the original does
            let v = ((i as u32 * 82) | 1) as f32 / 902.0;
            let mut plate = Sprite::new(t("plate"), [x, y + 1.0, x + 704.0, y + 81.0]).uv([
                0.0,
                v,
                1.0,
                v + 80.0 / 902.0,
            ]);
            plate.color = ui::lerp_color(BROWN, RED, item.highlight.value()).with_alpha(a);
            plate.draw(pass, builder, projection);
            let v = item.ty as f32 * 82.0 / 902.0;
            Sprite::new(t("menu"), [x, y, x + 704.0, y + 82.0])
                .uv([0.0, v, 1.0, v + 82.0 / 902.0])
                .draw(pass, builder, projection);
            let flash = item.flash.value();
            if flash > 0.0 {
                ui::fill(
                    pass,
                    projection,
                    [x, y + 1.0, x + 704.0, y + 81.0],
                    FloatColor4::WHITE.with_alpha(flash),
                );
            }
            // a fading item is covered by the background again
            if a < 1.0 {
                let (u, v) = (x / 1920.0, y / 1080.0);
                ui::draw_texture(
                    pass,
                    background.as_texture_source(),
                    projection,
                    [x, y, x + 704.0, y + 82.0],
                    [u, v, u + 704.0 / 1920.0, v + 82.0 / 1080.0],
                    FloatColor4::WHITE.with_alpha(1.0 - a),
                    builder,
                );
            }
        }

        let dim = self.dim.value();
        if dim > 0.0 {
            ui::fill(pass, projection, full, FloatColor4::BLACK.with_alpha(dim));
        }
        if let Some(child) = &self.child {
            child.render(pass);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::background_index;

    #[test]
    fn progress_picks_the_background() {
        assert_eq!(background_index(0), 0);
        assert_eq!(background_index(17), 5);
        assert_eq!(background_index(18), 6);
        assert_eq!(background_index(21), 8);
        assert_eq!(background_index(30), 12);
        assert_eq!(background_index(31), 13);
    }
}
