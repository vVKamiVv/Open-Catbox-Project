//! `shin::ConfigMenu` (ctor `0x71000586b0`, update `0x710005b270`, render `0x710005b520`, keys `0x710005cc20`): the
//! settings screen, three tabs of sliders and On/Off buttons over `/config.txa`. Settings change live (every player
//! reads them each frame); closing writes the save file when anything changed. The pointer works as the original's
//! touch listener: tapping an option or a tab, grabbing a slider's knob, dragging and flicking the list.
//! The visual touch feedback setting (v17) turns the tap ripples and drag stars of [`crate::ui::touch_effect`] on
//! and off.

use std::sync::Arc;

use glam::{Mat4, Vec2, vec2, vec3};
use shin_core::{format::save::Settings, time::Tweener};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, render_pass::RenderPass, resize::CanvasSize,
};
use tracing::warn;

mod controls;

use super::{MenuAssets, Task, TaskCx, dialog::Dialog, save_notice::SaveNotice};
use crate::{
    adv::hud::{OUTLINE, text_layout},
    asset::{executable::EngineText, texture_archive::NamedTextures},
    layer::{
        text_block::TextBlock,
        user::minigame::{self, Sprite},
    },
    port_settings,
    ui::{
        self, NineSlice,
        gesture::{Coast, Gesture, GestureRecognizer},
    },
};
use shin_core::primitives::color::FloatColor4;

const RESET_QUESTION: EngineText = EngineText(0x710015f87a, "Reset ALL settings to default?");
/// The notice of the standby setting
const STANDBY_NOTICE: EngineText = EngineText(
    0x7100161d8e,
    "Automatic sleep will now be disabled@rwhen using Auto Mode and the Music Box.",
);

/// The list's viewport: items from (136, 252), 1648 wide
const LIST_X: f32 = 136.0;
const LIST_Y: f32 = 252.0;
const LIST_W: f32 = 1648.0;
const ITEM_H: f32 = 88.0;
const ITEM_STEP: f32 = 102.0;
/// The list's view height (`0x29a`); each page is at least this tall
const VIEW: f32 = 666.0;

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
    /// PC tab: buttons as [`Kind::Buttons`] with their texts drawn in the game's font
    Texts(&'static [Text]),
    /// PC tab: one wide button showing the selected option, stepped through by left and right (or tapping its
    /// halves); for lists too long for a row of buttons
    Cycle(&'static [Text]),
    /// PC tab: an entry that opens another screen (A or a tap)
    Open,
}

impl Kind {
    /// How many values the item takes
    fn count(self) -> usize {
        match self {
            Kind::Slider(_) => 101,
            Kind::Buttons(options) => options.len(),
            Kind::Texts(options) | Kind::Cycle(options) => options.len(),
            Kind::Open => 1,
        }
    }
}

const OFF_ON: &[u8] = &[1, 0];
const HIDE_SHOW: &[u8] = &[5, 4];

/// A port text in English and Japanese (the PC tab has no art: the original has no such settings)
#[derive(Clone, Copy, PartialEq, Eq)]
struct Text(&'static str, &'static str);

impl Text {
    const fn same(text: &'static str) -> Self {
        Self(text, text)
    }

    fn get(self, japanese: bool) -> &'static str {
        if japanese { self.1 } else { self.0 }
    }
}

/// An item's label: a row of `left`, or a text drawn in the game's font
#[derive(Clone, Copy, PartialEq, Eq)]
enum Label {
    Art(u8),
    Text(Text),
}

/// One row of InitMenu (`0x7100058e90`): its tab, its label's row in `left`, and the setting it edits
struct ItemDef {
    page: usize,
    label: Label,
    kind: Kind,
    get: fn(&Settings) -> u8,
    set: fn(&mut Settings, u8),
}

/// The tabs: the original's three and the port's PC tab
const PAGES: usize = 4;
/// The PC tab, which edits [`port_settings`] rather than the save's settings
const PC_PAGE: usize = 3;
const PC_TAB: Text = Text::same("PC");

/// The left edge of tab `page`: 300 wide, centred on the screen together (the original's three start at 510)
fn tab_x(page: usize) -> f32 {
    960.0 - 150.0 * PAGES as f32 + 300.0 * page as f32
}

const WINDOW_MODES: &[Text] = &[
    Text("Windowed", "ウィンドウ"),
    Text("Borderless", "ボーダーレス"),
    Text("Fullscreen", "全画面"),
];
const WINDOW_SIZES: &[Text] = &[
    Text::same("1280 x 720"),
    Text::same("1600 x 900"),
    Text::same("1920 x 1080"),
    Text::same("2560 x 1440"),
    Text::same("3840 x 2160"),
];
const FRAME_CAPS: &[Text] = &[
    Text("Off", "なし"),
    Text::same("30"),
    Text::same("60"),
    Text::same("120"),
    Text::same("144"),
    Text::same("240"),
];
const SUPERSAMPLING: &[Text] = &[Text::same("1x"), Text::same("1.5x"), Text::same("2x")];
const CONFIRM_BUTTONS: &[Text] = &[Text("Right", "右"), Text("Bottom", "下")];
const BUTTON_PROMPTS: &[Text] = &[
    Text("Auto", "自動"),
    Text::same("Xbox"),
    Text::same("PlayStation"),
    Text::same("Switch"),
    Text("Keyboard", "キーボード"),
];

/// The index of `value` in `list`, or `default`
fn index_of<T: PartialEq>(list: &[T], value: T, default: usize) -> u8 {
    list.iter().position(|v| *v == value).unwrap_or(default) as u8
}

const ITEMS: &[ItemDef] = &[
    ItemDef {
        page: 0,
        label: Label::Art(0),
        kind: Kind::Slider(0),
        get: |s| s.v3_sysvol,
        set: |s, v| s.v3_sysvol = v,
    },
    ItemDef {
        page: 0,
        label: Label::Art(1),
        kind: Kind::Slider(0),
        get: |s| s.v0_bgmvol,
        set: |s, v| s.v0_bgmvol = v,
    },
    ItemDef {
        page: 0,
        label: Label::Art(2),
        kind: Kind::Slider(0),
        get: |s| s.v1_sfxvol,
        set: |s, v| s.v1_sfxvol = v,
    },
    ItemDef {
        page: 0,
        label: Label::Art(3),
        kind: Kind::Slider(0),
        get: |s| s.v2_voicevol,
        set: |s, v| s.v2_voicevol = v,
    },
    ItemDef {
        page: 0,
        label: Label::Art(4),
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v4_voicefocus as u8,
        set: |s, v| s.v4_voicefocus = v != 0,
    },
    ItemDef {
        page: 0,
        label: Label::Art(18),
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v5_voicepanapot as u8,
        set: |s, v| s.v5_voicepanapot = v != 0,
    },
    ItemDef {
        page: 1,
        label: Label::Art(8),
        kind: Kind::Slider(2),
        get: |s| s.v9_msgspeed,
        set: |s, v| s.v9_msgspeed = v,
    },
    ItemDef {
        page: 1,
        label: Label::Art(11),
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v11_disallowskipunread as u8,
        set: |s, v| s.v11_disallowskipunread = v != 0,
    },
    ItemDef {
        page: 1,
        label: Label::Art(15),
        kind: Kind::Slider(1),
        get: |s| s.v13_msgwinalpha,
        set: |s, v| s.v13_msgwinalpha = v,
    },
    ItemDef {
        page: 2,
        label: Label::Art(10),
        kind: Kind::Slider(2),
        get: |s| s.v10_skipspeed,
        set: |s, v| s.v10_skipspeed = v,
    },
    ItemDef {
        page: 2,
        label: Label::Art(16),
        kind: Kind::Buttons(HIDE_SHOW),
        get: |s| s.v15_showsongtitle as u8,
        set: |s, v| s.v15_showsongtitle = v != 0,
    },
    ItemDef {
        page: 2,
        label: Label::Art(19),
        kind: Kind::Buttons(HIDE_SHOW),
        get: |s| s.v14_showscenetitle as u8,
        set: |s, v| s.v14_showscenetitle = v != 0,
    },
    ItemDef {
        page: 2,
        label: Label::Art(17),
        kind: Kind::Buttons(HIDE_SHOW),
        get: |s| s.v17_showtoucheffect as u8,
        set: |s, v| s.v17_showtoucheffect = v != 0,
    },
    ItemDef {
        page: 2,
        label: Label::Art(12),
        kind: Kind::Buttons(OFF_ON),
        get: |s| s.v18 as u8,
        set: |s, v| s.v18 = v != 0,
    },
    // the port's PC tab: these read and write the port's settings file, not the save's settings
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("Window mode", "ウィンドウ")),
        kind: Kind::Texts(WINDOW_MODES),
        get: |_| port_settings::get().window_mode as u8,
        set: |_, v| {
            use port_settings::WindowMode::*;
            port_settings::update(|s| s.window_mode = [Windowed, Borderless, Exclusive][v as usize])
        },
    },
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("Window size", "画面サイズ")),
        kind: Kind::Cycle(WINDOW_SIZES),
        get: |_| {
            index_of(
                &port_settings::WINDOW_SIZES,
                port_settings::get().window_size,
                2,
            )
        },
        set: |_, v| {
            port_settings::update(|s| s.window_size = port_settings::WINDOW_SIZES[v as usize])
        },
    },
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("V-Sync", "垂直同期")),
        kind: Kind::Buttons(OFF_ON),
        get: |_| port_settings::get().vsync as u8,
        set: |_, v| port_settings::update(|s| s.vsync = v != 0),
    },
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("Frame-rate cap", "フレームレート上限")),
        kind: Kind::Cycle(FRAME_CAPS),
        get: |_| {
            index_of(
                &port_settings::FRAME_CAPS,
                port_settings::get().frame_cap,
                0,
            )
        },
        set: |_, v| port_settings::update(|s| s.frame_cap = port_settings::FRAME_CAPS[v as usize]),
    },
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("Supersampling", "スーパーサンプリング")),
        kind: Kind::Texts(SUPERSAMPLING),
        get: |_| {
            index_of(
                &port_settings::SUPERSAMPLING,
                port_settings::get().supersampling,
                0,
            )
        },
        set: |_, v| {
            port_settings::update(|s| s.supersampling = port_settings::SUPERSAMPLING[v as usize])
        },
    },
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("Button prompts", "ボタン表示")),
        kind: Kind::Cycle(BUTTON_PROMPTS),
        get: |_| port_settings::get().button_prompts as u8,
        set: |_, v| {
            use port_settings::ButtonPrompts::*;
            port_settings::update(|s| {
                s.button_prompts = [Auto, Xbox, PlayStation, Switch, Keyboard][v as usize]
            })
        },
    },
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("Confirm button", "決定ボタン")),
        kind: Kind::Texts(CONFIRM_BUTTONS),
        get: |_| port_settings::get().bottom_confirms as u8,
        set: |_, v| port_settings::update(|s| s.bottom_confirms = v == 1),
    },
    // the Controls screen
    ItemDef {
        page: PC_PAGE,
        label: Label::Text(Text("Controls…", "操作設定")),
        kind: Kind::Open,
        get: |_| 0,
        set: |_, _| {},
    },
];

/// The standby item: turning it on shows a notice (`$_14` `0x710005fdd0`)
const STANDBY_LABEL: Label = Label::Art(12);
/// The sizes of the PC tab's labels and options, near the art's lettering
const LABEL_SIZE: f32 = 36.0;
const OPTION_SIZE: f32 = 32.0;
/// The labels' white and shadow, as the art's
const TEXT_BORDER: FloatColor4 = FloatColor4::from_rgba(0.1, 0.07, 0.07, 1.0);
/// The tabs' dark brown lettering and its light edge, as the art's
const TAB_TEXT: FloatColor4 = FloatColor4::from_rgba(0.22, 0.13, 0.06, 1.0);
const TAB_BORDER: FloatColor4 = FloatColor4::from_rgba(1.0, 0.92, 0.7, 1.0);
/// A cycle's button in the list's space: where the last two option buttons would be
const CYCLE_X0: f32 = 1132.0;
const CYCLE_X1: f32 = 1624.0;

struct Item {
    def: &'static ItemDef,
    /// Top in the list's space
    y: f32,
    focused: bool,
    /// A slider's focus glow (`+0x70`)
    focus: Tweener,
    /// A button item's per-option highlight and press flash (a cycle's one button: its press flash)
    options: Vec<(Tweener, Tweener)>,
    /// PC tab: the label and the options' texts in the game's font
    label: Option<TextBlock>,
    texts: Vec<TextBlock>,
}

impl Item {
    fn new(def: &'static ItemDef, y: f32, text: &impl Fn(Text, f32) -> TextBlock) -> Self {
        let buttons = match def.kind {
            Kind::Slider(_) | Kind::Open => 0,
            Kind::Buttons(options) => options.len(),
            Kind::Texts(options) => options.len(),
            Kind::Cycle(_) => 1,
        };
        let texts = match def.kind {
            Kind::Texts(options) | Kind::Cycle(options) => {
                options.iter().map(|&t| text(t, OPTION_SIZE)).collect()
            }
            _ => Vec::new(),
        };
        Self {
            def,
            y,
            focused: false,
            focus: Tweener::new(0.0),
            options: (0..buttons)
                .map(|_| (Tweener::new(0.0), Tweener::new(0.0)))
                .collect(),
            label: match def.label {
                Label::Art(_) => None,
                Label::Text(label) => Some(text(label, LABEL_SIZE)),
            },
            texts,
        }
    }

    /// The tweener focusing moves: a slider's glow, or the selected option's highlight (a cycle glows whole)
    fn focus_tweener(&mut self, settings: &Settings) -> &mut Tweener {
        match self.def.kind {
            Kind::Slider(_) | Kind::Cycle(_) | Kind::Open => &mut self.focus,
            Kind::Buttons(_) | Kind::Texts(_) => {
                &mut self.options[(self.def.get)(settings) as usize].0
            }
        }
    }

    /// A row of option buttons, whose highlights follow the selected option
    fn has_buttons(&self) -> bool {
        matches!(self.def.kind, Kind::Buttons(_) | Kind::Texts(_))
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

    /// The option under `x` in the list's space (`0x7100056fd0`): 240 wide, 12 apart, right-aligned to 1624; a
    /// cycle's one button spans the last two
    fn option_at(&self, x: f32) -> Option<usize> {
        if let Kind::Cycle(_) = self.def.kind {
            return (CYCLE_X0..=CYCLE_X1).contains(&x).then_some(0);
        }
        (0..self.options.len()).find(|&k| self.option_x(k) <= x && x <= self.option_x(k) + 240.0)
    }

    /// A slider's knob is under `at` in the list's space (`0x71000580e0`): within 48 of `value * 6 + 1000` in the row
    fn knob_at(&self, settings: &Settings, at: Vec2) -> bool {
        let knob = (self.def.get)(settings) as f32 * 6.0 + 1000.0;
        matches!(self.def.kind, Kind::Slider(_))
            && self.y <= at.y
            && at.y <= self.y + ITEM_H
            && knob - 48.0 <= at.x
            && at.x < knob + 48.0
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
    Controls(Box<controls::Controls>),
}

/// What the current touch went down on (`+0x41c..+0x428`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Touched {
    /// Outside the band y 150..992, or the close or Reset button once dragged
    Nothing,
    Close,
    Reset,
    Tab(usize),
    /// The list: the item under the down point, `None` once the touch scrolls the list
    List(Option<usize>),
}

pub struct ConfigMenu {
    assets: MenuAssets,
    textures: Arc<NamedTextures>,
    state: State,
    fade: f32,
    page: usize,
    /// The cursor of each page, remembered across tab changes
    cursors: [usize; PAGES],
    pages: [Vec<Item>; PAGES],
    /// Per tab: selected (cross-fades `tab_off` to `tab_on`) and the press flash (scales it up)
    tabs: [(Tweener, Tweener); PAGES],
    /// The PC tab's label, in the game's font as the tab art has none
    pc_tab: TextBlock,
    /// The arrows of the PC tab's cycles
    arrows: [TextBlock; 2],
    close_flash: Tweener,
    default_flash: Tweener,
    /// The settings when the screen opened (`+0x534`)
    opened_with: Settings,
    /// The settings as of this frame, for drawing
    live: Settings,
    /// The reset dialog waits for the Reset button's flash
    reset_pending: bool,
    child: Option<Child>,
    /// The save notice while closing writes the changed settings
    saving: Option<SaveNotice>,
    /// The touch input (`GestureRecognizer`, down `0x710005d4b0`)
    gestures: GestureRecognizer,
    touched: Option<Touched>,
    /// The touched button item's option (`+0x74`)
    option: Option<usize>,
    /// The grabbed slider's value at the grab and the drag's summed dx (`+0x60..+0x68`)
    grab: Option<(u8, f32)>,
    /// The list's scroll (`+0x4b8`) and its coasting after a flick (`+0x424`)
    scroll: f32,
    coast: Coast,
    /// For clipping the list
    target_size: (u32, u32),
    japanese: bool,
}

fn build_pages(text: &impl Fn(Text, f32) -> TextBlock) -> [Vec<Item>; PAGES] {
    let mut pages: [Vec<Item>; PAGES] = Default::default();
    for def in ITEMS {
        let y = ITEM_STEP * pages[def.page].len() as f32;
        pages[def.page].push(Item::new(def, y, text));
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
        // the executable's texts tell the language; the PC tab's texts follow it
        let japanese = !RESET_QUESTION.get().is_ascii();
        let text = |t: Text, size: f32| {
            TextBlock::new(
                cx.pre_render,
                &assets.system_font,
                t.get(japanese),
                text_layout(1000.0, size),
                OUTLINE,
            )
        };
        let mut menu = Self {
            assets: assets.clone(),
            textures,
            state: State::Opening,
            fade: 0.0,
            page: 0,
            cursors: [0; PAGES],
            pages: build_pages(&text),
            tabs: std::array::from_fn(|page| {
                (Tweener::new((page == 0) as u8 as f32), Tweener::new(0.0))
            }),
            pc_tab: text(PC_TAB, LABEL_SIZE),
            arrows: [
                text(Text::same("<"), OPTION_SIZE),
                text(Text::same(">"), OPTION_SIZE),
            ],
            close_flash: Tweener::new(0.0),
            default_flash: Tweener::new(0.0),
            opened_with: settings,
            live: settings,
            reset_pending: false,
            child: None,
            saving: None,
            gestures: GestureRecognizer::new(false),
            touched: None,
            option: None,
            grab: None,
            scroll: 0.0,
            coast: Coast::default(),
            target_size: (1920, 1080),
            japanese,
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
        if old == index {
            return;
        }
        self.pages[page][old].set_focus(settings, false, true);
        self.cursors[page] = index;
        self.pages[page][index].set_focus(settings, true, true);
    }

    /// Port-only (the PC tab is taller than the view): the list scrolls to show the item the keys moved to
    fn reveal(&mut self, index: usize) {
        let y = self.pages[self.page][index].y;
        self.scroll = self.scroll.clamp((y + ITEM_H - VIEW).max(0.0), y);
    }

    /// Opens what an [`Kind::Open`] item opens: the Controls screen
    fn open(&mut self, cx: &mut TaskCx, index: usize) {
        if let Kind::Open = self.pages[self.page][index].def.kind {
            cx.sound("decide");
            let screen = controls::Controls::new(cx, &self.assets, self.japanese);
            self.child = Some(Child::Controls(Box::new(screen)));
        }
    }

    /// Writes a new value through the item's setter, with the "move" sound when it changed
    fn set_value(&mut self, cx: &mut TaskCx, index: usize, value: u8) {
        let item = &mut self.pages[self.page][index];
        let old = (item.def.get)(&cx.global.settings);
        if old == value {
            return;
        }
        if item.has_buttons() {
            minigame::tween_now(&mut item.options[old as usize].0, 0.0, 0.2);
            minigame::tween_now(&mut item.options[value as usize].0, 1.0, 0.25);
        }
        self.write(cx, index, value);
        cx.sound("move");
    }

    /// The item's setter; the standby item set to On shows its notice, even when it already was (`0x710005fdd0`)
    fn write(&mut self, cx: &mut TaskCx, index: usize, value: u8) {
        let def = self.pages[self.page][index].def;
        (def.set)(&mut cx.global.settings, value);
        if def.label == STANDBY_LABEL && value == 1 {
            self.child = Some(Child::Notice(Dialog::ok(
                cx.pre_render,
                &self.assets,
                &STANDBY_NOTICE.get(),
            )));
        }
    }

    /// The item's left and right (vfunc 6): a slider by 5 within 0-100, buttons (and the PC tab's cycles) by one option
    /// without wrapping
    fn adjust(&mut self, cx: &mut TaskCx, step: i32) {
        let index = self.cursor();
        let item = &self.pages[self.page][index];
        let value = (item.def.get)(&cx.global.settings) as i32;
        let value = match item.def.kind {
            Kind::Slider(_) => (value + 5 * step).clamp(0, 100),
            kind => (value + step).clamp(0, kind.count() as i32 - 1),
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
            self.select_page(&settings, (self.page + PAGES - 1) % PAGES);
            cx.sound("move");
        } else if input.r {
            self.select_page(&settings, (self.page + 1) % PAGES);
            cx.sound("move");
        } else if input.up {
            if self.cursor() > 0 {
                self.select_item(&settings, self.cursor() - 1);
                self.reveal(self.cursor());
                cx.sound("move");
            }
        } else if input.down {
            if self.cursor() + 1 < n {
                self.select_item(&settings, self.cursor() + 1);
                self.reveal(self.cursor());
                cx.sound("move");
            }
        } else if input.confirm {
            self.open(cx, self.cursor());
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

    /// `point` in the list's space: x - 136, y - 252 + scroll
    fn list_point(&self, point: Vec2) -> Vec2 {
        Vec2::new(point.x - LIST_X, point.y - LIST_Y + self.scroll)
    }

    /// The page is at least the 666 px view tall
    fn max_scroll(&self) -> f32 {
        self.pages[self.page]
            .last()
            .map_or(0.0, |item| item.y + ITEM_H - VIEW)
            .max(0.0)
    }

    /// The down hit (`0x710005d4b0`): the close and Reset buttons, then the band y 150..992 with the other tabs
    /// (`0x710005d280`) and the list x 136..1784, y 252..918; `None` is not tracked
    fn touched_at(&self, point: Vec2) -> Option<Touched> {
        if (32.0..116.0).contains(&point.y) {
            if (1764.0..1848.0).contains(&point.x) {
                return Some(Touched::Close);
            }
            if (1446.0..1752.0).contains(&point.x) {
                return Some(Touched::Reset);
            }
        }
        if !(150.0..992.0).contains(&point.y) {
            return Some(Touched::Nothing);
        }
        let tab = (0..PAGES).find(|&page| {
            let x = tab_x(page);
            page != self.page && minigame::contains([x, 144.0, x + 300.0, 211.5], point)
        });
        if let Some(page) = tab {
            return Some(Touched::Tab(page));
        }
        if !minigame::contains([LIST_X, LIST_Y, LIST_X + LIST_W, 918.0], point) {
            return None;
        }
        let y = self.list_point(point).y;
        Some(Touched::List(
            self.pages[self.page]
                .iter()
                .position(|item| item.y <= y && y < item.y + ITEM_H),
        ))
    }

    /// The config's gesture listener (down `0x710005d4b0`, press `0x710005d7d0`, tap `0x710005d8e0`, drag
    /// `0x710005da90`, flick `0x710005dd30`, end `0x710005d710`): a press moves the cursor and brightens what is under
    /// it, a tap acts, a drag on a slider's knob sets its value, and other drags and flicks scroll the list
    fn handle_pointer(&mut self, cx: &mut TaskCx) {
        for gesture in self.gestures.update(cx.input.pointer, cx.frames) {
            let settings = cx.global.settings;
            match gesture {
                Gesture::Down(at) => {
                    self.touched = self.touched_at(at);
                    self.option = None;
                    self.grab = None;
                    match self.touched {
                        None => self.gestures.cancel(),
                        // touching a coasting list stops it, and that touch selects nothing
                        Some(Touched::List(_)) if self.coast.is_moving() => {
                            self.coast.stop();
                            self.touched = Some(Touched::List(None));
                        }
                        Some(Touched::List(Some(index))) => {
                            self.option = self.pages[self.page][index].option_at(at.x - LIST_X);
                        }
                        _ => {}
                    }
                }
                Gesture::Press => match self.touched {
                    Some(Touched::Close) => minigame::raise(&mut self.close_flash),
                    Some(Touched::Reset) => minigame::raise(&mut self.default_flash),
                    Some(Touched::Tab(page)) => minigame::raise(&mut self.tabs[page].1),
                    Some(Touched::List(Some(index))) => {
                        self.select_item(&settings, index);
                        let at = self.list_point(cx.input.pointer.position.unwrap_or_default());
                        let item = &mut self.pages[self.page][index];
                        if let Some(k) = self.option {
                            minigame::raise(&mut item.options[k].1);
                        } else if item.knob_at(&settings, at) {
                            self.grab = Some(((item.def.get)(&settings), 0.0));
                        }
                    }
                    _ => {}
                },
                Gesture::Tap(at) => match self.touched.take() {
                    Some(Touched::Close) => {
                        cx.sound("decide");
                        self.close();
                    }
                    Some(Touched::Reset) => {
                        cx.sound("decide");
                        self.reset();
                    }
                    Some(Touched::Tab(page)) => {
                        minigame::flash(&mut self.tabs[page].1);
                        cx.sound("decide");
                        self.select_page(&settings, page);
                    }
                    Some(Touched::List(Some(index))) => {
                        self.select_item(&settings, index);
                        let item = &mut self.pages[self.page][index];
                        if let Kind::Cycle(options) = item.def.kind {
                            // PC tab: the button's left half steps back, its right half on
                            if self.option.take().is_some() {
                                let middle = (CYCLE_X0 + CYCLE_X1) / 2.0;
                                let step = if at.x - LIST_X < middle { -1 } else { 1 };
                                let value = (item.def.get)(&settings) as i32 + step;
                                minigame::flash(&mut item.options[0].1);
                                let last = options.len() as i32 - 1;
                                self.write(cx, index, value.clamp(0, last) as u8);
                                cx.sound("decide");
                            }
                        } else if let Kind::Open = item.def.kind {
                            self.open(cx, index);
                        } else if let Some(k) = self.option.take() {
                            // `0x7100057110`: the option is selected, flashes and is written, with the decide sound
                            let old = (item.def.get)(&settings) as usize;
                            if old != k {
                                minigame::tween_now(&mut item.options[old].0, 0.0, 0.2);
                                minigame::tween_now(&mut item.options[k].0, 1.0, 0.25);
                            }
                            minigame::flash(&mut item.options[k].1);
                            self.write(cx, index, k as u8);
                            cx.sound("decide");
                        }
                    }
                    _ => {}
                },
                Gesture::Drag(from, to) => match self.touched {
                    Some(Touched::Nothing | Touched::Close | Touched::Reset) => {
                        minigame::lower(&mut self.close_flash);
                        minigame::lower(&mut self.default_flash);
                        self.touched = Some(Touched::Nothing);
                    }
                    Some(Touched::Tab(page)) => minigame::lower(&mut self.tabs[page].1),
                    Some(Touched::List(item)) => {
                        if let Some(index) = item {
                            if self.drag_item(cx, index, from, to) {
                                self.select_item(&settings, index);
                                continue;
                            }
                            self.touched = Some(Touched::List(None));
                        }
                        let scroll = (self.scroll - (to.y - from.y)).clamp(0.0, self.max_scroll());
                        self.scroll_to(&settings, scroll);
                    }
                    None => {}
                },
                Gesture::Flick(velocity) => {
                    if self.touched == Some(Touched::List(None)) {
                        self.coast.start(velocity.y);
                    }
                }
                Gesture::End => {
                    if let (Some(Touched::List(Some(index))), Some(k)) = (self.touched, self.option)
                    {
                        minigame::lower(&mut self.pages[self.page][index].options[k].1);
                    }
                    self.touched = None;
                    self.option = None;
                    self.grab = None;
                }
                Gesture::LongPress => {}
            }
        }
    }

    /// An item's drag (button `0x7100057240`, slider `0x7100058160`); false lets the list scroll instead. A button
    /// drops its option's highlight; a slider grabbed by the press, or dragged from its knob within 40 degrees of the
    /// horizontal, takes value = start + trunc(sum dx / 6), 0..100, written without a sound
    fn drag_item(&mut self, cx: &mut TaskCx, index: usize, from: Vec2, to: Vec2) -> bool {
        let settings = cx.global.settings;
        let start = self.list_point(from);
        let item = &mut self.pages[self.page][index];
        if let Some(k) = self.option.take() {
            minigame::lower(&mut item.options[k].1);
        }
        if self.grab.is_none() {
            let angle = (to.y - from.y).atan2(to.x - from.x).to_degrees().abs();
            if !item.knob_at(&settings, start) || (40.0..=140.0).contains(&angle) {
                return false;
            }
            self.grab = Some(((item.def.get)(&settings), 0.0));
        }
        let Some((value, sum)) = &mut self.grab else {
            return false;
        };
        *sum += to.x - from.x;
        let new = (*value as i32 + (*sum / 6.0) as i32).clamp(0, 100) as u8;
        if new != (item.def.get)(&settings) {
            self.write(cx, index, new);
        }
        true
    }

    /// The list scrolled by a drag (`0x710005da90`) or coasting (`0x710005b270`): the cursor moves to stay on an item
    /// fully inside the 666 px view
    fn scroll_to(&mut self, settings: &Settings, scroll: f32) {
        let items = &self.pages[self.page];
        let first = items
            .iter()
            .position(|item| item.y >= scroll)
            .unwrap_or(items.len() - 1);
        let last = items
            .iter()
            .rposition(|item| item.y + ITEM_H <= scroll + VIEW)
            .unwrap_or(0);
        let cursor = self.cursor().clamp(first, last.max(first));
        self.select_item(settings, cursor);
        self.scroll = scroll;
    }

    fn update_child(&mut self, cx: &mut TaskCx) {
        let Some(child) = &mut self.child else {
            return;
        };
        let (dialog, reset) = match child {
            Child::Reset(dialog) => (dialog, true),
            Child::Notice(dialog) => (dialog, false),
            Child::Controls(screen) => {
                if !screen.update(cx) {
                    self.child = None;
                }
                return;
            }
        };
        let running = dialog.update(&mut cx.reborrow(cx.focused));
        let events = dialog.take_events();
        if !running {
            self.child = None;
        }
        // `0x71000604c0`: Yes, as the dialog starts closing: every setting back to its default
        if reset && events.contains(&(0, 0)) {
            // the items are rebuilt with the new values; the focused one keeps its highlight. The PC tab's items
            // read the port's settings themselves, so the highlights drop before anything resets
            let old = cx.global.settings;
            let focused: Vec<bool> = self
                .pages
                .iter()
                .flatten()
                .map(|item| item.focused)
                .collect();
            for item in self.pages.iter_mut().flatten() {
                item.set_focus(&old, false, false);
            }
            cx.global.settings = Settings::default();
            // the port's settings are among "ALL settings"; the reserved bindings are not settings of this screen
            port_settings::update(|s| {
                *s = port_settings::PortSettings {
                    bindings: std::mem::take(&mut s.bindings),
                    ..Default::default()
                }
            });
            let new = cx.global.settings;
            for (item, focused) in self.pages.iter_mut().flatten().zip(focused) {
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
        match (item.def.label, &item.label) {
            (Label::Art(row), _) => {
                let v = row as f32 * 72.0 / 1440.0;
                Sprite::new(t("left"), [24.0, y + 8.0, 776.0, y + 80.0])
                    .uv([0.0, v, 1.0, v + 0.05])
                    .draw(pass, builder, list);
            }
            (Label::Text(_), Some(label)) => {
                // where the art's labels start, centred on the row
                let at = vec2(44.0, y + (ITEM_H - label.size().y) / 2.0);
                draw_text_at(pass, label, list, at, 1.0);
            }
            (Label::Text(_), None) => {}
        }

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
            Kind::Texts(_) => {
                // as the buttons above, with the option's text in place of a row of `right`
                for (k, text) in item.texts.iter().enumerate() {
                    let x = item.option_x(k);
                    let selected = value as usize == k;
                    let (highlight, flash) = &item.options[k];
                    let background = if selected { "button_on" } else { "button_off" };
                    Sprite::new(t(background), [x, y, x + 240.0, y + ITEM_H])
                        .glow(highlight.value() * 0.5)
                        .flash(flash.value())
                        .draw(pass, builder, list);
                    let center = vec2(x + 120.0, y + ITEM_H / 2.0);
                    draw_text_at(pass, text, list, center - text.size() / 2.0, 1.0);
                }
            }
            Kind::Cycle(options) => {
                // one selected-looking button over the last two buttons' place: the art's rounded ends, its middle
                // stretched between them
                let (x0, x1) = (CYCLE_X0, CYCLE_X1);
                let (glow, flash) = (item.focus.value() * 0.5, item.options[0].1.value());
                for (left, right, u0, u1) in [
                    (x0, x0 + 60.0, 0.0, 0.25),
                    (x0 + 60.0, x1 - 60.0, 0.25, 0.75),
                    (x1 - 60.0, x1, 0.75, 1.0),
                ] {
                    Sprite::new(t("button_on"), [left, y, right, y + ITEM_H])
                        .uv([u0, 0.0, u1, 1.0])
                        .glow(glow)
                        .flash(flash)
                        .draw(pass, builder, list);
                }
                let middle = y + ITEM_H / 2.0;
                let text = &item.texts[(value as usize).min(options.len() - 1)];
                let center = vec2((x0 + x1) / 2.0, middle);
                draw_text_at(pass, text, list, center - text.size() / 2.0, 1.0);
                // the arrows dim at the ends, which the steps do not wrap past
                let [back, on] = &self.arrows;
                let back_alpha = if value == 0 { 0.35 } else { 1.0 };
                let on_alpha = if value as usize + 1 >= options.len() {
                    0.35
                } else {
                    1.0
                };
                let at = vec2(x0 + 40.0, middle) - back.size() / 2.0;
                draw_text_at(pass, back, list, at, back_alpha);
                let at = vec2(x1 - 40.0, middle) - on.size() / 2.0;
                draw_text_at(pass, on, list, at, on_alpha);
            }
            Kind::Open => {}
        }
    }
}

/// A PC-tab text in the art's white with its dark outline, its padded top-left corner at `at`
fn draw_text_at(pass: &mut RenderPass, text: &TextBlock, transform: Mat4, at: Vec2, alpha: f32) {
    ui::draw_text(
        pass,
        text,
        transform * Mat4::from_translation(at.extend(0.0)),
        FloatColor4::WHITE.with_alpha(alpha),
        TEXT_BORDER.with_alpha(alpha),
    );
}

impl Task for ConfigMenu {
    fn update(&mut self, cx: &mut TaskCx) -> bool {
        let frames = cx.frames;
        let size = cx.pre_render.resize_source.handle::<CanvasSize>().get();
        self.target_size = (size.width, size.height);
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
                            &RESET_QUESTION.get(),
                            1,
                        )));
                    }
                } else if cx.focused {
                    self.handle_pointer(cx);
                    if self.coast.is_moving() {
                        let scroll =
                            self.coast
                                .update(self.scroll, 0.0, self.max_scroll(), cx.frames);
                        let settings = cx.global.settings;
                        self.scroll_to(&settings, scroll);
                    }
                    // touches and the list moving hold the buttons off (`0x710006b860`)
                    if !self.gestures.is_active()
                        && !self.coast.is_moving()
                        && self.state == State::Active
                    {
                        self.handle_keys(cx);
                    }
                }
            }
            State::Closing => {
                // `$_17`: after the flash, changed settings are written behind the save notice
                if let Some(notice) = &mut self.saving {
                    if !notice.update(&mut cx.reborrow(cx.focused)) {
                        self.saving = None;
                        self.state = State::FadingOut;
                    }
                } else if self.close_flash.is_idle() {
                    if cx.global.settings != self.opened_with {
                        self.saving = Some(SaveNotice::new(cx.pre_render, &self.assets, None));
                    } else {
                        self.state = State::FadingOut;
                    }
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

        if let Some(Child::Controls(screen)) = &self.child {
            Sprite::new(t("caption"), [480.0, top, 1440.0, top + 128.0])
                .draw(pass, builder, projection);
            screen.render(pass, &self.textures, slid, top, 1080.0 - f * 88.0 + 20.0);
            return;
        }

        // only the PC tab is taller than the 666 px view: the list is clipped to it (the scroll bar is left out)
        ui::clip(
            pass,
            Some([0.0, LIST_Y, 1920.0, LIST_Y + VIEW]),
            self.target_size,
        );
        let list = slid * Mat4::from_translation(vec3(LIST_X, LIST_Y - self.scroll, 0.0));
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
        ui::clip(pass, None, self.target_size);
        // the window's edges over the list's clip
        Sprite::new(t("window"), [136.0, 247.0, 1784.0, 252.0])
            .uv([0.04825, 0.08513, 0.95175, 0.09113])
            .draw(pass, builder, slid);
        Sprite::new(t("window"), [136.0, 918.0, 1784.0, 923.0])
            .uv([0.04825, 0.88969, 0.95175, 0.89568])
            .draw(pass, builder, slid);

        for (page, (selected, flash)) in self.tabs.iter().enumerate() {
            let s = 1.0 + 0.15 * flash.value();
            let transform = slid
                * Mat4::from_translation(vec3(tab_x(page) + 150.0, 144.0 + 33.75, 0.0))
                * Mat4::from_scale(vec3(s, s, 1.0))
                * Mat4::from_translation(vec3(-150.0, -33.75, 0.0));
            let selected = selected.value();
            // the art has the original three tabs, labels included; the PC tab is the first one's ends with its
            // plain middle stretched between them, and its label in the game's font
            let pieces: &[([f32; 2], [f32; 2])] = if page == PC_PAGE {
                &[
                    ([0.0, 60.0], [0.0, 0.2]),
                    ([60.0, 240.0], [0.19, 0.2]),
                    ([240.0, 300.0], [0.8, 1.0]),
                ]
            } else {
                &[([0.0, 300.0], [0.0, 1.0])]
            };
            let v = if page == PC_PAGE {
                0.0
            } else {
                page as f32 / 3.0
            };
            for &([x0, x1], [u0, u1]) in pieces {
                // shortcut: the original mixes the two textures in one draw (`0x71000c70d0`); drawing the selected
                // one over the other differs only where both are see-through
                Sprite::new(t("tab_off"), [x0, 0.0, x1, 67.5])
                    .uv([u0, v, u1, v + 1.0 / 3.0])
                    .draw(pass, builder, transform);
                Sprite::new(t("tab_on"), [x0, 0.0, x1, 67.5])
                    .uv([u0, v, u1, v + 1.0 / 3.0])
                    .alpha(selected)
                    .draw(pass, builder, transform);
            }
            if page == PC_PAGE {
                // the art's dark lettering, brighter when selected
                // a little high, as the art's capitals sit
                let at = vec2(150.0, 30.0) - self.pc_tab.size() / 2.0;
                ui::draw_text(
                    pass,
                    &self.pc_tab,
                    transform * Mat4::from_translation(at.extend(0.0)),
                    TAB_TEXT.with_alpha(0.6 + 0.4 * selected),
                    TAB_BORDER.with_alpha(0.5 * (0.6 + 0.4 * selected)),
                );
            }
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
            let rect = [x, y, x + 48.0, y + 48.0];
            ui::prompts::draw(
                pass,
                builder,
                projection,
                t("key"),
                help.glyph,
                rect,
                FloatColor4::WHITE,
            );
            right = x - 16.0;
        }

        if let Some(Child::Reset(dialog) | Child::Notice(dialog)) = &self.child {
            dialog.render(pass);
        }
        if let Some(notice) = &self.saving {
            notice.render(pass);
        }
    }
}
