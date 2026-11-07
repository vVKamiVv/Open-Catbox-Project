//! Port-only: the Controls screen, opened from the PC tab's "Controls…". It takes the Config screen's place inside its
//! window: one row per remappable action with its keyboard/mouse and controller bindings. A or a tap on a binding
//! waits for the new key or button (Escape or B held for half a second, a click or 5 seconds give up; a quick
//! Escape or B binds it), refusing one another action already reads in the same place; Y or the Default button puts
//! every binding back. Changes are written to `settings.json` at once (see [`crate::bindings`]).

use std::collections::HashMap;

use glam::{Mat4, Vec2, vec2, vec3};
use shin_core::{primitives::color::FloatColor4, time::Tweener};
use shin_input::inputs::{GamepadButton, KeyCode};
use shin_render::{render_pass::RenderPass, resize::CanvasSize};

use super::{
    FOCUS, HELP_ADJUST, HELP_DEFAULT, HELP_SAVE, HELP_SELECT, Help, ITEM_H, ITEM_STEP, LABEL_SIZE,
    LIST_W, LIST_X, LIST_Y, OPTION_SIZE, Text, VIEW, draw_text_at,
};
use crate::{
    adv::{
        hud::{OUTLINE, text_layout},
        menu::{MenuAssets, Task, TaskCx, dialog::Dialog},
    },
    app::AppAction,
    asset::texture_archive::NamedTextures,
    bindings::{self, Input},
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    ui::{
        self,
        gesture::{Coast, Gesture, GestureRecognizer},
        prompts,
    },
};

/// The rows: the remappable actions, labelled
const ROWS: &[(AppAction, Text)] = &[
    (AppAction::Enter, Text("Advance / Confirm", "進む・決定")),
    (AppAction::Cancel, Text("Cancel / Back", "キャンセル・戻る")),
    (AppAction::ButtonY, Text("System menu", "システムメニュー")),
    (AppAction::ButtonX, Text("Backlog", "バックログ")),
    (AppAction::AutoToggle, Text("Auto mode", "オートモード")),
    (AppAction::SkipToggle, Text("Skip mode", "スキップモード")),
    (
        AppAction::HoldSkip,
        Text("Hold to fast-forward", "押している間スキップ"),
    ),
    (AppAction::Plus, Text("Next chapter", "次のチャプター")),
    (AppAction::Minus, Text("Previous chapter", "前のチャプター")),
    (
        AppAction::Save,
        Text("Save (gallery / quick save)", "保存・クイックセーブ"),
    ),
    (AppAction::RollBack, Text("Roll back", "ロールバック")),
    (AppAction::PageUp, Text("Page up", "ページアップ")),
    (AppAction::PageDown, Text("Page down", "ページダウン")),
    (
        AppAction::ToggleFullscreen,
        Text("Fullscreen", "全画面切り替え"),
    ),
];

const HEADERS: [Text; 3] = [
    Text("Action", "操作"),
    Text("Keyboard / Mouse", "キーボード・マウス"),
    Text("Controller", "コントローラー"),
];
const WAIT_KEY: Text = Text("Press a key…", "キーを押してください");
const WAIT_BUTTON: Text = Text("Press a button…", "ボタンを押してください");
const HOLD_TO_CANCEL: Text = Text("Hold Esc or B to cancel", "Esc か B を長押しでキャンセル");
const RESET_QUESTION: Text = Text(
    "Reset all controls to default?",
    "操作設定をすべて初期値に戻しますか？",
);
const IN_USE: Text = Text("Already used by: ", "使用中：");

/// The two binding cells of a row, in the list's space
const CELLS: [[f32; 2]; 2] = [[720.0, 1164.0], [1180.0, 1624.0]];
/// Escape or B held this long gives up; nothing pressed for this long too
const HOLD_CANCEL_FRAMES: f32 = 30.0;
const TIMEOUT_FRAMES: f32 = 300.0;
const NOTICE_FRAMES: f32 = 150.0;
/// The controller cells' glyphs, larger than the key help's 48
const GLYPH: f32 = 64.0;

/// Waiting for the new input of a row's cell
struct Capture {
    row: usize,
    /// 0 keyboard and mouse, 1 controller
    column: usize,
    /// Held when the wait started or last frame: only new presses count
    previous: Vec<Input>,
    /// Escape or B, bound when released quickly, giving up when held
    pending: Option<(Input, f32)>,
    elapsed: f32,
    pointer_held: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touched {
    Nothing,
    Close,
    Default,
    /// A row, and the cell under the down point
    Row(usize, Option<usize>),
    /// The list, scrolling
    List,
}

pub(super) struct Controls {
    assets: MenuAssets,
    japanese: bool,
    labels: Vec<TextBlock>,
    headers: Vec<TextBlock>,
    waits: [TextBlock; 2],
    hold_to_cancel: TextBlock,
    /// The keyboard cells' texts, by text
    names: HashMap<String, TextBlock>,
    generation: u32,
    cursor: usize,
    column: usize,
    /// Per row: the focus glow; per cell: the press flash
    focus: Vec<Tweener>,
    flashes: Vec<[Tweener; 2]>,
    scroll: f32,
    coast: Coast,
    gestures: GestureRecognizer,
    touched: Option<Touched>,
    capture: Option<Capture>,
    /// After a capture: the game sees no input until everything is released (or a second has passed)
    releasing: Option<f32>,
    notice: Option<(TextBlock, f32)>,
    dialog: Option<Dialog>,
    reset_pending: bool,
    close_flash: Tweener,
    default_flash: Tweener,
    closing: bool,
    target_size: (u32, u32),
    blink: f32,
}

impl Controls {
    pub fn new(cx: &TaskCx, assets: &MenuAssets, japanese: bool) -> Self {
        let text = |t: Text, size: f32| {
            TextBlock::new(
                cx.pre_render,
                &assets.system_font,
                t.get(japanese),
                text_layout(1400.0, size),
                OUTLINE,
            )
        };
        let mut focus: Vec<Tweener> = ROWS.iter().map(|_| Tweener::new(0.0)).collect();
        focus[0] = Tweener::new(1.0);
        Self {
            assets: assets.clone(),
            japanese,
            labels: ROWS
                .iter()
                .map(|&(_, label)| text(label, LABEL_SIZE))
                .collect(),
            headers: HEADERS.iter().map(|&h| text(h, OPTION_SIZE)).collect(),
            waits: [text(WAIT_KEY, OPTION_SIZE), text(WAIT_BUTTON, OPTION_SIZE)],
            hold_to_cancel: text(HOLD_TO_CANCEL, 28.0),
            names: HashMap::new(),
            generation: bindings::generation().wrapping_sub(1),
            cursor: 0,
            column: 0,
            focus,
            flashes: ROWS
                .iter()
                .map(|_| [Tweener::new(0.0), Tweener::new(0.0)])
                .collect(),
            scroll: 0.0,
            coast: Coast::default(),
            gestures: GestureRecognizer::new(false),
            touched: None,
            capture: None,
            releasing: None,
            notice: None,
            dialog: None,
            reset_pending: false,
            close_flash: Tweener::new(0.0),
            default_flash: Tweener::new(0.0),
            closing: false,
            target_size: (1920, 1080),
            blink: 0.0,
        }
    }

    fn row_y(row: usize) -> f32 {
        ITEM_STEP * row as f32
    }

    fn max_scroll() -> f32 {
        (Self::row_y(ROWS.len() - 1) + ITEM_H - VIEW).max(0.0)
    }

    /// The keyboard cell's text: every keyboard and mouse input, by its short name
    fn keyboard_text(table: &bindings::Table, action: AppAction) -> String {
        let names: Vec<String> = table[action]
            .iter()
            .filter(|i| !i.is_pad())
            .map(|i| i.label())
            .collect();
        if names.is_empty() {
            "—".to_owned()
        } else {
            names.join(" / ")
        }
    }

    /// Lays the keyboard cells out again when the bindings changed
    fn refresh_names(&mut self, cx: &TaskCx) {
        let generation = bindings::generation();
        if generation == self.generation {
            return;
        }
        self.generation = generation;
        let table = bindings::table();
        let font = &self.assets.system_font;
        for &(action, _) in ROWS {
            let name = Self::keyboard_text(&table, action);
            self.names.entry(name).or_insert_with_key(|name| {
                TextBlock::new(
                    cx.pre_render,
                    font,
                    name,
                    text_layout(2000.0, OPTION_SIZE),
                    OUTLINE,
                )
            });
        }
    }

    fn select(&mut self, row: usize) {
        if row == self.cursor {
            return;
        }
        minigame::tween_now(&mut self.focus[self.cursor], 0.0, 0.2);
        minigame::tween_now(&mut self.focus[row], 1.0, 0.25);
        self.cursor = row;
        // keep it inside the view
        let y = Self::row_y(row);
        if y < self.scroll {
            self.scroll = y;
        } else if y + ITEM_H > self.scroll + VIEW {
            self.scroll = y + ITEM_H - VIEW;
        }
    }

    fn start_capture(&mut self, cx: &TaskCx, column: usize) {
        cx.sound("decide");
        self.column = column;
        minigame::flash(&mut self.flashes[self.cursor][column]);
        bindings::set_capturing(true);
        self.blink = 0.0;
        self.capture = Some(Capture {
            row: self.cursor,
            column,
            previous: bindings::held(),
            pending: None,
            elapsed: 0.0,
            pointer_held: cx.input.pointer.held,
        });
    }

    /// The wait for a new input, once a frame; `Some(result)` when it is over (`None` inside: given up)
    fn update_capture(&mut self, cx: &TaskCx) -> Option<Option<Input>> {
        let capture = self.capture.as_mut()?;
        capture.elapsed += cx.frames;
        let held = bindings::held();
        let pointer_pressed = cx.input.pointer.held && !capture.pointer_held;
        capture.pointer_held = cx.input.pointer.held;
        if let Some((input, frames)) = &mut capture.pending {
            if held.contains(input) {
                *frames += cx.frames;
                return (*frames >= HOLD_CANCEL_FRAMES).then_some(None);
            }
            return Some(Some(*input));
        }
        if pointer_pressed || capture.elapsed >= TIMEOUT_FRAMES {
            return Some(None);
        }
        let pad = capture.column == 1;
        let new: Vec<Input> = held
            .iter()
            .copied()
            .filter(|i| !capture.previous.contains(i))
            .collect();
        capture.previous = held;
        let escape = Input::Key(KeyCode::Escape);
        let b = Input::Pad(GamepadButton::B);
        for input in new {
            if input == escape || input == b {
                if input.is_pad() == pad {
                    // a tap binds it, a hold gives up
                    capture.pending = Some((input, 0.0));
                    return None;
                }
                return Some(None);
            }
            if input.is_pad() == pad {
                return Some(Some(input));
            }
        }
        None
    }

    fn finish_capture(&mut self, cx: &mut TaskCx, result: Option<Input>) {
        let Some(capture) = self.capture.take() else {
            return;
        };
        self.releasing = Some(0.0);
        let Some(input) = result else {
            cx.sound("cancel");
            return;
        };
        let action = ROWS[capture.row].0;
        match bindings::rebind(action, input) {
            Ok(()) => {
                cx.sound("decide");
                minigame::flash(&mut self.flashes[capture.row][capture.column]);
            }
            Err(other) => {
                cx.sound("cancel");
                let name = ROWS
                    .iter()
                    .find(|(a, _)| *a == other)
                    .map_or("?", |(_, label)| label.get(self.japanese));
                let message = format!("{}{name}", IN_USE.get(self.japanese));
                let text = TextBlock::new(
                    cx.pre_render,
                    &self.assets.system_font,
                    &message,
                    text_layout(1600.0, OPTION_SIZE),
                    OUTLINE,
                );
                self.notice = Some((text, NOTICE_FRAMES));
            }
        }
    }

    fn hit(&self, at: Vec2) -> Touched {
        if (32.0..116.0).contains(&at.y) {
            if (1764.0..1848.0).contains(&at.x) {
                return Touched::Close;
            }
            if (1446.0..1752.0).contains(&at.x) {
                return Touched::Default;
            }
        }
        if !minigame::contains([LIST_X, LIST_Y, LIST_X + LIST_W, LIST_Y + VIEW], at) {
            return Touched::Nothing;
        }
        let (x, y) = (at.x - LIST_X, at.y - LIST_Y + self.scroll);
        let row = (0..ROWS.len()).find(|&r| (Self::row_y(r)..Self::row_y(r) + ITEM_H).contains(&y));
        match row {
            Some(row) => Touched::Row(
                row,
                CELLS.iter().position(|[x0, x1]| (*x0..*x1).contains(&x)),
            ),
            None => Touched::List,
        }
    }

    fn close(&mut self, cx: &TaskCx, sound: &str) {
        cx.sound(sound);
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.closing = true;
    }

    fn ask_reset(&mut self, cx: &TaskCx) {
        cx.sound("decide");
        self.default_flash = Tweener::new(0.0);
        minigame::flash(&mut self.default_flash);
        self.reset_pending = true;
    }

    fn handle_pointer(&mut self, cx: &mut TaskCx) {
        for gesture in self.gestures.update(cx.input.pointer, cx.frames) {
            match gesture {
                Gesture::Down(at) => {
                    let touched = self.hit(at);
                    if touched == Touched::Nothing {
                        self.gestures.cancel();
                        self.touched = None;
                        continue;
                    }
                    if self.coast.is_moving() && matches!(touched, Touched::Row(..) | Touched::List)
                    {
                        self.coast.stop();
                        self.touched = Some(Touched::List);
                        continue;
                    }
                    self.touched = Some(touched);
                }
                Gesture::Press => match self.touched {
                    Some(Touched::Close) => minigame::raise(&mut self.close_flash),
                    Some(Touched::Default) => minigame::raise(&mut self.default_flash),
                    Some(Touched::Row(row, cell)) => {
                        self.select(row);
                        if let Some(column) = cell {
                            self.column = column;
                            minigame::raise(&mut self.flashes[row][column]);
                        }
                    }
                    _ => {}
                },
                Gesture::Tap(_) => match self.touched.take() {
                    Some(Touched::Close) => self.close(cx, "decide"),
                    Some(Touched::Default) => self.ask_reset(cx),
                    Some(Touched::Row(row, cell)) => {
                        self.select(row);
                        match cell {
                            Some(column) => self.start_capture(cx, column),
                            None => cx.sound("move"),
                        }
                    }
                    _ => {}
                },
                Gesture::Drag(from, to) => match self.touched {
                    Some(Touched::Row(row, cell)) => {
                        if let Some(column) = cell {
                            minigame::lower(&mut self.flashes[row][column]);
                        }
                        self.touched = Some(Touched::List);
                        self.scroll =
                            (self.scroll - (to.y - from.y)).clamp(0.0, Self::max_scroll());
                    }
                    Some(Touched::List) => {
                        self.scroll =
                            (self.scroll - (to.y - from.y)).clamp(0.0, Self::max_scroll());
                    }
                    Some(Touched::Close | Touched::Default) => {
                        minigame::lower(&mut self.close_flash);
                        minigame::lower(&mut self.default_flash);
                        self.touched = Some(Touched::Nothing);
                    }
                    _ => {}
                },
                Gesture::Flick(velocity) => {
                    if self.touched == Some(Touched::List) {
                        self.coast.start(velocity.y);
                    }
                }
                Gesture::End => {
                    if let Some(Touched::Row(row, Some(column))) = self.touched {
                        minigame::lower(&mut self.flashes[row][column]);
                    }
                    self.touched = None;
                }
                Gesture::LongPress => {}
            }
        }
    }

    fn handle_keys(&mut self, cx: &mut TaskCx) {
        let input = cx.input;
        if input.up && self.cursor > 0 {
            self.select(self.cursor - 1);
            cx.sound("move");
        } else if input.down && self.cursor + 1 < ROWS.len() {
            self.select(self.cursor + 1);
            cx.sound("move");
        } else if (input.left && self.column == 1) || (input.right && self.column == 0) {
            self.column = 1 - self.column;
            cx.sound("move");
        } else if input.confirm {
            self.start_capture(cx, self.column);
        } else if input.y {
            self.ask_reset(cx);
        } else if input.cancel {
            self.close(cx, "cancel");
        }
    }

    /// False once closed
    pub fn update(&mut self, cx: &mut TaskCx) -> bool {
        let size = cx.pre_render.resize_source.handle::<CanvasSize>().get();
        self.target_size = (size.width, size.height);
        self.refresh_names(cx);
        let frames = cx.frames;
        self.blink += frames;
        if let Some((_, left)) = &mut self.notice {
            *left -= frames;
            if *left <= 0.0 {
                self.notice = None;
            }
        }
        for tweener in self
            .focus
            .iter_mut()
            .chain(self.flashes.iter_mut().flatten())
            .chain([&mut self.close_flash, &mut self.default_flash])
        {
            ui::advance(tweener, frames);
        }

        if self.capture.is_some() {
            if let Some(result) = self.update_capture(cx) {
                self.finish_capture(cx, result);
            }
            return true;
        }
        if let Some(waited) = &mut self.releasing {
            *waited += frames;
            if (bindings::held().is_empty() && !cx.input.pointer.held) || *waited >= 60.0 {
                self.releasing = None;
                bindings::set_capturing(false);
                self.gestures.cancel();
            }
            return true;
        }
        if let Some(dialog) = &mut self.dialog {
            let running = dialog.update(&mut cx.reborrow(cx.focused));
            if dialog.take_events().contains(&(0, 0)) {
                bindings::reset();
            }
            if !running {
                self.dialog = None;
            }
            return true;
        }
        if self.reset_pending {
            if self.default_flash.is_idle() {
                self.reset_pending = false;
                let question = RESET_QUESTION.get(self.japanese);
                self.dialog = Some(Dialog::yes_no(cx.pre_render, &self.assets, question, 1));
            }
            return true;
        }
        if self.closing {
            return !self.close_flash.is_idle();
        }
        if !cx.focused {
            return true;
        }
        self.handle_pointer(cx);
        if self.coast.is_moving() {
            self.scroll = self
                .coast
                .update(self.scroll, 0.0, Self::max_scroll(), frames);
        }
        if !self.gestures.is_active() && !self.coast.is_moving() && !self.closing {
            self.handle_keys(cx);
        }
        true
    }

    /// A binding cell: the art's button with its ends kept and its middle stretched
    fn draw_cell(
        pass: &mut RenderPass,
        t: &NamedTextures,
        list: Mat4,
        [x0, x1]: [f32; 2],
        y: f32,
        on: bool,
        glow: f32,
        flash: f32,
    ) {
        let art = t.get(if on { "button_on" } else { "button_off" });
        for (left, right, u0, u1) in [
            (x0, x0 + 60.0, 0.0, 0.25),
            (x0 + 60.0, x1 - 60.0, 0.25, 0.75),
            (x1 - 60.0, x1, 0.75, 1.0),
        ] {
            Sprite::new(art, [left, y, right, y + ITEM_H])
                .uv([u0, 0.0, u1, 1.0])
                .glow(glow)
                .flash(flash)
                .draw(pass, ui::builder(), list);
        }
    }

    /// Inside the Config screen's window, which `slid` moves in: the column headers where the tabs are, the rows, the
    /// close and Default buttons at `top` and the key help at `bottom`
    pub fn render(
        &self,
        pass: &mut RenderPass,
        t: &NamedTextures,
        slid: Mat4,
        top: f32,
        bottom: f32,
    ) {
        let projection = ui::projection();
        let builder = ui::builder();
        let table = bindings::table();
        let pad_style = prompts::pad_style();

        // the headers, over the cells' columns
        // where the tabs are, above the window
        let header_y = 164.0;
        let at = vec2(LIST_X + 44.0, header_y);
        draw_text_at(pass, &self.headers[0], slid, at, 1.0);
        for (k, [x0, x1]) in CELLS.iter().enumerate() {
            let text = &self.headers[k + 1];
            let at = vec2(LIST_X + (x0 + x1) / 2.0 - text.size().x / 2.0, header_y);
            draw_text_at(pass, text, slid, at, 1.0);
        }

        ui::clip(
            pass,
            Some([0.0, LIST_Y, 1920.0, LIST_Y + VIEW]),
            self.target_size,
        );
        let list = slid * Mat4::from_translation(vec3(LIST_X, LIST_Y - self.scroll, 0.0));
        for (row, &(action, _)) in ROWS.iter().enumerate() {
            let y = Self::row_y(row);
            if y + ITEM_H < self.scroll || y > self.scroll + VIEW {
                continue;
            }
            let focus = self.focus[row].value();
            if focus > 0.0 {
                FOCUS.draw(
                    pass,
                    t.get("focus"),
                    list,
                    [0.0, y, LIST_W, y + ITEM_H],
                    FloatColor4::WHITE.with_alpha(0.75 * focus),
                );
            }
            let label = &self.labels[row];
            draw_text_at(
                pass,
                label,
                list,
                vec2(44.0, y + (ITEM_H - label.size().y) / 2.0),
                1.0,
            );

            let capturing = self
                .capture
                .as_ref()
                .filter(|c| c.row == row)
                .map(|c| c.column);
            for (column, &cell) in CELLS.iter().enumerate() {
                let selected = row == self.cursor && column == self.column;
                let flash = self.flashes[row][column].value();
                Self::draw_cell(
                    pass,
                    t,
                    list,
                    cell,
                    y,
                    selected,
                    if selected { 0.5 * focus } else { 0.0 },
                    flash,
                );
                let center = vec2((cell[0] + cell[1]) / 2.0, y + ITEM_H / 2.0);
                if capturing == Some(column) {
                    // the wait blinks
                    let alpha = 0.55 + 0.45 * (self.blink / 60.0 * std::f32::consts::TAU).cos();
                    let text = &self.waits[column];
                    draw_text_at(pass, text, list, center - text.size() / 2.0, alpha);
                    continue;
                }
                if column == 0 {
                    if let Some(text) = self.names.get(&Self::keyboard_text(&table, action)) {
                        let width = cell[1] - cell[0] - 48.0;
                        let scale = (width / text.size().x).min(1.0);
                        let transform = list
                            * Mat4::from_translation(center.extend(0.0))
                            * Mat4::from_scale(vec3(scale, scale, 1.0));
                        draw_text_at(pass, text, transform, -text.size() / 2.0, 1.0);
                    }
                } else {
                    let pads: Vec<Input> = table[action]
                        .iter()
                        .copied()
                        .filter(|i| i.is_pad())
                        .collect();
                    let n = pads.len() as f32;
                    let mut x = center.x - (n * GLYPH + (n - 1.0).max(0.0) * 8.0) / 2.0;
                    for input in pads {
                        prompts::draw_pad(
                            pass,
                            builder,
                            list,
                            t.get("key"),
                            pad_style,
                            input,
                            [x, center.y - GLYPH / 2.0, x + GLYPH, center.y + GLYPH / 2.0],
                            FloatColor4::WHITE,
                        );
                        x += GLYPH + 8.0;
                    }
                }
            }
        }
        // the lines between the rows
        let line = t.get("line");
        for row in 0..ROWS.len() {
            let y = Self::row_y(row) + ITEM_H;
            for (x0, x1, u0, u1) in [
                (0.0, 8.0, 0.0, 0.25),
                (8.0, LIST_W - 8.0, 0.25, 0.75),
                (LIST_W - 8.0, LIST_W, 0.75, 1.0),
            ] {
                Sprite::new(line, [x0, y, x1, y + 14.0])
                    .uv([u0, 0.0, u1, 1.0])
                    .draw(pass, builder, list);
            }
        }
        ui::clip(pass, None, self.target_size);

        Sprite::new(t.get("close"), [1764.0, top + 32.0, 1848.0, top + 116.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);
        Sprite::new(t.get("default"), [1446.0, top + 32.0, 1752.0, top + 116.0])
            .flash(self.default_flash.value())
            .draw(pass, builder, projection);

        // under the list: the conflict notice, or how to give up a wait
        let message = match (&self.notice, &self.capture) {
            (_, Some(_)) => Some((&self.hold_to_cancel, 1.0)),
            (Some((text, left)), None) => Some((text, (left / 20.0).min(1.0))),
            _ => None,
        };
        if let Some((text, alpha)) = message {
            let at = vec2(960.0 - text.size().x / 2.0, 932.0);
            draw_text_at(pass, text, slid, at, alpha);
        }

        // the key help: back, defaults, change, select
        let mut right = 1872.0;
        // "change the value" shows A here: A rebinds
        let adjust = Help {
            glyph: 3,
            ..HELP_ADJUST
        };
        for help in [HELP_SAVE, HELP_DEFAULT, adjust, HELP_SELECT] {
            let v = help.row / 240.0;
            Sprite::new(
                t.get("help_config"),
                [right - help.width, bottom, right, bottom + 48.0],
            )
            .uv([0.0, v, help.width / 320.0, v + 0.2])
            .draw(pass, builder, projection);
            let x = right - help.width + 2.0 - 48.0;
            let rect = [x, bottom, x + 48.0, bottom + 48.0];
            prompts::draw(
                pass,
                builder,
                projection,
                t.get("key"),
                help.glyph,
                rect,
                FloatColor4::WHITE,
            );
            right = x - 16.0;
        }

        if let Some(dialog) = &self.dialog {
            dialog.render(pass);
        }
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        // never leave the game deaf
        if self.capture.is_some() || self.releasing.is_some() {
            bindings::set_capturing(false);
        }
    }
}
