//! The rules page (`shin::MurdererLayer::Rule`: constructor `0x710008f8c0`, update `0x710008fbb0`, draw
//! `0x710008fcd0`, keys `0x71000902c0`): one picture of text in a frame; only "back" does anything.

use shin_core::time::{Ticks, Tweener};

use super::{
    BACK_RECT, Handover, SOUND_CANCEL, Screen, Shared,
    draw::{Drawer, SCREEN},
    top_menu::TopMenu,
};
use crate::layer::user::minigame::{self, MinigameInput, Sprite};

/// The frame `right` (384 x 448) cut at these screen and texture edges
const FRAME_XS: [f32; 4] = [264.0, 280.0, 1320.0, 1672.0];
const FRAME_YS: [f32; 4] = [164.0, 180.0, 516.0, 932.0];
const FRAME_US: [f32; 4] = [0.0, 1.0 / 24.0, 1.0 / 12.0, 1.0];
const FRAME_VS: [f32; 4] = [0.0, 1.0 / 28.0, 1.0 / 14.0, 1.0];
const TEXT_RECT: [f32; 4] = [300.0, 180.0, 1420.0, 900.0];
/// The top menu item this page belongs to
const MENU_ITEM: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Leaving,
    Done,
}

#[derive(Clone)]
pub struct Rule {
    state: State,
    back_flash: Tweener,
}

impl Rule {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            back_flash: Tweener::new(0.0),
        }
    }

    pub fn update(&mut self, delta: Ticks, _shared: &mut Shared) -> Option<Handover> {
        let mut handover = None;
        if self.state == State::Leaving && self.back_flash.is_idle() {
            handover = Some(Handover::crossfade(
                30.0,
                Some(Screen::Top(TopMenu::new(MENU_ITEM))),
            ));
            self.state = State::Done;
        }
        self.back_flash.update(delta);
        handover
    }

    pub fn handle_input(&mut self, input: &MinigameInput, shared: &mut Shared) {
        if self.state == State::Idle && input.cancel {
            shared.sounds.push(SOUND_CANCEL);
            minigame::flash(&mut self.back_flash);
            self.state = State::Leaving;
        }
    }

    pub fn auto_pick(&mut self, shared: &mut Shared) {
        self.handle_input(
            &MinigameInput {
                cancel: true,
                ..Default::default()
            },
            shared,
        );
    }

    pub fn draw(&self, drawer: &mut Drawer, _shared: &Shared, background: bool) {
        if background {
            drawer.image("different_space_2d", SCREEN);
            return;
        }
        drawer.nine_slice("right", FRAME_XS, FRAME_YS, FRAME_US, FRAME_VS);
        drawer.image("rule_text", TEXT_RECT);
        draw_back(drawer, &self.back_flash);
    }
}

/// The back button with its flash (program 5)
pub fn draw_back(drawer: &mut Drawer, flash: &Tweener) {
    let texture = drawer.texture("back");
    drawer.sprite(Sprite::new(texture, BACK_RECT).flash(flash.value()));
}
