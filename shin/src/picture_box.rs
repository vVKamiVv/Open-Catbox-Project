//! The Picture Box (`shin::CGMode`, ctor `0x7100047040`, update `0x7100047ff0`, render `0x71000482b0`, keys
//! `0x7100049190`): the scenario's PictureBoxInfo entries as thumbnails, four to a row, over `/cgmode.txa`; locked
//! entries show the logo card, partly unlocked ones a "u/n" counter. A opens the entry's unlocked pictures full
//! screen (`CGMode::Viewer` `0x710004a6e0`), one after another with 30-frame crossfades.
//!
//! shortcut: the viewer's pictures cannot be panned with the left stick; the d-pad and the pointer pan them.

use std::sync::Arc;

use enum_map::EnumMap;
use glam::{Mat4, Vec2, vec3};
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::SaveData,
        scenario::{
            info::{BgmId, PictureId},
            instruction_elements::FromNumber,
        },
    },
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tween, Tweener},
    vm::command::types::{LayerProperty, Volume},
};
use shin_input::{ActionState, Pointer};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, render_texture::RenderTexture,
    resize::CanvasSize,
};
use tracing::warn;

use crate::{
    adv::{
        hud::{OUTLINE, text_layout},
        menu::MenuAssets,
    },
    app::AppAction,
    asset::{picture::Picture, system::AssetServer, texture_archive::NamedTextures},
    audio::{BgmPlayer, play_sys_se},
    layer::{
        DrawableLayer as _, Layer as _, render_layer_without_bg,
        render_params::TransformParams,
        text_block::TextBlock,
        user::{
            PictureLayer,
            minigame::{self, Sprite},
        },
    },
    render::PreRenderContext,
    ui::{
        self, MenuInput,
        gesture::{Coast, Gesture, GestureRecognizer},
    },
    update::{AdvUpdatable as _, AdvUpdateContext, UpdateContext},
};

const COLUMNS: usize = 4;
const CELL_W: f32 = 412.0;
const ROW: f32 = 242.0;
const VIEW: f32 = 726.0;
const LIST_Y: f32 = 190.0;
const CLIP: [f32; 4] = [136.0, 185.0, 1784.0, 921.0];
const BORDER: FloatColor4 = FloatColor4::from_rgba(0.1843, 0.0980, 0.0784, 1.0);
/// The entry transition (`CGMode::First`), after which BGM 6 starts
const ENTRY_FRAMES: f32 = 60.0;

struct Item {
    /// The thumbnail in `cgmode.txa`
    name: String,
    pictures: Vec<u16>,
    unlocked: Vec<bool>,
    highlight: Tweener,
    flash: Tweener,
}

impl Item {
    fn unlocked_count(&self) -> usize {
        self.unlocked.iter().filter(|&&unlocked| unlocked).count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// `CGMode::First`: the crossfade from the title, then BGM 6
    Entering(u32),
    Active,
    /// The item flashes, then the viewer opens
    Deciding,
    Viewing,
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

/// `CGMode::Viewer`
struct Viewer {
    item: usize,
    /// The position in the entry's pictures (`+0xc4`)
    position: Option<usize>,
    picture: Option<PictureLayer>,
    /// The picture's size
    size: (f32, f32),
    show_marks: bool,
    fit: bool,
    pan: (f32, f32),
    /// The old screen fading out over the new one (`+0xe0`)
    transition: Option<(RenderTexture, Tweener)>,
    finished: bool,
    /// The viewer's own touch input, and the pan's speed after a flick (`+0xc8`)
    gestures: GestureRecognizer,
    velocity: Vec2,
}

impl Viewer {
    /// The picture is tweening between fit and 1:1
    fn is_scaling(&self) -> bool {
        self.picture.as_ref().is_some_and(|picture| {
            !picture
                .properties()
                .property_tweener(LayerProperty::ScaleX)
                .is_idle()
        })
    }
}

pub struct PictureBox {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    textures: Arc<NamedTextures>,
    pub bgm: BgmPlayer,
    pub global: SaveData,
    items: Vec<Item>,
    height: f32,
    cursor: usize,
    scroll: Tweener,
    close_flash: Tweener,
    completion: TextBlock,
    viewer: Option<Viewer>,
    state: State,
    target_size: (u32, u32),
    /// The list's touch input (`GestureRecognizer`, listener `0x7100049ba0` and its neighbours)
    gestures: GestureRecognizer,
    coast: Coast,
    touch: Option<Touch>,
}

impl PictureBox {
    pub fn new(
        pre_render: &PreRenderContext,
        asset_server: &AssetServer,
        audio: Arc<AudioManager>,
        assets: MenuAssets,
        global: SaveData,
        bgm: BgmPlayer,
    ) -> anyhow::Result<Self> {
        let textures = asset_server.load_sync::<NamedTextures>("/cgmode.txa")?;
        let items = assets
            .scenario
            .info_tables()
            .picture_box_info
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let pictures = entry
                    .picture_ids
                    .0
                    .iter()
                    .map(|id| id.into_i32() as u16)
                    .collect::<Vec<_>>();
                let unlocked = pictures
                    .iter()
                    .map(|&id| global.save_vectors.unlocked_cgs.contains(id as usize))
                    .collect();
                Item {
                    name: entry.name.as_str().to_string(),
                    pictures,
                    unlocked,
                    highlight: Tweener::new((i == 0) as i32 as f32),
                    flash: Tweener::new(0.0),
                }
            })
            .collect::<Vec<_>>();
        let height = (items.len().div_ceil(COLUMNS) as f32 * ROW).max(VIEW);

        let unlocked: usize = items.iter().map(Item::unlocked_count).sum();
        let total: usize = items.iter().map(|item| item.pictures.len()).sum();
        let rate = unlocked * 1000 / total.max(1);
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
            bgm,
            global,
            items,
            height,
            cursor: 0,
            scroll: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            completion,
            viewer: None,
            state: State::Entering(0),
            target_size: (1920, 1080),
            gestures: GestureRecognizer::new(false),
            coast: Coast::default(),
            touch: None,
        })
    }

    /// Done: the title should open with its cursor on the Picture Box
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
        self.height - VIEW
    }

    fn row_y(index: usize) -> f32 {
        ROW * (index / COLUMNS) as f32
    }

    /// `0x7100049720`
    fn set_cursor(&mut self, to: usize) {
        minigame::tween_now(&mut self.items[self.cursor].highlight, 0.0, 0.2);
        self.cursor = to;
        minigame::tween_now(&mut self.items[to].highlight, 1.0, 0.25);
        let y = Self::row_y(to);
        let scroll = self.scroll.target_value();
        let target = if y < scroll {
            y
        } else if y - scroll + ROW > VIEW {
            (y + ROW - VIEW).min(self.max_scroll())
        } else {
            return;
        };
        ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
    }

    fn handle_keys(&mut self, assets: &AssetServer, input: &MenuInput) {
        let n = self.items.len();
        if input.confirm {
            self.decide(assets);
            return;
        }
        if input.cancel {
            self.sound(assets, "cancel");
            self.close();
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
            -3
        } else if input.r {
            3
        } else {
            0
        };
        if h != 0 || v != 0 {
            let rows = n.div_ceil(COLUMNS) as i32;
            let row = (self.cursor as i32 / 4 + v).clamp(0, rows - 1);
            let column = (self.cursor as i32 % 4 + h).clamp(0, 3);
            let to = ((4 * row + column) as usize).min(n - 1);
            if to != self.cursor {
                self.sound(assets, "move");
                self.set_cursor(to);
            }
            return;
        }
        if input.scroll != 0.0 {
            let scroll = (self.scroll.value() + input.scroll * 32.0).clamp(0.0, self.max_scroll());
            self.scroll = Tweener::new(scroll);
            self.follow_cursor(scroll);
        }
    }

    /// `0x7100048190`: the cursor follows by rows into the view
    fn follow_cursor(&mut self, scroll: f32) {
        let n = self.items.len();
        let mut to = self.cursor as i32;
        while to >= 0 && (to as usize) < n {
            let y = Self::row_y(to as usize);
            if y < scroll {
                to += COLUMNS as i32;
            } else if y + ROW > scroll + VIEW {
                to -= COLUMNS as i32;
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

    /// `0x7100049540`: the item flashes; an entry with a picture unlocked opens in the viewer
    fn decide(&mut self, assets: &AssetServer) {
        let item = &mut self.items[self.cursor];
        item.flash = Tweener::new(0.0);
        minigame::flash(&mut item.flash);
        if item.unlocked_count() == 0 {
            self.sound(assets, "error");
        } else {
            self.sound(assets, "decide");
            self.state = State::Deciding;
        }
    }

    /// `0x71000493e0`: the close button flashes, then the title
    fn close(&mut self) {
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.state = State::Closing;
    }

    /// `0x7100049a60`: the item under a point of the list, `Some(None)` between items, `None` outside the list. The
    /// items are 412 x 242 cells (the ctor's rects); their x is lost in the decompile, so the four columns fill the
    /// list's x 136..1784, the port's render centring the thumbnails in them.
    fn item_at(&self, at: Vec2) -> Option<Option<usize>> {
        if !minigame::contains([136.0, LIST_Y, 1784.0, 916.0], at) {
            return None;
        }
        let y = at.y - LIST_Y + self.scroll.value();
        let index = (y / ROW) as usize * COLUMNS + ((at.x - 136.0) / CELL_W) as usize;
        Some((index < self.items.len()).then_some(index))
    }

    /// The list's gesture listener (down `0x7100049ba0`, press `0x7100049de0`, tap `0x7100049ec0`, drag
    /// `0x7100049ff0`, flick `0x710004a160`): a press raises the item and moves the cursor to it, a tap opens it, and
    /// drags and flicks scroll the list with the cursor following
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
                // the flash and the close both restart the raised highlight
                Gesture::Tap(_) => match self.touch.take() {
                    Some(Touch::Close) => {
                        self.sound(assets, "decide");
                        self.close();
                    }
                    Some(Touch::List(Some(index))) => {
                        self.set_cursor(index);
                        self.decide(assets);
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
        let input = MenuInput::new(state, pointer);

        match self.state {
            State::Entering(elapsed) => {
                let elapsed = elapsed + frames.round().max(1.0) as u32;
                if elapsed as f32 >= ENTRY_FRAMES {
                    self.play_bgm(context.asset_server);
                    self.state = State::Active;
                } else {
                    self.state = State::Entering(elapsed);
                }
            }
            State::Active => {
                self.handle_pointer(context.asset_server, pointer, frames);
                // `0x7100047ff0`: a flick coasts the list, the cursor following
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
                    self.handle_keys(context.asset_server, &input);
                }
            }
            State::Deciding => {
                if self.items[self.cursor].flash.is_idle() {
                    self.viewer = Some(Viewer {
                        item: self.cursor,
                        position: None,
                        picture: None,
                        size: (1920.0, 1080.0),
                        show_marks: true,
                        fit: false,
                        pan: (0.0, 0.0),
                        transition: None,
                        finished: false,
                        gestures: GestureRecognizer::new(false),
                        velocity: Vec2::ZERO,
                    });
                    // the viewer's ctor shows the first unlocked picture
                    self.next_picture(context);
                    self.state = State::Viewing;
                }
            }
            State::Viewing => {
                self.update_viewer(context, &input);
                if self.viewer.is_none() {
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
        if let Some(picture) = self
            .viewer
            .as_mut()
            .and_then(|viewer| viewer.picture.as_mut())
        {
            let update = AdvUpdateContext {
                frame_id: context.frame_id,
                delta_ticks: context.delta_ticks,
                asset_server: context.asset_server,
                device: context.pre_render.device,
                queue: context.pre_render.queue,
                are_animations_allowed: true,
            };
            picture.update(&update);
            picture.pre_render(context.pre_render, &TransformParams::default());
        }

        ui::advance(&mut self.scroll, frames);
        ui::advance(&mut self.close_flash, frames);
        for item in &mut self.items {
            ui::advance(&mut item.highlight, frames);
            ui::advance(&mut item.flash, frames);
        }
    }

    /// `CGMode::First`: BGM 6 at full volume, which also unlocks it in the Music Box
    fn play_bgm(&mut self, asset_server: &AssetServer) {
        let bgm_id = BgmId::from_number(6);
        let info = self.assets.scenario.info_tables().bgm_info(bgm_id);
        match asset_server.load_sync(info.path()) {
            Ok(track) => {
                self.bgm.play(
                    bgm_id,
                    track,
                    info.display_name.as_str(),
                    true,
                    Volume(1.0),
                    Tween::linear(Ticks::ZERO),
                );
                crate::adv::unlocks::bgm_started(
                    &mut self.global.save_vectors,
                    &self.assets.scenario,
                    bgm_id,
                );
            }
            Err(e) => warn!("Could not load the Picture Box's BGM: {e:?}"),
        }
    }

    /// The screen as it is now, without the page marks, as the old image of a 30-frame crossfade
    fn snapshot(&mut self, ctx: &mut PreRenderContext) {
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        viewer.show_marks = false;
        viewer.transition = None;
        let mut texture = ctx.new_render_texture("picture_box/transition".to_string());
        let depth_stencil = ctx.depth_stencil;
        let mut pass = ctx.begin_pass(
            texture.as_texture_target(),
            Some(depth_stencil),
            "picture_box/transition",
        );
        self.render(&mut pass);
        drop(pass);
        let viewer = self.viewer.as_mut().unwrap();
        viewer.show_marks = true;
        let mut fade = Tweener::new(1.0);
        ui::tween(&mut fade, 0.0, 30.0, Easing::Linear);
        viewer.transition = Some((texture, fade));
    }

    /// `Viewer::Start` (`0x710004aae0`): the entry's next unlocked picture, over a crossfade; false when there is none
    fn next_picture(&mut self, context: &mut UpdateContext) -> bool {
        let Some(viewer) = &self.viewer else {
            return false;
        };
        let item = &self.items[viewer.item];
        let start = viewer.position.map_or(0, |position| position + 1);
        let Some(position) = (start..item.pictures.len()).find(|&k| item.unlocked[k]) else {
            self.viewer.as_mut().unwrap().position = None;
            return false;
        };
        let info = self
            .assets
            .scenario
            .info_tables()
            .picture_info(PictureId::from_number(item.pictures[position] as i32));
        let name = info.name.as_str().to_string();
        let picture = match context
            .asset_server
            .load_sync::<Picture>(&format!("/picture/{}.pic", name.to_ascii_lowercase()))
        {
            Ok(picture) => Some(picture),
            Err(e) => {
                warn!("Could not load the picture {name}: {e:?}");
                None
            }
        };
        self.snapshot(context.pre_render);

        let viewer = self.viewer.as_mut().unwrap();
        viewer.position = Some(position);
        viewer.picture = picture.map(|picture| {
            let (w, h) = (
                picture.effective_width as f32,
                picture.effective_height as f32,
            );
            // the picture's centre is the origin of its scale and its place on screen
            let (cx, cy) = (
                w / 2.0 - picture.origin_x as f32,
                h / 2.0 - picture.origin_y as f32,
            );
            viewer.size = (w, h);
            let mut layer = PictureLayer::new(picture, Some(name));
            let props = layer.properties_mut();
            for (property, value) in [
                (LayerProperty::ScaleOriginX, cx),
                (LayerProperty::ScaleOriginY, cy),
                (LayerProperty::NegatedTranslationX, cx),
                (LayerProperty::NegatedTranslationY, cy),
            ] {
                props.property_tweener_mut(property).fast_forward_to(value);
            }
            layer
        });
        Self::layout(viewer, false);
        true
    }

    /// `Viewer::Exit` (`0x710004b7b0`): back to the gallery over a crossfade
    fn exit_viewer(&mut self, context: &mut UpdateContext) {
        self.snapshot(context.pre_render);
        let viewer = self.viewer.as_mut().unwrap();
        viewer.picture = None;
        viewer.finished = true;
    }

    /// `Viewer::layout` (`0x710004b200`): fit to the screen, or 1:1 panned within the picture
    fn layout(viewer: &mut Viewer, animate: bool) {
        let (w, h) = viewer.size;
        let (scale, x, y) = if viewer.fit {
            (1000.0 / (w / 1920.0).max(h / 1080.0), 0.0, 0.0)
        } else {
            let (x_limit, y_limit) = ((w / 2.0 - 960.0).max(0.0), (h / 2.0 - 540.0).max(0.0));
            viewer.pan.0 = viewer.pan.0.clamp(-x_limit, x_limit);
            viewer.pan.1 = viewer.pan.1.clamp(-y_limit, y_limit);
            (1000.0, viewer.pan.0, -viewer.pan.1)
        };
        let Some(picture) = &mut viewer.picture else {
            return;
        };
        let props = picture.properties_mut();
        for (property, value) in [
            (LayerProperty::ScaleX, scale),
            (LayerProperty::ScaleY, scale),
            (LayerProperty::TranslateX, x),
            (LayerProperty::TranslateY, y),
        ] {
            let tweener = props.property_tweener_mut(property);
            if animate {
                ui::tween(tweener, value, 10.0, Easing::Linear);
            } else {
                tweener.fast_forward_to(value);
            }
        }
    }

    fn update_viewer(&mut self, context: &mut UpdateContext, input: &MenuInput) {
        let frames = context.delta_ticks.as_f32();
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        if let Some((_, fade)) = &mut viewer.transition {
            ui::advance(fade, frames);
            if fade.is_idle() {
                viewer.transition = None;
            }
        }
        if viewer.finished {
            if viewer.transition.is_none() {
                self.viewer = None;
            }
            return;
        }

        // `0x710004b0f0`: a flick's speed pans each frame, without dt, slowing by 0.95^dt, until the picture is fit,
        // gone or scaling
        if viewer.velocity != Vec2::ZERO {
            if viewer.picture.is_some() && !viewer.is_scaling() && !viewer.fit {
                viewer.pan.0 += viewer.velocity.x;
                viewer.pan.1 -= viewer.velocity.y;
                viewer.velocity *= 0.95f32.powf(frames);
                Self::layout(viewer, false);
            } else {
                viewer.velocity = Vec2::ZERO;
            }
        }

        // `Viewer::Main` (`0x710004a940`): only once the picture's transition is over
        if viewer.transition.is_none() {
            if self.handle_viewer_pointer(context, input.pointer, frames) {
                return;
            }
            // a touch holds the buttons off (`0x710006b860`)
            if self.viewer.as_ref().unwrap().gestures.is_active() {
                return;
            }
            if input.cancel {
                self.sound(context.asset_server, "cancel");
                self.exit_viewer(context);
                return;
            }
            if input.confirm {
                self.sound(context.asset_server, "decide");
                if !self.next_picture(context) {
                    self.exit_viewer(context);
                }
                return;
            }
            let viewer = self.viewer.as_mut().unwrap();
            if input.x && viewer.picture.is_some() {
                viewer.fit = !viewer.fit;
                Self::layout(viewer, true);
            } else {
                if input.y {
                    viewer.show_marks = !viewer.show_marks;
                }
                if viewer.picture.is_some() && !viewer.is_scaling() && !viewer.fit {
                    let step = 16.0 * frames;
                    let mut d = (0.0, 0.0);
                    if input.held_up {
                        d.1 = -step;
                    } else if input.held_down {
                        d.1 = step;
                    }
                    if input.held_left {
                        d.0 = step;
                    } else if input.held_right {
                        d.0 = -step;
                    }
                    if d != (0.0, 0.0) {
                        viewer.pan.0 += d.0;
                        viewer.pan.1 += d.1;
                        Self::layout(viewer, false);
                    }
                }
            }
        }
    }

    /// The viewer's gesture listener (down `0x710004baf0`, tap `0x710004bb50`, drag `0x710004bc10`, flick
    /// `0x710004bcb0`): a touch stops the coasting, a tap shows the next picture or leaves, and drags and flicks pan
    /// a picture shown 1:1, vertically by -0.5625 as the original; true once the viewer moved on
    fn handle_viewer_pointer(
        &mut self,
        context: &mut UpdateContext,
        pointer: Pointer,
        frames: f32,
    ) -> bool {
        let gestures = self
            .viewer
            .as_mut()
            .unwrap()
            .gestures
            .update(pointer, frames);
        for gesture in gestures {
            let viewer = self.viewer.as_mut().unwrap();
            let pannable = viewer.picture.is_some() && !viewer.fit;
            match gesture {
                Gesture::Down(_) => viewer.velocity = Vec2::ZERO,
                Gesture::Tap(_) => {
                    self.sound(context.asset_server, "decide");
                    if !self.next_picture(context) {
                        self.exit_viewer(context);
                    }
                    return true;
                }
                Gesture::Drag(from, to) if pannable => {
                    viewer.pan.0 += to.x - from.x;
                    viewer.pan.1 += (to.y - from.y) * -0.5625;
                    Self::layout(viewer, false);
                }
                Gesture::Flick(velocity) if pannable => {
                    viewer.velocity = Vec2::new(velocity.x, velocity.y * 0.5625);
                }
                _ => {}
            }
        }
        false
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
            let (column, row) = ((i % COLUMNS) as f32, (i / COLUMNS) as f32);
            let y = 203.0 + ROW * row - scroll;
            if !(-ROW..1080.0).contains(&y) {
                continue;
            }
            let s = 1.0 + 0.05 * item.highlight.value();
            let transform = projection
                * Mat4::from_translation(vec3(150.0 + CELL_W * column + 192.0, y + 108.0, 0.0))
                * Mat4::from_scale(vec3(s, s, 1.0))
                * Mat4::from_translation(vec3(-192.0, -108.0, 0.0));
            let rect = [0.0, 0.0, 384.0, 216.0];
            Sprite::new(t(&item.name), rect).draw(pass, opaque, transform);
            let (unlocked, count) = (item.unlocked_count(), item.pictures.len());
            if unlocked == 0 {
                Sprite::new(t("logo"), rect).draw(pass, builder, transform);
            } else if unlocked < count {
                self.draw_number(pass, transform, &format!("{unlocked}/{count}"));
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
        ui::clip(pass, None, self.target_size);

        let bg = t("bg");
        let clear = FloatColor4::WHITE.with_alpha(0.0);
        ui::gradient_sprite(
            pass,
            bg,
            projection,
            [136.0, 185.0, 1784.0, 190.0],
            [0.070833, 0.171296, 0.929167, 0.175926],
            FloatColor4::WHITE,
            clear,
        );
        ui::gradient_sprite(
            pass,
            bg,
            projection,
            [136.0, 916.0, 1784.0, 921.0],
            [0.070833, 0.848148, 0.929167, 0.852778],
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

        // the key help, right to left (`0x7100048e20`, `0x710016e760`)
        let grey = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);
        let white = FloatColor4::WHITE;
        let view = if self.items[self.cursor].unlocked_count() > 0 {
            white
        } else {
            grey
        };
        let mut x = self.draw_help(pass, 1872.0, &[4], 48.0, 72.0, white);
        x = self.draw_help(pass, x, &[3], 96.0, 74.0, view);
        self.draw_help(pass, x, &[7, 0], 0.0, 206.0, white);

        ui::draw_text(
            pass,
            &self.completion,
            projection * Mat4::from_translation(vec3(48.0, 1012.0, 0.0)),
            FloatColor4::WHITE,
            BORDER,
        );

        if let Some(viewer) = &self.viewer {
            self.render_viewer(pass, viewer);
        }
    }

    /// `Viewer` render (`0x710004b4b0`): the picture over black, the transition, the page marks
    fn render_viewer(&self, pass: &mut RenderPass, viewer: &Viewer) {
        let projection = ui::projection();
        let full = [0.0, 0.0, 1920.0, 1080.0];
        if let Some(picture) = &viewer.picture {
            ui::fill_with(
                pass,
                projection,
                full,
                FloatColor4::BLACK,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
            );
            render_layer_without_bg(pass, &TransformParams::default(), picture, 0);
        }
        if let Some((texture, fade)) = &viewer.transition {
            ui::draw_texture(
                pass,
                texture.as_texture_source(),
                projection,
                full,
                [0.0, 0.0, 1.0, 1.0],
                FloatColor4::WHITE.with_alpha(fade.value()),
                ui::builder(),
            );
        }
        let item = &self.items[viewer.item];
        let n = item.pictures.len();
        if viewer.show_marks && n > 1 {
            for (i, &unlocked) in item.unlocked.iter().enumerate() {
                let x = 1905.0 - 30.0 * n as f32 + 30.0 * i as f32;
                let mut mark =
                    Sprite::new(self.textures.get("check"), [x, 1025.0, x + 32.0, 1065.0]);
                mark.color = if viewer.position == Some(i) {
                    FloatColor4::from_rgba(0.5647, 1.0, 0.502, 1.0)
                } else if unlocked {
                    FloatColor4::WHITE
                } else {
                    FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0)
                };
                mark.draw(pass, ui::builder(), projection);
            }
        }
    }

    /// `0x7100048bd0`: "u/n" in the `num` glyphs, right-aligned at x 376 of the thumbnail
    fn draw_number(&self, pass: &mut RenderPass, transform: Mat4, text: &str) {
        let glyph = |c: char| match c {
            '0'..='9' => Some(((c as u32 - '0' as u32) as f32 * 24.0, 24.0)),
            '/' => Some((240.0, 24.0)),
            _ => None,
        };
        let advance: f32 = text
            .chars()
            .map(|c| if glyph(c).is_some() { 19.0 } else { 16.0 })
            .sum();
        let mut pen = 376.0 - advance;
        for c in text.chars() {
            let Some((u, w)) = glyph(c) else {
                pen += 16.0;
                continue;
            };
            let x = pen - 2.5;
            Sprite::new(self.textures.get("num"), [x, 180.0, x + w, 212.0])
                .uv([u / 512.0, 0.0, (u + w) / 512.0, 32.0 / 80.0])
                .draw(pass, ui::builder(), transform);
            pen += w - 5.0;
        }
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
            self.textures.get("help_cgmode"),
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
