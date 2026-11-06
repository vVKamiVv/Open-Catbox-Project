//! `shin::ConfigMenu` (ctor `0x71000586b0`, update `0x710005b270`, render `0x710005b520`, keys `0x710005cc20`): the
//! settings screen, three tabs of sliders and On/Off buttons over `/config.txa`. Settings change live (every player
//! reads them each frame); closing writes the save file when anything changed.
//!
//! shortcut: the "Writing save data" notice the original shows while it writes is not drawn, and the visual touch
//! feedback setting has no effect, the port having no touch screen.

use std::sync::Arc;

use glam::{Mat4, Vec2, vec3};
use shin_core::{format::save::Settings, time::Tweener};
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use tracing::warn;

use super::{AdvRequest, MenuAssets, Task, TaskCx, dialog::Dialog};
use crate::{
    asset::texture_archive::NamedTextures,
    layer::user::minigame::{self, Sprite},
    ui::{self, NineSlice},
};
use shin_core::primitives::color::FloatColor4;

/// "Reset ALL settings to default?" (`0x710015f87a`, English IPS)
const RESET_QUESTION: &str = "Reset ALL settings to default?";
/// The notice of the standby setting (`0x7100161d8e`, English IPS)
const STANDBY_NOTICE: &str =
    "Automatic sleep will now be disabled@rwhen using Auto Mode and the Music Box.";

/// The list's viewport: items from (136, 252), 1648 wide
const LIST_X: f32 = 136.0;
const LIST_Y: f32 = 252.0;
const LIST_W: f32 = 1648.0;
const ITEM_H: f32 = 88.0;
const ITEM_STEP: f32 = 102.0;

/// The focused item's frame (`0x7100057690`): 512 px ends from the texture's outer fifths, 12 px top and bottom
const FOCUS: NineSlice = NineSlice {
    insets: [512.0, 512.0, 12.0, 12.0],
    us: [0.0, 0.4, 0.6, 1.0],
    vs: [0.0, 0.375, 0.625, 1.0],
};

/// The key help (`0x710016f53c`): the key's glyph in `key`, the label's row and width in `help_config`
#[derive(Clone, Copy)]
struct Help {
    glyph: u8,
    row: f32,
    width: f32,
}
const HELP_SELECT: Help = Help {
    glyph: 1,
    row: 0.0,
    width: 206.0,
};
const HELP_SAVE: Help = Help {
    glyph: 4,
    row: 48.0,
    width: 72.0,
};
const HELP_PAGES: Help = Help {
    glyph: 7,
    row: 96.0,
    width: 236.0,
};
const HELP_DEFAULT: Help = Help {
    glyph: 6,
    row: 144.0,
    width: 174.0,
};
const HELP_ADJUST: Help = Help {
    glyph: 2,
    row: 192.0,
    width: 174.0,
};

#[derive(Clone, Copy)]
enum Kind {
    /// A 0-100 slider; the type picks the art of its - and + (0 volume, 1 opacity, 2 speed)
    Slider(u8),
    /// Buttons for the options, each showing a row of `right` (0 On, 1 Off, 4 Show, 5 Hide); the setting is the
    /// selected option's index
    Buttons(&'static [u8]),
}

const OFF_ON: &[u8] = &[1, 0];
const HIDE_SHOW: &[u8] = &[5, 4];

/// One row of InitMenu (`0x7100058e90`): its tab, its label's row in `left`, and the setting it edits
struct ItemDef {
    page: usize,
    label: u8,
    kind: Kind,
    get: fn(&Settings) -> u8,
    set: fn(&mut Settings, u8),
}

const ITEMS: &[ItemDef] = &[
    ItemDef {
        page: 0,
        label: 0,
        kind: Kind::Slider(0),
        get: |s| s.v3_sysvol,
        set: |s, v| s.v3_sysvol = v,
    },
    ItemDef {
        page: 0,
        label: 1,
        kind: Kind::Slider(0),
        get: |s| s.v0_bgmvol,
        set: |s, v| s.v0_bgmvol = v,
    },
    ItemDef {
        page: 0,
        label: 2,
        kind: Kind::Slider(0),
        get: |s| s.v1_sfxvol,
        set: |s, v| s.v1_sfxvol = v,
    },
    ItemDef {
        page: 0,
        label: 3,
        kind: Kind::Slider(0),
        get: |s| s.v2_voicevol,
        set: |s, v| s.v2_voicevol = v,
    },
    ItemDef {
        page: 0,
        label: 4,
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v4_voicefocus as u8,
        set: |s, v| s.v4_voicefocus = v != 0,
    },
    ItemDef {
        page: 0,
        label: 18,
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v5_voicepanapot as u8,
        set: |s, v| s.v5_voicepanapot = v != 0,
    },
    ItemDef {
        page: 1,
        label: 8,
        kind: Kind::Slider(2),
        get: |s| s.v9_msgspeed,
        set: |s, v| s.v9_msgspeed = v,
    },
    ItemDef {
        page: 1,
        label: 11,
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v11_disallowskipunread as u8,
        set: |s, v| s.v11_disallowskipunread = v != 0,
    },
    ItemDef {
        page: 1,
        label: 15,
        kind: Kind::Slider(1),
        get: |s| s.v13_msgwinalpha,
        set: |s, v| s.v13_msgwinalpha = v,
    },
    ItemDef {
        page: 2,
        label: 10,
        kind: Kind::Slider(2),
        get: |s| s.v10_skipspeed,
        set: |s, v| s.v10_skipspeed = v,
    },
    ItemDef {
        page: 2,
        label: 16,
        kind: Kind::Buttons(HIDE_SHOW),
        get: |s| s.v15_showsongtitle as u8,
        set: |s, v| s.v15_showsongtitle = v != 0,
    },
    ItemDef {
        page: 2,
        label: 19,
        kind: Kind::Buttons(HIDE_SHOW),
        get: |s| s.v14_showscenetitle as u8,
        set: |s, v| s.v14_showscenetitle = v != 0,
    },
    ItemDef {
        page: 2,
        label: 17,
        kind: Kind::Buttons(HIDE_SHOW),
        get: |s| s.v17_showtoucheffect as u8,
        set: |s, v| s.v17_showtoucheffect = v != 0,
    },
    ItemDef {
        page: 2,
        label: 12,
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v18 as u8,
        set: |s, v| s.v18 = v != 0,
    },
];

/// The standby item: turning it on shows a notice (`$_14` `0x710005fdd0`)
const STANDBY_LABEL: u8 = 12;

struct Item {
    def: &'static ItemDef,
    /// Top in the list's space
    y: f32,
    focused: bool,
    /// A slider's focus glow (`+0x70`)
    focus: Tweener,
    /// A button item's per-option highlight and press flash
    options: Vec<(Tweener, Tweener)>,
}

impl Item {
    fn new(def: &'static ItemDef, y: f32) -> Self {
        let options = match def.kind {
            Kind::Slider(_) => Vec::new(),
            Kind::Buttons(options) => options
                .iter()
                .map(|_| (Tweener::new(0.0), Tweener::new(0.0)))
                .collect(),
        };
        Self {
            def,
            y,
            focused: false,
            focus: Tweener::new(0.0),
            options,
        }
    }

    /// The tweener focusing moves: a slider's glow, or the selected option's highlight
    fn focus_tweener(&mut self, settings: &Settings) -> &mut Tweener {
        match self.def.kind {
            Kind::Slider(_) => &mut self.focus,
            Kind::Buttons(_) => &mut self.options[(self.def.get)(settings) as usize].0,
        }
    }

    /// vfuncs 3 and 4: to 1 at 0.25 per frame, back at 0.2, or at once
    fn set_focus(&mut self, settings: &Settings, focused: bool, animate: bool) {
        self.focused = focused;
        let tweener = self.focus_tweener(settings);
        let (target, rate) = if focused { (1.0, 0.25) } else { (0.0, 0.2) };
        if animate {
            minigame::tween_now(tweener, target, rate);
        } else {
            *tweener = Tweener::new(target);
        }
    }

    fn update(&mut self, frames: f32) {
        ui::advance(&mut self.focus, frames);
        for (highlight, flash) in &mut self.options {
            ui::advance(highlight, frames);
            ui::advance(flash, frames);
        }
    }

    /// The x of option `k` in the list's space
    fn option_x(&self, k: usize) -> f32 {
        1384.0 - 252.0 * (self.options.len() - 1 - k) as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Fading in (`First`, `$_16`)
    Opening,
    Active,
    /// The close button's flash plays (`$_17`)
    Closing,
    /// Fading out (`$_18`)
    FadingOut,
    Done,
}

enum Child {
    Reset(Dialog),
    Notice(Dialog),
}

pub struct ConfigMenu {
    assets: MenuAssets,
    textures: Arc<NamedTextures>,
    state: State,
    fade: f32,
    page: usize,
    /// The cursor of each page, remembered across tab changes
    cursors: [usize; 3],
    pages: [Vec<Item>; 3],
    /// Per tab: selected (cross-fades `tab_off` to `tab_on`) and the press flash (scales it up)
    tabs: [(Tweener, Tweener); 3],
    close_flash: Tweener,
    default_flash: Tweener,
    /// The settings when the screen opened (`+0x534`)
    opened_with: Settings,
    /// The settings as of this frame, for drawing
    live: Settings,
    /// The reset dialog waits for the Reset button's flash
    reset_pending: bool,
    child: Option<Child>,
    pointer_held: bool,
    /// The slider being dragged, with the pointer's x at the last frame
    drag: Option<(usize, f32)>,
    drag_value: f32,
}

fn build_pages() -> [Vec<Item>; 3] {
    let mut pages: [Vec<Item>; 3] = Default::default();
    for def in ITEMS {
        let y = ITEM_STEP * pages[def.page].len() as f32;
        pages[def.page].push(Item::new(def, y));
    }
    pages
}

impl ConfigMenu {
    /// The screen over the caller; `None` when `/config.txa` cannot be read
    pub fn new(cx: &TaskCx, assets: &MenuAssets) -> Option<Self> {
        let textures = match cx.assets.load_sync::<NamedTextures>("/config.txa") {
            Ok(textures) => textures,
            Err(e) => {
                warn!("Could not load /config.txa: {e:?}");
                return None;
            }
        };
        let settings = cx.global.settings;
        let mut menu = Self {
            assets: assets.clone(),
            textures,
            state: State::Opening,
            fade: 0.0,
            page: 0,
            cursors: [0; 3],
            pages: build_pages(),
            tabs: [
                (Tweener::new(1.0), Tweener::new(0.0)),
                (Tweener::new(0.0), Tweener::new(0.0)),
                (Tweener::new(0.0), Tweener::new(0.0)),
            ],
            close_flash: Tweener::new(0.0),
            default_flash: Tweener::new(0.0),
            opened_with: settings,
            live: settings,
            reset_pending: false,
            child: None,
            pointer_held: false,
            drag: None,
            drag_value: 0.0,
        };
        menu.pages[0][0].set_focus(&settings, true, false);
        Some(menu)
    }

    fn cursor(&self) -> usize {
        self.cursors[self.page]
    }

    /// `0x710005cf10`: the tab and its remembered cursor
    fn select_page(&mut self, settings: &Settings, page: usize) {
        let old = self.page;
        minigame::tween_now(&mut self.tabs[old].0, 0.0, 0.25);
        let cursor = self.cursor();
        self.pages[old][cursor].set_focus(settings, false, false);
        self.page = page;
        minigame::tween_now(&mut self.tabs[page].0, 1.0, 0.25);
        let cursor = self.cursor();
        self.pages[page][cursor].set_focus(settings, true, false);
    }

    /// `0x710005d120`
    fn select_item(&mut self, settings: &Settings, index: usize) {
        let page = self.page;
        let old = self.cursors[page];
        self.pages[page][old].set_focus(settings, false, true);
        self.cursors[page] = index;
        self.pages[page][index].set_focus(settings, true, true);
    }

    /// Writes a new value through the item's setter, with the "move" sound when it changed
    fn set_value(&mut self, cx: &mut TaskCx, index: usize, value: u8) {
        let item = &mut self.pages[self.page][index];
        let old = (item.def.get)(&cx.global.settings);
        if old == value {
            return;
        }
        if let Kind::Buttons(_) = item.def.kind {
            minigame::tween_now(&mut item.options[old as usize].0, 0.0, 0.2);
            minigame::tween_now(&mut item.options[value as usize].0, 1.0, 0.25);
        }
        (item.def.set)(&mut cx.global.settings, value);
        if item.def.label == STANDBY_LABEL && value == 1 {
            self.child = Some(Child::Notice(Dialog::ok(
                cx.pre_render,
                &self.assets,
                STANDBY_NOTICE,
            )));
        }
        cx.sound("move");
    }

    /// The item's left and right (vfunc 6): a slider by 5 within 0-100, buttons by one option without wrapping
    fn adjust(&mut self, cx: &mut TaskCx, step: i32) {
        let index = self.cursor();
        let item = &self.pages[self.page][index];
        let value = (item.def.get)(&cx.global.settings) as i32;
        let value = match item.def.kind {
            Kind::Slider(_) => (value + 5 * step).clamp(0, 100),
            Kind::Buttons(options) => (value + step).clamp(0, options.len() as i32 - 1),
        };
        self.set_value(cx, index, value as u8);
    }

    /// `0x710005c980`: the close button flashes, then the save is written if anything changed and the screen fades
    fn close(&mut self) {
        self.state = State::Closing;
        self.close_flash = Tweener::new(0.0);
        minigame::flash(&mut self.close_flash);
    }

    /// `0x710005c700`: the Reset button flashes, then asks
    fn reset(&mut self) {
        self.default_flash = Tweener::new(0.0);
        minigame::flash(&mut self.default_flash);
        self.reset_pending = true;
    }

    fn handle_keys(&mut self, cx: &mut TaskCx) {
        let input = cx.input;
        let settings = cx.global.settings;
        let n = self.pages[self.page].len();
        if input.l {
            self.select_page(&settings, (self.page + 2) % 3);
            cx.sound("move");
        } else if input.r {
            self.select_page(&settings, (self.page + 1) % 3);
            cx.sound("move");
        } else if input.up {
            if self.cursor() > 0 {
                self.select_item(&settings, self.cursor() - 1);
                cx.sound("move");
            }
        } else if input.down {
            if self.cursor() + 1 < n {
                self.select_item(&settings, self.cursor() + 1);
                cx.sound("move");
            }
        } else if input.y {
            cx.sound("decide");
            self.reset();
        } else if input.cancel {
            cx.sound("cancel");
            self.close();
        } else if input.left {
            self.adjust(cx, -1);
        } else if input.right {
            self.adjust(cx, 1);
        }
    }

    fn item_at(&self, point: Vec2) -> Option<usize> {
        self.pages[self.page].iter().position(|item| {
            minigame::contains(
                [
                    LIST_X,
                    LIST_Y + item.y,
                    LIST_X + LIST_W,
                    LIST_Y + item.y + ITEM_H,
                ],
                point,
            )
        })
    }

    /// The pointer: tabs, the close and Reset buttons, an item's option, or a slider dragged sideways (value += dx / 6)
    fn handle_pointer(&mut self, cx: &mut TaskCx) {
        let pointer = cx.input.pointer;
        let went_down = pointer.held && !self.pointer_held;
        let went_up = !pointer.held && self.pointer_held;
        self.pointer_held = pointer.held;
        let Some(point) = pointer.position else {
            return;
        };

        if let Some((index, last_x)) = self.drag {
            if went_up {
                self.drag = None;
                return;
            }
            self.drag_value = (self.drag_value + (point.x - last_x) / 6.0).clamp(0.0, 100.0);
            self.drag = Some((index, point.x));
            self.set_value(cx, index, self.drag_value.round() as u8);
            return;
        }
        if !went_down {
            return;
        }

        let settings = cx.global.settings;
        if minigame::contains([1764.0, 32.0, 1848.0, 116.0], point) {
            cx.sound("cancel");
            self.close();
            return;
        }
        if minigame::contains([1446.0, 32.0, 1752.0, 116.0], point) {
            cx.sound("decide");
            self.reset();
            return;
        }
        for page in 0..3 {
            let x = 510.0 + 300.0 * page as f32;
            if minigame::contains([x, 144.0, x + 300.0, 211.5], point) {
                if page != self.page {
                    minigame::flash(&mut self.tabs[page].1);
                    self.select_page(&settings, page);
                    cx.sound("decide");
                }
                return;
            }
        }
        let Some(index) = self.item_at(point) else {
            return;
        };
        if index != self.cursor() {
            self.select_item(&settings, index);
        }
        let item = &mut self.pages[self.page][index];
        match item.def.kind {
            Kind::Slider(_) => {
                self.drag_value = (item.def.get)(&settings) as f32;
                self.drag = Some((index, point.x));
            }
            Kind::Buttons(options) => {
                let x = point.x - LIST_X;
                if let Some(k) = (0..options.len()).find(|&k| {
                    let left = item.option_x(k);
                    left <= x && x < left + 240.0
                }) {
                    minigame::flash(&mut item.options[k].1);
                    self.set_value(cx, index, k as u8);
                }
            }
        }
    }

    fn update_child(&mut self, cx: &mut TaskCx) {
        let Some(child) = &mut self.child else {
            return;
        };
        let (dialog, reset) = match child {
            Child::Reset(dialog) => (dialog, true),
            Child::Notice(dialog) => (dialog, false),
        };
        let running = dialog.update(&mut cx.reborrow(cx.focused));
        let events = dialog.take_events();
        if !running {
            self.child = None;
        }
        // `0x71000604c0`: Yes, as the dialog starts closing: every setting back to its default
        if reset && events.contains(&(0, 0)) {
            let old = cx.global.settings;
            cx.global.settings = Settings::default();
            let new = cx.global.settings;
            // the items are rebuilt with the new values; the focused one keeps its highlight
            for item in self.pages.iter_mut().flatten() {
                let focused = item.focused;
                item.set_focus(&old, false, false);
                if focused {
                    item.set_focus(&new, true, false);
                }
            }
        }
    }

    fn draw_item(&self, pass: &mut RenderPass, list: Mat4, item: &Item) {
        let builder = ui::builder();
        let t = |name: &str| self.textures.get(name);
        let y = item.y;
        if item.focused {
            FOCUS.draw(
                pass,
                t("focus"),
                list,
                [0.0, y, LIST_W, y + ITEM_H],
                FloatColor4::WHITE.with_alpha(0.75),
            );
        }
        let v = item.def.label as f32 * 72.0 / 1440.0;
        Sprite::new(t("left"), [24.0, y + 8.0, 776.0, y + 80.0])
            .uv([0.0, v, 1.0, v + 0.05])
            .draw(pass, builder, list);

        let value = (item.def.get)(&self.live);
        match item.def.kind {
            Kind::Slider(ty) => {
                // `0x71000582e0`
                let slider = t("slider");
                let r = (ty + 1) as f32 * 0.25;
                Sprite::new(slider, [976.0, y + 4.0, 1096.0, y + 52.0])
                    .uv([0.18519, r, 0.37037, r + 0.25])
                    .draw(pass, builder, list);
                Sprite::new(slider, [1504.0, y + 4.0, 1624.0, y + 52.0])
                    .uv([0.48148, r, 0.66667, r + 0.25])
                    .draw(pass, builder, list);
                Sprite::new(slider, [976.0, y + 40.0, 1624.0, y + 88.0])
                    .uv([0.0, 0.0, 1.0, 0.25])
                    .draw(pass, builder, list);
                let x = 976.0 + 6.0 * value as f32;
                Sprite::new(slider, [x, y + 5.0, x + 48.0, y + 77.0])
                    .uv([0.07407, 0.625, 0.14815, 1.0])
                    .glow(item.focus.value() * 0.5)
                    .draw(pass, builder, list);
            }
            Kind::Buttons(options) => {
                // `0x71000572a0`
                for (k, &row) in options.iter().enumerate() {
                    let x = item.option_x(k);
                    let selected = value as usize == k;
                    let (highlight, flash) = &item.options[k];
                    let (glow, flash) = (highlight.value() * 0.5, flash.value());
                    let background = if selected { "button_on" } else { "button_off" };
                    Sprite::new(t(background), [x, y, x + 240.0, y + ITEM_H])
                        .glow(glow)
                        .flash(flash)
                        .draw(pass, builder, list);
                    let u = if selected { 0.5 } else { 0.0 };
                    let v = row as f32 * 64.0 / 640.0;
                    Sprite::new(t("right"), [x, y + 12.0, x + 240.0, y + 76.0])
                        .uv([u, v, u + 0.5, v + 0.1])
                        .glow(glow)
                        .flash(flash)
                        .draw(pass, builder, list);
                }
            }
        }
    }
}

impl Task for ConfigMenu {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        let frames = cx.frames;
        match self.state {
            State::Opening => {
                self.fade = (self.fade + 0.1 * frames).min(1.0);
                if self.fade >= 1.0 {
                    self.state = State::Active;
                }
            }
            State::Active => {
                if self.child.is_some() {
                    self.update_child(cx);
                } else if self.reset_pending {
                    if self.default_flash.is_idle() {
                        self.reset_pending = false;
                        self.child = Some(Child::Reset(Dialog::yes_no(
                            cx.pre_render,
                            &self.assets,
                            RESET_QUESTION,
                            1,
                        )));
                    }
                } else if cx.focused {
                    self.handle_pointer(cx);
                    if self.drag.is_none() && self.state == State::Active {
                        self.handle_keys(cx);
                    }
                }
            }
            State::Closing => {
                if self.close_flash.is_idle() {
                    if cx.global.settings != self.opened_with {
                        cx.requests.push(AdvRequest::WriteSave);
                    }
                    self.state = State::FadingOut;
                }
            }
            State::FadingOut => {
                self.fade = (self.fade - 0.1 * frames).max(0.0);
                if self.fade <= 0.0 {
                    self.state = State::Done;
                }
            }
            State::Done => return false,
        }
        self.live = cx.global.settings;

        for item in self.pages.iter_mut().flatten() {
            item.update(frames);
        }
        for (selected, flash) in &mut self.tabs {
            ui::advance(selected, frames);
            ui::advance(flash, frames);
        }
        ui::advance(&mut self.close_flash, frames);
        ui::advance(&mut self.default_flash, frames);
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
        // blend 3: added on top
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

        // the window, the list and the tabs slide in from the right
        let slid = projection * Mat4::from_translation(vec3(1920.0 - f * 1920.0, 0.0, 0.0));
        Sprite::new(t("window"), [48.0, 176.0, 48.0 + 1824.0, 176.0 + 834.0])
            .draw(pass, builder, slid);

        // shortcut: no page holds more items than the 666 px list shows, so the list is neither clipped nor scrolled
        // and its scroll bar is left out
        let list = slid * Mat4::from_translation(vec3(LIST_X, LIST_Y, 0.0));
        for item in &self.pages[self.page] {
            self.draw_item(pass, list, item);
        }
        let line = t("line");
        for item in &self.pages[self.page] {
            let y = item.y + ITEM_H;
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
        // the window's edges over the list's clip
        Sprite::new(t("window"), [136.0, 247.0, 1784.0, 252.0])
            .uv([0.04825, 0.08513, 0.95175, 0.09113])
            .draw(pass, builder, slid);
        Sprite::new(t("window"), [136.0, 918.0, 1784.0, 923.0])
            .uv([0.04825, 0.88969, 0.95175, 0.89568])
            .draw(pass, builder, slid);

        for (page, (selected, flash)) in self.tabs.iter().enumerate() {
            let s = 1.0 + 0.15 * flash.value();
            let transform =
                slid * Mat4::from_translation(vec3(
                    510.0 + 300.0 * page as f32 + 150.0,
                    144.0 + 33.75,
                    0.0,
                )) * Mat4::from_scale(vec3(s, s, 1.0))
                    * Mat4::from_translation(vec3(-150.0, -33.75, 0.0));
            let v = page as f32 / 3.0;
            let selected = selected.value();
            // shortcut: the original mixes the two textures in one draw (`0x71000c70d0`); drawing the selected one
            // over the other differs only where both are see-through
            Sprite::new(t("tab_off"), [0.0, 0.0, 300.0, 67.5])
                .uv([0.0, v, 1.0, v + 1.0 / 3.0])
                .draw(pass, builder, transform);
            Sprite::new(t("tab_on"), [0.0, 0.0, 300.0, 67.5])
                .uv([0.0, v, 1.0, v + 1.0 / 3.0])
                .alpha(selected)
                .draw(pass, builder, transform);
        }

        Sprite::new(t("caption"), [480.0, top, 1440.0, top + 128.0])
            .draw(pass, builder, projection);
        Sprite::new(t("close"), [1764.0, top + 32.0, 1848.0, top + 116.0])
            .flash(self.close_flash.value())
            .draw(pass, builder, projection);
        Sprite::new(t("default"), [1446.0, top + 32.0, 1752.0, top + 116.0])
            .flash(self.default_flash.value())
            .draw(pass, builder, projection);

        // the key help, right to left from x 1872 (`0x7100057a10`)
        let y = 1080.0 - f * 88.0 + 20.0;
        let mut right = 1872.0;
        for help in [
            HELP_SAVE,
            HELP_DEFAULT,
            HELP_PAGES,
            HELP_ADJUST,
            HELP_SELECT,
        ] {
            let v = help.row / 240.0;
            Sprite::new(t("help_config"), [right - help.width, y, right, y + 48.0])
                .uv([0.0, v, help.width / 320.0, v + 0.2])
                .draw(pass, builder, projection);
            let x = right - help.width + 2.0 - 48.0;
            let (u, v) = (
                (help.glyph & 7) as f32 / 8.0,
                (help.glyph >> 3) as f32 / 2.0,
            );
            Sprite::new(t("key"), [x, y, x + 48.0, y + 48.0])
                .uv([u, v, u + 0.125, v + 0.5])
                .draw(pass, builder, projection);
            right = x - 16.0;
        }

        if let Some(Child::Reset(dialog) | Child::Notice(dialog)) = &self.child {
            dialog.render(pass);
        }
    }
}
