//! "Re-read the story" (`shin::MurdererLayer::ReplayMenu`: constructor `0x710008dad0`, update `0x710008e450`, draw
//! `0x710008e670`, keys `0x710008efa0`): six episode thumbnails in a 3 x 2 grid. Picking one zooms its thumbnail to
//! full screen and answers with its index (0-5).

use glam::{Vec2, vec2};
use shin_core::time::{Easing, Ticks, Tweener};

use super::{
    Handover, SOUND_CANCEL, Screen, Shared, Target,
    draw::{Drawer, SCREEN, lerp2, rect, white},
    move_highlight,
    rule::{back_hit, draw_back},
    top_menu::TopMenu,
};
use crate::layer::user::minigame::{
    self, MinigameInput, SOUND_DECIDE, SOUND_MOVE, Sprite, contains, frames,
};

const ITEMS: usize = 6;
const COLUMNS: usize = 3;
const ORIGIN: (f32, f32) = (124.0, 193.0);
const STEP: (f32, f32) = (564.0, 326.0);
const SIZE: (f32, f32) = (544.0, 306.0);
/// Small thumbnails (`0x71001db760`) and the full-screen pictures they zoom into (`0x71001db730`)
const SMALL: [&str; ITEMS] = [
    "thumb_1_s",
    "thumb_2_s",
    "thumb_4_s",
    "thumb_56_s",
    "thumb_7_s",
    "thumb_8_s",
];
const LARGE: [&str; ITEMS] = [
    "thumb_1", "thumb_2", "thumb_4", "thumb_56", "thumb_7", "thumb_8",
];
/// The top menu item this screen belongs to
const MENU_ITEM: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// Waiting for the pick flash
    Picked,
    /// The thumbnail zooms; the answer is given when it has
    Zooming,
    Answered,
    /// Waiting for the back button's flash
    Leaving,
    Done,
}

#[derive(Clone)]
struct Item {
    highlight: Tweener,
    flash: Tweener,
}

#[derive(Clone)]
pub struct ReplayMenu {
    cursor: usize,
    state: State,
    back_flash: Tweener,
    items: Vec<Item>,
    zoom: Tweener,
}

fn item_rect(i: usize) -> [f32; 4] {
    rect(
        ORIGIN.0 + (i % COLUMNS) as f32 * STEP.0,
        ORIGIN.1 + (i / COLUMNS) as f32 * STEP.1,
        SIZE.0,
        SIZE.1,
    )
}

impl ReplayMenu {
    pub fn new() -> Self {
        Self {
            cursor: 0,
            state: State::Idle,
            back_flash: Tweener::new(0.0),
            items: (0..ITEMS)
                .map(|i| Item {
                    highlight: Tweener::new(if i == 0 { 1.0 } else { 0.0 }),
                    flash: Tweener::new(0.0),
                })
                .collect(),
            zoom: Tweener::new(0.0),
        }
    }

    pub fn update(&mut self, delta: Ticks, shared: &mut Shared) -> Option<Handover> {
        let mut handover = None;
        if self.state == State::Picked && self.items[self.cursor].flash.is_idle() {
            self.items[self.cursor]
                .highlight
                .enqueue(0.0, frames(30.0, Easing::Linear));
            // a 30-frame hold, then the zoom over 60 frames
            self.zoom.enqueue(0.0, frames(30.0, Easing::Linear));
            self.zoom.enqueue(1.0, frames(60.0, Easing::SineOut));
            self.state = State::Zooming;
        }
        if self.state == State::Zooming && self.zoom.is_idle() {
            shared.result = Some(self.cursor as i32);
            self.state = State::Answered;
        }
        if self.state == State::Leaving && self.back_flash.is_idle() {
            handover = Some(Handover::crossfade(
                30.0,
                Some(Screen::Top(TopMenu::new(MENU_ITEM))),
            ));
            self.state = State::Done;
        }
        self.back_flash.update(delta);
        self.zoom.update(delta);
        for item in &mut self.items {
            item.flash.update(delta);
            item.highlight.update(delta);
        }
        handover
    }

    pub fn handle_input(&mut self, input: &MinigameInput, shared: &mut Shared) {
        if self.state != State::Idle {
            return;
        }
        let (mut row, mut col) = (self.cursor / COLUMNS, self.cursor % COLUMNS);
        let pressed = input.pressed;
        // everything wraps; up wins over down, left over right
        if pressed.down {
            row = if self.cursor < COLUMNS { row + 1 } else { 0 };
        }
        if pressed.up {
            row = if self.cursor < COLUMNS { 1 } else { row - 1 };
        }
        if pressed.right {
            col = if col < COLUMNS - 1 { col + 1 } else { 0 };
        }
        if pressed.left {
            col = if col > 0 { col - 1 } else { COLUMNS - 1 };
        }
        let target = col + row * COLUMNS;
        if target != self.cursor {
            shared.sounds.push(SOUND_MOVE);
            move_highlight(&mut self.items, self.cursor, target, |item| {
                &mut item.highlight
            });
            self.cursor = target;
        } else if input.confirm {
            shared.sounds.push(SOUND_DECIDE);
            minigame::flash(&mut self.items[self.cursor].flash);
            self.state = State::Picked;
        } else if input.cancel {
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
            for (i, item) in self.items.iter().enumerate() {
                let bounds = item_rect(i);
                let texture = drawer.texture(SMALL[i]);
                drawer.sprite(Sprite::new(texture, bounds).flash(item.flash.value()));
                drawer.flat(bounds, white(item.highlight.value() * 0.5));
            }
            return;
        }
        draw_back(drawer, &self.back_flash);
        if matches!(self.state, State::Zooming | State::Answered) {
            // the picked thumbnail grows to cover the screen while crossfading into the large picture
            // (program 6, `mix(small, large, z)`: the same as the large one drawn over the small one at alpha z,
            // as both are opaque)
            let z = self.zoom.value();
            let [x0, y0, x1, y1] = item_rect(self.cursor);
            let centre = lerp2(
                vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                vec2(960.0, 540.0),
                z,
            );
            let half = vec2(SIZE.0 + 1376.0 * z, SIZE.1 + 774.0 * z) / 2.0;
            let bounds = [
                centre.x - half.x,
                centre.y - half.y,
                centre.x + half.x,
                centre.y + half.y,
            ];
            let small = drawer.texture(SMALL[self.cursor]);
            let large = drawer.texture(LARGE[self.cursor]);
            drawer.sprite(Sprite::new(small, bounds));
            drawer.sprite(Sprite::new(large, bounds).alpha(z));
        }
    }
}

/// Pointer slots 10-17 (`0x710008f2a0`-`0x710008f820`)
impl ReplayMenu {
    pub fn pointer_hit(&self, position: Vec2) -> Option<Target> {
        if self.state != State::Idle {
            return None;
        }
        if back_hit(position) {
            return Some(Target::Back);
        }
        (0..ITEMS)
            .find(|&i| contains(item_rect(i), position))
            .map(Target::Item)
    }

    fn point_at(&mut self, item: usize) {
        if item != self.cursor {
            move_highlight(&mut self.items, self.cursor, item, |item| &mut item.highlight);
            self.cursor = item;
        }
    }

    pub fn pointer_press(&mut self, target: Target) {
        match target {
            Target::Item(item) => {
                minigame::raise(&mut self.items[item].flash);
                self.point_at(item);
            }
            _ => minigame::raise(&mut self.back_flash),
        }
    }

    pub fn pointer_release(&mut self, target: Target, shared: &mut Shared) {
        shared.sounds.push(SOUND_DECIDE);
        match target {
            Target::Item(item) => {
                self.point_at(item);
                minigame::flash(&mut self.items[item].flash);
                self.state = State::Picked;
            }
            _ => {
                minigame::flash(&mut self.back_flash);
                self.state = State::Leaving;
            }
        }
    }

    pub fn pointer_cancel(&mut self, target: Target) {
        match target {
            Target::Item(item) => minigame::lower(&mut self.items[item].flash),
            _ => minigame::lower(&mut self.back_flash),
        }
    }
}
