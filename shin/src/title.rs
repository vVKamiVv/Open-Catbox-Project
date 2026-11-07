//! The title screen (`shin::Title`, ctor `0x710010b130`, update `0x710010c9b0`, render `0x710010cc30`, keys
//! `0x710010e1a0`): a background that follows the story's progress, the logo, and the menu items with their
//! description panels, all from `/title.txa`. Load and Config open over it; Continue starts the auto slot.
//!
//! The BGM player is the one every scene shares: the title's track crossfades in over whatever was playing, or only
//! takes its volume when it is already the one playing.

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
    ColorBlendType, RenderRequestBuilder, gpu_texture::GpuTexture, render_pass::RenderPass,
    render_texture::RenderTexture,
};
use tracing::{info, warn};

use crate::{
    adv::menu::{
        AdvRequest, MenuAssets, Task, TaskCx,
        chars::Chars,
        config_menu::ConfigMenu,
        dialog::Dialog,
        manual::Manual,
        save_menu::{Mode, SaveMenu},
        tips::{Tips, TipsFilter},
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
    render::PreRenderContext,
    ui::{
        self, MenuInput,
        serif::{self, SerifText, UiText},
    },
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
/// The port's Exit, last: it has no row in `menu` nor a description panel, so both are drawn ([`ExitTexts`])
const EXIT: u8 = 11;

const DESCRIPTIONS: [&str; 12] = [
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
    "exit",
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
    /// Characters or Tips: the other items leave and the chosen one rises to a header (`MenuItemOut $_2`)
    Leaving(u8),
    /// The episode list of Characters or Tips is open
    SubMenu,
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
    /// `CreateBustupMode`
    CharacterBox,
    /// `CreateBGMMode`
    MusicBox,
    /// `CreateMovieMode`
    MovieBox,
    /// Continue or a bookmark from Load
    Load(GameDataEntry),
    /// The port's Exit: the game closes
    Quit,
}

pub struct Title {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    textures: Arc<NamedTextures>,
    background: PictureLayer,
    /// The background rendered each frame (`+0x2c8`), drawn under everything and over fading items
    background_texture: Option<RenderTexture>,
    pub bgm: BgmPlayer,
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
    /// The episodes with characters (bits 8-15 of the ctor's mask), which also have tips (bits 8-14)
    episodes: u32,
    /// The first row's y
    top: f32,
    sub_menu: Option<SubMenu>,
    /// Load, Config, Manual, Tips or Characters over the title
    child: Option<Box<dyn Task>>,
    requests: Vec<AdvRequest>,
    exit: Option<TitleExit>,
    write_requested: bool,
    pointer_held: bool,
    pressed: Option<usize>,
    /// Exit's label and description, made on the first update (they need the GPU)
    exit_texts: Option<ExitTexts>,
    /// A message to show in an OK dialog (why the save file could not be used)
    pub alert: Option<String>,
}

/// The port's Exit item, in the art's lettering: Gelasio, as near the art's Georgia-like faces
struct ExitTexts {
    label: SerifText,
    heading: UiText,
    body: UiText,
}

impl ExitTexts {
    /// The menu's bold white lettering with its dark drop shadow: capitals 43 tall
    const LABEL: serif::Style = serif::Style {
        face: serif::Face::Serif,
        weight: 700.0,
        size: 62.0,
        squeeze: 1.0,
        shadow: (glam::Vec2::new(3.0, 3.0), 0.7),
    };
    /// The description panels' heading (capitals 42 tall) and text (32), regular, without a shadow
    const HEADING: serif::Style = serif::Style {
        face: serif::Face::Serif,
        weight: 400.0,
        size: 61.0,
        squeeze: 1.0,
        shadow: (glam::Vec2::ZERO, 0.0),
    };
    const BODY: serif::Style = serif::Style {
        size: 46.0,
        ..Self::HEADING
    };

    fn new(ctx: &PreRenderContext, assets: &MenuAssets) -> Self {
        let japanese = crate::adv::menu::config_menu::japanese();
        let font = &assets.system_font;
        let (heading, body) = if japanese {
            ("終了", "ゲームを終了します。")
        } else {
            ("Exit", "Close the game.")
        };
        Self {
            // the menu art is in English in both languages
            label: SerifText::new(ctx.device, ctx.queue, "Exit", Self::LABEL),
            heading: UiText::new(ctx, font, heading, Self::HEADING, 44.0),
            body: UiText::new(ctx, font, body, Self::BODY, 36.0),
        }
    }

    /// Right-aligned where the art's labels end (x 662 of the 704-wide row), capitals 38.5 down the 82-tall row
    fn draw_label(&self, pass: &mut RenderPass, x: f32, y: f32) {
        let left = x + 670.0 - self.label.size.x;
        self.label
            .draw(pass, ui::projection(), left, y + 38.5, FloatColor4::WHITE);
    }

    /// A description panel: an empty one built from Config's (its edges, and its plain lower half stretched), with
    /// the heading and the text where the art has them
    fn draw_description(&self, pass: &mut RenderPass, panel: &GpuTexture, alpha: f32) {
        let projection = ui::projection();
        let builder = ui::builder();
        let (x, y) = (96.0, 448.0);
        for (top, bottom, v0, v1) in [
            (0.0, 20.0, 0.0, 20.0),
            (20.0, 480.0, 300.0, 310.0),
            (480.0, 500.0, 480.0, 500.0),
        ] {
            Sprite::new(panel, [x, y + top, x + 880.0, y + bottom])
                .uv([0.0, v0 / 500.0, 1.0, v1 / 500.0])
                .alpha(alpha)
                .draw(pass, builder, projection);
        }
        let white = FloatColor4::WHITE.with_alpha(alpha);
        let border = FloatColor4::BLACK.with_alpha(alpha);
        self.heading.draw(pass, projection, x + 38.0, y + 52.0, white, border);
        self.body.draw(pass, projection, x + 35.0, y + 139.0, white, border);
    }
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
        mut bgm: BgmPlayer,
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
        types.push(EXIT);

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
            episodes: mask >> 8,
            top,
            sub_menu: None,
            child: None,
            requests: Vec::new(),
            exit: None,
            write_requested: false,
            pointer_held: false,
            pressed: None,
            exit_texts: None,
            alert: None,
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
        if ty == CHARACTERS || ty == TIPS {
            // the other items leave 4 frames apart; then the chosen one rises to y 228 as a header
            let mut k = 0;
            for (i, item) in self.items.iter_mut().enumerate() {
                if i == self.cursor {
                    continue;
                }
                let delay = 4.0 * k as f32;
                ui::hold(&mut item.alpha, delay);
                ui::tween(&mut item.alpha, 0.0, 10.0, Easing::Linear);
                ui::hold(&mut item.x, delay);
                ui::tween(&mut item.x, ITEM_X + 100.0, 10.0, Easing::SineIn);
                k += 1;
            }
            let delay = 4.0 * (self.items.len() - 1) as f32 + 30.0;
            let item = &mut self.items[self.cursor];
            ui::hold(&mut item.y, delay);
            ui::tween(&mut item.y, 228.0, 30.0, Easing::SineInOut);
            ui::hold(&mut item.highlight, delay);
            ui::tween(&mut item.highlight, 0.0, 30.0, Easing::SineOut);
            self.state = State::Leaving(ty);
            return;
        }
        self.state = State::Deciding(ty);
    }

    /// `MenuItemIn(1)` (`0x710010d670`): back from the episode list, the header returns to its row and the others
    /// slide in after it
    fn items_back(&mut self) {
        let top = self.top;
        let cursor = self.cursor;
        let mut k = 0;
        for (i, item) in self.items.iter_mut().enumerate() {
            if i == cursor {
                ui::tween(
                    &mut item.y,
                    top + ITEM_H * i as f32,
                    30.0,
                    Easing::SineInOut,
                );
                ui::tween(&mut item.highlight, 1.0, 30.0, Easing::Linear);
                continue;
            }
            let delay = 30.0 + 4.0 * k as f32;
            item.alpha = Tweener::new(0.0);
            ui::hold(&mut item.alpha, delay);
            ui::tween(&mut item.alpha, 1.0, 10.0, Easing::Linear);
            item.x = Tweener::new(ITEM_X + 100.0);
            ui::hold(&mut item.x, delay);
            ui::tween(&mut item.x, ITEM_X, 10.0, Easing::SineOut);
            item.flash = Tweener::new(0.0);
            k += 1;
        }
        self.description = Some(DESCRIPTIONS[self.items[cursor].ty as usize]);
        self.state = State::ItemsIn;
    }

    /// The screen of an episode in the list: the tips of that episode, or its characters
    fn open_episode(&mut self, context: &mut UpdateContext, characters: bool, episode: u8) {
        minigame::tween_now(&mut self.dim, 0.5, 0.1);
        let assets = self.assets.clone();
        let cx = self.task_cx(context, true, MenuInput::default());
        self.child = if characters {
            // `0x71001108b8`: the episode's own grid, from its first page
            Chars::new(&cx, &assets, episode as i32, 0)
                .map(|chars| Box::new(chars) as Box<dyn Task>)
        } else {
            Tips::new(&cx, &assets, TipsFilter::Episode(episode))
                .map(|tips| Box::new(tips) as Box<dyn Task>)
        };
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
            LOAD | CONFIG | MANUAL => {
                minigame::tween_now(&mut self.dim, 0.5, 0.1);
                let assets = self.assets.clone();
                let cx = self.task_cx(context, true, MenuInput::default());
                let child: Option<Box<dyn Task>> = match ty {
                    LOAD => SaveMenu::new(&cx, &assets, Mode::Load)
                        .map(|m| Box::new(m) as Box<dyn Task>),
                    CONFIG => ConfigMenu::new(&cx, &assets).map(|m| Box::new(m) as Box<dyn Task>),
                    _ => Manual::new(&cx).map(|m| Box::new(m) as Box<dyn Task>),
                };
                self.child = child;
            }
            CHARACTER_BOX => {
                self.exit = Some(TitleExit::CharacterBox);
                self.state = State::Done;
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
            EXIT => {
                self.exit = Some(TitleExit::Quit);
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
        if self.exit_texts.is_none() {
            self.exit_texts = Some(ExitTexts::new(context.pre_render, &self.assets));
        }
        let settings = self.global.settings;
        self.bgm.set_gain(settings.v0_bgmvol, 1.0);
        crate::audio::sys_se::set_setting_volume(settings.v3_sysvol);
        crate::audio::sys_se::set_movie_setting_volumes(
            settings.v0_bgmvol,
            settings.v1_sfxvol,
            settings.v2_voicevol,
        );

        let input = MenuInput::new(state, pointer);
        if self.child.is_none()
            && let Some(message) = self.alert.take()
        {
            self.child = Some(Box::new(Dialog::ok(
                context.pre_render,
                &self.assets,
                &message,
            )));
        }
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
                State::Leaving(ty) => {
                    if !self.items_moving() {
                        // `Title::SubMenu` (`0x710010f0a0`)
                        let count = if ty == CHARACTERS { 8 } else { 7 };
                        let episodes = (0..count)
                            .filter(|k| self.episodes & (1 << k) != 0)
                            .collect();
                        let sub_menu = SubMenu::new(ty == CHARACTERS, episodes);
                        ui::hold(
                            &mut self.description_alpha,
                            4.0 * sub_menu.items.len() as f32,
                        );
                        ui::tween(&mut self.description_alpha, 1.0, 30.0, Easing::Linear);
                        self.sub_menu = Some(sub_menu);
                        self.state = State::SubMenu;
                    }
                }
                State::SubMenu => {
                    let Some(mut sub_menu) = self.sub_menu.take() else {
                        self.state = State::Active;
                        return;
                    };
                    match sub_menu.update(&input, frames) {
                        SubMenuEvent::None => {}
                        SubMenuEvent::Sound(name) => self.sound(context, name),
                        SubMenuEvent::Open(episode) => {
                            self.open_episode(context, sub_menu.characters, episode)
                        }
                        SubMenuEvent::Closed => {
                            self.items_back();
                        }
                    }
                    if self.state == State::SubMenu {
                        self.sub_menu = Some(sub_menu);
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
        let sub_items = self
            .sub_menu
            .iter_mut()
            .flat_map(|sub| sub.items.iter_mut());
        for item in self.items.iter_mut().chain(sub_items) {
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
            let alpha = self.description_alpha.value();
            if description == DESCRIPTIONS[EXIT as usize] {
                if let Some(texts) = &self.exit_texts {
                    texts.draw_description(pass, t("text_config"), alpha);
                }
            } else {
                Sprite::new(t(description), [96.0, 448.0, 976.0, 948.0])
                    .alpha(alpha)
                    .draw(pass, builder, projection);
            }
        }

        // `0x710010cf90`
        for (i, item) in self.items.iter().enumerate() {
            let (x, y, a) = (item.x.value(), item.y.value(), item.alpha.value());
            // the plate's rows are read with the menu's height (902), as the original does
            // (the port's twelfth row, Exit, reuses the plate's last)
            let v = ((i.min(10) as u32 * 82) | 1) as f32 / 902.0;
            let mut plate = Sprite::new(t("plate"), [x, y + 1.0, x + 704.0, y + 81.0]).uv([
                0.0,
                v,
                1.0,
                v + 80.0 / 902.0,
            ]);
            plate.color = ui::lerp_color(BROWN, RED, item.highlight.value()).with_alpha(a);
            plate.draw(pass, builder, projection);
            if item.ty == EXIT {
                if let Some(texts) = &self.exit_texts {
                    texts.draw_label(pass, x, y);
                }
            } else {
                let v = item.ty as f32 * 82.0 / 902.0;
                Sprite::new(t("menu"), [x, y, x + 704.0, y + 82.0])
                    .uv([0.0, v, 1.0, v + 82.0 / 902.0])
                    .draw(pass, builder, projection);
            }
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
        // `0x710010ff80`: the episode list, flat-coloured rows faded by redrawing the background over them
        for item in self.sub_menu.iter().flat_map(|sub| &sub.items) {
            let (x, y, a) = (item.x.value(), item.y.value(), item.alpha.value());
            if a.abs() < f32::EPSILON {
                continue;
            }
            ui::fill(
                pass,
                projection,
                [x, y + 1.0, x + 400.0, y + 71.0],
                ui::lerp_color(BROWN, RED, item.highlight.value()).with_alpha(a),
            );
            let v = item.ty as f32 * 0.125;
            Sprite::new(t("menu2"), [x + 80.0, y + 4.0, x + 400.0, y + 68.0])
                .uv([0.0, v, 1.0, v + 0.125])
                .draw(pass, builder, projection);
            let flash = item.flash.value();
            if flash > 0.0 {
                ui::fill(
                    pass,
                    projection,
                    [x, y, x + 400.0, y + 72.0],
                    FloatColor4::WHITE.with_alpha(flash),
                );
            }
            if a < 1.0 {
                let (u, v) = (x / 1920.0, y / 1080.0);
                ui::draw_texture(
                    pass,
                    background.as_texture_source(),
                    projection,
                    [x, y, x + 400.0, y + 72.0],
                    [u, v, u + 0.208333, v + 0.066667],
                    FloatColor4::WHITE.with_alpha(1.0 - a),
                    builder,
                );
            }
        }
        if let Some(child) = &self.child {
            child.render(pass);
        }
    }
}

/// What the episode list asks of the title
enum SubMenuEvent {
    None,
    Sound(&'static str),
    /// Open this episode's screen
    Open(u8),
    /// The list has left: the title's items come back
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubState {
    /// The rows slide in (`$_11` waits for them)
    In,
    Active,
    /// A row flashes, then its screen opens (`$_12`)
    Deciding,
    /// The rows leave (`$_13` waits for them)
    Out,
}

/// `Title::SubMenu` (ctor `0x710010f0a0`, keys `0x71001102a0`): the episodes of Characters or Tips, by row of `menu2`
struct SubMenu {
    characters: bool,
    /// Rows with `ty` = the episode
    items: Vec<Item>,
    cursor: usize,
    state: SubState,
    gestures: ui::gesture::GestureRecognizer,
    /// The row the touch went down on (`+0xbc`)
    touched: Option<usize>,
}

impl SubMenu {
    const X: f32 = 1488.0;

    fn new(characters: bool, episodes: Vec<u8>) -> Self {
        let items = episodes
            .into_iter()
            .enumerate()
            .map(|(k, episode)| {
                let delay = 4.0 * k as f32;
                let mut alpha = Tweener::new(0.0);
                ui::hold(&mut alpha, delay);
                ui::tween(&mut alpha, 1.0, 10.0, Easing::Linear);
                let mut x = Tweener::new(Self::X + 100.0);
                ui::hold(&mut x, delay);
                ui::tween(&mut x, Self::X, 10.0, Easing::SineOut);
                Item {
                    ty: episode,
                    alpha,
                    highlight: Tweener::new((k == 0) as i32 as f32),
                    x,
                    y: Tweener::new(320.0 + 72.0 * k as f32),
                    flash: Tweener::new(0.0),
                }
            })
            .collect();
        Self {
            characters,
            items,
            cursor: 0,
            state: SubState::In,
            gestures: ui::gesture::GestureRecognizer::new(false),
            touched: None,
        }
    }

    fn moving(&self) -> bool {
        self.items
            .iter()
            .any(|item| !item.alpha.is_idle() || !item.x.is_idle())
    }

    fn update(&mut self, input: &MenuInput, frames: f32) -> SubMenuEvent {
        match self.state {
            SubState::In => {
                if !self.moving() {
                    self.state = SubState::Active;
                }
                SubMenuEvent::None
            }
            // a touch holds the keys off (`0x710006b860`)
            SubState::Active => match self.handle_pointer(input.pointer, frames) {
                SubMenuEvent::None if !self.gestures.is_active() => self.handle_input(input),
                event => event,
            },
            SubState::Deciding => {
                if self.items[self.cursor].flash.is_idle() {
                    self.state = SubState::Active;
                    return SubMenuEvent::Open(self.items[self.cursor].ty);
                }
                SubMenuEvent::None
            }
            SubState::Out => {
                if self.moving() {
                    SubMenuEvent::None
                } else {
                    SubMenuEvent::Closed
                }
            }
        }
    }

    fn handle_input(&mut self, input: &MenuInput) -> SubMenuEvent {
        let n = self.items.len();
        if input.confirm {
            return self.decide();
        }
        if input.cancel {
            return self.close();
        }
        let to = if input.down {
            (self.cursor + 1) % n
        } else if input.up {
            (self.cursor + n - 1) % n
        } else {
            return SubMenuEvent::None;
        };
        if to == self.cursor {
            return SubMenuEvent::None;
        }
        self.set_cursor(to);
        SubMenuEvent::Sound("move")
    }

    /// `0x71001103b0` with 0: the row flashes, then its screen opens
    fn decide(&mut self) -> SubMenuEvent {
        let flash = &mut self.items[self.cursor].flash;
        *flash = Tweener::new(0.0);
        minigame::flash(flash);
        self.state = SubState::Deciding;
        SubMenuEvent::Sound("decide")
    }

    /// `0x71001103b0` with 1: the rows leave
    fn close(&mut self) -> SubMenuEvent {
        for (k, item) in self.items.iter_mut().enumerate() {
            let delay = 4.0 * k as f32;
            ui::hold(&mut item.alpha, delay);
            ui::tween(&mut item.alpha, 0.0, 10.0, Easing::Linear);
            ui::hold(&mut item.x, delay);
            ui::tween(&mut item.x, Self::X + 100.0, 10.0, Easing::SineIn);
        }
        self.state = SubState::Out;
        SubMenuEvent::Sound("cancel")
    }

    /// `0x71001106d0`: the highlight moves to row `to`
    fn set_cursor(&mut self, to: usize) {
        if to == self.cursor {
            return;
        }
        minigame::tween_now(&mut self.items[self.cursor].highlight, 0.0, 0.05);
        self.cursor = to;
        minigame::tween_now(&mut self.items[to].highlight, 1.0, 0.1);
    }

    /// The rows' gesture listener (down `0x7100110b30`, hit `0x7100110a30`, press `0x7100110bb0`, tap
    /// `0x7100110c70`, drag `0x7100110d30`): a press brightens a row and makes it the cursor, a tap decides it or,
    /// off the rows, closes the list
    fn handle_pointer(&mut self, pointer: Pointer, frames: f32) -> SubMenuEvent {
        use ui::gesture::Gesture;
        let mut event = SubMenuEvent::None;
        for gesture in self.gestures.update(pointer, frames) {
            match gesture {
                // the rows as drawn, 400 x 72
                Gesture::Down(at) => {
                    self.touched = self.items.iter().position(|item| {
                        let (x, y) = (item.x.value(), item.y.value());
                        minigame::contains([x, y, x + 400.0, y + 72.0], at)
                    })
                }
                Gesture::Press => {
                    if let Some(k) = self.touched {
                        minigame::raise(&mut self.items[k].flash);
                        self.set_cursor(k);
                    }
                }
                Gesture::Tap(_) => {
                    event = match self.touched {
                        Some(k) => {
                            self.set_cursor(k);
                            self.decide()
                        }
                        None => self.close(),
                    }
                }
                Gesture::Drag(..) => {
                    if let Some(k) = self.touched.take() {
                        minigame::lower(&mut self.items[k].flash);
                    }
                }
                _ => {}
            }
        }
        event
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
