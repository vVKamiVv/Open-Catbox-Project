//! Browsing the "purple" statements (`shin::MurdererLayer::Check`: constructor `0x7100093420`, update
//! `0x7100094aa0`, draw `0x7100095020`, keys `0x7100096740`), a viewer that never answers. Mode 0 is the family tree:
//! picking a living person shows a faint bust of them and their testimony. Mode 1 is a scrolling list of 23 pieces of
//! evidence under six chapter headers that stick to the top of the list.
//!
//! shortcut: the testimony and evidence texts (`0x71001db528`, `0x71001db270`) are strings inside the game's
//! executable, which the port does not have, so the text panel stays empty; everything else is drawn. Upgrade if the
//! port ever reads the player's own executable.

use glam::vec2;
use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tweener},
};

use super::{
    Handover, SOUND_CANCEL, Screen, Shared,
    draw::{self, Drawer, Rect, SCREEN, rect, white},
    hint_menu::{LEFT_US, LEFT_VS, draw_scroll_bar},
    move_highlight,
    rule::draw_back,
    select::draw_lines,
    top_menu::TopMenu,
    tree::{self, BUSTS, CONFIRM, NODES, PORTRAITS},
};
use crate::layer::user::minigame::{self, MinigameInput, SOUND_MOVE, Sprite, frames};

const ROW: f32 = 90.0;
const VIEW: f32 = 720.0;
/// The list area; mode 1's rows also stay below the sticky header band
const CLIP: Rect = [146.0, 180.0, 1774.0, 900.0];
const ROW_CLIP: Rect = [146.0, 270.0, 1774.0, 900.0];
/// Mode 1's list: which entries (90 px apart) are chapter headers; the others are the 23 clues in order
const HEADER_ENTRIES: [usize; 6] = [0, 6, 14, 19, 23, 26];
const LIST_ENTRIES: usize = 29;
const CLUES: f32 = 23.0;
const CHAPTERS: f32 = 6.0;
/// The tree in this screen: 32 x 36 cells from (166, 198); nodes 96 x 108
const LINE_ORIGIN: (f32, f32) = (166.0, 198.0);
const NODE_ORIGIN: (f32, f32) = (146.0, 180.0);
/// The victims' portraits are tinted (0xffff0f0f)
const VICTIM_COLOR: FloatColor4 = FloatColor4::from_rgba(1.0, 15.0 / 255.0, 15.0 / 255.0, 1.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Leaving,
    Done,
}

#[derive(Clone)]
struct Item {
    /// Mode 0: the person's index in the tree; mode 1: the clue's row in `chap_text`
    id: usize,
    rect: Rect,
    highlight: Tweener,
    flash: Tweener,
}

#[derive(Clone)]
struct Header {
    row: f32,
    y: f32,
    next_y: f32,
    flash: Tweener,
}

#[derive(Clone)]
pub struct Check {
    mode: usize,
    state: State,
    cursor: usize,
    items: Vec<Item>,
    headers: Vec<Header>,
    scroll: Tweener,
    back_flash: Tweener,
    /// The person whose bust is shown (mode 0)
    bust: Option<&'static str>,
}

impl Check {
    pub fn new(mode: usize) -> Self {
        let item = |id, rect| Item {
            id,
            rect,
            highlight: Tweener::new(0.0),
            flash: Tweener::new(0.0),
        };
        let mut items = Vec::new();
        let mut headers: Vec<Header> = Vec::new();
        if mode == 0 {
            for (i, node) in NODES
                .iter()
                .enumerate()
                .filter(|(_, node)| node.id != CONFIRM)
            {
                let cell = tree::cell(node) * vec2(32.0, 36.0) + vec2(20.0 - 32.0, 18.0 - 36.0);
                items.push(item(i, rect(cell.x, cell.y, 96.0, 108.0)));
            }
        } else {
            for entry in 0..LIST_ENTRIES {
                let y = entry as f32 * ROW;
                if HEADER_ENTRIES.contains(&entry) {
                    if let Some(last) = headers.last_mut() {
                        last.next_y = y;
                    }
                    headers.push(Header {
                        row: headers.len() as f32,
                        y,
                        next_y: y,
                        flash: Tweener::new(0.0),
                    });
                } else {
                    items.push(item(items.len(), rect(20.0, y, 480.0, ROW)));
                }
            }
        }
        items[0].highlight.fast_forward_to(1.0);
        Self {
            mode,
            state: State::Idle,
            cursor: 0,
            bust: if mode == 0 {
                BUSTS[NODES[0].id as usize]
            } else {
                None
            },
            items,
            headers,
            scroll: Tweener::new(0.0),
            back_flash: Tweener::new(0.0),
        }
    }

    fn height(&self) -> f32 {
        if self.mode == 0 {
            VIEW
        } else {
            LIST_ENTRIES as f32 * ROW
        }
    }

    /// `0x7100096a80`
    fn select(&mut self, index: usize) {
        if index != self.cursor {
            move_highlight(&mut self.items, self.cursor, index, |item| {
                &mut item.highlight
            });
            self.cursor = index;
        }
        if self.mode == 0 {
            // victims have no bust, and picking them keeps the previous one
            if let Some(bust) = BUSTS[NODES[self.items[index].id].id as usize] {
                self.bust = Some(bust);
            }
            return;
        }
        let [_, y, _, bottom] = self.items[index].rect;
        let scroll = self.scroll.value();
        let goal = if y - scroll < ROW {
            (y - ROW).max(0.0)
        } else if bottom - scroll > VIEW {
            (self.height() - VIEW).min(bottom - VIEW)
        } else {
            scroll
        };
        self.scroll.enqueue_now(goal, frames(4.0, Easing::Linear));
    }

    pub fn update(&mut self, delta: Ticks, _shared: &mut Shared) -> Option<Handover> {
        let mut handover = None;
        if self.state == State::Leaving && self.back_flash.is_idle() {
            handover = Some(Handover::crossfade(
                30.0,
                Some(Screen::Top(TopMenu::new(self.mode + 2))),
            ));
            self.state = State::Done;
        }
        self.scroll.update(delta);
        for item in &mut self.items {
            item.highlight.update(delta);
            item.flash.update(delta);
        }
        for header in &mut self.headers {
            header.flash.update(delta);
        }
        self.back_flash.update(delta);
        handover
    }

    pub fn handle_input(&mut self, input: &MinigameInput, shared: &mut Shared) {
        if self.state != State::Idle || !self.scroll.is_idle() {
            return;
        }
        let pressed = input.pressed;
        if self.mode == 0 {
            let dx = if pressed.left {
                -1.0
            } else if pressed.right {
                1.0
            } else {
                0.0
            };
            let dy = if pressed.up {
                -1.0
            } else if pressed.down {
                1.0
            } else {
                0.0
            };
            if dx != 0.0 || dy != 0.0 {
                let cells = self
                    .items
                    .iter()
                    .map(|item| tree::cell(&NODES[item.id]))
                    .collect::<Vec<_>>();
                let candidates = (0..self.items.len())
                    .filter(|&i| i != self.cursor && !NODES[self.items[i].id].victim);
                let target = cells[self.cursor] + vec2(dx, dy);
                if let Some(found) = tree::pick(&cells, candidates, target, vec2(dx, dy)) {
                    shared.sounds.push(SOUND_MOVE);
                    self.select(found);
                }
                return;
            }
        } else {
            let last = self.items.len() - 1;
            let target = if pressed.up {
                Some(self.cursor.saturating_sub(1))
            } else if pressed.down {
                Some((self.cursor + 1).min(last))
            } else {
                None
            };
            if let Some(target) = target.filter(|&target| target != self.cursor) {
                shared.sounds.push(SOUND_MOVE);
                self.select(target);
                return;
            }
        }
        if input.cancel {
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
        drawer.nine_slice(
            "left",
            [130.0, 466.0, 666.0, 698.0],
            [164.0, 564.0, 900.0, 932.0],
            LEFT_US,
            LEFT_VS,
        );
        if self.mode == 0 {
            draw_lines(
                drawer,
                vec2(LINE_ORIGIN.0, LINE_ORIGIN.1),
                vec2(32.0, 36.0),
                (0.0, 0.0),
            );
            for item in &self.items {
                let [x0, y0, x1, y1] = item.rect;
                let bounds = [
                    x0 + NODE_ORIGIN.0,
                    y0 + NODE_ORIGIN.1,
                    x1 + NODE_ORIGIN.0,
                    y1 + NODE_ORIGIN.1,
                ];
                let node = &NODES[item.id];
                drawer.image("non", bounds);
                let portrait = drawer.texture(PORTRAITS[node.id as usize]);
                drawer.sprite(Sprite {
                    color: if node.victim {
                        VICTIM_COLOR
                    } else {
                        FloatColor4::WHITE
                    },
                    ..Sprite::new(portrait, bounds)
                });
                drawer.flat(bounds, white(item.highlight.value() * 0.5));
                drawer.flat(bounds, white(item.flash.value()));
            }
        } else {
            let scroll = self.scroll.value();
            let menu = drawer.texture("chap_menu");
            let text = drawer.texture("chap_text");
            for item in &self.items {
                let y = item.rect[1] - scroll + 180.0;
                let glow = item.highlight.value() * 0.5;
                let flash = item.flash.value();
                let v = item.id as f32 / CLUES;
                for sprite in [
                    Sprite::new(menu, [166.0, y + 1.0, 646.0, y + 89.0]),
                    Sprite::new(text, [166.0, y + 13.0, 646.0, y + 77.0]).uv([
                        0.0,
                        v,
                        1.0,
                        v + 1.0 / CLUES,
                    ]),
                ] {
                    if let Some(sprite) = draw::clip(sprite.glow(glow).flash(flash), ROW_CLIP) {
                        drawer.sprite(sprite);
                    }
                }
            }
            let chapter = drawer.texture("chapter");
            for header in &self.headers {
                // a header sticks to the top until the next one pushes it up
                let d = header.y - scroll;
                let shown = if d >= 0.0 {
                    d
                } else {
                    (header.next_y - scroll - ROW).min(0.0)
                };
                let v = header.row / CHAPTERS;
                let sprite = Sprite::new(chapter, [166.0, shown + 181.0, 646.0, shown + 269.0])
                    .uv([0.0, v, 1.0, v + 1.0 / CHAPTERS])
                    .flash(header.flash.value());
                if let Some(sprite) = draw::clip(sprite, CLIP) {
                    drawer.sprite(sprite);
                }
            }
            draw_scroll_bar(drawer, (654.0, 180.0), scroll, self.height());
        }
        drawer.nine_slice(
            "right2",
            [690.0, 706.0, 1774.0, 1806.0],
            [164.0, 180.0, 900.0, 932.0],
            [0.0, 0.25, 0.5, 1.0],
            [0.0, 0.25, 0.5, 1.0],
        );
        if self.mode == 0 {
            if let Some(bust) = self.bust.and_then(|name| drawer.bust(name)) {
                drawer.sprite(Sprite::new(bust, [1088.0, 72.0, 2048.0, 1152.0]).alpha(0.25));
            }
        } else {
            let hana = drawer.texture("hana");
            drawer.sprite(Sprite {
                color: FloatColor4::from_rgba(0.58, 0.36, 0.0, 0.25),
                ..Sprite::new(hana, [1160.0, 272.0, 1920.0, 1152.0])
            });
        }
        draw_back(drawer, &self.back_flash);
    }
}
