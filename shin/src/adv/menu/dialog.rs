//! The Yes/No and OK dialog every screen shares (ctor `0x71000223d0`, update `0x71000239c0`, render `0x7100024260`):
//! a message over a dimmed screen with a row (or column) of buttons, that grows in and out of the screen's centre.
//!
//! Results come as events like the original's callback: `(0, button)` when the close starts and `(1, button)` when
//! it has ended, `button` being the chosen index, -1 for cancel.
//!
//! shortcut: the original draws the box into a render texture and fades and scales that; here every piece is drawn
//! with the fade, so overlapping pieces blend slightly differently while it fades.

use glam::{Mat4, Vec2, vec2, vec3};
use shin_core::{
    layout::LayoutParams,
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
};
use shin_render::render_pass::RenderPass;

use super::{MenuAssets, Task, TaskCx};
use crate::{
    adv::hud::{OUTLINE, text_layout},
    asset::executable::EngineText,
    layer::{text_block::TextBlock, user::minigame},
    render::PreRenderContext,
    ui::{self, NineSlice},
};

/// The button labels (はい / いいえ / 確認)
const YES: EngineText = EngineText(0x7100161a50, "Yes");
const NO: EngineText = EngineText(0x7100161d57, "No");
const OK: EngineText = EngineText(0x7100163da3, "OK");

/// `0x7100024590`'s tables (insets `0x7100166514`, u `0x7100166554`, v `0x7100166594`): the frame, a button (also
/// the pressed flash) and the cursor's button
const FRAME: NineSlice = NineSlice {
    insets: [16.0, 32.0, 16.0, 32.0],
    us: [0.0, 0.25, 0.5, 1.0],
    vs: [0.0, 0.25, 0.5, 1.0],
};
const BUTTON: NineSlice = NineSlice {
    insets: [8.0; 4],
    us: [0.5, 0.625, 0.75, 0.875],
    vs: [0.0, 0.25, 0.5, 0.75],
};
const SELECTED: NineSlice = NineSlice {
    insets: [8.0; 4],
    us: [0.0, 0.125, 0.25, 0.375],
    vs: [0.0, 0.25, 0.5, 0.75],
};

/// Where the content sits inside the box, and how much bigger than the content the box is (the frame's 16 px
/// shadow is on the right and bottom)
const PADDING: f32 = 30.0;
const BOX_EXTRA: f32 = 76.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Opening,
    Active,
    /// Waiting for the chosen button's flash
    Closing,
    /// Shrinking away
    Closed,
    Done,
}

struct Button {
    /// left, top, right, bottom in the content's space
    rect: [f32; 4],
    text: TextBlock,
    highlight: Tweener,
    flash: Tweener,
}

pub struct Dialog {
    assets: MenuAssets,
    message: TextBlock,
    buttons: Vec<Button>,
    /// Buttons stacked (`+0x210` = 1) when a row would not fit in three quarters of the width
    column: bool,
    /// The box's top-left corner and size, the content inside it at `PADDING`
    box_pos: Vec2,
    box_size: Vec2,
    scale: Tweener,
    alpha: Tweener,
    state: State,
    cursor: i32,
    cancellable: bool,
    close_timer: f32,
    /// The pointer as last frame, and the button a press went down on
    pointer_held: bool,
    pressed: Option<usize>,
    events: Vec<(u8, i32)>,
}

fn layout(size: f32, line_spacing: f32, soft_breaks: bool) -> LayoutParams {
    LayoutParams {
        line_padding_above: 4.0,
        line_padding_below: 4.0,
        line_padding_between: line_spacing,
        perform_soft_breaks: soft_breaks,
        word_wrap: false,
        always_leave_space_for_rubi: false,
        ..text_layout(1440.0, size)
    }
}

impl Dialog {
    /// A dialog with `buttons` (none for a notice), the cursor on `cursor`; B cancels it when `cancellable`
    pub fn new(
        ctx: &PreRenderContext,
        assets: &MenuAssets,
        message: &str,
        buttons: &[&str],
        cursor: i32,
        cancellable: bool,
    ) -> Self {
        let font = &assets.system_font;
        let message = TextBlock::new(ctx, font, message, layout(54.0, 3.0, true), OUTLINE);
        let mut width = message.size().x.max(600.0);
        let mut y = message.size().y + 8.0;

        let mut buttons = buttons
            .iter()
            .map(|label| {
                let text = TextBlock::new(ctx, font, label, layout(44.0, 0.0, false), OUTLINE);
                let size = text.size() + vec2(80.0, 24.0);
                Button {
                    rect: [0.0, 0.0, size.x, size.y],
                    text,
                    highlight: Tweener::new(0.0),
                    flash: Tweener::new(0.0),
                }
            })
            .collect::<Vec<_>>();

        let count = buttons.len() as f32;
        let max_w = buttons
            .iter()
            .map(|b| b.rect[2].max(180.0))
            .fold(0.0, f32::max);
        let max_h = buttons.iter().map(|b| b.rect[3]).fold(0.0, f32::max);
        let column = count * max_w > 0.75 * width;
        let content_height = if buttons.is_empty() {
            y - 8.0
        } else if !column {
            let gap = if buttons.len() >= 2 {
                ((0.75 * width - count * max_w) / (count - 1.0)).min(30.0)
            } else {
                0.0
            };
            let x0 = (width - (count * max_w + (count - 1.0) * gap)) / 2.0;
            for (k, button) in buttons.iter_mut().enumerate() {
                let x = x0 + k as f32 * (max_w + gap);
                let h = button.rect[3];
                let top = y + (max_h - h) / 2.0;
                button.rect = [x, top, x + max_w, top + h];
            }
            y + max_h
        } else {
            let button_w = (0.75 * width).max(max_w);
            width = width.max(button_w);
            let x = (width - button_w) / 2.0;
            for button in &mut buttons {
                let h = button.rect[3];
                button.rect = [x, y, x + button_w, y + h];
                y += h + 8.0;
            }
            y - 8.0
        };

        let cursor = if buttons.is_empty() {
            -1
        } else if (0..buttons.len() as i32).contains(&cursor) {
            cursor
        } else {
            0
        };
        if cursor >= 0 {
            buttons[cursor as usize].highlight = Tweener::new(1.0);
        }

        let mut scale = Tweener::new(1.4);
        let mut alpha = Tweener::new(0.0);
        ui::tween(&mut scale, 1.0, 12.0, Easing::SineOut);
        ui::tween(&mut alpha, 1.0, 12.0, Easing::Linear);

        Self {
            assets: assets.clone(),
            message,
            buttons,
            column,
            box_pos: vec2(
                (1920.0 - width) / 2.0 - PADDING,
                (1080.0 - content_height) / 2.0 - PADDING,
            ),
            box_size: vec2(width + BOX_EXTRA, content_height + BOX_EXTRA),
            scale,
            alpha,
            state: State::Opening,
            cursor,
            cancellable,
            close_timer: 0.0,
            pointer_held: false,
            pressed: None,
            events: Vec::new(),
        }
    }

    /// A Yes/No question (`0x71000222d0`), the cursor on `cursor` (the original's callers start on No, 1)
    pub fn yes_no(ctx: &PreRenderContext, assets: &MenuAssets, message: &str, cursor: i32) -> Self {
        Self::new(ctx, assets, message, &[&YES.get(), &NO.get()], cursor, true)
    }

    /// A notice with an OK button (`0x71000223a0`)
    pub fn ok(ctx: &PreRenderContext, assets: &MenuAssets, message: &str) -> Self {
        Self::new(ctx, assets, message, &[&OK.get()], 0, true)
    }

    /// The abort predicate came true (`0x71000239c0` state 2): the dialog closes with button -2
    pub fn close_now(&mut self) {
        if self.state == State::Active {
            if self.cursor >= 0 {
                minigame::tween_now(&mut self.buttons[self.cursor as usize].highlight, 0.0, 0.1);
            }
            self.cursor = -2;
            self.state = State::Closing;
        }
    }

    /// What happened since the last call: `(0, button)` as the dialog starts closing, `(1, button)` once it is gone
    pub fn take_events(&mut self) -> Vec<(u8, i32)> {
        std::mem::take(&mut self.events)
    }

    fn move_cursor(&mut self, cx: &TaskCx, to: i32) {
        if to == self.cursor {
            return;
        }
        if self.cursor >= 0 {
            minigame::tween_now(&mut self.buttons[self.cursor as usize].highlight, 0.0, 0.1);
        }
        minigame::tween_now(&mut self.buttons[to as usize].highlight, 1.0, 1.0);
        self.cursor = to;
        cx.sound("move");
    }

    fn decide(&mut self, cx: &TaskCx) {
        cx.sound("decide");
        let flash = &mut self.buttons[self.cursor as usize].flash;
        *flash = Tweener::new(0.0);
        minigame::flash(flash);
        self.state = State::Closing;
    }

    fn handle_keys(&mut self, cx: &TaskCx) {
        let input = &cx.input;
        if input.confirm {
            if self.cursor >= 0 {
                self.decide(cx);
            } else {
                cx.sound("error");
            }
            return;
        }
        if input.cancel && self.cancellable {
            cx.sound("cancel");
            if self.cursor >= 0 {
                minigame::tween_now(&mut self.buttons[self.cursor as usize].highlight, 0.0, 0.1);
            }
            self.cursor = -1;
            self.state = State::Closing;
            return;
        }
        let n = self.buttons.len() as i32;
        if n == 0 {
            return;
        }
        // a row moves with left/right and jumps with up/down, a column the other way round
        let (prev, next, first, last, parallel_held) = if self.column {
            (
                input.up,
                input.down,
                input.left,
                input.right,
                input.held_up || input.held_down,
            )
        } else {
            (
                input.left,
                input.right,
                input.up,
                input.down,
                input.held_left || input.held_right,
            )
        };
        let to = if prev {
            (if self.cursor > 0 { self.cursor } else { n }) - 1
        } else if next {
            if self.cursor + 1 < n {
                self.cursor + 1
            } else {
                0
            }
        } else if first && !parallel_held {
            0
        } else if last && !parallel_held {
            n - 1
        } else {
            return;
        };
        self.move_cursor(cx, to);
    }

    /// The button under `point` (screen space), if any
    fn button_at(&self, point: Vec2) -> Option<usize> {
        let origin = self.box_pos + PADDING;
        self.buttons.iter().position(|button| {
            let [x0, y0, x1, y1] = button.rect;
            minigame::contains(
                [x0 + origin.x, y0 + origin.y, x1 + origin.x, y1 + origin.y],
                point,
            )
        })
    }

    /// A press on a button raises its flash and moves the cursor there; releasing on it decides
    fn handle_pointer(&mut self, cx: &TaskCx) {
        let pointer = cx.input.pointer;
        let went_down = pointer.held && !self.pointer_held;
        let went_up = !pointer.held && self.pointer_held;
        self.pointer_held = pointer.held;
        let hit = pointer.position.and_then(|p| self.button_at(p));
        if went_down {
            if let Some(button) = hit {
                self.pressed = Some(button);
                minigame::raise(&mut self.buttons[button].flash);
                self.move_cursor(cx, button as i32);
            }
        } else if went_up {
            if let Some(pressed) = self.pressed.take() {
                if hit == Some(pressed) {
                    self.cursor = pressed as i32;
                    self.decide(cx);
                } else {
                    minigame::lower(&mut self.buttons[pressed].flash);
                }
            }
        }
    }
}

impl Task for Dialog {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        match self.state {
            State::Opening => {
                if self.scale.is_idle() && self.alpha.is_idle() {
                    self.state = State::Active;
                }
            }
            State::Active => {
                if cx.focused {
                    self.handle_pointer(cx);
                    if self.state == State::Active && self.pressed.is_none() {
                        self.handle_keys(cx);
                    }
                }
            }
            State::Closing => {
                let flashing =
                    self.cursor >= 0 && !self.buttons[self.cursor as usize].flash.is_idle();
                if !flashing {
                    ui::tween(&mut self.scale, 0.5, 12.0, Easing::SineIn);
                    ui::tween(&mut self.alpha, 0.0, 12.0, Easing::Linear);
                    self.events.push((0, self.cursor));
                    self.close_timer = 5.0;
                    self.state = State::Closed;
                }
            }
            State::Closed => {
                self.close_timer -= cx.frames;
                if self.scale.is_idle() && self.alpha.is_idle() && self.close_timer <= 0.0 {
                    self.events.push((1, self.cursor));
                    self.state = State::Done;
                }
            }
            State::Done => return false,
        }

        ui::advance(&mut self.scale, cx.frames);
        ui::advance(&mut self.alpha, cx.frames);
        for button in &mut self.buttons {
            ui::advance(&mut button.highlight, cx.frames);
            ui::advance(&mut button.flash, cx.frames);
        }
        true
    }

    fn render(&self, pass: &mut RenderPass) {
        let alpha = self.alpha.value();
        if alpha <= 0.0 {
            return;
        }
        let projection = ui::projection();
        ui::fill(
            pass,
            projection,
            [0.0, 0.0, 1920.0, 1080.0],
            FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.5 * alpha),
        );

        let centre = self.box_pos + self.box_size / 2.0;
        let s = self.scale.value();
        let transform = projection
            * Mat4::from_translation(centre.extend(0.0))
            * Mat4::from_scale(vec3(s, s, 1.0))
            * Mat4::from_translation((-centre).extend(0.0));
        let fade = |c: FloatColor4| c.with_alpha(c.a * alpha);

        let frame = self.assets.system_textures.get("dialog_frame");
        let button_texture = self.assets.system_textures.get("dialog_button");
        let [x, y] = self.box_pos.to_array();
        let [w, h] = self.box_size.to_array();
        FRAME.draw(
            pass,
            frame,
            transform,
            [x, y, x + w, y + h],
            fade(FloatColor4::WHITE),
        );

        let content = transform * Mat4::from_translation(vec3(x + PADDING, y + PADDING, 0.0));
        draw_text(
            pass,
            &self.message,
            content,
            fade(FloatColor4::WHITE),
            fade(FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.8)),
        );

        for (index, button) in self.buttons.iter().enumerate() {
            let t = button.highlight.value();
            // the colours a highlighted button moves to (table `0x7100024f60`)
            let outline = ui::lerp_color(
                FloatColor4::from_rgba(0.2, 0.5725, 0.957, 0.0),
                FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.8),
                t,
            );
            let text = ui::lerp_color(
                FloatColor4::from_rgba(1.0, 1.0, 1.0, 0.7),
                FloatColor4::WHITE,
                t,
            );
            let frame = ui::lerp_color(
                FloatColor4::from_rgba(0.0, 0.0, 0.0, 0.5),
                FloatColor4::from_rgba(0.7, 0.7, 0.7, 0.7),
                t,
            );
            BUTTON.draw(pass, button_texture, content, button.rect, fade(frame));
            let [x0, y0, x1, y1] = button.rect;
            let size = button.text.size();
            let top_left = vec2((x0 + x1 - size.x) / 2.0, (y0 + y1 - size.y) / 2.0);
            draw_text(
                pass,
                &button.text,
                content * Mat4::from_translation(top_left.extend(0.0)),
                fade(text),
                fade(outline),
            );
            if index as i32 == self.cursor {
                SELECTED.draw(
                    pass,
                    button_texture,
                    content,
                    button.rect,
                    fade(FloatColor4::WHITE),
                );
            }
            let flash = button.flash.value();
            if flash > 0.0 {
                BUTTON.draw(
                    pass,
                    button_texture,
                    content,
                    button.rect,
                    FloatColor4::from_rgba(1.0, 1.0, 1.0, flash * alpha),
                );
            }
        }
    }
}

/// A text object in `tint` with an outline in `border`, its padded top-left corner at the origin of `transform`
fn draw_text(
    pass: &mut RenderPass,
    text: &TextBlock,
    transform: Mat4,
    tint: FloatColor4,
    border: FloatColor4,
) {
    text.render(
        pass,
        ui::builder(),
        transform * Mat4::from_translation(vec3(OUTLINE, OUTLINE, 0.0)),
        tint,
        border,
    );
}
