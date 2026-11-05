//! The last choice ([`LayerType::Quiz`](shin_core::vm::command::types::LayerType::Quiz) kind 3): two doors, picked by
//! looking at one and confirming.
//!
//! Re-implemented from the behaviour of `shin::LastLayer` in the Switch v1.0.3 executable (constructor
//! `0x710006d2d0`, update `0x710006e4e0`, draw `0x710006e5d0`, result `0x710006f870`) and its input handler
//! `shin::LastLayer::LastOverlayLayer` (keys `0x710006ff60`).
//!
//! The scene starts on a front view with a plate for each door. Left or right flashes that plate and pans the scene
//! 960 px over 20 frames so the door is centred, its frame pulsing; confirm there picks it (answer 0 left, 1 right),
//! the opposite direction pans back to the front view. Input waits while the pan or the front view's fade runs, and
//! the pan waits for the plate flashes.

use std::{f32::consts::TAU, fmt::Debug, sync::Arc};

use glam::{Mat4, Vec2, vec3};
use shin_core::time::{Easing, Tweener};
use shin_render::{
    PassKind, gpu_texture::GpuTexture, render_pass::RenderPass, shaders::types::RenderClone,
};

use crate::{
    asset::texture_archive::TextureArchive,
    layer::{
        LayerProperties, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::minigame::{
            self, Minigame, MinigameInput, PointerTarget, SOUND_DECIDE, SOUND_MOVE, Sprite, contains,
            frames,
        },
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

/// All of `/last.txa`
#[derive(TextureArchive)]
pub struct LastTextures {
    #[txa(name = "bg")]
    pub bg: GpuTexture,
    #[txa(name = "front")]
    pub front: GpuTexture,
    /// The two door plates of the front view, side by side
    #[txa(name = "select")]
    pub select: GpuTexture,
    /// The "back" buttons of the door views, side by side
    #[txa(name = "back")]
    pub back: GpuTexture,
    #[txa(name = "shadow")]
    pub shadow: GpuTexture,
    /// The door plates of the door views, side by side
    #[txa(name = "plate")]
    pub plate: GpuTexture,
    #[txa(name = "flash_door")]
    pub flash_door: GpuTexture,
    /// Covers the scene beyond the front view's edges while it pans
    #[txa(name = "obi")]
    pub obi: GpuTexture,
}

/// How far the scene pans to centre a door
const PAN: f32 = 960.0;
const PAN_FRAMES: f32 = 20.0;
/// The door view's fade when a door is picked
const PICK_FADE_FRAMES: f32 = 5.0;
/// The door frame's pulse: one cycle per 100 frames (`0x710015a928`), alpha (sin + 1) x 0.2
const PULSE_PER_FRAME: f32 = 0.01;
const PULSE_AMPLITUDE: f32 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Front,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    Picked,
    Done,
}

#[derive(Clone, RenderClone)]
pub struct LastLayerImpl {
    textures: Arc<LastTextures>,
    side: Side,
    state: State,
    pan: Tweener,
    front_alpha: Tweener,
    door_alpha: Tweener,
    select_flash: [Tweener; 2],
    back_flash: Tweener,
    plate_flash: Tweener,
    pulse_phase: f32,
    sounds: Vec<&'static str>,
    /// What the pointer is over (`+0x29c`): the left plate or the door (0), the right plate or "back" (1)
    pointer_target: Option<usize>,
}

impl LastLayerImpl {
    pub fn new(textures: Arc<LastTextures>) -> Self {
        Self {
            textures,
            side: Side::Front,
            state: State::Waiting,
            pan: Tweener::new(0.0),
            front_alpha: Tweener::new(1.0),
            door_alpha: Tweener::new(0.0),
            select_flash: [Tweener::new(0.0), Tweener::new(0.0)],
            back_flash: Tweener::new(0.0),
            plate_flash: Tweener::new(0.0),
            pulse_phase: 0.0,
            sounds: Vec::new(),
            pointer_target: None,
        }
    }

    fn look_at(&mut self, side: Side) {
        self.sounds.push(SOUND_MOVE);
        let (flash, pan) = match side {
            Side::Left => (&mut self.select_flash[0], PAN),
            Side::Right => (&mut self.select_flash[1], -PAN),
            Side::Front => (&mut self.back_flash, 0.0),
        };
        minigame::flash(flash);
        let front = if side == Side::Front { 1.0 } else { 0.0 };
        self.pan.enqueue(pan, frames(PAN_FRAMES, Easing::SineOut));
        self.front_alpha
            .enqueue(front, frames(PAN_FRAMES, Easing::Linear));
        self.door_alpha
            .enqueue(1.0 - front, frames(PAN_FRAMES, Easing::Linear));
        if side != Side::Front {
            self.pulse_phase = 0.0;
        }
        self.side = side;
    }

    fn pick(&mut self) {
        self.sounds.push(SOUND_DECIDE);
        self.door_alpha
            .enqueue(0.0, frames(PICK_FADE_FRAMES, Easing::Linear));
        minigame::flash(&mut self.plate_flash);
        self.state = State::Picked;
    }

    /// The flash a pointer target lights: the plates on the front view, the door plate or "back" on a door view
    fn target_flash(&mut self, target: usize) -> &mut Tweener {
        match (self.side, target) {
            (Side::Front, 0) => &mut self.select_flash[0],
            (Side::Front, _) => &mut self.select_flash[1],
            (_, 0) => &mut self.plate_flash,
            _ => &mut self.back_flash,
        }
    }

    fn view_is_moving(&self) -> bool {
        !self.pan.is_idle() || !self.front_alpha.is_idle()
    }
}

/// The pointer targets, in 1920x1080 top-left space (`0x710006f160` front view, `0x710006f340` left door,
/// `0x710006f530` right door): (rect, target)
fn pointer_targets(side: Side) -> &'static [([f32; 4], usize)] {
    const fn r(x: f32, y: f32, w: f32, h: f32) -> [f32; 4] {
        [x, y, x + w, y + h]
    }
    const FRONT: [([f32; 4], usize); 2] = [(r(272.0, 816.0, 624.0, 176.0), 0), (r(1024.0, 816.0, 624.0, 176.0), 1)];
    const LEFT: [([f32; 4], usize); 3] = [
        (r(496.0, 96.0, 918.0, 800.0), 0),
        (r(768.0, 880.0, 436.0, 176.0), 0),
        (r(1416.0, 816.0, 480.0, 176.0), 1),
    ];
    const RIGHT: [([f32; 4], usize); 3] = [
        (r(506.0, 96.0, 918.0, 800.0), 0),
        (r(720.0, 880.0, 436.0, 176.0), 0),
        (r(24.0, 816.0, 480.0, 176.0), 1),
    ];
    match side {
        Side::Front => &FRONT,
        Side::Left => &LEFT,
        Side::Right => &RIGHT,
    }
}

/// `LastOverlayLayer` slots 18-25 (`0x7100070290`-`0x71000705d0`)
impl PointerTarget for LastLayerImpl {
    fn pointer_hit(&mut self, position: Vec2) -> bool {
        self.pointer_target = None;
        if self.state == State::Waiting && !self.view_is_moving() {
            self.pointer_target = pointer_targets(self.side)
                .iter()
                .find(|(rect, _)| contains(*rect, position))
                .map(|&(_, target)| target);
        }
        self.pointer_target.is_some()
    }

    fn pointer_leave(&mut self) {
        self.pointer_target = None;
    }

    fn pointer_press(&mut self) {
        if let Some(target) = self.pointer_target {
            minigame::raise(self.target_flash(target));
        }
    }

    fn pointer_release(&mut self) {
        let Some(target) = self.pointer_target.take() else { return };
        match (self.side, target) {
            (Side::Front, 0) => self.look_at(Side::Left),
            (Side::Front, _) => self.look_at(Side::Right),
            (_, 0) => self.pick(),
            _ => self.look_at(Side::Front),
        }
    }

    fn pointer_cancel(&mut self) {
        // drops whichever of the two flashes a press could have raised
        for target in 0..2 {
            minigame::lower(self.target_flash(target));
        }
        self.pointer_target = None;
    }
}

impl Minigame for LastLayerImpl {
    fn handle_input(&mut self, input: &MinigameInput) {
        if self.state != State::Waiting || self.view_is_moving() {
            return;
        }
        let pressed = input.pressed;
        match self.side {
            Side::Front if pressed.left => self.look_at(Side::Left),
            Side::Front if pressed.right => self.look_at(Side::Right),
            Side::Left | Side::Right if input.confirm => self.pick(),
            Side::Left if pressed.right => self.look_at(Side::Front),
            Side::Right if pressed.left => self.look_at(Side::Front),
            _ => {}
        }
    }

    fn result(&self) -> Option<i32> {
        (self.state == State::Done).then_some(match self.side {
            Side::Right => 1,
            // the original reports its side index, which is 0 for the left door
            _ => 0,
        })
    }

    fn is_interactive(&self) -> bool {
        self.state == State::Waiting
    }

    fn auto_pick(&mut self) {
        if self.state == State::Waiting {
            self.side = Side::Left;
            self.pick();
        }
    }

    fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
}

pub type LastLayer = NewDrawableLayerWrapper<LastLayerImpl>;

impl LastLayer {
    pub fn new(textures: Arc<LastTextures>) -> Self {
        Self::from_inner(LastLayerImpl::new(textures))
    }
}

impl NewDrawableLayerNeedsSeparatePass for LastLayerImpl {}

impl NewDrawableLayer for LastLayerImpl {
    #[tracing::instrument(skip_all)]
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        &DrawableParams {
            color_multiplier, ..
        }: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind != PassKind::Transparent || color_multiplier.a <= 0.0 {
            return;
        }
        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );
        let pan = self.pan.value();
        let transform =
            transform.compute_final_transform() * Mat4::from_translation(vec3(pan, 0.0, 0.0));
        let builder = minigame::builder(stencil_ref);
        let textures = &self.textures;
        let alpha = color_multiplier.a;
        let draw = |pass: &mut RenderPass, sprite: Sprite| {
            let sprite = Sprite {
                color: sprite.color.with_alpha(sprite.color.a * alpha),
                ..sprite
            };
            sprite.draw(pass, builder, transform);
        };

        pass.push_debug("LastLayer");
        // the original draws the background in the opaque pass, as one quad 3840 wide with uvs 0-2 and a repeating
        // sampler
        draw(
            pass,
            Sprite::new(&textures.bg, [-1920.0, -540.0, 0.0, 540.0]),
        );
        draw(
            pass,
            Sprite::new(&textures.bg, [0.0, -540.0, 1920.0, 540.0]),
        );

        let pulse = ((self.pulse_phase * TAU).sin() * PULSE_AMPLITUDE + PULSE_AMPLITUDE)
            * self.door_alpha.value();
        let plate_flash = self.plate_flash.value();
        let back_flash = self.back_flash.value();
        if pan > 0.0 {
            // looking at the left door
            draw(
                pass,
                Sprite::new(&textures.back, [-504.0, 276.0, -24.0, 452.0])
                    .uv([0.5, 0.0, 1.0, 1.0])
                    .flash(back_flash),
            );
            let plate = [-1248.0, 340.0, -624.0, 516.0];
            draw(pass, Sprite::new(&textures.shadow, plate));
            draw(
                pass,
                Sprite::new(&textures.plate, plate)
                    .uv([0.0, 0.0, 0.5, 1.0])
                    .flash(plate_flash),
            );
            let door = [-1424.0, -492.0, -506.0, 352.0];
            draw(pass, Sprite::new(&textures.flash_door, door).alpha(pulse));
            draw(
                pass,
                Sprite::new(&textures.flash_door, door).alpha(plate_flash),
            );
        }
        if pan < 0.0 {
            // looking at the right door: the same pieces mirrored
            draw(
                pass,
                Sprite::new(&textures.back, [24.0, 276.0, 504.0, 452.0])
                    .uv([0.0, 0.0, 0.5, 1.0])
                    .flash(back_flash),
            );
            let plate = [624.0, 340.0, 1248.0, 516.0];
            draw(pass, Sprite::new(&textures.shadow, plate));
            draw(
                pass,
                Sprite::new(&textures.plate, plate)
                    .uv([0.5, 0.0, 1.0, 1.0])
                    .flash(plate_flash),
            );
            let door = [506.0, -492.0, 1424.0, 352.0];
            let mirrored = [1.0, 0.0, 0.0, 1.0];
            draw(
                pass,
                Sprite::new(&textures.flash_door, door)
                    .uv(mirrored)
                    .alpha(pulse),
            );
            draw(
                pass,
                Sprite::new(&textures.flash_door, door)
                    .uv(mirrored)
                    .alpha(plate_flash),
            );
        }
        let front = self.front_alpha.value();
        if front > 0.0 {
            draw(
                pass,
                Sprite::new(&textures.front, [-960.0, -540.0, 960.0, 540.0]).alpha(front),
            );
            draw(
                pass,
                Sprite::new(&textures.select, [-688.0, 276.0, -64.0, 452.0])
                    .uv([0.0, 0.0, 0.5, 1.0])
                    .alpha(front)
                    .flash(self.select_flash[0].value()),
            );
            draw(
                pass,
                Sprite::new(&textures.select, [64.0, 276.0, 688.0, 452.0])
                    .uv([0.5, 0.0, 1.0, 1.0])
                    .alpha(front)
                    .flash(self.select_flash[1].value()),
            );
            draw(
                pass,
                Sprite::new(&textures.obi, [-1920.0, -540.0, -960.0, 540.0]).alpha(front),
            );
            draw(
                pass,
                Sprite::new(&textures.obi, [960.0, -540.0, 1920.0, 540.0]).alpha(front),
            );
        }
        pass.pop_debug();
    }

    fn update_from_props(&mut self, _props: &LayerProperties) {}
}

impl NewDrawableLayerFastForward for LastLayerImpl {
    fn fast_forward(&mut self) {}
}

impl AdvUpdatable for LastLayerImpl {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        let delta = ctx.delta_ticks;
        let flashing = self
            .select_flash
            .iter()
            .chain([&self.back_flash])
            .any(|flash| !flash.is_idle());
        if !flashing {
            self.pan.update(delta);
            self.front_alpha.update(delta);
            self.door_alpha.update(delta);
        }
        for flash in self
            .select_flash
            .iter_mut()
            .chain([&mut self.back_flash, &mut self.plate_flash])
        {
            flash.update(delta);
        }
        self.pulse_phase = (self.pulse_phase + delta.as_f32() * PULSE_PER_FRAME).fract();
        if self.state == State::Picked && self.plate_flash.is_idle() {
            self.state = State::Done;
        }
    }
}

impl Debug for LastLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LastLayer")
            .field("side", &self.side)
            .field("state", &self.state)
            .finish()
    }
}
