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
