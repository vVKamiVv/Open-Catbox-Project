//! The culprit game's top menu (`shin::MurdererLayer::TopMenu`: constructor `0x710008be00`, update `0x710008c5b0`,
//! draw `0x710008ccd0`, keys `0x710008d4b0`): six plates on the right, Beatrice on the left.

use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tweener},
};

use super::{
    Handover, Screen, Shared,
    check::Check,
    draw::{Drawer, SCREEN, rect, white},
    hint_menu::HintMenu,
    move_highlight,
    replay_menu::ReplayMenu,
    rule::Rule,
    select::Select,
};
use crate::layer::user::minigame::{self, MinigameInput, SOUND_DECIDE, SOUND_MOVE, Sprite, frames};

const ITEMS: usize = 6;
/// The item that names the culprit
const CULPRIT_ITEM: usize = 5;
const ITEM_X: f32 = 1000.0;
const ITEM_Y: f32 = 441.0;
const ITEM_STEP: f32 = 96.0;
const ITEM_W: f32 = 704.0;
const ITEM_H: f32 = 94.0;
/// `plate` is 820 px tall: one 96 px row per item, 94 px of it drawn
const PLATE_HEIGHT: f32 = 820.0;
/// `topmenu` holds the six labels, 80 px each
const LABEL_OFFSET: (f32, f32) = (48.0, 8.0);
const LABEL_SIZE: (f32, f32) = (640.0, 80.0);
/// The plate colour at hover 0 and 1 (`0x710022cfc0`)
const PLATE_DIM: [f32; 3] = [0.2157, 0.1059, 0.1059];
const PLATE_LIT: [f32; 3] = [1.0, 0.1608, 0.1608];

/// Beatrice in four pieces: (x, y, w, h) relative to x 239 (`0x710022cff0`) and the texture per expression
/// (`0x71001db650`): the laughing face changes when the culprit is about to be named
const BEATRICE_X: f32 = 239.0;
const BEATRICE: [((f32, f32, f32, f32), [&str; 2]); 4] = [
    ((259.0, 256.0, 352.0, 160.0), ["Ber_Body1", "Ber_Body1"]),
    ((147.0, 416.0, 208.0, 208.0), ["Ber_Body2", "Ber_Body2"]),
    ((0.0, 624.0, 672.0, 456.0), ["Ber_Body3", "Ber_Body3"]),
    (
        (355.0, 416.0, 256.0, 208.0),
        ["Ber_Akuwarai2", "Ber_AkuwaraiA1"],
    ),
];

const SOUND_EXECUTE: &str = "execute";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// Waiting for the picked item's flash
    Picked,
    /// Waiting for the transition and the hold to end
    Leaving,
    Done,
}

#[derive(Clone)]
struct Item {
    hover: Tweener,
    flash: Tweener,
}

#[derive(Clone)]
pub struct TopMenu {
    cursor: usize,
    state: State,
    hold: Tweener,
    items: Vec<Item>,
    expression: usize,
}

impl TopMenu {
    pub fn new(cursor: usize) -> Self {
        Self {
            cursor,
            state: State::Idle,
            hold: Tweener::new(0.0),
            items: (0..ITEMS)
                .map(|i| Item {
                    hover: Tweener::new(if i == cursor { 1.0 } else { 0.0 }),
                    flash: Tweener::new(0.0),
                })
                .collect(),
            expression: 0,
        }
    }

    pub fn update(&mut self, delta: Ticks, shared: &mut Shared) -> Option<Handover> {
        let mut handover = None;
        if self.state == State::Picked && self.items[self.cursor].flash.is_idle() {
            if self.cursor == CULPRIT_ITEM {
                // a 10-frame crossfade to Beatrice's other face, then a 30-frame hold
                let before = Screen::Top(self.clone());
                self.expression = 1;
                self.hold.enqueue(0.0, frames(30.0, Easing::Linear));
                handover = Some(Handover {
                    snapshot_of: Some(before),
                    ..Handover::crossfade(10.0, None)
                });
            }
            self.state = State::Leaving;
        }
        if self.state == State::Leaving
            && handover.is_none()
            && !shared.transition_running
            && self.hold.is_idle()
        {
            let next = |screen| Some(Handover::crossfade(30.0, Some(screen)));
            handover = match self.cursor {
                0 => next(Screen::Replay(ReplayMenu::new())),
                1 => next(Screen::Rule(Rule::new())),
                2 => next(Screen::Check(Check::new(0))),
                3 => next(Screen::Check(Check::new(1))),
                4 => next(Screen::Hint(HintMenu::new(shared.param))),
                _ => {
                    shared.sounds.push(SOUND_EXECUTE);
                    Some(Handover::glass(60.0, Screen::Select(Select::new())))
                }
            };
            self.state = State::Done;
        }
        self.hold.update(delta);
        for item in &mut self.items {
            item.hover.update(delta);
            item.flash.update(delta);
        }
        handover
    }

    pub fn handle_input(&mut self, input: &MinigameInput, shared: &mut Shared) {
        if self.state != State::Idle {
            return;
        }
        let target = if input.pressed.up {
            Some((self.cursor + ITEMS - 1) % ITEMS)
        } else if input.pressed.down {
            Some((self.cursor + 1) % ITEMS)
        } else {
            None
        };
        if let Some(target) = target {
            shared.sounds.push(SOUND_MOVE);
            let (old, new) = (self.cursor, target);
            move_highlight(&mut self.items, old, new, |item| &mut item.hover);
            self.cursor = target;
        } else if input.confirm {
            shared.sounds.push(SOUND_DECIDE);
            minigame::flash(&mut self.items[self.cursor].flash);
            self.state = State::Picked;
        }
    }

    pub fn auto_pick(&mut self, shared: &mut Shared) {
        if self.state == State::Idle {
            let (old, new) = (self.cursor, CULPRIT_ITEM);
            if old != new {
                move_highlight(&mut self.items, old, new, |item| &mut item.hover);
            }
            self.cursor = CULPRIT_ITEM;
            self.handle_input(
                &MinigameInput {
                    confirm: true,
                    ..Default::default()
                },
                shared,
            );
        }
    }

    pub fn draw(&self, drawer: &mut Drawer, _shared: &Shared, background: bool) {
        if background {
            drawer.image("different_space_2d", SCREEN);
            for ((x, y, w, h), textures) in BEATRICE {
                drawer.image(textures[self.expression], rect(BEATRICE_X + x, y, w, h));
            }
            return;
        }
        let plate = drawer.texture("plate");
        let labels = drawer.texture("topmenu");
        for (i, item) in self.items.iter().enumerate() {
            let y = ITEM_Y + ITEM_STEP * i as f32;
            let bounds = rect(ITEM_X, y, ITEM_W, ITEM_H);
            let hover = item.hover.value();
            let [r, g, b] =
                std::array::from_fn(|c| PLATE_DIM[c] + (PLATE_LIT[c] - PLATE_DIM[c]) * hover);
            let v = (ITEM_STEP * i as f32 + 1.0) / PLATE_HEIGHT;
            drawer.sprite(Sprite {
                color: FloatColor4::from_rgba(r, g, b, 1.0),
                ..Sprite::new(plate, bounds).uv([0.0, v, 1.0, v + ITEM_H / PLATE_HEIGHT])
            });
            let v = i as f32 / ITEMS as f32;
            drawer.sprite(
                Sprite::new(
                    labels,
                    rect(
                        ITEM_X + LABEL_OFFSET.0,
                        y + LABEL_OFFSET.1,
                        LABEL_SIZE.0,
                        LABEL_SIZE.1,
                    ),
                )
                .uv([0.0, v, 1.0, v + 1.0 / ITEMS as f32]),
            );
            drawer.flat(bounds, white(item.flash.value()));
        }
    }
}
