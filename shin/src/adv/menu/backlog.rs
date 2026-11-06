//! The backlog (`shin::MessageBrowser`, ctor `0x71000a20b0`, update `0x71000a3d40`, render `0x71000a4020`, keys
//! `0x71000a6930`): the current chapter's lines, choices and chapter titles from its replay, in newrodin at size 40
//! over `/logview.txa`. A voiced line plays its voice again on A; Y asks to start reading from the selected line.
//!
//! shortcut: the chapter list (X) is not there, and the list cannot be dragged or flung with the pointer.

use std::{collections::HashMap, sync::Arc};

use glam::{Mat4, vec3};
use shin_core::{
    layout::{LayoutParams, MessageTextParser, ParsedCommand},
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
    vm::command::types::{MessageTextLayout, Volume},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, resize::CanvasSize,
};
use tracing::warn;

use super::{
    AdvRequest, MenuAssets, Task, TaskCx,
    chapter_list::{ChapterList, ListEvent},
    dialog::Dialog,
};
use crate::{
    adv::{Log, LogKind},
    asset::texture_archive::NamedTextures,
    audio::{VoicePlayFlags, VoicePlayer},
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    ui::{self, NineSlice},
};

/// "Start reading from the selected line?" (`0x710015c8eb`, English IPS)
const JUMP_QUESTION: &str = "Start reading from the selected line?";

const VIEW: f32 = 730.0;
const LIST_X: f32 = 136.0;
const LIST_Y: f32 = 188.0;
const ROW_W: f32 = 1648.0;
const GAP: f32 = 14.0;
const CLIP: [f32; 4] = [136.0, 183.0, 1784.0, 918.0];
/// The text's outline in pixels and its colour (47, 25, 20)
const OUTLINE: f32 = 2.5;
const BORDER: FloatColor4 = FloatColor4::from_rgba(0.18431, 0.09804, 0.07843, 1.0);
/// Where a line's text starts in its row (both measured against the Switch release in Eden)
const TEXT_X: f32 = 130.0;
const TEXT_Y: f32 = 5.0;

const FOCUS: NineSlice = NineSlice {
    insets: [512.0, 512.0, 12.0, 12.0],
    us: [0.0, 0.4, 0.6, 1.0],
    vs: [0.0, 0.375, 0.625, 1.0],
};

/// The backlog's layouter (`MessageBrowser::Layouter` `vfunc_4`, `vfunc_5`): the text up to the first
/// line break is the speaker's name, written as 【name】 on its own line, or left out when empty (narration, choices,
/// chapter titles)
fn name_line(text: &str) -> String {
    match text.split_once("@r") {
        Some(("", rest)) => rest.to_string(),
        Some((name, rest)) => format!("【{name}】@r{rest}"),
        // no line break: the bracket opens and is never closed
        None => format!("【{text}"),
    }
}

/// `0x71000a3510`: newrodin, width 1480, size 40 at 97 % width, 6 px above and 4 below, no rubi room
fn layout() -> LayoutParams {
    LayoutParams {
        layout_width: 1480.0,
        text_alignment: MessageTextLayout::Justify,
        line_padding_above: 6.0,
        line_padding_below: 4.0,
        line_padding_between: 0.0,
        rubi_size: 16.0,
        text_size: 40.0,
        base_font_horizontal_scale: 0.9697,
        follow_kinsoku_shori_rules: true,
        always_leave_space_for_rubi: false,
        perform_soft_breaks: true,
    }
}

struct Line {
    kind: LogKind,
    /// The text with its colour (`0x710016ee40`: 999 for lines, 977 choices and quizzes, 799 chapters)
    text: String,
    /// The chapter list's label of a marker
    label: String,
    save_position: u32,
    y: f32,
    height: f32,
    jump_flash: Tweener,
    voice_flash: Tweener,
}

impl Line {
    fn voiced(&self) -> bool {
        matches!(self.kind, LogKind::Message { voiced: true })
    }

    /// The line's voices as one spec for the voice player
    fn voices(&self) -> String {
        MessageTextParser::new(&self.text)
            .filter_map(|c| match c {
                ParsedCommand::Voice(voice) => Some(voice),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("|")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Opening,
    Active,
    /// The line flashes before the jump question, then the question
    Asking,
    /// The close button flashes
    Closing,
    /// Fading out, then gone or jumping
    Exiting,
    Done,
}

pub struct Backlog {
    assets: MenuAssets,
    textures: Arc<NamedTextures>,
    lines: Vec<Line>,
    cursor: usize,
    top: f32,
    bottom: f32,
    scroll: Tweener,
    fade: f32,
    state: State,
    close_flash: Tweener,
    dialog: Option<Dialog>,
    jump: Option<u32>,
    /// The laid-out lines on screen, by index
    texts: HashMap<usize, TextBlock>,
    /// The game's current line
    current: usize,
    /// X: the chapter list over the backlog (`+0xf8`)
    chapters: Option<ChapterList>,
    voice: VoicePlayer,
    target_size: (u32, u32),
}

impl Backlog {
    pub fn new(cx: &TaskCx, assets: &MenuAssets, log: Log) -> Option<Self> {
        let textures = match cx.assets.load_sync::<NamedTextures>("/logview.txa") {
            Ok(textures) => textures,
            Err(e) => {
                warn!("Could not load /logview.txa: {e:?}");
                return None;
            }
        };
        let font = &assets.medium_font;
        let mut lines = log
            .entries
            .into_iter()
            .map(|entry| {
                let color = match entry.kind {
                    LogKind::Message { .. } => 999,
                    LogKind::Choice | LogKind::Quiz => 977,
                    LogKind::Chapter => 799,
                };
                let text = format!("@c{color}.{}", name_line(&entry.text));
                // the layout's own height, without the outline's padding (`0x71000a2780`)
                let mut height = TextBlock::measure(font, &text, layout(), 0.0).y;
                if matches!(entry.kind, LogKind::Message { voiced: true }) {
                    height = height.max(76.0);
                }
                Line {
                    kind: entry.kind,
                    text,
                    label: entry.label,
                    save_position: entry.save_position,
                    y: 0.0,
                    height: height + 20.0,
                    jump_flash: Tweener::new(0.0),
                    voice_flash: Tweener::new(0.0),
                }
            })
            .collect::<Vec<_>>();

        // the current line at 0, the others below and above it 14 px apart (`0x71000a2780`)
        let current = log.current;
        let mut bottom = 0.0;
        for line in &mut lines[current..] {
            line.y = bottom;
            bottom += line.height + GAP;
        }
        bottom -= GAP;
        let mut top = 0.0;
        for line in lines[..current].iter_mut().rev() {
            top -= GAP + line.height;
            line.y = top;
        }
        // the current line ends at the bottom of the view
        let scroll = (lines[current].height - VIEW).clamp(top, (bottom - VIEW).max(top));

        Some(Self {
            assets: assets.clone(),
            textures,
            lines,
            cursor: current,
            top,
            bottom,
            scroll: Tweener::new(scroll),
            fade: 0.0,
            state: State::Opening,
            close_flash: Tweener::new(0.0),
            dialog: None,
            jump: None,
            texts: HashMap::new(),
            current,
            chapters: None,
            voice: VoicePlayer::new(cx.audio.clone(), cx.asset_server.clone()),
            target_size: (1920, 1080),
        })
    }

    fn max_scroll(&self) -> f32 {
        (self.bottom - VIEW).max(self.top)
    }

    fn line_top(&self, index: usize) -> f32 {
        self.lines[index].y - self.scroll.value() + LIST_Y
    }

    fn visible(&self, index: usize) -> bool {
        let top = self.line_top(index);
        top < CLIP[3] && top + self.lines[index].height > CLIP[1]
    }

    /// The cursor, and the scroll that keeps it in view over 4 frames
    fn move_cursor(&mut self, cx: &TaskCx, to: usize) {
        if to == self.cursor {
            return;
        }
        cx.sound("move");
        self.cursor = to;
        let line = &self.lines[to];
        let scroll = self.scroll.target_value();
        let target = if line.y < scroll {
            line.y.max(self.top)
        } else if line.y + line.height - scroll > VIEW {
            (line.y + line.height - VIEW).clamp(self.top, self.max_scroll())
        } else {
            return;
        };
        ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
    }

    /// The chapter list's selection: the cursor on `to`, brought into view over 4 frames from where it is now
    fn scroll_to(&mut self, to: usize) {
        self.cursor = to;
        let line = &self.lines[to];
        let current = self.scroll.value();
        let target = if line.y < current {
            line.y.max(self.top)
        } else if line.y + line.height - current > VIEW {
            (line.y + line.height - VIEW).clamp(self.top, self.max_scroll())
        } else {
            return;
        };
        self.scroll = Tweener::new(current);
        ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
    }

    /// `0x71000a7100`: the cursor line flashes, then the jump question
    fn ask_jump(&mut self, cx: &TaskCx) {
        cx.sound("decide");
        let flash = &mut self.lines[self.cursor].jump_flash;
        *flash = Tweener::new(0.0);
        minigame::flash(flash);
        self.state = State::Asking;
    }

    fn close(&mut self, cx: &TaskCx) {
        cx.sound("cancel");
        self.voice.stop();
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.state = State::Closing;
    }

    /// `0x71000a6de0`: a voiced line plays its voice again on the backlog's own player
    fn play_voice(&mut self) {
        let line = &mut self.lines[self.cursor];
        if !line.voiced() {
            return;
        }
        line.voice_flash = Tweener::new(0.0);
        minigame::flash(&mut line.voice_flash);
        let voices = line.voices();
        self.voice.play(
            &self.assets.scenario,
            &voices,
            0,
            0,
            VoicePlayFlags::ENABLE_CHARACTER_MUTING,
            Volume::default(),
        );
    }

    fn handle_keys(&mut self, cx: &mut TaskCx) {
        let input = cx.input;
        let last = self.lines.len() - 1;
        let step = |d: i32| (self.cursor as i32 + d).clamp(0, last as i32) as usize;
        let to = if input.up {
            Some(step(-1))
        } else if input.down {
            Some(step(1))
        } else if input.left {
            Some(step(-4))
        } else if input.right {
            Some(step(4))
        } else if input.r {
            (self.cursor + 1..self.lines.len()).find(|&i| self.lines[i].kind.is_marker())
        } else if input.l {
            (0..self.cursor)
                .rev()
                .find(|&i| self.lines[i].kind.is_marker())
        } else {
            None
        };
        if let Some(to) = to.filter(|&to| to != self.cursor) {
            self.move_cursor(cx, to);
            return;
        }
        if input.cancel {
            self.close(cx);
        } else if input.confirm {
            self.play_voice();
        } else if input.y {
            self.ask_jump(cx);
        } else if input.x {
            cx.sound("decide");
            let markers = self
                .lines
                .iter()
                .map(|line| (line.kind, line.label.as_str()))
                .collect::<Vec<_>>();
            self.chapters = Some(ChapterList::new(&markers, self.cursor, self.current));
        } else if input.scroll != 0.0 {
            let scroll =
                (self.scroll.value() + input.scroll * 32.0).clamp(self.top, self.max_scroll());
            self.scroll = Tweener::new(scroll);
            // the cursor onto a line fully in view
            let in_view = |line: &Line| line.y >= scroll && line.y + line.height <= scroll + VIEW;
            if !in_view(&self.lines[self.cursor]) {
                if let Some(i) = (0..self.lines.len()).find(|&i| in_view(&self.lines[i])) {
                    self.cursor = i;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_help(
        &self,
        pass: &mut RenderPass,
        right: f32,
        y: f32,
        keys: &[u8],
        row: f32,
        width: f32,
        color: FloatColor4,
    ) -> f32 {
        let projection = ui::projection();
        let v = row / 288.0;
        let mut label = Sprite::new(
            self.textures.get("help_logview"),
            [right - width, y, right, y + 48.0],
        )
        .uv([0.0, v, width / 320.0, v + 48.0 / 288.0]);
        label.color = color;
        label.draw(pass, ui::builder(), projection);
        let mut x = right - width + 2.0;
        for &key in keys {
            x -= 48.0;
            let (u, v) = ((key & 7) as f32 / 8.0, (key >> 3) as f32 / 2.0);
            let mut icon = Sprite::new(self.textures.get("key"), [x, y, x + 48.0, y + 48.0]).uv([
                u,
                v,
                u + 0.125,
                v + 0.5,
            ]);
            icon.color = color;
            icon.draw(pass, ui::builder(), projection);
        }
        x - 16.0
    }
}

impl Task for Backlog {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        let frames = cx.frames;
        let size = cx.pre_render.resize_source.handle::<CanvasSize>().get();
        self.target_size = (size.width, size.height);
        let settings = cx.global.settings;
        self.voice
            .set_settings(settings.v2_voicevol, settings.v19, settings.v5_voicepanapot);

        match self.state {
            State::Opening => {
                self.fade = (self.fade + 0.1 * frames).min(1.0);
                if self.fade >= 1.0 {
                    self.state = State::Active;
                }
            }
            State::Active => {
                if let Some(mut chapters) = self.chapters.take() {
                    let flash_done = self.close_flash.is_idle();
                    match chapters.update(cx, &self.assets, flash_done) {
                        ListEvent::None => {}
                        ListEvent::Select(line) => self.scroll_to(line),
                        ListEvent::Close => {
                            self.close_flash = Tweener::new(0.0);
                            minigame::flash(&mut self.close_flash);
                        }
                        ListEvent::Voice => self.play_voice(),
                        ListEvent::Jump => self.ask_jump(cx),
                    }
                    if !chapters.is_done() {
                        self.chapters = Some(chapters);
                    }
                } else if cx.focused && self.scroll.is_idle() {
                    self.handle_keys(cx);
                }
            }
            State::Asking => {
                if let Some(dialog) = &mut self.dialog {
                    let running = dialog.update(&mut cx.reborrow(cx.focused));
                    if dialog.take_events().contains(&(1, 0)) {
                        // `0x71000ad1f0`: the backlog closes and the screen rebuilds at the line's save point
                        self.jump = Some(self.lines[self.cursor].save_position);
                    }
                    if !running {
                        self.dialog = None;
                        // `$_11`: the answer also closes the chapter list
                        if let Some(chapters) = &mut self.chapters {
                            chapters.close();
                        }
                        if self.jump.is_some() {
                            self.voice.stop();
                            self.close_flash = Tweener::new(0.0);
                            self.state = State::Closing;
                        } else {
                            self.state = State::Active;
                        }
                    }
                } else if self.lines[self.cursor].jump_flash.is_idle() {
                    self.dialog = Some(Dialog::yes_no(
                        cx.pre_render,
                        &self.assets,
                        JUMP_QUESTION,
                        1,
                    ));
                }
            }
            State::Closing => {
                if self.close_flash.is_idle() {
                    // `0x71000a67e0`: the adventure screen resumes now, unless it is about to be rebuilt
                    if self.jump.is_none() {
                        cx.requests.push(AdvRequest::SystemMenuClosed);
                    }
                    self.state = State::Exiting;
                }
            }
            State::Exiting => {
                self.fade = (self.fade - 0.1 * frames).max(0.0);
                if self.fade <= 0.0 {
                    if let Some(save_position) = self.jump {
                        cx.requests.push(AdvRequest::JumpTo(save_position));
                    }
                    self.state = State::Done;
                }
            }
            State::Done => return false,
        }

        ui::advance(&mut self.scroll, frames);
        ui::advance(&mut self.close_flash, frames);
        for line in &mut self.lines {
            ui::advance(&mut line.jump_flash, frames);
            ui::advance(&mut line.voice_flash, frames);
        }

        // the visible lines are laid out, the others dropped
        for index in 0..self.lines.len() {
            if !self.visible(index) {
                self.texts.remove(&index);
            } else if !self.texts.contains_key(&index) {
                let text = TextBlock::new(
                    cx.pre_render,
                    &self.assets.medium_font,
                    &self.lines[index].text,
                    layout(),
                    OUTLINE,
                );
                self.texts.insert(index, text);
            }
        }
        true
    }

    fn render(&self, pass: &mut RenderPass) {
        let f = self.fade;
        if f <= 0.0 {
            return;
        }
        let projection = ui::projection();
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let top = f * 160.0 - 160.0;

        Sprite::new(t("bg"), [0.0, 0.0, 1920.0, 1080.0])
            .alpha(f)
            .draw(pass, builder, projection);
        let additive = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer2);
        Sprite::new(t("obi"), [0.0, top, 1920.0, top + 160.0])
            .uv([0.0, 0.0, 1.0, 0.5])
            .alpha(0.25)
            .draw(pass, additive, projection);
        let bottom = 1080.0 - f * 160.0;
        Sprite::new(t("obi"), [0.0, bottom, 1920.0, bottom + 160.0])
            .uv([0.0, 0.5, 1.0, 1.0])
            .alpha(0.25)
            .draw(pass, additive, projection);

        let slide = 1920.0 - f * 1920.0;
        let slid = projection * Mat4::from_translation(vec3(slide, 0.0, 0.0));
        Sprite::new(t("window"), [48.0, 112.0, 48.0 + 1824.0, 112.0 + 898.0])
            .draw(pass, builder, slid);

        let [x0, y0, x1, y1] = CLIP;
        ui::clip(
            pass,
            Some([x0 + slide, y0, x1 + slide, y1]),
            self.target_size,
        );
        for (index, line) in self.lines.iter().enumerate() {
            if !self.visible(index) {
                continue;
            }
            let row = slid * Mat4::from_translation(vec3(LIST_X, self.line_top(index), 0.0));
            let h = line.height;
            for (x0, x1, u0, u1) in [
                (0.0, 8.0, 0.0, 0.25),
                (8.0, ROW_W - 8.0, 0.25, 0.75),
                (ROW_W - 8.0, ROW_W, 0.75, 1.0),
            ] {
                Sprite::new(t("line"), [x0, h, x1, h + GAP])
                    .uv([u0, 0.0, u1, 1.0])
                    .draw(pass, builder, row);
            }
            if index == self.cursor {
                FOCUS.draw(
                    pass,
                    t("focus"),
                    row,
                    [0.0, 0.0, ROW_W, h],
                    FloatColor4::WHITE.with_alpha(0.75),
                );
            }
            if let Some(text) = self.texts.get(&index) {
                ui::draw_text(
                    pass,
                    text,
                    row * Mat4::from_translation(vec3(TEXT_X, TEXT_Y, 0.0)),
                    FloatColor4::WHITE,
                    BORDER,
                );
            }
            if line.voiced() {
                let y = (h - 76.0) / 2.0;
                Sprite::new(t("voice"), [32.0, y, 108.0, y + 76.0])
                    .flash(line.voice_flash.value())
                    .draw(pass, builder, row);
            }
            let flash = line.jump_flash.value();
            if flash > 0.0 {
                ui::fill(
                    pass,
                    row,
                    [0.0, 0.0, ROW_W, h],
                    FloatColor4::WHITE.with_alpha(flash),
                );
            }
        }
        ui::clip(pass, None, self.target_size);

        let window = t("window");
        Sprite::new(window, [136.0, 183.0, 1784.0, 188.0])
            .uv([0.04825, 0.07906, 0.95175, 0.08463])
            .draw(pass, builder, slid);
        Sprite::new(window, [136.0, 918.0, 1784.0, 923.0])
            .uv([0.04825, 0.89755, 0.95175, 0.90312])
            .draw(pass, builder, slid);
        ui::draw_scroll_bar(
            pass,
            self.assets.system_textures.get("scroll"),
            slid,
            (1788.0, LIST_Y),
            VIEW,
            self.scroll.value() - self.top,
            self.bottom - self.top,
        );

        if let Some(chapters) = &self.chapters {
            chapters.render(pass, &self.textures, self.target_size);
        }

        Sprite::new(t("caption"), [480.0, top, 1440.0, top + 128.0])
            .draw(pass, builder, projection);
        Sprite::new(t("close"), [1764.0, top + 32.0, 1848.0, top + 116.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);

        // the key help, right to left (`0x71000a6320`, `0x7100172558`)
        let y = 1080.0 - f * 88.0 + 20.0;
        let grey = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);
        let voice_color = if self.lines[self.cursor].voiced() {
            FloatColor4::WHITE
        } else {
            grey
        };
        let mut x = self.draw_help(pass, 1872.0, y, &[4], 48.0, 72.0, FloatColor4::WHITE);
        x = self.draw_help(pass, x, y, &[3], 96.0, 140.0, voice_color);
        if self.chapters.is_none() {
            x = self.draw_help(pass, x, y, &[5], 192.0, 266.0, FloatColor4::WHITE);
        }
        x = self.draw_help(pass, x, y, &[6], 144.0, 240.0, FloatColor4::WHITE);
        self.draw_help(pass, x, y, &[7, 0], 0.0, 206.0, FloatColor4::WHITE);

        if let Some(dialog) = &self.dialog {
            dialog.render(pass);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::name_line;

    #[test]
    fn the_name_line_is_bracketed_or_dropped() {
        assert_eq!(name_line("Nanjo@r\"Hi.\""), "【Nanjo】@r\"Hi.\"");
        assert_eq!(name_line("@rThe old physician."), "The old physician.");
        assert_eq!(name_line("@rTitle@rChapter"), "Title@rChapter");
    }
}
