//! Browsing the "purple" statements (`shin::MurdererLayer::Check`: constructor `0x7100093420`, update
//! `0x7100094aa0`, draw `0x7100095020`, keys `0x7100096740`), a viewer that never answers. Mode 0 is the family tree:
//! picking a living person shows a faint bust of them and their testimony. Mode 1 is a scrolling list of 23 pieces of
//! evidence under six chapter headers that stick to the top of the list.
//!
//! The testimony and evidence texts are strings inside the game's executable (`0x71001db528`, `0x71001db270`),
//! translated by the English patch's IPS; they are read from the player's own executable when one is found
//! (`asset::executable`) and the panel stays empty otherwise.

use glam::{Vec2, vec2};
use shin_core::layout::LayoutParams;
use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tweener},
};

use super::{
    Handover, SOUND_CANCEL, Screen, Shared, Target,
    draw::{self, Drawer, Rect, SCREEN, rect, white},
    hint_menu::{LEFT_US, LEFT_VS, draw_scroll_bar},
    move_highlight,
    rule::{back_hit, draw_back},
    select::draw_lines,
    top_menu::TopMenu,
    tree::{self, BUSTS, CONFIRM, NODES, PORTRAITS},
};
use crate::asset::executable;
use crate::layer::text_block::{OUTLINE, object_layout};
use crate::layer::user::minigame::{
    self, MinigameInput, SOUND_DECIDE, SOUND_MOVE, Sprite, contains, frames,
};

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
    /// The testimony or evidence shown, and how far it is scrolled
    text: String,
    text_scroll: f32,
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
            text: String::new(),
            text_scroll: 0.0,
        }
        .with_text_of(0)
    }

    fn with_text_of(mut self, item: usize) -> Self {
        self.set_text(item);
        self
    }

    /// `0x7100094480`: the testimony of the picked person, or the picked piece of evidence; scrolled to the top
    fn set_text(&mut self, item: usize) {
        let address = if self.mode == 0 {
            TESTIMONIES + 8 * NODES[self.items[item].id].id as u64
        } else {
            let entry = (self.items[item].rect[1] / ROW) as u64;
            EVIDENCE + EVIDENCE_ENTRY * entry + EVIDENCE_BODY
        };
        self.text = executable::text_at_pointer(address).unwrap_or_default();
        self.text_scroll = 0.0;
    }

    /// The text the screen shows (laid out by the layer)
    pub fn text(&self) -> &str {
        &self.text
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
        self.set_text(index);
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
        if !input.cancel && input.text_scroll != 0.0 {
            // the right stick scrolls the text, 32 px per (60 Hz) frame at full tilt
            let limit = (shared.text_height - VIEW).max(0.0);
            self.text_scroll =
                (self.text_scroll + input.text_scroll * 32.0 * input.frames).clamp(0.0, limit);
            return;
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
                let shown = header_shown(header, scroll);
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
        let text_height = drawer.text_height();
        if self.mode == 0 && text_height > VIEW {
            draw_scroll_hint(drawer);
        }
        // the text object is offset by -3 so its outline padding sits left of x 726 (`0x71001019f0`)
        drawer.clip_to(CLIP);
        drawer.draw_text(vec2(726.0 - OUTLINE, 180.0 - self.text_scroll));
        drawer.unclip();
        if text_height > VIEW {
            draw_scroll_bar(drawer, (1762.0, 180.0), self.text_scroll, text_height);
        }
        draw_back(drawer, &self.back_flash);
    }
}

/// The text object's layout (`0x7100094480`, built by `TextObject::layout` `0x7100101360`): 1024 px wide (1030 less
/// the 3 px outline on each side), size 32, no padding, and room for rubi above every line (the rubi size 0 becomes
/// 0.4 x the text size), which makes the lines 48 px apart; drawn in system.fnt
pub fn text_layout() -> LayoutParams {
    object_layout(1030.0, 32.0)
}

/// The testimonies by person (`0x71001db528`, 17 pointers) and the evidence list (`0x71001db270`, 29 entries of
/// (index, flags, title, body))
const TESTIMONIES: u64 = 0x71_001d_b528;
const EVIDENCE: u64 = 0x71_001d_b270;
const EVIDENCE_ENTRY: u64 = 0x18;
const EVIDENCE_BODY: u64 = 0x10;

/// The button hint "Scroll" with the right stick glyph, right-aligned at (1790, 920) (`0x71000964c0`, hint 3):
/// `help_chars` row at 144 of 336, 176 px wide, and glyph 9 of `key` (8 x 2 glyphs of 48 px) 48 px to its left
fn draw_scroll_hint(drawer: &mut Drawer) {
    const RIGHT: f32 = 1790.0;
    const TOP: f32 = 920.0;
    const WIDTH: f32 = 176.0;
    const GLYPH: u8 = 9;
    let label = drawer.texture("help_chars");
    let v = 144.0 / 336.0;
    drawer.sprite(
        Sprite::new(label, [RIGHT - WIDTH, TOP, RIGHT, TOP + 48.0]).uv([0.0, v, WIDTH / 320.0, v + 1.0 / 7.0]),
    );
    let key = drawer.texture("key");
    let x = RIGHT - WIDTH + 2.0 - 48.0;
    drawer.key_glyph(key, GLYPH, [x, TOP, x + 48.0, TOP + 48.0]);
}

/// Where a header is drawn in the list: it sticks to the top until the next one pushes it up
fn header_shown(header: &Header, scroll: f32) -> f32 {
    let d = header.y - scroll;
    if d >= 0.0 { d } else { (header.next_y - scroll - ROW).min(0.0) }
}

/// Pointer slots 10-17 (`0x7100097020`-`0x7100097880`); dragging the list and the text is not done
impl Check {
    pub fn pointer_hit(&self, position: Vec2) -> Option<Target> {
        if self.state != State::Idle {
            return None;
        }
        if back_hit(position) {
            return Some(Target::Back);
        }
        let scroll = self.scroll.value();
        if let Some(header) = self.headers.iter().position(|header| {
            let y = header_shown(header, scroll) + 180.0;
            contains([146.0, y, 666.0, y + ROW], position)
        }) {
            return Some(Target::Header(header));
        }
        if !contains([146.0, 180.0, 666.0, 900.0], position) {
            return None;
        }
        // the current item is skipped (`0x7100096c50`)
        let local = if self.mode == 0 {
            position - vec2(NODE_ORIGIN.0, NODE_ORIGIN.1)
        } else {
            position - vec2(146.0, 180.0 - scroll)
        };
        (0..self.items.len())
            .filter(|&i| i != self.cursor)
            .filter(|&i| self.mode != 0 || !NODES[self.items[i].id].victim)
            .find(|&i| contains(self.items[i].rect, local))
            .map(Target::Item)
    }

    fn target_flash(&mut self, target: Target) -> &mut Tweener {
        match target {
            Target::Item(item) => &mut self.items[item].flash,
            Target::Header(header) => &mut self.headers[header].flash,
            Target::Back => &mut self.back_flash,
        }
    }

    pub fn pointer_press(&mut self, target: Target) {
        minigame::raise(self.target_flash(target));
        if let Target::Item(item) = target {
            self.select(item);
        }
    }

    pub fn pointer_release(&mut self, target: Target, shared: &mut Shared) {
        shared.sounds.push(SOUND_DECIDE);
        minigame::flash(self.target_flash(target));
        match target {
            Target::Item(item) => self.select(item),
            Target::Header(header) => {
                let goal = self.headers[header].y.clamp(0.0, self.height() - VIEW);
                self.scroll.enqueue_now(goal, frames(4.0, Easing::Linear));
            }
            Target::Back => self.state = State::Leaving,
        }
    }

    pub fn pointer_cancel(&mut self, target: Target) {
        minigame::lower(self.target_flash(target));
    }
}
