//! The backlog's chapter list (`shin::MessageBrowser::ChapterSelector`, ctor `0x71000a83d0`, update `0x71000a8e90`,
//! render `0x71000a96d0`), opened with X: the chapter's start, its choices, chapter titles and minigames, the
//! backlog's cursor and the game's current line, and its end, as right-aligned labels 60 px apart. Moving through it
//! moves the backlog's cursor to the entry's line; A plays that line's voice and Y jumps there, as in the backlog.
//!
//! With the pointer the list drags and flicks vertically and a flick to the right closes it, as the backlog does.
//!
//! shortcut: the minigames are all labelled "quiz" (the port's log does not keep the minigame's kind), and a tap
//! only selects the row under it (what the original's tap does here was not traced).

use std::collections::HashMap;

use glam::{Mat4, vec3};
use shin_core::{
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
};
use shin_render::render_pass::RenderPass;

use super::{MenuAssets, TaskCx};
use crate::{
    adv::{
        LogKind,
        hud::{OUTLINE, text_layout},
    },
    asset::{executable::text_at, texture_archive::NamedTextures},
    layer::{text_block::TextBlock, user::minigame::Sprite},
    ui::{
        self,
        gesture::{Coast, Gesture, GestureRecognizer},
    },
};

const ROW: f32 = 60.0;
/// The rows' view: 12 rows from y 240
const VIEW: f32 = 720.0;

/// What the list asks of the backlog
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListEvent {
    None,
    /// Put the backlog's cursor on this line
    Select(usize),
    /// B: the backlog's close button flashes while the list fades
    Close,
    /// A: the cursor line's voice
    Voice,
    /// Y: the backlog's jump to its cursor line
    Jump,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// 5 frames in
    Opening,
    Active,
    /// After the close button's flash, 5 frames out
    Closing,
    Done,
}

struct Entry {
    /// 0 start, 1 marker, 97 the backlog's cursor, 98 the game's line, 99 both, 100 end
    kind: u8,
    line: usize,
    label: String,
    color: FloatColor4,
}

pub(super) struct ChapterList {
    entries: Vec<Entry>,
    cursor: usize,
    open: f32,
    /// The rows' scroll in pixels (`+0x88`)
    scroll: Tweener,
    gestures: GestureRecognizer,
    /// The flick's speed (`+0xec`)
    coast: Coast,
    rows: HashMap<usize, TextBlock>,
    state: State,
}

/// A label from the (patched) executable, or the English release's text without it
fn engine_text(address: u64, fallback: &str) -> String {
    text_at(address).unwrap_or_else(|| fallback.to_string())
}

impl ChapterList {
    /// `0x71000a83d0`: `markers` are the backlog's lines with their kind and chapter-list label
    pub fn new(markers: &[(LogKind, &str)], cursor: usize, current: usize) -> Self {
        let tint = |r, g, b| FloatColor4::from_rgba(r, g, b, 1.0);
        let ends = tint(0.875, 0.875, 1.0);
        let here = tint(0.75, 1.0, 0.75);
        let mut entries = vec![Entry {
            kind: 0,
            line: 0,
            label: engine_text(0x7100163263, "Start"),
            color: ends,
        }];
        for (line, &(kind, label)) in markers.iter().enumerate() {
            let color = match kind {
                LogKind::Message { .. } => continue,
                LogKind::Choice => tint(1.0, 0.75, 0.75),
                LogKind::Chapter => tint(0.75, 1.0, 1.0),
                LogKind::Quiz => tint(1.0, 0.7778, 0.7778),
            };
            entries.push(Entry {
                kind: 1,
                line,
                label: label.to_string(),
                color,
            });
        }
        if cursor == current {
            entries.push(Entry {
                kind: 99,
                line: cursor,
                label: engine_text(0x710015dd4e, "Current position"),
                color: here,
            });
        } else {
            entries.push(Entry {
                kind: 97,
                line: cursor,
                label: engine_text(0x7100162105, "Highlight position"),
                color: here,
            });
            entries.push(Entry {
                kind: 98,
                line: current,
                label: engine_text(0x7100161aae, "In-game position"),
                color: here,
            });
        }
        entries.push(Entry {
            kind: 100,
            line: markers.len().saturating_sub(1),
            label: engine_text(0x7100161ac7, "End"),
            color: ends,
        });
        // `0x71000adb90`: by line, then by kind
        entries.sort_by_key(|entry| (entry.line, entry.kind));

        let cursor = entries
            .iter()
            .position(|entry| entry.kind | 2 == 99)
            .unwrap_or(0);
        let n = entries.len() as f32;
        let first = (ROW * cursor as f32 - 330.0).clamp(0.0, (ROW * n - VIEW).max(0.0));
        Self {
            entries,
            cursor,
            open: 0.0,
            scroll: Tweener::new((first / ROW).floor() * ROW),
            rows: HashMap::new(),
            gestures: GestureRecognizer::new(false),
            coast: Coast::default(),
            state: State::Opening,
        }
    }

    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// The backlog's jump question has been answered (`$_11`): the list leaves
    pub fn close(&mut self) {
        if self.state != State::Done {
            self.state = State::Closing;
        }
    }

    fn max_scroll(&self) -> f32 {
        (ROW * self.entries.len() as f32 - VIEW).max(0.0)
    }

    /// `0x71000a9420`: the cursor, the row brought into view over 4 frames
    fn select(&mut self, to: usize) -> ListEvent {
        self.cursor = to;
        let current = self.scroll.value();
        let y = ROW * to as f32 - current;
        let target = if y < ROW {
            Some((ROW * (to as f32 - 1.0)).max(0.0))
        } else if y + ROW > 660.0 {
            Some((ROW * to as f32 - 600.0).min(self.max_scroll()))
        } else {
            None
        };
        if let Some(target) = target {
            self.scroll = Tweener::new(current);
            ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
        }
        ListEvent::Select(self.entries[to].line)
    }

    /// `0x71000a8e90`; `flash_done` is whether the backlog's close button has stopped flashing
    /// Scrolls to `scroll` (clamped) at once; the cursor follows onto the nearest row fully in view (`0x71000a9590`)
    fn scroll_by_pointer(&mut self, scroll: f32) -> Option<ListEvent> {
        let scroll = scroll.clamp(0.0, self.max_scroll());
        self.scroll = Tweener::new(scroll);
        let top = (scroll / ROW).ceil() as usize;
        let bottom = (((scroll + VIEW) / ROW).floor() as usize).saturating_sub(1);
        let follow = self
            .cursor
            .clamp(top, bottom.max(top))
            .min(self.entries.len() - 1);
        (follow != self.cursor).then(|| {
            self.cursor = follow;
            ListEvent::Select(self.entries[follow].line)
        })
    }

    pub fn update(&mut self, cx: &mut TaskCx, assets: &MenuAssets, flash_done: bool) -> ListEvent {
        let mut event = ListEvent::None;
        let mut stick = false;
        match self.state {
            State::Opening => {
                self.open = (self.open + 0.2 * cx.frames).min(1.0);
                if self.open >= 1.0 {
                    self.state = State::Active;
                }
            }
            State::Active if cx.focused => {
                let flat = |v: glam::Vec2| v.y.abs() <= v.x.abs() * 0.577_350_3;
                for gesture in self.gestures.update(cx.input.pointer, cx.frames) {
                    match gesture {
                        Gesture::Down(_) if self.coast.is_moving() => {
                            self.coast.stop();
                            self.gestures.cancel();
                        }
                        Gesture::Tap(at) if (240.0..960.0).contains(&at.y) => {
                            let row = ((at.y - 240.0 + self.scroll.value()) / ROW) as usize;
                            if row < self.entries.len() && row != self.cursor {
                                cx.sound("move");
                                event = self.select(row);
                            }
                        }
                        Gesture::Drag(from, to) if !flat(to - from) => {
                            let scroll = self.scroll.value() - (to.y - from.y);
                            event = self.scroll_by_pointer(scroll).unwrap_or(event);
                            stick = true;
                        }
                        Gesture::Flick(v) if flat(v) && v.x > 0.0 => {
                            cx.sound("cancel");
                            self.state = State::Closing;
                            return ListEvent::Close;
                        }
                        Gesture::Flick(v) if !flat(v) => self.coast.start(v.y),
                        _ => {}
                    }
                }
                if self.coast.is_moving() {
                    let scroll =
                        self.coast
                            .update(self.scroll.value(), 0.0, self.max_scroll(), cx.frames);
                    event = self.scroll_by_pointer(scroll).unwrap_or(event);
                    stick = true;
                }
                if self.gestures.is_active() || self.coast.is_moving() || !self.scroll.is_idle() {
                    return event;
                }
                let input = cx.input;
                let n = self.entries.len() as i32;
                let c = self.cursor as i32;
                let to = if input.up {
                    c - 1
                } else if input.down {
                    c + 1
                } else if input.left {
                    c - 4
                } else if input.right {
                    c + 4
                } else if input.r {
                    n
                } else if input.l {
                    0
                } else if input.x {
                    self.entries
                        .iter()
                        .position(|entry| entry.kind | 2 == 99)
                        .map_or(c, |i| i as i32)
                } else {
                    c
                };
                let to = to.clamp(0, n - 1) as usize;
                if to != self.cursor {
                    cx.sound("move");
                    event = self.select(to);
                } else if input.cancel {
                    cx.sound("cancel");
                    self.state = State::Closing;
                    event = ListEvent::Close;
                } else if input.confirm {
                    event = ListEvent::Voice;
                } else if input.y {
                    event = ListEvent::Jump;
                } else if input.scroll != 0.0 {
                    let scroll = self.scroll.value() + input.scroll * 32.0;
                    event = self.scroll_by_pointer(scroll).unwrap_or(event);
                    stick = true;
                }
            }
            State::Active => {}
            State::Closing => {
                if flash_done {
                    self.open = (self.open - 0.2 * cx.frames).max(0.0);
                    if self.open <= 0.0 {
                        self.state = State::Done;
                    }
                }
            }
            State::Done => {}
        }

        // the rows snap to whole rows when nothing moves them
        let scroll = self.scroll.value();
        let rest = scroll % ROW;
        if !stick && self.scroll.is_idle() && rest != 0.0 {
            let target = scroll - rest + if rest >= 30.0 { ROW } else { 0.0 };
            ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
        }
        // the scroll steps a whole frame per update (`0x71000a9284`)
        ui::advance(&mut self.scroll, 1.0);

        // the rows in view are laid out (system.fnt, 48 px, 1024 wide), the others dropped
        let scroll = self.scroll.value();
        let visible = |r: usize| {
            let y = ROW * r as f32 - scroll;
            y + ROW > 0.0 && y < VIEW
        };
        self.rows.retain(|&r, _| visible(r));
        for r in (0..self.entries.len()).filter(|&r| visible(r)) {
            self.rows.entry(r).or_insert_with(|| {
                let layout = LayoutParams {
                    always_leave_space_for_rubi: false,
                    perform_soft_breaks: false,
                    ..text_layout(1024.0 + 2.0 * OUTLINE, 48.0)
                };
                TextBlock::new(
                    cx.pre_render,
                    &assets.system_font,
                    &self.entries[r].label,
                    layout,
                    OUTLINE,
                )
            });
        }
        event
    }

    /// `0x71000a96d0`: the dim, the rows, the header, the rules and the scroll bar, all sliding in from the right
    pub fn render(&self, pass: &mut RenderPass, textures: &NamedTextures, target_size: (u32, u32)) {
        let slide = 1920.0 - 1920.0 * self.open;
        let m = ui::projection() * Mat4::from_translation(vec3(slide, 0.0, 0.0));
        let dim = FloatColor4::from_rgba(0.502, 0.502, 0.502, 0.753);
        let clear = dim.with_alpha(0.0);
        ui::fill(pass, m, [420.0, 0.0, 1920.0, 1080.0], dim);
        ui::fill_corners(
            pass,
            m,
            [0.0, 0.0, 420.0, 1080.0],
            [clear, dim, clear, dim],
            ui::builder(),
        );

        let scroll = self.scroll.value();
        ui::clip(
            pass,
            Some([slide, 240.0, 1920.0 + slide, 960.0]),
            target_size,
        );
        let rows = m * Mat4::from_translation(vec3(0.0, 240.0 - scroll, 0.0));
        for (&r, text) in &self.rows {
            let entry = &self.entries[r];
            let mut color = entry.color;
            if r != self.cursor {
                color =
                    FloatColor4::from_rgba(color.r * 0.7, color.g * 0.7, color.b * 0.7, color.a);
            }
            // anchored by its right edge at x 1800
            let x = 1800.0 - text.size().x - OUTLINE;
            let y = ROW * r as f32 + 3.0 - OUTLINE;
            ui::draw_text(
                pass,
                text,
                rows * Mat4::from_translation(vec3(x, y, 0.0)),
                color,
                FloatColor4::BLACK.with_alpha(color.a),
            );
        }
        ui::clip(pass, None, target_size);

        Sprite::new(textures.get("chapter"), [1152.0, 144.0, 1800.0, 240.0]).draw(
            pass,
            ui::builder(),
            m,
        );
        ui::fill(pass, m, [420.0, 234.0, 1920.0, 240.0], FloatColor4::WHITE);
        ui::fill(pass, m, [420.0, 960.0, 1920.0, 966.0], FloatColor4::WHITE);

        let height = ROW * self.entries.len() as f32;
        if height > VIEW {
            ui::fill(
                pass,
                m,
                [1808.0, 240.0, 1824.0, 960.0],
                FloatColor4::from_rgba(0.063, 0.063, 0.251, 0.376),
            );
            let t0 = (scroll * VIEW / height).floor();
            let t1 = ((scroll * VIEW + VIEW * VIEW) / height).floor();
            ui::fill(
                pass,
                m,
                [1808.0, 240.0 + t0, 1824.0, 240.0 + t1],
                FloatColor4::from_rgba(0.753, 0.753, 0.753, 0.753),
            );
        }
    }
}
