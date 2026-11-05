//! "View hints" (`shin::MurdererLayer::HintMenu`: constructor `0x71000906b0`, update `0x7100091700`, draw
//! `0x7100091a80`, keys `0x7100092770`): a scrolling list of the hints unlocked so far. The layer's parameter `p`
//! (the attempt count) unlocks hints 0..p; the newest one shows as "??????????". Picking hint i answers i + 6.

use glam::Vec2;
use shin_core::time::{Easing, Ticks, Tweener};

use super::{
    Handover, SOUND_CANCEL, Screen, Shared, Target,
    draw::{self, Drawer, Rect, SCREEN},
    move_highlight,
    rule::{back_hit, draw_back},
    top_menu::TopMenu,
};
use crate::layer::user::minigame::{
    self, MinigameInput, SOUND_DECIDE, SOUND_MOVE, Sprite, contains, frames,
};

const MAX_HINTS: i32 = 21;
const ROW: f32 = 90.0;
const VIEW: f32 = 720.0;
/// The list is drawn inside this rect (the original masks it with the stencil)
const LIST_CLIP: Rect = [280.0, 180.0, 1640.0, 900.0];
const LIST_TOP: f32 = 180.0;
/// `hint_text` holds 22 rows: "??????????", then the 21 hints
const TEXT_ROWS: f32 = 22.0;
/// The frame `left` (384 x 448) cut at these screen and texture edges
const FRAME_XS: [f32; 4] = [264.0, 600.0, 1640.0, 1672.0];
const FRAME_YS: [f32; 4] = [164.0, 564.0, 900.0, 932.0];
pub const LEFT_US: [f32; 4] = [0.0, 0.875, 0.916_667, 1.0];
pub const LEFT_VS: [f32; 4] = [0.0, 0.892_857, 0.928_571, 1.0];
/// The scroll bar: at x 1628 from y 180, 720 long
const BAR_ORIGIN: (f32, f32) = (1628.0, 180.0);
/// The top menu item this screen belongs to
const MENU_ITEM: usize = 4;
/// The answer of hint 0
const FIRST_ANSWER: i32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// Waiting for the pick flash, then a 60-frame timer
    Picked,
    Waiting,
    Answered,
    Leaving,
    Done,
}

#[derive(Clone)]
struct Item {
    text_row: f32,
    highlight: Tweener,
    flash: Tweener,
}

#[derive(Clone)]
pub struct HintMenu {
    state: State,
    cursor: usize,
    items: Vec<Item>,
    delay: Tweener,
    scroll: Tweener,
    back_flash: Tweener,
}

impl HintMenu {
    pub fn new(param: i32) -> Self {
        let count = (param + 1).clamp(0, MAX_HINTS) as usize;
        let cursor = if (0..MAX_HINTS).contains(&param) {
            param as usize
        } else {
            0
        };
        let items = (0..count)
            .map(|i| Item {
                // the newest hint is still hidden
                text_row: if i as i32 == param {
                    0.0
                } else {
                    i as f32 + 1.0
                },
                highlight: Tweener::new(if i == cursor { 1.0 } else { 0.0 }),
                flash: Tweener::new(0.0),
            })
            .collect::<Vec<_>>();
        let height = count as f32 * ROW;
        // the cursor row ends the view when it would otherwise be below it
        let bottom = cursor as f32 * ROW + ROW;
        let scroll = if bottom > VIEW {
            (bottom - VIEW).min(height - VIEW)
        } else {
            0.0
        };
        Self {
            state: State::Idle,
            cursor,
            items,
            delay: Tweener::new(0.0),
            scroll: Tweener::new(scroll),
            back_flash: Tweener::new(0.0),
        }
    }

    fn height(&self) -> f32 {
        self.items.len() as f32 * ROW
    }

    /// `0x7100092a60`: moves the cursor and scrolls its row into view over 4 frames
    fn set_cursor(&mut self, target: usize) {
        if target == self.cursor {
            return;
        }
        move_highlight(&mut self.items, self.cursor, target, |item| {
            &mut item.highlight
        });
        self.cursor = target;
        let row_top = target as f32 * ROW;
        let scroll = self.scroll.value();
        let goal = if row_top < scroll {
            row_top
        } else if row_top - scroll + ROW > VIEW {
            (row_top + ROW - VIEW).min(self.height() - VIEW)
        } else {
            scroll
        };
        self.scroll.enqueue_now(goal, frames(4.0, Easing::Linear));
    }

    pub fn update(&mut self, delta: Ticks, shared: &mut Shared) -> Option<Handover> {
        let mut handover = None;
        if self.state == State::Picked && self.items[self.cursor].flash.is_idle() {
            self.state = State::Waiting;
        }
        if self.state == State::Waiting && self.delay.is_idle() {
            shared.result = Some(self.cursor as i32 + FIRST_ANSWER);
            self.state = State::Answered;
        }
        if self.state == State::Leaving && self.back_flash.is_idle() {
            handover = Some(Handover::crossfade(
                30.0,
                Some(Screen::Top(TopMenu::new(MENU_ITEM))),
            ));
            self.state = State::Done;
        }
        self.delay.update(delta);
        self.scroll.update(delta);
        for item in &mut self.items {
            item.highlight.update(delta);
            item.flash.update(delta);
        }
        self.back_flash.update(delta);
        handover
    }

    pub fn handle_input(&mut self, input: &MinigameInput, shared: &mut Shared) {
        if self.state != State::Idle || !self.scroll.is_idle() || self.items.is_empty() {
            return;
        }
        let last = self.items.len() - 1;
        let target = if input.pressed.up {
            Some(self.cursor.saturating_sub(1))
        } else if input.pressed.down {
            Some((self.cursor + 1).min(last))
        } else {
            None
        };
        if let Some(target) = target.filter(|&target| target != self.cursor) {
            shared.sounds.push(SOUND_MOVE);
            self.set_cursor(target);
        } else if input.confirm {
            shared.sounds.push(SOUND_DECIDE);
            minigame::flash(&mut self.items[self.cursor].flash);
            // the answer comes 60 frames after the confirm
            self.delay.enqueue(0.0, frames(60.0, Easing::Linear));
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
            return;
        }
        drawer.nine_slice("left", FRAME_XS, FRAME_YS, LEFT_US, LEFT_VS);
        let menu = drawer.texture("hint_menu");
        let text = drawer.texture("hint_text");
        let scroll = self.scroll.value();
        for (i, item) in self.items.iter().enumerate() {
            let top = i as f32 * ROW - scroll + LIST_TOP;
            let glow = item.highlight.value() * 0.5;
            let flash = item.flash.value();
            let v = item.text_row / TEXT_ROWS;
            for sprite in [
                Sprite::new(menu, [320.0, top + 1.0, 1600.0, top + 89.0]),
                Sprite::new(text, [352.0, top + 13.0, 1568.0, top + 77.0]).uv([
                    0.0,
                    v,
                    1.0,
                    v + 1.0 / TEXT_ROWS,
                ]),
            ] {
                if let Some(sprite) = draw::clip(sprite.glow(glow).flash(flash), LIST_CLIP) {
                    drawer.sprite(sprite);
                }
            }
        }
        let height = self.height();
        if height > VIEW {
            draw_scroll_bar(drawer, BAR_ORIGIN, scroll, height);
        }
        draw_back(drawer, &self.back_flash);
    }
}

/// The engine's vertical scroll bar (`0x71000ef580`), 720 long, for a view of 720 over `height`: a track and a
/// thumb from `scroll` (`systex.txa`'s `scroll`, 64 x 64: track on the left half, thumb on the right, each in
/// three 8 / middle / 8 px caps)
pub fn draw_scroll_bar(drawer: &mut Drawer, (x, y): (f32, f32), scroll: f32, height: f32) {
    let scroll = scroll.trunc();
    if scroll < 0.0 || scroll + VIEW > height {
        return;
    }
    let texture = drawer.system_texture("scroll");
    let length = VIEW - 16.0;
    // the bar's own coordinates start 8 px down so its caps fit inside the 720 px
    let top = y + 8.0;
    let piece = |drawer: &mut Drawer, u0: f32, y0: f32, y1: f32, v0: f32| {
        drawer.sprite(Sprite::new(texture, [x, top + y0, x + 16.0, top + y1]).uv([
            u0,
            v0,
            u0 + 0.5,
            v0 + 0.25,
        ]));
    };
    for (u0, start, end) in [
        (0.0, 0.0, length),
        (
            0.5,
            length * scroll / height,
            length * (scroll + VIEW) / height,
        ),
    ] {
        piece(drawer, u0, start - 8.0, start, 0.0);
        piece(drawer, u0, start, end, 0.25);
        piece(drawer, u0, end, end + 8.0, 0.5);
    }
}

/// Pointer slots 10-17 (`0x7100092d20`-`0x7100093380`); dragging and flinging the list are not done
impl HintMenu {
    pub fn pointer_hit(&self, position: Vec2) -> Option<Target> {
        if self.state != State::Idle {
            return None;
        }
        if back_hit(position) {
            return Some(Target::Back);
        }
        if !contains(LIST_CLIP, position) {
            return None;
        }
        let scroll = self.scroll.value();
        (0..self.items.len())
            .find(|&i| {
                let top = i as f32 * ROW + LIST_TOP - scroll;
                contains([320.0, top, 1600.0, top + ROW], position)
            })
            .map(Target::Item)
    }

    pub fn pointer_press(&mut self, target: Target) {
        match target {
            Target::Item(item) => {
                minigame::raise(&mut self.items[item].flash);
                self.set_cursor(item);
            }
            _ => minigame::raise(&mut self.back_flash),
        }
    }

    pub fn pointer_release(&mut self, target: Target, shared: &mut Shared) {
        shared.sounds.push(SOUND_DECIDE);
        match target {
            Target::Item(item) => {
                self.set_cursor(item);
                minigame::flash(&mut self.items[item].flash);
                self.delay.enqueue(0.0, frames(60.0, Easing::Linear));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocked_hints_follow_the_attempt_count() {
        let menu = HintMenu::new(3);
        assert_eq!(menu.items.len(), 4);
        assert_eq!(menu.cursor, 3);
        assert_eq!(menu.items[3].text_row, 0.0);
        assert_eq!(menu.items[0].text_row, 1.0);
        // the 11th row (index 10) would end at 990: scrolled so it ends the 720 px view
        assert_eq!(HintMenu::new(10).scroll.value(), 270.0);
        assert_eq!(HintMenu::new(30).items.len(), 21);
    }
}
