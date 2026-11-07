//! Input: the raw state of the keyboard, mouse and gamepads, and per-action states over time.
//!
//! [`RawInputAccumulator`] collects winit's keyboard and mouse events and polls gilrs for gamepads (buttons, and stick
//! directions as virtual buttons, pressed past 0.7 and released below 0.4); [`RawInputAccumulator::start_frame`]
//! gives the frame's
//! [`RawInputState`], which also records the device used last (for the button prompts). An app's [`Action`] type
//! lowers that state to its actions (the game does it through its bindings table, `shin/src/bindings.rs`), and
//! [`ActionsState`] tracks each action's held, clicked and repeat states. Gamepad buttons are named by the Switch
//! role of their position ([`inputs::GamepadButton`]); the "Confirm button" setting swaps the south and east buttons
//! where they enter ([`inputs::set_bottom_confirms`]).

pub mod inputs;

mod action;
mod raw_input_state;

pub use action::{Action, ActionSignal, ActionState, ActionsState, DummyAction};
pub use raw_input_state::{RawInputAccumulator, RawInputState};

/// The mouse as the game sees it: its position on the 1920x1080 canvas (none when outside the canvas) and whether the
/// left button is held
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Pointer {
    pub position: Option<glam::Vec2>,
    pub held: bool,
}
