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

use crate::ui::gesture::{Gesture, GestureRecognizer};

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

/// The touch handlers of the original's overlay layers (slots 18-25) and the choice menu, in gesture terms
pub trait PointerTarget {
    /// Slot 18, on the touch's down: whether something is under `position` (1920x1080 top-left), remembered for the
    /// calls below
    fn pointer_hit(&mut self, _position: Vec2) -> bool {
        false
    }
    /// Slot 20: held still on it for 0.15 s
    fn pointer_press(&mut self) {}
    /// Slot 21: tapped: activate it
    fn pointer_release(&mut self) {}
    /// Slots 22 (long press), 23 (drag) and 25 (cancel): undo the press; nothing is activated
    fn pointer_cancel(&mut self) {}
    /// Port-only (the Switch has no hover): the mouse moved to `position` with no button held
    fn pointer_hover(&mut self, _position: Vec2) {}
}

/// Turns the pointer into the handler calls as the ADV's touch listener (`0x71000154f0`..`0x7100015dd0`, its
/// recognizer with long presses) dispatches them: the down hit-tests, and only a touch that went down on a target
/// sends it the press, the tap, or one cancel for a long press, a drag or an end without a tap
#[derive(Debug, Clone)]
pub struct PointerTracker {
    gestures: GestureRecognizer,
    /// The down hit a target (the listener's `+0x8f4`)
    accepted: bool,
    /// Where the pointer was last frame, to send hovers on a move only
    last_position: Option<Vec2>,
}

impl Default for PointerTracker {
    fn default() -> Self {
        Self {
            gestures: GestureRecognizer::new(true),
            accepted: false,
            last_position: None,
        }
    }
}

impl PointerTracker {
    /// One frame (`frames` long at 60 fps) of the pointer
    pub fn dispatch<T: PointerTarget + ?Sized>(
        &mut self,
        pointer: Pointer,
        frames: f32,
        target: &mut T,
    ) {
        for gesture in self.gestures.update(pointer, frames) {
            match gesture {
                Gesture::Down(at) => self.accepted = target.pointer_hit(at),
                Gesture::Press if self.accepted => target.pointer_press(),
                Gesture::Tap(_) if self.accepted => {
                    self.accepted = false;
                    target.pointer_release();
                }
                Gesture::LongPress | Gesture::Drag(..) | Gesture::End if self.accepted => {
                    self.accepted = false;
                    target.pointer_cancel();
                }
                _ => {}
            }
        }
        let moved =
            pointer.position != std::mem::replace(&mut self.last_position, pointer.position);
        if let Some(position) = pointer
            .position
            .filter(|_| moved && !pointer.held && !self.gestures.is_active())
        {
            target.pointer_hover(position);
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
    tween_now(tweener, FLASH_PEAK, FLASH_RATE);
}

/// Drops a raised flash
pub fn lower(tweener: &mut Tweener) {
    tween_now(tweener, 0.0, FLASH_RATE);
}

/// `rect` (left, top, right, bottom) contains `point`, edges as the original tests them
pub fn contains([x0, y0, x1, y1]: [f32; 4], point: Vec2) -> bool {
    x0 <= point.x && point.x < x1 && y0 <= point.y && point.y < y1
}

pub const SOUND_MOVE: &str = "move";
pub const SOUND_DECIDE: &str = "decide";
pub const SOUND_ERROR: &str = "error";

/// The pick flash the minigames share: to 0.7 and back at 0.2 per frame (`0x710015a584`, `0x710015a954`)
pub const FLASH_PEAK: f32 = 0.7;
pub const FLASH_RATE: f32 = 0.2;

/// A linear tween from `from` to `to` at `rate` per frame: the original's tween flag 0x40, where the duration is
/// `|to - from| / rate` (`0x71000bb620`)
pub fn at_rate(from: f32, to: f32, rate: f32) -> Tween {
    Tween {
        duration: Ticks::from_f32((to - from).abs() / rate),
        easing: Easing::Linear,
    }
}

/// Moves `tweener` from its current value to `target` at `rate` per frame, starting now
pub fn tween_now(tweener: &mut Tweener, target: f32, rate: f32) {
    let tween = at_rate(tweener.value(), target, rate);
    tweener.enqueue_now(target, tween);
}

pub fn frames(frames: f32, easing: Easing) -> Tween {
    Tween {
        duration: Ticks::from_f32(frames),
        easing,
    }
}

/// Starts the shared pick flash on `tweener`
pub fn flash(tweener: &mut Tweener) {
    tween_now(tweener, FLASH_PEAK, FLASH_RATE);
    tweener.enqueue(0.0, at_rate(FLASH_PEAK, 0.0, FLASH_RATE));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the calls; anything left of x = 100 is a target
    #[derive(Default)]
    struct Log(Vec<&'static str>);

    impl PointerTarget for Log {
        fn pointer_hit(&mut self, position: Vec2) -> bool {
            self.0.push("hit");
            position.x < 100.0
        }
        fn pointer_press(&mut self) {
            self.0.push("press");
        }
        fn pointer_release(&mut self) {
            self.0.push("tap");
        }
        fn pointer_cancel(&mut self) {
            self.0.push("cancel");
        }
        fn pointer_hover(&mut self, _position: Vec2) {
            self.0.push("hover");
        }
    }

    fn at(x: f32, y: f32, held: bool) -> Pointer {
        Pointer {
            position: Some(vec2(x, y)),
            held,
        }
    }

    fn run(frames: &[(Pointer, f32)]) -> Vec<&'static str> {
        let mut tracker = PointerTracker::default();
        let mut log = Log::default();
        for &(pointer, frames) in frames {
            tracker.dispatch(pointer, frames, &mut log);
        }
        log.0
    }

    #[test]
    fn a_quick_click_taps_and_a_held_one_presses_first() {
        let quick = [
            (at(50.0, 0.0, false), 1.0),
            (at(50.0, 0.0, true), 1.0),
            (at(50.0, 0.0, false), 1.0),
        ];
        assert_eq!(run(&quick), ["hover", "hit", "tap"]);
        let held = [
            (at(50.0, 0.0, true), 1.0),
            (at(50.0, 0.0, true), 10.0),
            (at(50.0, 0.0, false), 1.0),
        ];
        assert_eq!(run(&held), ["hit", "press", "tap"]);
    }

    #[test]
    fn a_long_press_or_a_drag_cancels_and_never_picks() {
        let long = [
            (at(50.0, 0.0, true), 1.0),
            (at(50.0, 0.0, true), 70.0),
            (at(50.0, 0.0, false), 1.0),
        ];
        assert_eq!(run(&long), ["hit", "press", "cancel"]);
        let drag = [
            (at(50.0, 0.0, true), 1.0),
            (at(50.0, 40.0, true), 1.0),
            (at(50.0, 80.0, false), 1.0),
        ];
        // released somewhere else: the mouse is now over that place
        assert_eq!(run(&drag), ["hit", "cancel", "hover"]);
    }

    #[test]
    fn a_down_off_the_targets_sends_nothing_more() {
        let miss = [
            (at(150.0, 0.0, true), 1.0),
            (at(150.0, 0.0, true), 10.0),
            (at(150.0, 0.0, false), 1.0),
        ];
        assert_eq!(run(&miss), ["hit"]);
    }

    #[test]
    fn hovers_come_on_moves_with_no_button_held() {
        let moves = [
            (at(10.0, 0.0, false), 1.0),
            (at(10.0, 0.0, false), 1.0),
            (at(20.0, 0.0, false), 1.0),
            (at(30.0, 0.0, true), 1.0),
        ];
        assert_eq!(run(&moves), ["hover", "hover", "hit"]);
    }
}
