//! `shin::SaveMenu` (ctor `0x71000d5b40`, update `0x71000d6640`, render `0x71000d71d0`, keys `0x71000d9560`): the
//! save and load screens, a scrolling list of the 100 bookmarks from `/saveload.txa`. Each visible bookmark shows its
//! number, date and the SAVEINFO texts of its scene, which the original gets by replaying the save in the background;
//! saving, loading and removing ask first.
//!
//! shortcut: the list cannot be dragged or flung with the pointer, and the "Writing save data" notice is not drawn.

use std::sync::Arc;

use glam::{Mat4, Vec2, vec3};
use shin_core::{
    format::save::{GameData, SaveData},
    primitives::color::FloatColor4,
    time::{Easing, Tweener},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, render_texture::RenderTexture,
    resize::CanvasSize,
};
use shin_tasks::ComputeTask;
use tracing::warn;

use super::{
    AdvRequest, MenuAssets, Task, TaskCx,
    dialog::Dialog,
    thumbnail::{THUMBNAIL_UV, ThumbnailBuild},
};
use crate::{
    adv::{
        Bookmark,
        hud::{OUTLINE, text_layout},
    },
    asset::texture_archive::NamedTextures,
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    ui::{self, NineSlice},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Save,
    Load,
}

/// The English IPS's texts (`0x7100162fd0`, `0x7100162147`, `0x7100162c2a`, `0x7100162c58`, `0x710015f8f8`)
const MOVE_QUESTION: &str = "Move this bookmark to the current page?";
const PLACE_QUESTION: &str = "Place this bookmark on the current page?";
const LOAD_QUESTION: &str = "Continue reading from this bookmark?";
const DELETE_QUESTION: &str = "Remove this bookmark?";
const UNUSED: &str = "Unused bookmark";
/// An executable literal the IPS leaves alone
const LOADING: &str = "loading...";

const SLOTS: usize = 100;
/// A bookmark every 186 px, 172 tall, in a 730 px view from y 188
const PITCH: f32 = 186.0;
const SLOT_H: f32 = 172.0;
const VIEW: f32 = 730.0;
const LIST_Y: f32 = 188.0;
const MAX_SCROLL: f32 = SLOTS as f32 * PITCH - 14.0 - VIEW;
/// The list's clip (the stencil quad)
const CLIP: [f32; 4] = [136.0, 183.0, 1784.0, 923.0];

const FOCUS: NineSlice = NineSlice {
    insets: [512.0, 512.0, 12.0, 12.0],
    us: [0.0, 0.4, 0.6, 1.0],
    vs: [0.0, 0.375, 0.625, 1.0],
};

/// The key help (`0x7100173ef0`): keys (the second -1 for none), the label's row and width in `help_saveload`
struct Help {
    keys: [i8; 2],
    row: f32,
    width: f32,
}
const HELP_SELECT: Help = Help {
    keys: [7, 0],
    row: 0.0,
    width: 206.0,
};
const HELP_BACK: Help = Help {
    keys: [4, -1],
    row: 48.0,
    width: 72.0,
};
const HELP_SAVE: Help = Help {
    keys: [3, -1],
    row: 96.0,
    width: 110.0,
};
const HELP_LOAD: Help = Help {
    keys: [3, -1],
    row: 144.0,
    width: 106.0,
};
const HELP_DELETE: Help = Help {
    keys: [6, -1],
    row: 192.0,
    width: 74.0,
};

/// The `num` glyphs: (u, v, w, h) of the slot number's digits and '.' and of the date's digits, '/' and ':'
/// (`0x7100173e40`, `0x7100173d80`)
fn big_glyph(c: char) -> Option<[f32; 4]> {
    match c {
        '0'..='9' => Some([36.0 * (c as u8 - b'0') as f32, 32.0, 36.0, 48.0]),
        '.' => Some([396.0, 32.0, 18.0, 48.0]),
        _ => None,
    }
}

fn small_glyph(c: char) -> Option<[f32; 4]> {
    match c {
        '0'..='9' => Some([24.0 * (c as u8 - b'0') as f32, 0.0, 24.0, 32.0]),
        '/' => Some([240.0, 0.0, 24.0, 32.0]),
        ':' => Some([264.0, 0.0, 14.0, 32.0]),
        _ => None,
    }
}

/// The date as the original prints it (`"%04d/%02d/%02d %2d:%02d"`)
fn date_string(data: &GameData) -> String {
    data.date_time.format("%Y/%m/%d %_H:%M").to_string()
}

/// What a bookmark shows: nothing yet, or its text objects
enum SlotTexts {
    One(TextBlock),
    Two(TextBlock, TextBlock),
}

struct Slot {
    /// The replayed save's SAVEINFO texts and scene, kept while the menu is open
    bookmark: Option<Bookmark>,
    /// Built while on screen, dropped when it leaves
    texts: Option<SlotTexts>,
    thumbnail: Option<RenderTexture>,
    flash: Tweener,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Fading in (`Start`, 10 frames)
    Opening,
    Active,
    /// The close button's flash plays
    Closing,
    /// Fading out (`Exit`), then gone or loading
    Exiting,
    Done,
}

/// What waits for the bookmark's flash, then asks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Save,
    Load,
    Delete,
}

pub struct SaveMenu {
    mode: Mode,
    assets: MenuAssets,
    textures: Arc<NamedTextures>,
    state: State,
    open: f32,
    cursor: usize,
    scroll: Tweener,
    close_flash: Tweener,
    slots: Vec<Slot>,
    /// The bookmarks as of this frame, for drawing
    saves: Vec<Option<GameData>>,
    /// The replay running for a bookmark's texts (`ThumbMaker::StateBuilder`)
    building: Option<(usize, ComputeTask<Bookmark>)>,
    /// The thumbnail being rendered (one at a time, as the original's renderer)
    drawing: Option<(usize, ThumbnailBuild)>,
    asking: Option<Ask>,
    dialog: Option<(Ask, Dialog)>,
    load_requested: bool,
    target_size: (u32, u32),
    pointer_held: bool,
    pressed: Option<Target>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Close,
    Slot(usize),
}

fn slot_layout() -> shin_core::layout::LayoutParams {
    shin_core::layout::LayoutParams {
        always_leave_space_for_rubi: false,
        ..text_layout(1278.0, 40.0)
    }
}

impl SaveMenu {
    /// The screen in `mode`; `None` when `/saveload.txa` cannot be read
    pub fn new(cx: &TaskCx, assets: &MenuAssets, mode: Mode) -> Option<Self> {
        let textures = match cx.assets.load_sync::<NamedTextures>("/saveload.txa") {
            Ok(textures) => textures,
            Err(e) => {
                warn!("Could not load /saveload.txa: {e:?}");
                return None;
            }
        };
        let cursor = (cx.global.save_menu_position as usize).min(SLOTS - 1);
        let scroll = (PITCH * cursor as f32 - 365.0 + 86.0).clamp(0.0, MAX_SCROLL);
        Some(Self {
            mode,
            assets: assets.clone(),
            textures,
            state: State::Opening,
            open: 0.0,
            cursor,
            scroll: Tweener::new(scroll),
            close_flash: Tweener::new(0.0),
            slots: (0..SLOTS)
                .map(|_| Slot {
                    bookmark: None,
                    texts: None,
                    thumbnail: None,
                    flash: Tweener::new(0.0),
                })
                .collect(),
            saves: cx.global.manual_save_slots.to_vec(),
            building: None,
            drawing: None,
            asking: None,
            dialog: None,
            load_requested: false,
            target_size: (1920, 1080),
            pointer_held: false,
            pressed: None,
        })
    }

    fn slot_top(&self, index: usize) -> f32 {
        LIST_Y + PITCH * index as f32 - self.scroll.value()
    }

    fn visible(&self, index: usize) -> bool {
        let y = self.slot_top(index);
        y < 1080.0 && y + SLOT_H >= 0.0
    }

    /// Moves the cursor and scrolls it into view over 4 frames
    fn move_cursor(&mut self, cx: &TaskCx, to: usize) {
        if to == self.cursor {
            return;
        }
        cx.sound("move");
        self.cursor = to;
        self.scroll_into_view();
    }

    fn scroll_into_view(&mut self) {
        let y = PITCH * self.cursor as f32;
        let scroll = self.scroll.target_value();
        let target = if y < scroll {
            y
        } else if y - scroll + SLOT_H > VIEW {
            (y + SLOT_H - VIEW).min(MAX_SCROLL)
        } else {
            return;
        };
        ui::tween(&mut self.scroll, target, 4.0, Easing::Linear);
    }

    fn flash_slot(&mut self, index: usize) {
        let flash = &mut self.slots[index].flash;
        *flash = Tweener::new(0.0);
        minigame::flash(flash);
    }

    /// `0x71000d97f0`: the bookmark flashes, then the save or load question
    fn decide(&mut self, cx: &TaskCx) {
        self.flash_slot(self.cursor);
        match self.mode {
            Mode::Save => {
                cx.sound("decide");
                self.asking = Some(Ask::Save);
            }
            Mode::Load if self.saves[self.cursor].is_some() => {
                cx.sound("decide");
                self.asking = Some(Ask::Load);
            }
            Mode::Load => cx.sound("error"),
        }
    }

    /// `0x71000d8fe0`: Y in the save screen removes a bookmark
    fn delete(&mut self, cx: &TaskCx) {
        self.flash_slot(self.cursor);
        if self.saves[self.cursor].is_some() {
            cx.sound("decide");
            self.asking = Some(Ask::Delete);
        } else {
            cx.sound("error");
        }
    }

    /// `0x71000d92c0`: the close button flashes, then the screen fades
    fn close(&mut self, cx: &TaskCx) {
        cx.sound("cancel");
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
        self.state = State::Closing;
    }

    fn handle_keys(&mut self, cx: &mut TaskCx) {
        let input = cx.input;
        let step: i32 = if input.up {
            -1
        } else if input.down {
            1
        } else if input.left {
            -4
        } else if input.right {
            4
        } else if input.l {
            -10
        } else if input.r {
            10
        } else {
            0
        };
        let to = (self.cursor as i32 + step).clamp(0, SLOTS as i32 - 1) as usize;
        if to != self.cursor {
            self.move_cursor(cx, to);
            return;
        }
        if input.confirm {
            self.decide(cx);
        } else if input.y && self.mode == Mode::Save {
            self.delete(cx);
        } else if input.cancel {
            self.close(cx);
        } else if input.scroll != 0.0 {
            // the right stick scrolls at once, the cursor following into view
            let scroll = (self.scroll.value() + input.scroll * 32.0).clamp(0.0, MAX_SCROLL);
            self.scroll = Tweener::new(scroll);
            while PITCH * (self.cursor as f32) < scroll && self.cursor + 1 < SLOTS {
                self.cursor += 1;
            }
            while PITCH * (self.cursor as f32) + SLOT_H > scroll + VIEW && self.cursor > 0 {
                self.cursor -= 1;
            }
        }
    }

    fn target_at(&self, point: Vec2) -> Option<Target> {
        if minigame::contains([1764.0, 32.0, 1848.0, 116.0], point) {
            return Some(Target::Close);
        }
        if !minigame::contains([136.0, LIST_Y, 1784.0, 918.0], point) {
            return None;
        }
        let k = self.scroll.value() - LIST_Y + point.y;
        (k.rem_euclid(PITCH) < SLOT_H)
            .then(|| (k / PITCH) as usize)
            .filter(|&slot| slot < SLOTS)
            .map(Target::Slot)
    }

    /// A press flashes the bookmark (or the close button); releasing on it decides
    fn handle_pointer(&mut self, cx: &mut TaskCx) {
        let pointer = cx.input.pointer;
        let went_down = pointer.held && !self.pointer_held;
        let went_up = !pointer.held && self.pointer_held;
        self.pointer_held = pointer.held;
        let hit = pointer.position.and_then(|p| self.target_at(p));
        if went_down {
            match hit {
                Some(Target::Slot(slot)) => {
                    minigame::raise(&mut self.slots[slot].flash);
                    self.cursor = slot;
                    self.scroll_into_view();
                }
                Some(Target::Close) => minigame::raise(&mut self.close_flash),
                None => {}
            }
            self.pressed = hit;
        } else if went_up {
            let Some(pressed) = self.pressed.take() else {
                return;
            };
            if hit != Some(pressed) {
                match pressed {
                    Target::Slot(slot) => minigame::lower(&mut self.slots[slot].flash),
                    Target::Close => minigame::lower(&mut self.close_flash),
                }
                return;
            }
            match pressed {
                Target::Slot(_) => self.decide(cx),
                Target::Close => {
                    cx.sound("decide");
                    self.close(cx);
                }
            }
        }
    }

    fn open_dialog(&mut self, cx: &TaskCx, ask: Ask) {
        let message = match ask {
            Ask::Save if self.saves[self.cursor].is_some() => MOVE_QUESTION,
            Ask::Save => PLACE_QUESTION,
            Ask::Load => LOAD_QUESTION,
            Ask::Delete => DELETE_QUESTION,
        };
        self.dialog = Some((ask, Dialog::yes_no(cx.pre_render, &self.assets, message, 1)));
    }

    /// Drops what the bookmark showed, so it is rebuilt from its new contents
    fn forget(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        slot.bookmark = None;
        slot.texts = None;
        slot.thumbnail = None;
        if matches!(self.building, Some((slot, _)) if slot == index) {
            self.building = None;
        }
        if matches!(self.drawing, Some((slot, _)) if slot == index) {
            self.drawing = None;
        }
    }

    fn update_dialog(&mut self, cx: &mut TaskCx) {
        let Some((ask, dialog)) = &mut self.dialog else {
            return;
        };
        let ask = *ask;
        let running = dialog.update(&mut cx.reborrow(cx.focused));
        let events = dialog.take_events();
        if !running {
            self.dialog = None;
        }
        for event in events {
            match (ask, event) {
                // `0x71000da920`: the bookmark takes the current page, and the file is written
                (Ask::Save, (0, 0)) => {
                    cx.requests.push(AdvRequest::SaveToSlot(self.cursor));
                    self.forget(self.cursor);
                }
                // `0x71000db120`: the screen fades out and the bookmark is loaded
                (Ask::Load, (1, 0)) => {
                    self.load_requested = true;
                    self.state = State::Exiting;
                }
                // `0x71000db5c0`
                (Ask::Delete, (0, 0)) => {
                    cx.global.manual_save_slots[self.cursor] = None;
                    cx.requests.push(AdvRequest::WriteSave);
                    self.forget(self.cursor);
                }
                _ => {}
            }
        }
    }

    /// The bookmarks' texts and thumbnails: built for the visible ones, dropped for the others. One replay at a time
    /// fetches the SAVEINFO texts and scene of the lowest visible bookmark that has none, and one thumbnail at a time
    /// is rendered from a replayed scene (`0x71000d6640`)
    fn update_slots(&mut self, cx: &mut TaskCx) {
        if let Some((slot, task)) = &mut self.building {
            if let Some(bookmark) = task.poll_naive() {
                let slot = *slot;
                self.slots[slot].bookmark = Some(bookmark);
                self.slots[slot].texts = None;
                self.building = None;
            }
        }
        if let Some((slot, build)) = &mut self.drawing {
            if let Some(texture) = build.poll(cx) {
                let slot = *slot;
                if self.visible(slot) {
                    self.slots[slot].thumbnail = Some(texture);
                }
                self.drawing = None;
            }
        }
        for index in 0..SLOTS {
            if !self.visible(index) {
                self.slots[index].texts = None;
                self.slots[index].thumbnail = None;
                continue;
            }
            if self.drawing.is_none() && self.slots[index].thumbnail.is_none() {
                if let Some(bookmark) = &self.slots[index].bookmark {
                    let build = ThumbnailBuild::start(cx, &self.assets.scenario, &bookmark.layers);
                    self.drawing = Some((index, build));
                }
            }
            if self.building.is_none() && self.slots[index].bookmark.is_none() {
                if let Some(data) = &self.saves[index] {
                    let scenario = self.assets.scenario.clone();
                    let global: SaveData = cx.global.clone();
                    let entry = data.entry.clone();
                    self.building = Some((
                        index,
                        shin_tasks::compute::spawn(move || {
                            crate::adv::replay_bookmark(&scenario, global, entry)
                        }),
                    ));
                }
            }
            if self.slots[index].texts.is_some() {
                continue;
            }
            let font = &self.assets.system_font;
            let text = |s: &str| TextBlock::new(cx.pre_render, font, s, slot_layout(), OUTLINE);
            self.slots[index].texts =
                Some(match (&self.saves[index], &self.slots[index].bookmark) {
                    (None, _) => SlotTexts::One(text(UNUSED)),
                    (Some(_), None) => SlotTexts::One(text(LOADING)),
                    (Some(_), Some(bookmark)) => {
                        let [first, second] = &bookmark.info;
                        SlotTexts::Two(text(first), text(second))
                    }
                });
        }
    }

    fn draw_number(&self, pass: &mut RenderPass, transform: Mat4, text: &str, x: f32, small: bool) {
        let num = self.textures.get("num");
        let glyph = if small { small_glyph } else { big_glyph };
        let (shrink, nudge, top) = if small {
            (5.0, 2.5, 18.0)
        } else {
            (8.0, 4.0, 2.0)
        };
        let space = if small { 16.0 } else { 20.0 };
        let advance = |c: char| glyph(c).map_or(space, |[_, _, w, _]| w - shrink);
        // the date is right-aligned at `x`
        let mut pen = if small {
            x - text.chars().map(advance).sum::<f32>()
        } else {
            x
        };
        for c in text.chars() {
            if let Some([u, v, w, h]) = glyph(c) {
                let left = pen - nudge;
                Sprite::new(num, [left, top, left + w, top + h])
                    .uv([u / 512.0, v / 80.0, (u + w) / 512.0, (v + h) / 80.0])
                    .draw(pass, ui::builder(), transform);
            }
            pen += advance(c);
        }
    }

    fn draw_slot(&self, pass: &mut RenderPass, list: Mat4, index: usize) {
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let transform = list * Mat4::from_translation(vec3(160.0, self.slot_top(index) + 5.0, 0.0));
        if index != 0 {
            for (x0, x1, u0, u1) in [
                (-24.0, -16.0, 0.0, 0.25),
                (-16.0, 1616.0, 0.25, 0.75),
                (1616.0, 1624.0, 0.75, 1.0),
            ] {
                Sprite::new(t("line"), [x0, -19.0, x1, -5.0])
                    .uv([u0, 0.0, u1, 1.0])
                    .draw(pass, builder, transform);
            }
        }
        if index == self.cursor {
            FOCUS.draw(
                pass,
                t("focus"),
                transform,
                [-24.0, -5.0, 1624.0, 167.0],
                FloatColor4::WHITE.with_alpha(0.75),
            );
        }
        let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
        Sprite::new(t("bar"), [312.0, 32.0, 1600.0, 48.0]).draw(pass, opaque, transform);
        self.draw_number(pass, transform, &format!("{}.", index + 1), 320.0, false);
        if let Some(data) = &self.saves[index] {
            self.draw_number(pass, transform, &date_string(data), 1592.0, true);
        }

        // white with a dark brown outline (`0x71000d6640`)
        let border = FloatColor4::from_rgba(0.1843, 0.0980, 0.0784, 1.0);
        let at = |y: f32| transform * Mat4::from_translation(vec3(317.0, y, 0.0));
        match &self.slots[index].texts {
            Some(SlotTexts::One(text)) => {
                ui::draw_text(pass, text, at(87.0), FloatColor4::WHITE, border)
            }
            Some(SlotTexts::Two(first, second)) => {
                ui::draw_text(pass, first, at(60.0), FloatColor4::WHITE, border);
                ui::draw_text(pass, second, at(114.0), FloatColor4::WHITE, border);
            }
            None => {}
        }

        match &self.slots[index].thumbnail {
            Some(thumbnail) => ui::draw_texture(
                pass,
                thumbnail.as_texture_source(),
                transform,
                [6.0, 0.0, 294.0, 162.0],
                THUMBNAIL_UV,
                FloatColor4::WHITE,
                opaque,
            ),
            None => {
                Sprite::new(t("nodata"), [6.0, 0.0, 294.0, 162.0]).draw(pass, opaque, transform)
            }
        }

        let flash = self.slots[index].flash.value();
        if flash > 0.0 {
            ui::fill(
                pass,
                transform,
                [-24.0, -5.0, 1624.0, 167.0],
                FloatColor4::WHITE.with_alpha(flash),
            );
        }
    }

    fn draw_help(
        &self,
        pass: &mut RenderPass,
        mut x: f32,
        y: f32,
        help: &Help,
        color: FloatColor4,
    ) -> f32 {
        let projection = ui::projection();
        let v = help.row / 240.0;
        let mut label = Sprite::new(
            self.textures.get("help_saveload"),
            [x - help.width, y, x, y + 48.0],
        )
        .uv([0.0, v, help.width / 320.0, v + 0.2]);
        label.color = color;
        label.draw(pass, ui::builder(), projection);
        x = x - help.width + 2.0;
        for key in help.keys.into_iter().filter(|&key| key >= 0) {
            x -= 48.0;
            let (u, v) = ((key & 7) as f32 * 0.125, (key >> 3) as f32 * 0.5);
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

impl Task for SaveMenu {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        let frames = cx.frames;
        let size = cx.pre_render.resize_source.handle::<CanvasSize>().get();
        self.target_size = (size.width, size.height);

        match self.state {
            State::Opening => {
                self.open = (self.open + 0.1 * frames).min(1.0);
                if self.open >= 1.0 {
                    self.state = State::Active;
                }
            }
            State::Active => {
                if self.dialog.is_some() {
                    self.update_dialog(cx);
                } else if let Some(ask) = self.asking {
                    if self.slots[self.cursor].flash.is_idle() {
                        self.asking = None;
                        self.open_dialog(cx, ask);
                    }
                } else if cx.focused {
                    self.handle_pointer(cx);
                    if self.pressed.is_none()
                        && self.scroll.is_idle()
                        && self.state == State::Active
                    {
                        self.handle_keys(cx);
                    }
                }
            }
            State::Closing => {
                if self.close_flash.is_idle() {
                    self.state = State::Exiting;
                }
            }
            State::Exiting => {
                self.open = (self.open - 0.1 * frames).max(0.0);
                if self.open <= 0.0 {
                    if self.load_requested {
                        cx.requests.push(AdvRequest::LoadSlot(self.cursor));
                    }
                    self.state = State::Done;
                }
            }
            State::Done => return false,
        }

        self.saves = cx.global.manual_save_slots.to_vec();
        self.update_slots(cx);
        ui::advance(&mut self.scroll, frames);
        ui::advance(&mut self.close_flash, frames);
        for slot in &mut self.slots {
            ui::advance(&mut slot.flash, frames);
        }
        true
    }

    fn render(&self, pass: &mut RenderPass) {
        let o = self.open;
        if o <= 0.0 {
            return;
        }
        let projection = ui::projection();
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);

        Sprite::new(t("bg"), [0.0, 0.0, 1920.0, 1080.0])
            .alpha(o)
            .draw(pass, builder, projection);
        let additive = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer2);
        Sprite::new(t("obi"), [0.0, 160.0 * o - 160.0, 1920.0, 160.0 * o])
            .uv([0.0, 0.0, 1.0, 0.5])
            .alpha(0.25)
            .draw(pass, additive, projection);
        Sprite::new(
            t("obi"),
            [0.0, 1080.0 - 160.0 * o, 1920.0, 1240.0 - 160.0 * o],
        )
        .uv([0.0, 0.5, 1.0, 1.0])
        .alpha(0.25)
        .draw(pass, additive, projection);

        let slide = 1920.0 - 1920.0 * o;
        let list = projection * Mat4::from_translation(vec3(slide, 0.0, 0.0));
        Sprite::new(t("window"), [48.0, 112.0, 48.0 + 1824.0, 112.0 + 898.0])
            .draw(pass, builder, list);

        let [x0, y0, x1, y1] = CLIP;
        ui::clip(
            pass,
            Some([x0 + slide, y0, x1 + slide, y1]),
            self.target_size,
        );
        for index in (0..SLOTS).filter(|&i| self.visible(i)) {
            self.draw_slot(pass, list, index);
        }
        ui::clip(pass, None, self.target_size);

        let window = t("window");
        let clear = FloatColor4::WHITE.with_alpha(0.0);
        ui::gradient_sprite(
            pass,
            window,
            list,
            [136.0, 183.0, 1784.0, 188.0],
            [0.048246, 0.079065, 0.951754, 0.084633],
            FloatColor4::WHITE,
            clear,
        );
        ui::gradient_sprite(
            pass,
            window,
            list,
            [136.0, 918.0, 1784.0, 923.0],
            [0.048246, 0.89755, 0.951754, 0.903118],
            clear,
            FloatColor4::WHITE,
        );
        ui::draw_scroll_bar(
            pass,
            self.assets.system_textures.get("scroll"),
            list,
            (1788.0, LIST_Y),
            VIEW,
            self.scroll.value(),
            SLOTS as f32 * PITCH - 14.0,
        );

        let c = 160.0 * o - 160.0;
        let caption = match self.mode {
            Mode::Save => "caption_save",
            Mode::Load => "caption_load",
        };
        Sprite::new(t(caption), [480.0, c, 1440.0, c + 128.0]).draw(pass, builder, projection);
        Sprite::new(t("close"), [1764.0, c + 32.0, 1848.0, c + 116.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);

        let y = 1080.0 - 88.0 * o + 20.0;
        let grey = FloatColor4::from_rgba(0.502, 0.502, 0.502, 1.0);
        let valid = self.saves[self.cursor].is_some();
        let mut x = self.draw_help(pass, 1872.0, y, &HELP_BACK, FloatColor4::WHITE);
        match self.mode {
            Mode::Save => {
                x = self.draw_help(pass, x, y, &HELP_SAVE, FloatColor4::WHITE);
                x = self.draw_help(
                    pass,
                    x,
                    y,
                    &HELP_DELETE,
                    if valid { FloatColor4::WHITE } else { grey },
                );
            }
            Mode::Load => {
                x = self.draw_help(
                    pass,
                    x,
                    y,
                    &HELP_LOAD,
                    if valid { FloatColor4::WHITE } else { grey },
                );
            }
        }
        self.draw_help(pass, x, y, &HELP_SELECT, FloatColor4::WHITE);

        if let Some((_, dialog)) = &self.dialog {
            dialog.render(pass);
        }
    }
}
