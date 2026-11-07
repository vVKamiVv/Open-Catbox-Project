//! The Character Box (`shin::BustupMode`, ctor `0x7100037bd0`, update `0x7100038740`, render `0x71000388a0`, keys
//! `0x7100039de0`): every character's bustups over the game's backgrounds, chosen in a five-row panel (character,
//! variant, pose, face, background). The scene is a screen of three layers, the secondary background (render position
//! 1001) behind the primary one (1000, both at depth 2000 and 200 %) and the bustup (depth 1000), shifted 200 px left
//! of the panel. A hides the panel (`BustupMode::Viewer`): the scene slides back to the centre and the d-pad pans the
//! camera, the backgrounds moving half as far as the bustup.
//!
//! A change of selection crossfades the scene over 10 frames, as the original's 10-frame wiper on the changed layer:
//! only that layer differs between the two pictures, so the two are the same.
//!
//! shortcut: bustups and backgrounds load synchronously (the original loads them in the background), and the viewer
//! cannot pan with the left stick.

use std::sync::Arc;

use enum_map::EnumMap;
use glam::{Mat4, Vec2, vec3};
use shin_audio::AudioManager;
use shin_core::{
    format::{
        save::SaveData,
        scenario::{
            info::{BgmId, BustupId, CharacterBoxSegment, PictureId},
            instruction_elements::FromNumber,
        },
    },
    primitives::color::{FloatColor4, UnormColor},
    time::{Easing, Ticks, Tween, Tweener},
    vm::command::types::{LayerProperty, Volume},
};
use shin_input::{ActionState, Pointer};
use shin_render::{
    gpu_texture::GpuTexture, render_pass::RenderPass, render_texture::RenderTexture,
};
use tracing::warn;

use crate::{
    adv::menu::{MenuAssets, manual::KeyHelp},
    app::AppAction,
    asset::{
        bustup::{Bustup, BustupArgs, CharacterId},
        picture::Picture,
        system::AssetServer,
        texture_archive::NamedTextures,
    },
    audio::{BgmPlayer, play_sys_se},
    export::{self, Exporter},
    layer::{
        DrawableLayer, Layer, render_layers_with_bg,
        render_params::TransformParams,
        user::{
            BustupLayer, PictureLayer,
            minigame::{self, Sprite},
        },
    },
    render::PreRenderContext,
    ui::{
        self, MenuInput,
        gesture::{Gesture, GestureRecognizer},
    },
    update::{AdvUpdatable as _, AdvUpdateContext, UpdateContext},
};

/// The entry transition (`BustupMode::First`), after which BGM 7 starts
const ENTRY_FRAMES: f32 = 60.0;
/// The scene's shift left of the panel (ScreenLayer TranslateX)
const SCENE_X: f32 = -200.0;
const COUNTER_COLOR: FloatColor4 = FloatColor4::from_rgba(0.8, 0.7333, 0.6667, 1.0);

/// The panel's rows: frame (x, y, kind: 0 = 144 px, 1 = 112 px), label row in `text`, arrows' x
const ROWS: [(f32, f32, usize, f32, [f32; 2]); 5] = [
    (1504.0, 248.0, 0, 0.0, [1424.0, 1648.0]),
    (1520.0, 392.0, 1, 2.0, [1440.0, 1632.0]),
    (1520.0, 504.0, 1, 3.0, [1440.0, 1632.0]),
    (1520.0, 616.0, 1, 4.0, [1440.0, 1632.0]),
    (1504.0, 728.0, 0, 5.0, [1424.0, 1648.0]),
];
/// The arrows' y by row
const ARROW_Y: [f32; 5] = [280.0, 408.0, 520.0, 632.0, 760.0];

type Characters = Vec<Vec<Vec<Vec<u16>>>>;

/// The CharacterBoxInfo as the game groups it (`0x71000e8f14`): character -> variant -> pose -> face (bustup id), and
/// the backgrounds (primary picture, offset of the secondary one)
struct Table {
    characters: Characters,
    backgrounds: Vec<(u16, u16)>,
}

/// An end closes the face list into a pose; level 1 (0x12) also the poses into a variant; level 2 (0x22) also the
/// variants into a character; empty groups are dropped
fn close_group(
    level: u8,
    faces: &mut Vec<u16>,
    poses: &mut Vec<Vec<u16>>,
    variants: &mut Vec<Vec<Vec<u16>>>,
    characters: &mut Characters,
) {
    if !faces.is_empty() {
        poses.push(std::mem::take(faces));
    }
    if level >= 1 && !poses.is_empty() {
        variants.push(std::mem::take(poses));
    }
    if level >= 2 && !variants.is_empty() {
        characters.push(std::mem::take(variants));
    }
}

fn group(segments: &[CharacterBoxSegment]) -> Table {
    let mut characters = Vec::new();
    let mut backgrounds = Vec::new();
    let (mut variants, mut poses, mut faces) = (Vec::new(), Vec::new(), Vec::new());
    for segment in segments {
        let level = match segment {
            CharacterBoxSegment::Background {
                primary_picture_id,
                secondary_picture_id_offset,
            } => {
                backgrounds.push((
                    primary_picture_id.into_i32() as u16,
                    *secondary_picture_id_offset,
                ));
                continue;
            }
            CharacterBoxSegment::Bustup { bustup_id } => {
                faces.push(bustup_id.into_i32() as u16);
                continue;
            }
            CharacterBoxSegment::EndExpressionGroup => 0,
            CharacterBoxSegment::EndPoseGroup => 1,
            CharacterBoxSegment::EndDefinition => 2,
        };
        close_group(
            level,
            &mut faces,
            &mut poses,
            &mut variants,
            &mut characters,
        );
    }
    close_group(2, &mut faces, &mut poses, &mut variants, &mut characters);
    Table {
        characters,
        backgrounds,
    }
}

/// What each character, variant and pose last showed (`+0x260`)
struct Selection {
    variant: usize,
    /// Per variant: its pose, and per pose its face
    poses: Vec<(usize, Vec<usize>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// `BustupMode::First`: the crossfade from the title, then BGM 7
    Entering(u32),
    Active,
    /// The hide button flashes, then the viewer opens
    Hiding,
    /// The scene slides to the centre
    ViewerOpening,
    Viewer,
    /// Everything slides back
    ViewerClosing,
    /// The close button flashes
    Closing,
    Done,
}

/// What a touch went down on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touch {
    Close,
    Hide,
    /// The panel's band: a row and its part (0 the left arrow, 1 the right one, 2 the frame), or nothing
    Panel(Option<(usize, usize)>),
}

pub struct CharacterBox {
    assets: MenuAssets,
    audio: Arc<AudioManager>,
    asset_server: Arc<AssetServer>,
    textures: Arc<NamedTextures>,
    pub bgm: BgmPlayer,
    pub global: SaveData,
    table: Table,
    selection: Vec<Selection>,
    character: usize,
    background: usize,
    row: usize,
    /// The secondary and primary backgrounds
    backgrounds: [Option<PictureLayer>; 2],
    bustup: Option<BustupLayer>,
    scene_x: Tweener,
    camera: [Tweener; 2],
    /// The UI's visibility (`+0xf0`) and the panel's offset (`+0x1f8`)
    ui: Tweener,
    panel: Tweener,
    close_flash: Tweener,
    hide_flash: Tweener,
    /// k = 2 * row + side
    arrows: [Tweener; 10],
    /// The old scene fading out over the new one after a change
    transition: Option<(RenderTexture, Tweener)>,
    state: State,
    /// The panel's and the viewer's touch input (`GestureRecognizer`, listeners `0x710003bd60` and `0x710003d150`
    /// with their neighbours), and the camera's speed after a flick (viewer `+0xc4`)
    gestures: GestureRecognizer,
    touch: Option<Touch>,
    velocity: Vec2,
    /// Port-only: the sprite's and the background's Save
    export: Exporter,
}

impl CharacterBox {
    pub fn new(
        asset_server: &Arc<AssetServer>,
        audio: Arc<AudioManager>,
        assets: MenuAssets,
        global: SaveData,
        bgm: BgmPlayer,
    ) -> anyhow::Result<Self> {
        let textures = asset_server.load_sync::<NamedTextures>("/bupmode.txa")?;
        let table = group(&assets.scenario.info_tables().character_box_info);
        anyhow::ensure!(
            !table.characters.is_empty() && !table.backgrounds.is_empty(),
            "the scenario has no Character Box"
        );
        let selection = table
            .characters
            .iter()
            .map(|variants| Selection {
                variant: 0,
                poses: variants
                    .iter()
                    .map(|poses| (0, vec![0; poses.len()]))
                    .collect(),
            })
            .collect();
        let export = Exporter::new(assets.system_font.clone());
        let mut character_box = Self {
            assets,
            audio,
            asset_server: asset_server.clone(),
            textures,
            bgm,
            global,
            table,
            selection,
            character: 0,
            background: 0,
            row: 0,
            backgrounds: [None, None],
            bustup: None,
            scene_x: Tweener::new(SCENE_X),
            camera: [Tweener::new(0.0), Tweener::new(0.0)],
            ui: Tweener::new(1.0),
            panel: Tweener::new(0.0),
            close_flash: Tweener::new(0.0),
            hide_flash: Tweener::new(0.0),
            arrows: std::array::from_fn(|_| Tweener::new(0.0)),
            transition: None,
            state: State::Entering(0),
            gestures: GestureRecognizer::new(false),
            touch: None,
            velocity: Vec2::ZERO,
            export,
        };
        character_box.load_backgrounds();
        character_box.load_bustup();
        Ok(character_box)
    }

    /// Done: the title should open with its cursor on the Character Box
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    pub fn shutdown(&mut self, frames: f32) {
        self.bgm
            .stop(Tween::linear(Ticks::from_f32(frames.max(4.0))));
    }

    fn sound(&self, name: &str) {
        play_sys_se(&self.audio, &self.asset_server, name, Volume::default());
    }

    fn current_selection(&self) -> (usize, usize, usize) {
        let selection = &self.selection[self.character];
        let (pose, faces) = &selection.poses[selection.variant];
        (selection.variant, *pose, faces[*pose])
    }

    /// The counts of the rows, for the arrows and the counters
    fn counts(&self) -> [usize; 5] {
        let (variant, pose, _) = self.current_selection();
        let variants = &self.table.characters[self.character];
        [
            self.table.characters.len(),
            variants.len(),
            variants[variant].len(),
            variants[variant][pose].len(),
            self.table.backgrounds.len(),
        ]
    }

    fn indices(&self) -> [usize; 5] {
        let (variant, pose, face) = self.current_selection();
        [self.character, variant, pose, face, self.background]
    }

    /// Port-only: the sprite as shown at rest (its base, face, first mouth and first eyes) and the primary background,
    /// as PNGs at full size
    fn save_scene(&mut self) {
        let (variant, pose, face) = self.current_selection();
        let id = self.table.characters[self.character][variant][pose][face];
        let tables = self.assets.scenario.info_tables();
        let bustup = tables.bustup_info(BustupId::from_number(id as i32));
        let background = tables.picture_info(PictureId::from_number(
            self.table.backgrounds[self.background].0 as i32,
        ));
        let (bustup_data, background_data) = (
            self.asset_server.open(bustup.path()),
            self.asset_server.open(background.path()),
        );
        let emotion = bustup.emotion.to_string();
        let root = export::folder(dirs_next::picture_dir());
        let sprite_name = [bustup.name.as_str(), &emotion]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_");
        let sprite_file = root
            .join("Characters")
            .join(format!("{}.png", export::file_name(&sprite_name)));
        let background_file = root.join("Backgrounds").join(format!(
            "{}.png",
            export::file_name(background.name.as_str())
        ));
        self.export.start(move || {
            export::save_png(
                &export::compose_bustup(&bustup_data?, &emotion)?,
                &sprite_file,
            )?;
            export::save_png(
                &export::decode_picture(&background_data?)?,
                &background_file,
            )?;
            Ok(root)
        });
    }

    /// `0x710003acd0`: the selected face's bustup, at depth 1000
    fn load_bustup(&mut self) {
        let (variant, pose, face) = self.current_selection();
        let id = self.table.characters[self.character][variant][pose][face];
        let info = self
            .assets
            .scenario
            .info_tables()
            .bustup_info(BustupId::from_number(id as i32));
        let args = BustupArgs {
            expression: info.emotion.to_string(),
            character_id: CharacterId::new(info.lipsync_character_id as i32),
            disable_animations: false,
        };
        let load = self
            .asset_server
            .load_with_args::<Bustup, _>(info.path(), args);
        match shin_tasks::block_on(load) {
            Ok(bustup) => {
                self.bustup = Some(BustupLayer::new(
                    bustup,
                    Some(info.name.to_string()),
                    info.lipsync_character_id,
                ));
            }
            Err(e) => warn!("Could not load the bustup {}: {e:?}", info.name),
        }
    }

    /// `0x710003b020`: the primary picture and, when its offset is not 0, the secondary one behind it, both at depth
    /// 2000 and 200 %
    fn load_backgrounds(&mut self) {
        let (primary, offset) = self.table.backgrounds[self.background];
        let ids = [(offset != 0).then(|| primary + offset), Some(primary)];
        for (slot, id) in self.backgrounds.iter_mut().zip(ids) {
            *slot = id.and_then(|id| {
                let info = self
                    .assets
                    .scenario
                    .info_tables()
                    .picture_info(PictureId::from_number(id as i32));
                let name = info.name.as_str().to_string();
                let path = format!("/picture/{}.pic", name.to_ascii_lowercase());
                match self.asset_server.load_sync::<Picture>(&path) {
                    Ok(picture) => {
                        let mut layer = PictureLayer::new(picture, Some(name));
                        let props = layer.properties_mut();
                        for (property, value) in [
                            (LayerProperty::TranslateZ, 2000.0),
                            (LayerProperty::ScaleX, 2000.0),
                            (LayerProperty::ScaleY, 2000.0),
                        ] {
                            props.property_tweener_mut(property).fast_forward_to(value);
                        }
                        Some(layer)
                    }
                    Err(e) => {
                        warn!("Could not load the background {name}: {e:?}");
                        None
                    }
                }
            });
        }
    }

    /// The scene as it is now, as the old picture of a 10-frame crossfade
    fn snapshot(&mut self, ctx: &mut PreRenderContext) {
        let mut texture = ctx.new_render_texture("character_box/transition".to_string());
        let depth_stencil = ctx.depth_stencil;
        let mut pass = ctx.begin_pass(
            texture.as_texture_target(),
            Some(depth_stencil),
            "character_box/transition",
        );
        self.render_scene(&mut pass);
        drop(pass);
        let mut fade = Tweener::new(1.0);
        ui::tween(&mut fade, 0.0, 10.0, Easing::Linear);
        self.transition = Some((texture, fade));
    }

    /// `0x710003a270`: the selected row's value moved by `delta`, clamped; false when it did not change
    fn change(&mut self, ctx: &mut PreRenderContext, delta: i32) -> bool {
        let row = self.row;
        let (old, count) = (self.indices()[row], self.counts()[row]);
        let new = (old as i32 + delta).clamp(0, count as i32 - 1) as usize;
        if new == old {
            return false;
        }
        self.sound("move");
        self.snapshot(ctx);
        let selection = &mut self.selection[self.character];
        match row {
            0 => self.character = new,
            1 => selection.variant = new,
            2 => selection.poses[selection.variant].0 = new,
            3 => {
                let (pose, faces) = &mut selection.poses[selection.variant];
                faces[*pose] = new;
            }
            _ => self.background = new,
        }
        if row == 4 {
            self.load_backgrounds();
        } else {
            self.load_bustup();
        }
        true
    }

    fn flash_arrow(&mut self, side: usize) {
        let arrow = &mut self.arrows[2 * self.row + side];
        *arrow = Tweener::new(0.0);
        minigame::flash(arrow);
    }

    /// `0x7100039de0`
    fn handle_keys(&mut self, ctx: &mut PreRenderContext, input: &MenuInput) {
        if input.cancel {
            self.sound("cancel");
            self.close();
        } else if input.confirm {
            self.sound("decide");
            self.hide();
        } else if input.up {
            if self.row >= 1 {
                self.row -= 1;
                self.sound("move");
            }
        } else if input.down {
            if self.row <= 3 {
                self.row += 1;
                self.sound("move");
            }
        } else {
            let (delta, side) = if input.left {
                (-1, 0)
            } else if input.right {
                (1, 1)
            } else if input.l {
                (-10, 0)
            } else if input.r {
                (10, 1)
            } else {
                return;
            };
            if self.change(ctx, delta) {
                self.flash_arrow(side);
            }
        }
    }

    /// `0x7100039f90`: the close button flashes, then the title
    fn close(&mut self) {
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.state = State::Closing;
    }

    /// `0x710003a0f0`: the panel slides 500 px right while the hide button flashes
    fn hide(&mut self) {
        self.hide_flash = Tweener::new(0.0);
        minigame::flash(&mut self.hide_flash);
        ui::tween(&mut self.panel, 500.0, 30.0, Easing::SineOut);
        self.state = State::Hiding;
    }

    /// `0x710003b3b0`: the row and part under a point of the panel, the arrows only when the value can move that way;
    /// the x are lost in the decompile, so they are the port's render's
    fn panel_at(&self, at: Vec2) -> Option<(usize, usize)> {
        let (indices, counts) = (self.indices(), self.counts());
        ROWS.iter()
            .enumerate()
            .find_map(|(row, &(x, y, kind, _, arrows))| {
                let s = if kind == 0 { 144.0 } else { 112.0 };
                if minigame::contains([x, y, x + s, y + s], at) {
                    return Some((row, 2));
                }
                let enabled = [indices[row] > 0, indices[row] + 2 <= counts[row]];
                let y = ARROW_Y[row];
                (0..2)
                    .find(|&side| {
                        let x = arrows[side];
                        enabled[side] && minigame::contains([x, y, x + 80.0, y + 80.0], at)
                    })
                    .map(|side| (row, side))
            })
    }

    /// The panel's gesture listener (down `0x710003bd60`, press `0x710003bfa0`, tap `0x710003c050`, drag
    /// `0x710003c1a0`): a press moves the row cursor and raises an arrow, a tap steps the value with an arrow (and
    /// flashes it) or presses the hide or close button; a drag only drops the highlights, and nothing is flicked
    fn handle_pointer(&mut self, ctx: &mut PreRenderContext, pointer: Pointer, frames: f32) {
        for gesture in self.gestures.update(pointer, frames) {
            match gesture {
                // a touch elsewhere is tracked but does nothing
                Gesture::Down(at) => {
                    self.touch = if minigame::contains([1764.0, 32.0, 1848.0, 116.0], at) {
                        Some(Touch::Close)
                    } else if minigame::contains([1668.0, 32.0, 1752.0, 116.0], at) {
                        Some(Touch::Hide)
                    } else if (162.0..980.0).contains(&at.y) {
                        Some(Touch::Panel(self.panel_at(at)))
                    } else {
                        None
                    };
                }
                Gesture::Press => match self.touch {
                    Some(Touch::Close) => minigame::raise(&mut self.close_flash),
                    Some(Touch::Hide) => minigame::raise(&mut self.hide_flash),
                    Some(Touch::Panel(Some((row, part)))) => {
                        self.row = row;
                        if part < 2 {
                            minigame::raise(&mut self.arrows[2 * row + part]);
                        }
                    }
                    _ => {}
                },
                // the flashes restart the raised highlights
                Gesture::Tap(_) => match self.touch.take() {
                    Some(Touch::Close) => {
                        self.sound("decide");
                        self.close();
                    }
                    Some(Touch::Hide) => {
                        self.sound("decide");
                        self.hide();
                    }
                    Some(Touch::Panel(Some((row, part)))) => {
                        self.row = row;
                        if part < 2 {
                            minigame::lower(&mut self.arrows[2 * row + part]);
                            if self.change(ctx, if part == 0 { -1 } else { 1 }) {
                                self.flash_arrow(part);
                            }
                        }
                    }
                    _ => {}
                },
                Gesture::Drag(..) | Gesture::End => match self.touch.take() {
                    Some(Touch::Close | Touch::Hide) => {
                        minigame::lower(&mut self.close_flash);
                        minigame::lower(&mut self.hide_flash);
                    }
                    Some(Touch::Panel(Some((row, part)))) if part < 2 => {
                        minigame::lower(&mut self.arrows[2 * row + part]);
                    }
                    _ => {}
                },
                Gesture::LongPress | Gesture::Flick(_) => {}
            }
        }
    }

    /// `0x710003ceb0`: everything slides back over 30 frames
    fn close_viewer(&mut self) {
        ui::tween(&mut self.scene_x, SCENE_X, 30.0, Easing::SineOut);
        for camera in &mut self.camera {
            ui::tween(camera, 0.0, 30.0, Easing::SineOut);
        }
        ui::tween(&mut self.ui, 1.0, 30.0, Easing::Linear);
        ui::tween(&mut self.panel, 0.0, 30.0, Easing::SineOut);
        self.velocity = Vec2::ZERO;
        self.state = State::ViewerClosing;
    }

    /// The camera moved against `d`, clamped to ±420 x ±130
    fn move_camera(&mut self, d: Vec2) {
        let [x, y] = &mut self.camera;
        *x = Tweener::new((x.value() - d.x).clamp(-420.0, 420.0));
        *y = Tweener::new((y.value() - d.y).clamp(-130.0, 130.0));
    }

    /// The viewer's gesture listener (down `0x710003d150`, tap `0x710003d1b0`, drag `0x710003d250`, flick
    /// `0x710003d330`): a touch stops the coasting, a tap goes back to the panel, and drags and flicks move the camera
    /// against the pointer, vertically by 0.5625 as the original
    fn handle_viewer_pointer(&mut self, pointer: Pointer, frames: f32) {
        let vertical = Vec2::new(1.0, 0.5625);
        for gesture in self.gestures.update(pointer, frames) {
            match gesture {
                Gesture::Down(_) => self.velocity = Vec2::ZERO,
                Gesture::Tap(_) => {
                    self.sound("decide");
                    self.close_viewer();
                }
                Gesture::Drag(from, to) => self.move_camera((to - from) * vertical),
                Gesture::Flick(velocity) => self.velocity = velocity * vertical,
                _ => {}
            }
        }
    }

    /// `0x710003cd50`: pan with the d-pad (10 px a frame), clamped to ±420 x ±130
    fn handle_viewer_keys(&mut self, input: &MenuInput, frames: f32) {
        if input.confirm || input.cancel {
            self.sound("cancel");
            self.close_viewer();
            return;
        }
        let step = 10.0 * frames;
        let dx = if input.held_left {
            -step
        } else if input.held_right {
            step
        } else {
            0.0
        };
        let dy = if input.held_up {
            -step
        } else if input.held_down {
            step
        } else {
            0.0
        };
        if dx != 0.0 || dy != 0.0 {
            let [x, y] = &mut self.camera;
            *x = Tweener::new((x.value() + dx).clamp(-420.0, 420.0));
            *y = Tweener::new((y.value() + dy).clamp(-130.0, 130.0));
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
        let (pointer, clicked) = self.export.pointer(pointer);
        let save = clicked || Exporter::pressed(state);
        let input = MenuInput::new(state, pointer);

        match self.state {
            State::Entering(elapsed) => {
                let elapsed = elapsed + frames.round().max(1.0) as u32;
                if elapsed as f32 >= ENTRY_FRAMES {
                    self.play_bgm();
                    self.state = State::Active;
                } else {
                    self.state = State::Entering(elapsed);
                }
            }
            State::Active => {
                self.handle_pointer(context.pre_render, pointer, frames);
                // a touch holds the buttons off (`0x710006b860`)
                if !self.gestures.is_active() && self.state == State::Active {
                    if save {
                        self.save_scene();
                    } else {
                        self.handle_keys(context.pre_render, &input);
                    }
                }
            }
            State::Hiding => {
                if self.hide_flash.is_idle() {
                    // `0x710003c840`
                    ui::tween(&mut self.scene_x, 0.0, 30.0, Easing::SineOut);
                    ui::tween(&mut self.ui, 0.0, 10.0, Easing::Linear);
                    self.state = State::ViewerOpening;
                }
            }
            State::ViewerOpening => {
                if self.scene_x.is_idle() {
                    self.state = State::Viewer;
                }
            }
            State::Viewer => {
                self.handle_viewer_pointer(pointer, frames);
                // `0x710003cac0`: a flick's speed moves the camera each frame (the original's 60, without dt), slowing by 0.95
                if self.velocity != Vec2::ZERO {
                    // the 60 Hz frames' moves summed, as the speed decays
                    self.move_camera(self.velocity * ui::decaying_sum(0.95, frames));
                    self.velocity *= 0.95f32.powf(frames);
                }
                if !self.gestures.is_active() && self.state == State::Viewer {
                    if save {
                        self.save_scene();
                    } else {
                        self.handle_viewer_keys(&input, frames);
                    }
                }
            }
            State::ViewerClosing => {
                if self.scene_x.is_idle() && self.panel.is_idle() {
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

        for tweener in [
            &mut self.scene_x,
            &mut self.ui,
            &mut self.panel,
            &mut self.close_flash,
            &mut self.hide_flash,
        ]
        .into_iter()
        .chain(&mut self.camera)
        .chain(&mut self.arrows)
        {
            ui::advance(tweener, frames);
        }
        if let Some((_, fade)) = &mut self.transition {
            ui::advance(fade, frames);
            if fade.is_idle() {
                self.transition = None;
            }
        }

        // the plane's camera moves every layer, the far ones less (`0x710003cac0`)
        let camera = [self.camera[0].value(), self.camera[1].value()];
        let set_camera = |layer: &mut dyn DrawableLayer| {
            let props = layer.properties_mut();
            props
                .property_tweener_mut(LayerProperty::CameraPositionX)
                .fast_forward_to(camera[0]);
            props
                .property_tweener_mut(LayerProperty::CameraPositionY)
                .fast_forward_to(camera[1]);
        };
        let update = AdvUpdateContext {
            frame_id: context.frame_id,
            delta_ticks: context.delta_ticks,
            asset_server: context.asset_server,
            device: context.pre_render.device,
            queue: context.pre_render.queue,
            are_animations_allowed: true,
        };
        let parent = self.scene_transform();
        for layer in self.backgrounds.iter_mut().flatten() {
            set_camera(layer);
            layer.update(&update);
            layer.pre_render(context.pre_render, &parent);
        }
        if let Some(bustup) = &mut self.bustup {
            set_camera(bustup);
            bustup.update(&update);
            bustup.pre_render(context.pre_render, &parent);
        }
        self.export
            .update(context.pre_render, &self.audio, &self.asset_server, frames);
    }

    /// `BustupMode::First`: BGM 7 at full volume, which also unlocks it in the Music Box
    fn play_bgm(&mut self) {
        let bgm_id = BgmId::from_number(7);
        let info = self.assets.scenario.info_tables().bgm_info(bgm_id);
        match self.asset_server.load_sync(info.path()) {
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
            Err(e) => warn!("Could not load the Character Box's BGM: {e:?}"),
        }
    }

    /// The screen layer's own transform: its TranslateX
    fn scene_transform(&self) -> TransformParams {
        TransformParams {
            transform: Mat4::from_translation(vec3(self.scene_x.value(), 0.0, 0.0)),
            ..TransformParams::default()
        }
    }

    fn render_scene(&self, pass: &mut RenderPass) {
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        // back to front, each with its own stencil value, as the layer group draws them
        let parent = self.scene_transform();
        let layers = self
            .backgrounds
            .iter()
            .flatten()
            .map(|layer| layer as &dyn Layer)
            .chain(self.bustup.iter().map(|layer| layer as &dyn Layer))
            .collect::<Vec<_>>();
        render_layers_with_bg(pass, &parent, &layers, |_, _, _| {}, true, 0);
        if let Some((texture, fade)) = &self.transition {
            ui::draw_texture(
                pass,
                texture.as_texture_source(),
                ui::projection(),
                [0.0, 0.0, 1920.0, 1080.0],
                [0.0, 0.0, 1.0, 1.0],
                FloatColor4::WHITE.with_alpha(fade.value()),
                ui::builder(),
            );
        }
    }

    pub fn render(&self, pass: &mut RenderPass) {
        self.render_scene(pass);
        self.export.hide_entry();
        self.export.draw_notice(pass);
        let projection = ui::projection();
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let p = self.panel.value();
        let (indices, counts) = (self.indices(), self.counts());

        for (row, &(x, y, kind, label, arrows)) in ROWS.iter().enumerate() {
            let x = x + p;
            let s = if kind == 0 { 144.0 } else { 112.0 };
            let selected = row == self.row;
            // `0x7100039290`: the frame, and its white silhouette over everything when the row is selected
            let frame = |pass: &mut RenderPass, highlight: bool| {
                let u = kind as f32 * 0.5;
                let v = if highlight { 0.5 } else { 0.0 };
                Sprite::new(t("icon"), [x, y, x + s, y + s])
                    .uv([u, v, u + s / 288.0, v + s / 288.0])
                    .alpha(if highlight { 0.3 } else { 1.0 })
                    .draw(pass, builder, projection);
            };
            frame(pass, false);
            match row {
                0 => Sprite::new(
                    t(&format!("char{:02}", indices[0] + 1)),
                    [x, y, x + s, y + s],
                )
                .draw(pass, builder, projection),
                4 => Sprite::new(t(&format!("bg{:03}", indices[4] + 1)), [x, y, x + s, y + s])
                    .draw(pass, builder, projection),
                _ => self.draw_counter(pass, x, y, indices[row], counts[row]),
            }
            if selected {
                frame(pass, true);
            }
            let (lx, ly) = (x + s / 2.0 - 72.0, y + s - 36.0);
            Sprite::new(t("text"), [lx, ly, lx + 144.0, ly + 32.0])
                .uv([0.0, label / 6.0, 1.0, (label + 1.0) / 6.0])
                .draw(pass, builder, projection);
            // `0x71000395e0`: grey at the ends
            let enabled = [indices[row] > 0, indices[row] + 2 <= counts[row]];
            for side in 0..2 {
                let (ax, ay) = (arrows[side] + p, ARROW_Y[row]);
                let rect = [ax, ay, ax + 80.0, ay + 80.0];
                let u = side as f32 * 0.5;
                let mut arrow = Sprite::new(t("arrow"), rect)
                    .uv([u, 0.0, u + 0.5, 0.5])
                    .flash(self.arrows[2 * row + side].value());
                if !enabled[side] {
                    arrow.color = FloatColor4::from_rgba(0.4, 0.4, 0.4, 1.0);
                }
                arrow.draw(pass, builder, projection);
                if selected {
                    Sprite::new(t("arrow"), rect)
                        .uv([u, 0.5, u + 0.5, 1.0])
                        .alpha(0.3)
                        .draw(pass, builder, projection);
                }
            }
        }

        let u = self.ui.value();
        if u <= 0.0 {
            return;
        }
        let y = 150.0 * u - 150.0 + 32.0;
        Sprite::new(t("close"), [1764.0, y, 1848.0, y + 84.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);
        Sprite::new(t("hidden"), [1668.0, y, 1752.0, y + 84.0])
            .flash(self.hide_flash.value())
            .draw(pass, builder, projection);

        // the key help, right to left (`0x7100039a40`, `0x7100166e24`)
        let help = KeyHelp {
            labels: t("help_bupmode"),
            keys: t("key"),
            label_height: 192.0,
            y: 1100.0 - 88.0 * u,
            glyph_offset: 0.0,
            glyph_step: 46.0,
            gap: 10.0,
        };
        let white = FloatColor4::WHITE;
        let mut x = help.draw(pass, 1872.0, &[4], 48.0, 80.0, white);
        x = help.draw(pass, x, &[3], 96.0, 180.0, white);
        x = help.draw(pass, x, &[7, 2], 144.0, 214.0, white);
        x = help.draw(pass, x, &[1], 0.0, 214.0, white);
        // port-only
        self.export.draw_entry(pass, t("key"), x, help.y, white);
    }

    /// `0x7100039770`: "i/n" in the `num` glyphs, centred in the 112 px frame
    fn draw_counter(&self, pass: &mut RenderPass, x: f32, y: f32, index: usize, count: usize) {
        let text = format!("{}/{}", index + 1, count);
        let w1 = if index <= 8 { 32.0 } else { 48.0 };
        let w2 = if count - 1 < 9 { 16.0 } else { 32.0 };
        let mut pen = x + 56.0 - (w1 + w2) / 2.0;
        let num: &GpuTexture = self.textures.get("num");
        for c in text.chars() {
            let g = c.to_digit(10).map_or(10.0, |d| d as f32);
            let mut glyph = Sprite::new(num, [pen - 1.0, y + 36.0, pen + 17.0, y + 76.0]).uv([
                g * 32.0 / 352.0,
                0.0,
                g * 32.0 / 352.0 + 18.0 / 352.0,
                1.0,
            ]);
            glyph.color = COUNTER_COLOR;
            glyph.draw(pass, ui::builder(), ui::projection());
            pen += 16.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use shin_core::format::scenario::{
        info::{BustupId, CharacterBoxSegment},
        instruction_elements::FromNumber,
    };

    use super::group;

    #[test]
    fn segments_group_into_characters() {
        let bustup = |id| CharacterBoxSegment::Bustup {
            bustup_id: BustupId::from_number(id),
        };
        let segments = vec![
            bustup(1),
            bustup(2),
            CharacterBoxSegment::EndExpressionGroup,
            bustup(3),
            CharacterBoxSegment::EndPoseGroup,
            bustup(4),
            CharacterBoxSegment::EndDefinition,
            CharacterBoxSegment::EndDefinition,
            bustup(5),
            CharacterBoxSegment::EndDefinition,
        ];
        let table = group(&segments);
        assert_eq!(
            table.characters,
            vec![
                vec![vec![vec![1, 2], vec![3]], vec![vec![4]]],
                vec![vec![vec![5]]]
            ]
        );
    }
}
