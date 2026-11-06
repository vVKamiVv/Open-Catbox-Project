//! The Movie Box (`shin::MovieMode`, ctor `0x7100087a40`, update `0x7100088290`, render `0x7100088310`, keys
//! `0x7100088c70`): the ten opening movies as thumbnails over `/moviemode.txa`, locked ones covered. A plays a movie
//! full screen (`MovieMode::Viewer` `0x7100089dc0`) after the gallery fades to black; A or B skips it.
//!
//! shortcut: the console's keep-awake request while a movie plays (`0x71000fc3b0`) has no PC equivalent here.

use std::sync::Arc;

use enum_map::EnumMap;
use glam::{Mat4, Vec2, vec3};
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::SaveData,
        scenario::{info::MovieId, instruction_elements::FromNumber},
    },
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
    vm::command::types::Volume,
};
use shin_input::{ActionState, Pointer};
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use tracing::warn;

use crate::{
    adv::{
        hud::{OUTLINE, text_layout},
        menu::MenuAssets,
    },
    app::AppAction,
    asset::{movie::Movie, system::AssetServer, texture_archive::NamedTextures},
    audio::play_sys_se,
    layer::{
        Layer as _, render_layer_without_bg,
        render_params::TransformParams,
        text_block::TextBlock,
        user::{
            minigame::{self, Sprite},
            movie_layer::{MovieArgs, MovieLayer},
        },
    },
    render::PreRenderContext,
    ui::{
        self, MenuInput,
        gesture::{Gesture, GestureRecognizer},
    },
    update::{AdvUpdatable as _, AdvUpdateContext, UpdateContext},
};

/// The movies the box lists (`0x7100171ba4`)
const MOVIES: i32 = 10;
const BORDER: FloatColor4 = FloatColor4::from_rgba(0.1843, 0.0980, 0.0784, 1.0);

struct Item {
    movie: i32,
    unlocked: bool,
    highlight: Tweener,
    flash: Tweener,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Active,
    /// The item flashes, then the viewer opens
    Deciding,
    Watching,
    Closing,
    Done,
}

/// What a touch went down on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    Close,
    Item(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewerState {
    /// The gallery fades to black over 30 frames
    Covering,
    Playing,
    /// The movie fades out over 30 frames
    Ending,
    /// The black cover lifts over 30 frames
    Uncovering,
}

/// `MovieMode::Viewer`
struct Viewer {
    movie: Option<MovieLayer>,
    movie_id: i32,
    state: ViewerState,
    /// The movie's picture and sound (`+0xc8`) and the black cover (`+0x120`)
    fade: Tweener,
    cover: Tweener,
    /// The viewer's own touch input
    gestures: GestureRecognizer,
}

pub struct MovieBox {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    textures: Arc<NamedTextures>,
    pub global: SaveData,
    items: Vec<Item>,
    cursor: usize,
    close_flash: Tweener,
    completion: TextBlock,
    viewer: Option<Viewer>,
    state: State,
    /// The gallery's touch input (`GestureRecognizer`, listener `0x7100089300` and its neighbours)
    gestures: GestureRecognizer,
    touch: Option<Touch>,
}

impl MovieBox {
    pub fn new(
        pre_render: &PreRenderContext,
        asset_server: &AssetServer,
        audio: Arc<AudioManager>,
        assets: MenuAssets,
        global: SaveData,
    ) -> anyhow::Result<Self> {
        let textures = asset_server.load_sync::<NamedTextures>("/moviemode.txa")?;
        let movies = assets.scenario.info_tables().movie_info.len() as i32;
        let items = (0..MOVIES.min(movies))
            .map(|movie| Item {
                movie,
                unlocked: global.save_vectors.unlocked_movies.contains(movie as usize),
                highlight: Tweener::new((movie == 0) as i32 as f32),
                flash: Tweener::new(0.0),
            })
            .collect::<Vec<_>>();
        let unlocked = items.iter().filter(|item| item.unlocked).count();
        let rate = unlocked * 1000 / items.len().max(1);
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
            audio,
            textures,
            global,
            items,
            cursor: 0,
            close_flash: Tweener::new(0.0),
            completion,
            viewer: None,
            state: State::Active,
            gestures: GestureRecognizer::new(false),
            touch: None,
        })
    }

    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    fn sound(&self, assets: &AssetServer, name: &str) {
        play_sys_se(&self.audio, assets, name, Volume::default());
    }

    fn handle_keys(&mut self, assets: &AssetServer, input: &MenuInput) {
        if input.cancel {
            self.sound(assets, "cancel");
            self.close();
            return;
        }
        if input.confirm {
            self.decide(assets);
            return;
        }
        let n = self.items.len();
        let rows = n.div_ceil(4);
        let mut column = self.cursor % 4;
        let mut row = self.cursor / 4;
        if input.left {
            column = column.saturating_sub(1);
        } else if input.right {
            column = (column + 1).min(3);
        }
        if input.down {
            row = (row + 1).min(rows - 1);
        } else if input.up {
            row = row.saturating_sub(1);
        }
        let to = (column + 4 * row).min(n - 1);
        if to != self.cursor {
            self.sound(assets, "move");
            self.set_cursor(to);
        }
    }

    fn set_cursor(&mut self, to: usize) {
        minigame::tween_now(&mut self.items[self.cursor].highlight, 0.0, 0.2);
        self.cursor = to;
        minigame::tween_now(&mut self.items[to].highlight, 1.0, 0.25);
    }

    /// `0x7100088fc0`: the item flashes; an unlocked movie plays
    fn decide(&mut self, assets: &AssetServer) {
        let item = &mut self.items[self.cursor];
        item.flash = Tweener::new(0.0);
        minigame::flash(&mut item.flash);
        if item.unlocked {
            self.sound(assets, "decide");
            self.state = State::Deciding;
        } else {
            self.sound(assets, "error");
        }
    }

    /// `0x7100088e60`: the close button flashes, then the title
    fn close(&mut self) {
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.state = State::Closing;
    }

    /// `0x7100089170`: the thumbnail under a point, 216 px tall centred in its cell; the x is lost in the decompile,
    /// so the thumbnails are the port's render's
    fn item_at(&self, at: Vec2) -> Option<usize> {
        if !minigame::contains([136.0, 190.0, 1784.0, 916.0], at) {
            return None;
        }
        self.items.iter().position(|item| {
            let (column, row) = ((item.movie % 4) as f32, (item.movie / 4) as f32);
            let (x, y) = (150.0 + 412.0 * column, 203.0 + 242.0 * row);
            minigame::contains([x, y, x + 384.0, y + 216.0], at)
        })
    }

    /// The gallery's gesture listener (down `0x7100089300`, press `0x71000894c0`, tap `0x71000895e0`, drag
    /// `0x7100089700`): a down off the thumbnails and the close button is not tracked, a press raises the item and
    /// moves the cursor to it, a tap plays it; a drag only drops the highlight (nothing scrolls)
    fn handle_pointer(&mut self, assets: &AssetServer, pointer: Pointer, frames: f32) {
        for gesture in self.gestures.update(pointer, frames) {
            match gesture {
                Gesture::Down(at) => {
                    self.touch = if minigame::contains([1764.0, 32.0, 1848.0, 116.0], at) {
                        Some(Touch::Close)
                    } else if (150.0..992.0).contains(&at.y) {
                        self.item_at(at).map(Touch::Item)
                    } else {
                        None
                    };
                    if self.touch.is_none() {
                        self.gestures.cancel();
                    }
                }
                Gesture::Press => match self.touch {
                    Some(Touch::Close) => minigame::raise(&mut self.close_flash),
                    Some(Touch::Item(index)) => {
                        minigame::raise(&mut self.items[index].flash);
                        self.set_cursor(index);
                    }
                    None => {}
                },
                // the flash and the close restart the raised highlight
                Gesture::Tap(_) => match self.touch.take() {
                    Some(Touch::Close) => {
                        self.sound(assets, "decide");
                        self.close();
                    }
                    Some(Touch::Item(index)) => {
                        self.set_cursor(index);
                        self.decide(assets);
                    }
                    None => {}
                },
                Gesture::Drag(..) | Gesture::End => match self.touch.take() {
                    Some(Touch::Close) => minigame::lower(&mut self.close_flash),
                    Some(Touch::Item(index)) => minigame::lower(&mut self.items[index].flash),
                    None => {}
                },
                Gesture::LongPress | Gesture::Flick(_) => {}
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
        crate::audio::sys_se::set_setting_volume(settings.v3_sysvol);
        crate::audio::sys_se::set_movie_setting_volumes(
            settings.v0_bgmvol,
            settings.v1_sfxvol,
            settings.v2_voicevol,
        );
        let input = MenuInput::new(state, pointer);
        match self.state {
            State::Active => {
                self.handle_pointer(context.asset_server, pointer, frames);
                // a touch holds the buttons off (`0x710006b860`)
                if !self.gestures.is_active() && self.state == State::Active {
                    self.handle_keys(context.asset_server, &input);
                }
            }
            State::Deciding => {
                if self.items[self.cursor].flash.is_idle() {
                    let mut cover = Tweener::new(0.0);
                    ui::tween(&mut cover, 1.0, 30.0, Easing::Linear);
                    self.viewer = Some(Viewer {
                        movie: None,
                        movie_id: self.items[self.cursor].movie,
                        state: ViewerState::Covering,
                        fade: Tweener::new(1.0),
                        cover,
                        gestures: GestureRecognizer::new(false),
                    });
                    self.state = State::Watching;
                }
            }
            State::Watching => {
                if self.update_viewer(context, &input) {
                    self.viewer = None;
                    self.state = State::Active;
                }
            }
            State::Closing => {
                if self.close_flash.is_idle() {
                    self.state = State::Done;
                }
            }
            State::Done => {}
        }
        ui::advance(&mut self.close_flash, frames);
        for item in &mut self.items {
            ui::advance(&mut item.highlight, frames);
            ui::advance(&mut item.flash, frames);
        }
    }

    /// True once the viewer is gone
    fn update_viewer(&mut self, context: &mut UpdateContext, input: &MenuInput) -> bool {
        let Some(viewer) = &mut self.viewer else {
            return true;
        };
        let frames = context.delta_ticks.as_f32();
        // `0x710008a900`: a tap anywhere skips the movie
        let tapped = viewer
            .gestures
            .update(input.pointer, frames)
            .iter()
            .any(|gesture| matches!(gesture, Gesture::Tap(_)));
        let mut exit_sound = None;
        match viewer.state {
            ViewerState::Covering => {
                if viewer.cover.is_idle() {
                    // `$_3`: once the screen is black the movie starts
                    let info = self
                        .assets
                        .scenario
                        .info_tables()
                        .movie_info(MovieId::from_number(viewer.movie_id));
                    match context.asset_server.load_sync::<Movie>(info.path()) {
                        Ok(movie) => {
                            viewer.movie = Some(MovieLayer::new(
                                context.pre_render.device,
                                &self.audio,
                                movie,
                                MovieArgs {
                                    volume_source: info.volume_source,
                                    transparency: info.transparency,
                                    local_volume: Volume(1.0),
                                    repeat: false,
                                },
                                None,
                            ));
                            viewer.state = ViewerState::Playing;
                        }
                        Err(e) => {
                            warn!("Could not load the movie: {e:?}");
                            viewer.state = ViewerState::Ending;
                        }
                    }
                }
            }
            ViewerState::Playing => {
                let ended = viewer
                    .movie
                    .as_ref()
                    .is_none_or(|movie| movie.is_finished());
                if ended {
                    viewer.state = ViewerState::Ending;
                } else if tapped || !viewer.gestures.is_active() && (input.confirm || input.cancel)
                {
                    // `0x710008a770`: A or B with "cancel", a tap with "decide"
                    exit_sound = Some(if tapped { "decide" } else { "cancel" });
                    ui::tween(&mut viewer.fade, 0.0, 30.0, Easing::Linear);
                    viewer.state = ViewerState::Ending;
                }
            }
            ViewerState::Ending => {
                let ended = viewer
                    .movie
                    .as_ref()
                    .is_none_or(|movie| movie.is_finished());
                if viewer.fade.is_idle() || ended {
                    viewer.movie = None;
                    ui::tween(&mut viewer.cover, 0.0, 30.0, Easing::Linear);
                    viewer.state = ViewerState::Uncovering;
                }
            }
            ViewerState::Uncovering => {
                if viewer.cover.is_idle() {
                    return true;
                }
            }
        }
        ui::advance(&mut viewer.fade, frames);
        ui::advance(&mut viewer.cover, frames);
        if let Some(movie) = &mut viewer.movie {
            movie.set_local_volume(viewer.fade.value());
            let update = AdvUpdateContext {
                frame_id: context.frame_id,
                delta_ticks: context.delta_ticks,
                asset_server: context.asset_server,
                device: context.pre_render.device,
                queue: context.pre_render.queue,
                are_animations_allowed: true,
            };
            movie.update(&update);
            movie.pre_render(context.pre_render, &TransformParams::default());
        }
        if let Some(name) = exit_sound {
            self.sound(context.asset_server, name);
        }
        false
    }

    pub fn render(&self, pass: &mut RenderPass) {
        let projection = ui::projection();
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
        Sprite::new(t("bg"), [0.0, 0.0, 1920.0, 1080.0]).draw(pass, opaque, projection);

        for item in &self.items {
            let (column, row) = ((item.movie % 4) as f32, (item.movie / 4) as f32);
            let s = 1.0 + 0.05 * item.highlight.value();
            let transform = projection
                * Mat4::from_translation(vec3(
                    150.0 + 412.0 * column + 192.0,
                    203.0 + 242.0 * row + 108.0,
                    0.0,
                ))
                * Mat4::from_scale(vec3(s, s, 1.0))
                * Mat4::from_translation(vec3(-192.0, -108.0, 0.0));
            let rect = [0.0, 0.0, 384.0, 216.0];
            Sprite::new(t(&format!("mv{:02}", item.movie + 1)), rect).draw(pass, opaque, transform);
            if !item.unlocked {
                Sprite::new(t("logo"), rect).draw(pass, builder, transform);
            }
            let highlight = item.highlight.value();
            if highlight > 0.0 {
                ui::fill(
                    pass,
                    transform,
                    rect,
                    FloatColor4::WHITE.with_alpha(highlight * 0.5),
                );
            }
            let flash = item.flash.value();
            if flash > 0.0 {
                ui::fill(pass, transform, rect, FloatColor4::WHITE.with_alpha(flash));
            }
        }
        Sprite::new(t("close"), [1764.0, 32.0, 1848.0, 116.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);

        // the key help, right to left (`0x7100088980`, `0x7100171bcc`)
        let grey = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);
        let watch = if self.items[self.cursor].unlocked {
            FloatColor4::WHITE
        } else {
            grey
        };
        let mut x = self.draw_help(pass, 1872.0, 4, 48.0, 72.0, FloatColor4::WHITE);
        x = self.draw_help(pass, x, 3, 96.0, 76.0, watch);
        self.draw_help(pass, x, 0, 0.0, 206.0, FloatColor4::WHITE);

        ui::draw_text(
            pass,
            &self.completion,
            projection * Mat4::from_translation(vec3(48.0, 1012.0, 0.0)),
            FloatColor4::WHITE,
            BORDER,
        );

        // the viewer: the black cover, then the movie at its fade
        if let Some(viewer) = &self.viewer {
            let cover = viewer.cover.value();
            let blend = if cover >= 1.0 {
                ColorBlendType::Opaque
            } else {
                ColorBlendType::Layer1
            };
            ui::fill_with(
                pass,
                projection,
                [0.0, 0.0, 1920.0, 1080.0],
                FloatColor4::BLACK.with_alpha(cover),
                RenderRequestBuilder::new().color_blend_type(blend),
            );
            if let Some(movie) = &viewer.movie {
                render_layer_without_bg(pass, &TransformParams::default(), movie, 0);
                let dark = 1.0 - viewer.fade.value();
                ui::fill(
                    pass,
                    projection,
                    [0.0, 0.0, 1920.0, 1080.0],
                    FloatColor4::BLACK.with_alpha(dark),
                );
            }
        }
    }

    fn draw_help(
        &self,
        pass: &mut RenderPass,
        right: f32,
        key: u8,
        row: f32,
        width: f32,
        color: FloatColor4,
    ) -> f32 {
        let projection = ui::projection();
        let y = 1012.0;
        let v = row / 144.0;
        let mut label = Sprite::new(
            self.textures.get("help_moviemode"),
            [right - width, y, right, y + 48.0],
        )
        .uv([0.0, v, width / 320.0, v + 1.0 / 3.0]);
        label.color = color;
        label.draw(pass, ui::builder(), projection);
        let x = right - width + 2.0 - 48.0;
        let (u, v) = ((key % 8) as f32 * 0.125, (key / 8) as f32 * 0.5);
        let mut icon = Sprite::new(self.textures.get("key"), [x, y, x + 48.0, y + 48.0]).uv([
            u,
            v,
            u + 0.125,
            v + 0.5,
        ]);
        icon.color = color;
        icon.draw(pass, ui::builder(), projection);
        x - 10.0
    }
}
