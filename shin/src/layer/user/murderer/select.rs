//! Naming the culprits (`shin::MurdererLayer::Select`: constructor `0x7100097930`, update `0x7100098660`, draw
//! `0x7100098970`, keys `0x7100099cf0`): the family tree with every person; confirm toggles a person as a culprit,
//! and "Confirm" submits. The answer is right only when exactly Rudolf, Kyrie and Battler are marked: it is 27, and
//! a wrong answer is 28 + the attempt count (at most 20).

use glam::{Vec2, vec2};
use shin_core::{
    primitives::color::FloatColor4,
    time::{Easing, Ticks, Tweener},
};

use super::{
    Handover, SOUND_CANCEL, Screen, Shared,
    draw::{Drawer, SCREEN, rect, white},
    hint_menu::{LEFT_US, LEFT_VS},
    move_highlight,
    rule::draw_back,
    top_menu::TopMenu,
    tree::{self, CONFIRM, LINES, NODES, PORTRAITS},
};
use crate::layer::user::minigame::{self, MinigameInput, SOUND_DECIDE, SOUND_MOVE, Sprite, frames};

const SOUND_ATTACK: &str = "attack";
const SOUND_LAUGH: &str = "ahaha";
const CORRECT: i32 = 27;
const FIRST_WRONG: i32 = 28;
/// The top menu item this screen belongs to
const MENU_ITEM: usize = 5;

/// The tree in this screen: 28 x 30 cells; nodes 88 x 99 (Confirm 420 x 88), drawn from (320, 200)
const CELL: Vec2 = vec2(28.0, 30.0);
const ORIGIN: Vec2 = vec2(320.0, 200.0);
/// Battler on the right, in three poses (`0x71001dbaa0`): (x, y, w, h) from x 800 (`0x710022d030`)
const BATTLER_X: f32 = 800.0;
const BATTLER: [((f32, f32, f32, f32), [Option<&str>; 3]); 4] = [
    (
        (128.0, 384.0, 812.0, 696.0),
        [Some("But_BodyA"), Some("But_BodyB"), Some("But_BodyB")],
    ),
    (
        (64.0, 272.0, 160.0, 112.0),
        [None, Some("But_FingerB1"), Some("But_FingerB1")],
    ),
    (
        (0.0, 384.0, 128.0, 160.0),
        [None, Some("But_FingerB2"), Some("But_FingerB2")],
    ),
    (
        (382.0, 0.0, 384.0, 384.0),
        [
            Some("But_Majime3"),
            Some("But_FutekiB2"),
            Some("But_KomaruB1"),
        ],
    ),
];
const FRAME_XS: [f32; 4] = [304.0, 640.0, 780.0, 812.0];
const FRAME_YS: [f32; 4] = [184.0, 584.0, 926.0, 958.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// Waiting for Confirm's flash
    Submitted,
    /// The white flash and a 60-frame wait
    Revealing,
    /// (wrong answers) Battler's troubled face, another 60 frames
    Reacting,
    Answered,
    Leaving,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pose {
    Neutral,
    Pointing,
    Troubled,
}

#[derive(Clone)]
struct Item {
    checked: bool,
    highlight: Tweener,
    flash: Tweener,
}

#[derive(Clone)]
pub struct Select {
    state: State,
    cursor: usize,
    correct: bool,
    timer: Tweener,
    items: Vec<Item>,
    pose: Pose,
    white: Tweener,
    back_flash: Tweener,
}

pub fn node_rect(index: usize) -> [f32; 4] {
    let node = &NODES[index];
    // the cell's centre, then the box around it
    let centre = tree::cell(node) * CELL + vec2(20.0, 18.0);
    let (offset, size) = if node.id == CONFIRM {
        (vec2(-196.0, -29.0), vec2(420.0, 88.0))
    } else {
        (vec2(-30.0, -34.5), vec2(88.0, 99.0))
    };
    let corner = ORIGIN + centre + offset;
    rect(corner.x, corner.y, size.x, size.y)
}

impl Select {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            cursor: 0,
            correct: false,
            timer: Tweener::new(0.0),
            items: (0..NODES.len())
                .map(|i| Item {
                    checked: false,
                    highlight: Tweener::new(if i == 0 { 1.0 } else { 0.0 }),
                    flash: Tweener::new(0.0),
                })
                .collect(),
            pose: Pose::Neutral,
            white: Tweener::new(0.0),
            back_flash: Tweener::new(0.0),
        }
    }

    /// Exactly the culprits are marked (`0x7100099cf0`)
    fn is_correct(&self) -> bool {
        NODES
            .iter()
            .zip(&self.items)
            .all(|(node, item)| node.id == CONFIRM || item.checked == node.culprit)
    }

    pub fn update(&mut self, delta: Ticks, shared: &mut Shared) -> Option<Handover> {
        let mut handover = None;
        if self.state == State::Submitted && self.items[self.cursor].flash.is_idle() {
            self.white.fast_forward_to(1.0);
            self.white.enqueue(0.0, frames(30.0, Easing::Linear));
            self.pose = Pose::Pointing;
            self.timer.enqueue(0.0, frames(60.0, Easing::Linear));
            self.state = State::Revealing;
        }
        if self.state == State::Revealing && self.white.is_idle() && self.timer.is_idle() {
            if !self.correct {
                shared.sounds.push(SOUND_LAUGH);
                let before = Screen::Select(self.clone());
                handover = Some(Handover {
                    snapshot_of: Some(before),
                    ..Handover::crossfade(10.0, None)
                });
                self.timer.enqueue(0.0, frames(60.0, Easing::Linear));
                self.pose = Pose::Troubled;
            }
            self.state = State::Reacting;
        }
        if self.state == State::Reacting
            && handover.is_none()
            && !shared.transition_running
            && self.timer.is_idle()
        {
            shared.result = Some(if self.correct {
                CORRECT
            } else {
                FIRST_WRONG + shared.param.min(20)
            });
            self.state = State::Answered;
        }
        if self.state == State::Leaving && self.back_flash.is_idle() {
            handover = Some(Handover::crossfade(
                30.0,
                Some(Screen::Top(TopMenu::new(MENU_ITEM))),
            ));
            self.state = State::Done;
        }
        self.timer.update(delta);
        for item in &mut self.items {
            item.highlight.update(delta);
            item.flash.update(delta);
        }
        self.white.update(delta);
        self.back_flash.update(delta);
        handover
    }

    pub fn handle_input(&mut self, input: &MinigameInput, shared: &mut Shared) {
        if self.state != State::Idle {
            return;
        }
        let pressed = input.pressed;
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
            let cells = NODES.iter().map(tree::cell).collect::<Vec<_>>();
            // on the wide Confirm button the target point moves 7 cells sideways
            let stretch = if NODES[self.cursor].id == CONFIRM {
                7.0
            } else {
                1.0
            };
            let target = cells[self.cursor] + vec2(dx * stretch, dy);
            let candidates = (0..NODES.len()).filter(|&i| i != self.cursor);
            if let Some(found) = tree::pick(&cells, candidates, target, vec2(dx, dy)) {
                shared.sounds.push(SOUND_MOVE);
                move_highlight(&mut self.items, self.cursor, found, |item| {
                    &mut item.highlight
                });
                self.cursor = found;
            }
        } else if input.confirm {
            minigame::flash(&mut self.items[self.cursor].flash);
            if NODES[self.cursor].id == CONFIRM {
                shared.sounds.push(SOUND_ATTACK);
                self.correct = self.is_correct();
                self.state = State::Submitted;
            } else {
                shared.sounds.push(SOUND_DECIDE);
                let item = &mut self.items[self.cursor];
                item.checked = !item.checked;
            }
        } else if input.cancel {
            shared.sounds.push(SOUND_CANCEL);
            minigame::flash(&mut self.back_flash);
            self.state = State::Leaving;
        }
    }

    /// Marks the culprits and submits
    pub fn auto_pick(&mut self, shared: &mut Shared) {
        if self.state != State::Idle {
            return;
        }
        for (node, item) in NODES.iter().zip(&mut self.items) {
            item.checked = node.culprit;
        }
        let confirm = NODES.iter().position(|node| node.id == CONFIRM).unwrap();
        move_highlight(&mut self.items, self.cursor, confirm, |item| {
            &mut item.highlight
        });
        self.cursor = confirm;
        self.handle_input(
            &MinigameInput {
                confirm: true,
                ..Default::default()
            },
            shared,
        );
    }

    pub fn draw(&self, drawer: &mut Drawer, _shared: &Shared, background: bool) {
        if background {
            drawer.image("different_space_1a", SCREEN);
            for ((x, y, w, h), textures) in BATTLER {
                if let Some(texture) = textures[self.pose as usize] {
                    drawer.image(texture, rect(BATTLER_X + x, y, w, h));
                }
            }
            return;
        }
        drawer.nine_slice("left", FRAME_XS, FRAME_YS, LEFT_US, LEFT_VS);
        drawer.image("sentaku", [320.0, 120.0, 780.0, 184.0]);
        draw_lines(drawer, vec2(340.0, 218.0), CELL, (2.0, 3.0));
        for (i, (node, item)) in NODES.iter().zip(&self.items).enumerate() {
            let bounds = node_rect(i);
            let glow = item.highlight.value() * 0.5;
            let flash = item.flash.value();
            if node.id == CONFIRM {
                let texture = drawer.texture("tokutei");
                drawer.sprite(Sprite::new(texture, bounds).glow(glow).flash(flash));
                continue;
            }
            drawer.image("non", bounds);
            drawer.image(PORTRAITS[node.id as usize], bounds);
            if item.checked {
                drawer.image("murderer", bounds);
            }
            drawer.flat(bounds, white(glow));
            drawer.flat(bounds, white(flash));
        }
        draw_back(drawer, &self.back_flash);
        drawer.flat(SCREEN, white(self.white.value()));
    }
}

/// The tree's connecting lines: cells of `cell` px from `origin`; each 32 x 36 tile of `line` cropped by `crop`
/// px on each side to the cell size
pub fn draw_lines(drawer: &mut Drawer, origin: Vec2, cell: Vec2, crop: (f32, f32)) {
    let texture = drawer.texture("line");
    for line in &LINES {
        let at = origin + vec2(line.cell.0 as f32, line.cell.1 as f32) * cell;
        let u = (line.tile.0 as f32 * 32.0 + crop.0) / 128.0;
        let v = (line.tile.1 as f32 * 36.0 + crop.1) / 144.0;
        let [a, r, g, b] = line.color.to_be_bytes().map(|c| c as f32 / 255.0);
        drawer.sprite(Sprite {
            color: FloatColor4::from_rgba(r, g, b, a),
            ..Sprite::new(texture, rect(at.x, at.y, cell.x, cell.y)).uv([
                u,
                v,
                u + cell.x / 128.0,
                v + cell.y / 144.0,
            ])
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_sit_where_the_original_puts_them() {
        assert_eq!(node_rect(0), [450.0, 213.5, 538.0, 312.5]);
        assert_eq!(node_rect(17), [340.0, 819.0, 760.0, 907.0]);
    }

    #[test]
    fn only_the_three_culprits_are_correct() {
        let mut select = Select::new();
        assert!(!select.is_correct());
        for (node, item) in NODES.iter().zip(&mut select.items) {
            item.checked = node.culprit;
        }
        assert!(select.is_correct());
        select.items[0].checked = true;
        assert!(!select.is_correct());
    }
}
