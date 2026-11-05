//! What the quiz layer type's minigames share: the input they read each frame, their result, and how they draw.
//!
//! In the original, each minigame layer installs an "overlay layer" as the input handler while a QUIZ command waits on
//! it; the handler reads the controller state (`0x71000c4900`), which has held directions in its first byte, a
//! confirm bit (byte 7 bit 4) and repeating direction presses in byte 11 (bits 0-3: up, down, left, right).

use glam::{Mat4, Vec2, Vec4, vec2};
use shin_input::Pointer;
use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tween, Tweener},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

/// One frame of controller state, as the minigames' input handlers read it
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MinigameInput {
    /// 60 Hz frames since the last input: the original reads input once per frame, the port once per rendered frame
    pub frames: f32,
    /// Directions held this frame
    pub held: Directions,
    /// Directions pressed this frame, with key repeat
    pub pressed: Directions,
    pub confirm: bool,
    pub cancel: bool,
    /// The right stick's vertical tilt, -1 to 1 (scrolls texts); PageUp/PageDown on a keyboard
    pub text_scroll: f32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Directions {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

/// The mouse/touch handlers of the original's overlay layers (slots 18-22): a hit test that remembers what is under
/// the pointer, and press, release and cancel acting on it
pub trait PointerTarget {
    /// Slot 18: whether something is under `position` (1920x1080 top-left), remembered for the calls below
    fn pointer_hit(&mut self, _position: Vec2) -> bool {
        false
    }
    /// Slot 19: the pointer left
    fn pointer_leave(&mut self) {}
    /// Slot 20: the button went down over what the hit test found
    fn pointer_press(&mut self) {}
    /// Slot 21: the button was released over it: activate it
    fn pointer_release(&mut self) {}
    /// Slot 22: the button was released elsewhere: undo the press
    fn pointer_cancel(&mut self) {}
}

/// Turns the mouse into the handler calls, like a button: hovering hit-tests, a press on a target raises it, and
/// releasing over a target activates it while releasing elsewhere cancels.
///
/// The original's touch dispatcher was not decompiled; this ordering is inferred from what the handlers do.
#[derive(Debug, Default, Clone, Copy)]
pub struct PointerTracker {
    held: bool,
    hovering: bool,
    pressed: bool,
}

impl PointerTracker {
    pub fn dispatch<T: PointerTarget + ?Sized>(&mut self, pointer: Pointer, target: &mut T) {
        let went_down = pointer.held && !self.held;
        let went_up = !pointer.held && self.held;
        self.held = pointer.held;
        if self.pressed && !went_up {
            // the press holds on to its target until the button is released
            return;
        }
        let hit = pointer.position.is_some_and(|position| target.pointer_hit(position));
        if self.hovering && !hit {
            target.pointer_leave();
        }
        self.hovering = hit;
        if went_up && self.pressed {
            self.pressed = false;
            if hit {
                target.pointer_release();
            } else {
                target.pointer_cancel();
            }
        } else if went_down && hit {
            self.pressed = true;
            target.pointer_press();
        }
    }
}

/// What the QUIZ command needs from a minigame layer
pub trait Minigame: PointerTarget {
    fn handle_input(&mut self, input: &MinigameInput);
    /// The answer, once the player has picked and the pick animation is over
    fn result(&self) -> Option<i32>;
    /// Whether the minigame is waiting for the player
    fn is_interactive(&self) -> bool;
    /// Picks an answer at once; debug fast-forwarding cannot wait for a player
    fn auto_pick(&mut self);
    /// System sounds started since the last call
    fn take_sounds(&mut self) -> Vec<&'static str>;
}

/// Raises a flash and keeps it there while the pointer is held
pub fn raise(tweener: &mut Tweener) {
    tweener.enqueue_now(FLASH_PEAK, seconds(FLASH_SECONDS));
}

/// Drops a raised flash
pub fn lower(tweener: &mut Tweener) {
    tweener.enqueue_now(0.0, seconds(FLASH_SECONDS));
}

/// `rect` (left, top, right, bottom) contains `point`, edges as the original tests them
pub fn contains([x0, y0, x1, y1]: [f32; 4], point: Vec2) -> bool {
    x0 <= point.x && point.x < x1 && y0 <= point.y && point.y < y1
}

pub const SOUND_MOVE: &str = "move";
pub const SOUND_DECIDE: &str = "decide";
pub const SOUND_ERROR: &str = "error";

/// The pick flash the minigames share: to 0.7 and back in 0.2 s each (`0x710015a584`, `0x710015a954`)
pub const FLASH_PEAK: f32 = 0.7;
pub const FLASH_SECONDS: f32 = 0.2;

pub fn seconds(seconds: f32) -> Tween {
    Tween {
        duration: Ticks::from_seconds(seconds),
        easing: Easing::Linear,
    }
}

pub fn frames(frames: f32, easing: Easing) -> Tween {
    Tween {
        duration: Ticks::from_f32(frames),
        easing,
    }
}

/// Starts the shared pick flash on `tweener`
pub fn flash(tweener: &mut Tweener) {
    tweener.enqueue_now(FLASH_PEAK, seconds(FLASH_SECONDS));
    tweener.enqueue(0.0, seconds(FLASH_SECONDS));
}

/// A textured quad as the minigames draw it (`0x71000c6c10` rect, `0x71000c6c60` uvs, `0x71000c6c90` vertex colour),
/// mixed toward white by `glow` then by `flash` (program 5; with both at 0 it equals the plain sprite program 2)
pub struct Sprite<'a> {
    pub texture: &'a GpuTexture,
    /// left, top, right, bottom in the layer's centred pixel space
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: FloatColor4,
    pub glow: f32,
    pub flash: f32,
}

impl<'a> Sprite<'a> {
    pub fn new(texture: &'a GpuTexture, rect: [f32; 4]) -> Self {
        Self {
            texture,
            rect,
            uv: [0.0, 0.0, 1.0, 1.0],
            color: FloatColor4::WHITE,
            glow: 0.0,
            flash: 0.0,
        }
    }

    pub fn uv(self, uv: [f32; 4]) -> Self {
        Self { uv, ..self }
    }

    pub fn alpha(self, alpha: f32) -> Self {
        Self {
            color: FloatColor4::WHITE.with_alpha(alpha),
            ..self
        }
    }

    pub fn glow(self, glow: f32) -> Self {
        Self { glow, ..self }
    }

    pub fn flash(self, flash: f32) -> Self {
        Self { flash, ..self }
    }

    pub fn draw(&self, pass: &mut RenderPass, builder: RenderRequestBuilder, transform: Mat4) {
        if self.color.a <= 0.0 {
            return;
        }
        let [x0, y0, x1, y1] = self.rect;
        let [u0, v0, u1, v1] = self.uv;
        let color = self.color.into_unorm();
        let vertex = |x: f32, y: f32, u: f32, v: f32| PosColTexVertex {
            position: vec2(x, y).extend(0.0),
            color,
            texture_position: vec2(u, v),
        };
        let vertices = [
            vertex(x0, y0, u0, v0),
            vertex(x1, y0, u1, v0),
            vertex(x0, y1, u0, v1),
            vertex(x1, y1, u1, v1),
        ];
        pass.run(builder.build(
            RenderProgramWithArguments::Button {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: self.texture.as_source(),
                transform,
                flash1: Vec4::new(1.0, 1.0, 1.0, self.glow),
                flash2: Vec4::new(1.0, 1.0, 1.0, self.flash),
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
}

/// The render state the minigames draw with: blend 2 whatever the layer's blend type, and the layer's stencil with
/// equal values passing (`setLayerDepthStencil(ref, 1, 0)`), so their overlapping sprites all draw
pub fn builder(stencil_ref: u8) -> RenderRequestBuilder {
    RenderRequestBuilder::new()
        .depth_stencil_shorthand(stencil_ref, true, false)
        .color_blend_type(ColorBlendType::Layer1)
}

/// Converts a rect in 1920x1080 top-left space to the centred space the layers draw in
pub fn from_top_left([x0, y0, x1, y1]: [f32; 4]) -> [f32; 4] {
    [x0 - 960.0, y0 - 540.0, x1 - 960.0, y1 - 540.0]
}
