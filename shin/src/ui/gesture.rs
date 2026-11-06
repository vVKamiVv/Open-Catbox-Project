//! The original's touch gestures (`shin::GestureRecognizer`, update `0x710006b070`): the menus' lists and pages are
//! dragged and flicked through this, each screen doing its own scrolling. The port feeds it the mouse's left button
//! as a single finger, in canvas pixels (the original scales the 1280x720 panel by 1.5 to the same space).
//!
//! The flick speed is the last frame's move, as in the original, but taken from the last frame the pointer moved
//! (if under two frames before the release): a mouse does not report every frame as the touch panel does, and the
//! release often lands in a frame without a move.
//!
//! shortcut: one finger only; the original averages several fingers into one point and a second finger cancels the
//! tap. Upgrade if the port ever takes touch input.

use glam::Vec2;
use shin_input::Pointer;

/// Moving further than this from the down point turns a press into a drag (8/960 of the panel's 1280: 16 canvas px)
const DRAG_START: f32 = 16.0;
/// After that, smaller moves than this (one panel pixel) send no drag
const DRAG_STEP: f32 = 1.5;
/// Released faster than this, in canvas px per frame, a drag ends in a flick (10/960 of 1280)
pub const FLICK_SPEED: f32 = 20.0;
/// Frames held still before `Press` (0.15 s) and `LongPress` (1.15 s)
const PRESS_FRAMES: f32 = 0.15 * 60.0;
const LONG_PRESS_FRAMES: f32 = 1.15 * 60.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// The button went down here; a screen that does not want this touch calls [`GestureRecognizer::cancel`]
    Down(Vec2),
    /// Held still for 0.15 s
    Press,
    /// Released without dragging or a long press; at the down position
    Tap(Vec2),
    /// Held still for 1.15 s, when enabled; no tap follows
    LongPress,
    /// The pointer moved from the first point to the second; the first drag starts at the down point
    Drag(Vec2, Vec2),
    /// Released while moving, in canvas px per frame
    Flick(Vec2),
    /// The touch is over (after a tap or a flick too)
    End,
}

#[derive(Debug, Default, Clone)]
pub struct GestureRecognizer {
    long_press: bool,
    touch: Option<Touch>,
    /// The button is still down after [`GestureRecognizer::cancel`]: ignored until released
    ignoring: bool,
}

#[derive(Debug, Clone)]
struct Touch {
    down: Vec2,
    /// Where the last drag ended
    last: Vec2,
    /// The pointer's position a frame ago, for the flick speed
    previous: Vec2,
    velocity: Vec2,
    /// Frames since the pointer last moved
    still_frames: f32,
    held_frames: f32,
    dragging: bool,
    pressed: bool,
    long_pressed: bool,
}

impl Touch {
    fn track(&mut self, at: Vec2, frames: f32) {
        if at == self.previous {
            self.still_frames += frames;
        } else {
            self.velocity = (at - self.previous) / frames.max(1e-3);
            self.still_frames = 0.0;
            self.previous = at;
        }
    }
}

impl GestureRecognizer {
    /// `long_press` as `0x710006aff0`: on in the ADV screen and the save list only
    pub fn new(long_press: bool) -> Self {
        Self {
            long_press,
            ..Self::default()
        }
    }

    /// A touch is down (`0x710006b860` returns true): the screens skip their button input then
    pub fn is_active(&self) -> bool {
        self.touch.is_some()
    }

    /// `0x710006b000`: drops the touch; nothing more comes until the button goes down again
    pub fn cancel(&mut self) {
        if self.touch.take().is_some() {
            self.ignoring = true;
        }
    }

    /// One frame (`frames` long, 1.0 at 60 fps) of the pointer
    pub fn update(&mut self, pointer: Pointer, frames: f32) -> Vec<Gesture> {
        let mut out = Vec::new();
        if self.ignoring {
            self.ignoring = pointer.held;
            return out;
        }
        match (&mut self.touch, pointer.held, pointer.position) {
            (None, true, Some(at)) => {
                self.touch = Some(Touch {
                    down: at,
                    last: at,
                    previous: at,
                    velocity: Vec2::ZERO,
                    still_frames: 0.0,
                    held_frames: 0.0,
                    dragging: false,
                    pressed: false,
                    long_pressed: false,
                });
                out.push(Gesture::Down(at));
            }
            (Some(touch), true, Some(at)) => {
                touch.track(at, frames);
                touch.held_frames += frames;
                if !touch.dragging && at.distance(touch.down) > DRAG_START {
                    touch.dragging = true;
                }
                if touch.dragging {
                    if at.distance(touch.last) > DRAG_STEP {
                        out.push(Gesture::Drag(touch.last, at));
                        touch.last = at;
                    }
                } else {
                    if !touch.pressed && touch.held_frames >= PRESS_FRAMES {
                        touch.pressed = true;
                        out.push(Gesture::Press);
                    }
                    if self.long_press
                        && !touch.long_pressed
                        && touch.held_frames >= LONG_PRESS_FRAMES
                    {
                        touch.long_pressed = true;
                        out.push(Gesture::LongPress);
                    }
                }
            }
            (Some(_), _, position) => {
                // released (or the pointer left the window)
                let mut touch = self.touch.take().unwrap();
                if let Some(at) = position {
                    touch.track(at, frames);
                }
                if touch.dragging {
                    if touch.still_frames < 2.0 && touch.velocity.length() > FLICK_SPEED {
                        out.push(Gesture::Flick(touch.velocity));
                    }
                } else if !touch.long_pressed {
                    out.push(Gesture::Tap(touch.down));
                }
                out.push(Gesture::End);
            }
            (None, _, _) => {}
        }
        out
    }
}

/// The lists' coasting after a flick (save list `0x71000d6640`, backlog `0x71000a3d40`, tips `0x7100108070`): the
/// offset moves by the speed each frame, clamped, and the speed decays by 0.95 a frame
#[derive(Debug, Default, Clone, Copy)]
pub struct Coast {
    velocity: f32,
}

impl Coast {
    pub fn start(&mut self, velocity: f32) {
        self.velocity = velocity;
    }

    pub fn stop(&mut self) {
        self.velocity = 0.0;
    }

    pub fn is_moving(&self) -> bool {
        self.velocity != 0.0
    }

    /// The offset after `frames`; content moves with the finger, so a downward flick lowers the offset
    pub fn update(&mut self, offset: f32, min: f32, max: f32, frames: f32) -> f32 {
        if self.velocity == 0.0 {
            return offset;
        }
        let next = (offset - self.velocity * frames).clamp(min, max.max(min));
        self.velocity *= 0.95f32.powf(frames);
        // it stops when slow, or at once when it is held at an end
        if self.velocity.abs() < 0.1 || next == offset {
            self.velocity = 0.0;
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, held: bool) -> Pointer {
        Pointer {
            position: Some(Vec2::new(x, y)),
            held,
        }
    }

    #[test]
    fn a_short_press_taps_and_a_long_move_drags_then_flicks() {
        let mut g = GestureRecognizer::new(false);
        let down = Vec2::new(100.0, 100.0);
        assert_eq!(g.update(at(100.0, 100.0, true), 1.0), [Gesture::Down(down)]);
        // within 16 px it stays a tap, reported at the down point
        assert!(g.update(at(110.0, 100.0, true), 1.0).is_empty());
        assert_eq!(
            g.update(at(110.0, 100.0, false), 1.0),
            [Gesture::Tap(down), Gesture::End]
        );

        g.update(at(100.0, 100.0, true), 1.0);
        // past 16 px the first drag starts at the down point
        assert_eq!(
            g.update(at(100.0, 130.0, true), 1.0),
            [Gesture::Drag(down, Vec2::new(100.0, 130.0))]
        );
        assert_eq!(
            g.update(at(100.0, 130.0, false), 1.0),
            [Gesture::Flick(Vec2::new(0.0, 30.0)), Gesture::End]
        );
    }

    #[test]
    fn a_cancelled_touch_is_ignored_until_released() {
        let mut g = GestureRecognizer::new(false);
        g.update(at(0.0, 0.0, true), 1.0);
        g.cancel();
        assert!(g.update(at(0.0, 50.0, true), 1.0).is_empty());
        assert!(g.update(at(0.0, 50.0, false), 1.0).is_empty());
        assert_eq!(
            g.update(at(5.0, 5.0, true), 1.0),
            [Gesture::Down(Vec2::new(5.0, 5.0))]
        );
    }

    #[test]
    fn coasting_decays_and_stops_at_the_ends() {
        let mut coast = Coast::default();
        coast.start(40.0);
        assert_eq!(coast.update(500.0, 0.0, 1000.0, 1.0), 460.0);
        assert!(coast.is_moving());
        coast.start(40.0);
        assert_eq!(coast.update(10.0, 0.0, 1000.0, 1.0), 0.0);
        assert_eq!(coast.update(0.0, 0.0, 1000.0, 1.0), 0.0);
        assert!(!coast.is_moving());
    }
}
