//! Port-only: the remappable controls. Every [`AppAction`] has a list of inputs (keys, mouse buttons, gamepad buttons
//! and stick directions, any number of each); the defaults below are the bindings the port always had, and
//! `settings.json`'s `bindings` map (action name to input names, e.g. `"system_menu": ["KeyM", "PadWest"]`) replaces
//! an action's whole list. Gamepad buttons are named by position (`PadSouth` is the Switch's B, Xbox A, PlayStation
//! ✕), as the game maps them. The Controls screen (Config → PC) edits the map.

use std::{
    collections::BTreeMap,
    sync::{
        LazyLock, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use enum_map::EnumMap;
use shin_input::{
    RawInputState,
    inputs::{GamepadButton, KeyCode, MouseButton, VirtualGamepadButton},
};
use tracing::warn;

use crate::{app::AppAction, port_settings};

/// One physical input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    Key(KeyCode),
    /// Not the left button, which is the pointer
    Mouse(MouseButton),
    Pad(GamepadButton),
    Stick(VirtualGamepadButton),
}

/// The pad buttons' names: by position, the Switch label they map to beside
const PAD_NAMES: [(GamepadButton, &str); 16] = [
    (GamepadButton::B, "PadSouth"),
    (GamepadButton::A, "PadEast"),
    (GamepadButton::Y, "PadWest"),
    (GamepadButton::X, "PadNorth"),
    (GamepadButton::Plus, "PadStart"),
    (GamepadButton::Minus, "PadSelect"),
    (GamepadButton::L, "PadL1"),
    (GamepadButton::R, "PadR1"),
    (GamepadButton::ZL, "PadL2"),
    (GamepadButton::ZR, "PadR2"),
    (GamepadButton::StickL, "PadL3"),
    (GamepadButton::StickR, "PadR3"),
    (GamepadButton::Up, "PadUp"),
    (GamepadButton::Down, "PadDown"),
    (GamepadButton::Left, "PadLeft"),
    (GamepadButton::Right, "PadRight"),
];
const STICK_NAMES: [(VirtualGamepadButton, &str); 8] = [
    (VirtualGamepadButton::StickLUp, "LStickUp"),
    (VirtualGamepadButton::StickLDown, "LStickDown"),
    (VirtualGamepadButton::StickLLeft, "LStickLeft"),
    (VirtualGamepadButton::StickLRight, "LStickRight"),
    (VirtualGamepadButton::StickRUp, "RStickUp"),
    (VirtualGamepadButton::StickRDown, "RStickDown"),
    (VirtualGamepadButton::StickRLeft, "RStickLeft"),
    (VirtualGamepadButton::StickRRight, "RStickRight"),
];
const MOUSE_NAMES: [(MouseButton, &str, &str); 4] = [
    (MouseButton::Right, "MouseRight", "Right click"),
    (MouseButton::Middle, "MouseMiddle", "Middle click"),
    (MouseButton::WheelUp, "WheelUp", "Wheel up"),
    (MouseButton::WheelDown, "WheelDown", "Wheel down"),
];

impl Input {
    /// A gamepad input (the Controls screen's second column), not the keyboard or mouse
    pub fn is_pad(self) -> bool {
        matches!(self, Input::Pad(_) | Input::Stick(_))
    }

    /// The name in `settings.json`: winit's key names (`KeyS`, `F5`, `Space`), `MouseRight`, `PadSouth`, `RStickUp`
    pub fn name(self) -> String {
        match self {
            Input::Key(key) => serde_json::to_value(key)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("{key:?}")),
            Input::Mouse(button) => find(&MOUSE_NAMES.map(|(b, n, _)| (b, n)), button)
                .unwrap_or("MouseLeft")
                .to_owned(),
            Input::Pad(button) => find(&PAD_NAMES, button).unwrap().to_owned(),
            Input::Stick(direction) => find(&STICK_NAMES, direction).unwrap().to_owned(),
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        if let Some(button) = by_name(&PAD_NAMES, name) {
            return Some(Input::Pad(button));
        }
        if let Some(direction) = by_name(&STICK_NAMES, name) {
            return Some(Input::Stick(direction));
        }
        if let Some(button) = by_name(&MOUSE_NAMES.map(|(b, n, _)| (b, n)), name) {
            return Some(Input::Mouse(button));
        }
        serde_json::from_value(serde_json::Value::String(name.to_owned()))
            .ok()
            .map(Input::Key)
    }

    /// What the screens show for a keyboard or mouse input: a short name (`A`, `Space`, `Esc`, `Right click`)
    pub fn label(self) -> String {
        let key = match self {
            Input::Key(key) => key,
            Input::Mouse(button) => {
                return MOUSE_NAMES
                    .iter()
                    .find(|(b, ..)| *b == button)
                    .map_or("Click", |(.., label)| label)
                    .to_owned();
            }
            Input::Pad(_) | Input::Stick(_) => return self.name(),
        };
        let short = match key {
            KeyCode::Backspace => "BkSp",
            KeyCode::Escape => "Esc",
            KeyCode::ControlLeft => "LCtrl",
            KeyCode::ControlRight => "RCtrl",
            KeyCode::ShiftLeft => "LShift",
            KeyCode::ShiftRight => "RShift",
            KeyCode::AltLeft => "LAlt",
            KeyCode::AltRight => "RAlt",
            KeyCode::PageUp => "PgUp",
            KeyCode::PageDown => "PgDn",
            KeyCode::ArrowUp => "Up",
            KeyCode::ArrowDown => "Down",
            KeyCode::ArrowLeft => "Left",
            KeyCode::ArrowRight => "Right",
            KeyCode::Equal => "=",
            KeyCode::Minus => "-",
            KeyCode::Comma => ",",
            KeyCode::Period => ".",
            KeyCode::Slash => "/",
            KeyCode::Backslash => "\\",
            KeyCode::Semicolon => ";",
            KeyCode::Quote => "'",
            KeyCode::BracketLeft => "[",
            KeyCode::BracketRight => "]",
            KeyCode::Backquote => "`",
            KeyCode::Delete => "Del",
            KeyCode::Insert => "Ins",
            _ => "",
        };
        if !short.is_empty() {
            return short.to_owned();
        }
        let name = self.name();
        for (prefix, with) in [("Key", ""), ("Digit", ""), ("Numpad", "Num")] {
            if let Some(rest) = name.strip_prefix(prefix) {
                return format!("{with}{rest}");
            }
        }
        name
    }

    fn held(self, raw: &RawInputState) -> bool {
        match self {
            Input::Key(key) => raw.keyboard.contains(&key),
            Input::Mouse(button) => raw.mouse.buttons[button],
            Input::Pad(button) => raw.gamepads.is_held(button),
            Input::Stick(direction) => raw.gamepads.is_vheld(direction),
        }
    }
}

fn by_name<T: Copy>(list: &[(T, &str)], name: &str) -> Option<T> {
    list.iter().find(|(_, n)| *n == name).map(|(v, _)| *v)
}

fn find<T: PartialEq + Copy>(list: &[(T, &'static str)], value: T) -> Option<&'static str> {
    list.iter().find(|(v, _)| *v == value).map(|(_, n)| *n)
}

/// The contexts an action is read in; two actions sharing an input must not share a context
pub const STORY: u8 = 1;
pub const MENUS: u8 = 2;

pub struct ActionDef {
    pub action: AppAction,
    /// The key in `settings.json`
    pub name: &'static str,
    pub contexts: u8,
    /// Shown on the Controls screen; the others (menu navigation, `--dev`) keep their defaults
    pub remappable: bool,
    pub defaults: &'static [Input],
}

use GamepadButton as P;
use Input::{Key as K, Mouse as M, Pad, Stick as S};
use KeyCode as C;
use VirtualGamepadButton as V;

const fn def(
    action: AppAction,
    name: &'static str,
    contexts: u8,
    remappable: bool,
    defaults: &'static [Input],
) -> ActionDef {
    ActionDef {
        action,
        name,
        contexts,
        remappable,
        defaults,
    }
}

/// Every action with its defaults, in the Controls screen's order
pub const ACTIONS: &[ActionDef] = &[
    def(
        AppAction::Enter,
        "confirm",
        STORY | MENUS,
        true,
        &[K(C::Space), K(C::Enter), Pad(P::A)],
    ),
    def(
        AppAction::Cancel,
        "cancel",
        STORY | MENUS,
        true,
        &[
            K(C::Backspace),
            K(C::Escape),
            M(MouseButton::Right),
            Pad(P::B),
        ],
    ),
    def(
        AppAction::ButtonY,
        "system_menu",
        STORY | MENUS,
        true,
        &[K(C::KeyY), Pad(P::Y)],
    ),
    def(
        AppAction::ButtonX,
        "backlog",
        STORY | MENUS,
        true,
        &[K(C::KeyX), Pad(P::X)],
    ),
    def(
        AppAction::AutoToggle,
        "auto",
        STORY | MENUS,
        true,
        &[K(C::KeyA), Pad(P::L)],
    ),
    def(
        AppAction::SkipToggle,
        "skip",
        STORY | MENUS,
        true,
        &[K(C::KeyS), Pad(P::R)],
    ),
    def(
        AppAction::HoldSkip,
        "hold_skip",
        STORY,
        true,
        &[K(C::ControlLeft)],
    ),
    def(
        AppAction::Plus,
        "next_chapter",
        STORY | MENUS,
        true,
        &[K(C::Equal), Pad(P::Plus)],
    ),
    def(
        AppAction::Minus,
        "previous_chapter",
        STORY | MENUS,
        true,
        &[K(C::Minus), Pad(P::Minus)],
    ),
    def(
        AppAction::Save,
        "save",
        STORY | MENUS,
        true,
        &[K(C::F5), K(C::KeyP), Pad(P::ZR)],
    ),
    def(
        AppAction::RollBack,
        "roll_back",
        STORY,
        true,
        &[K(C::ArrowUp), Pad(P::Up), S(V::StickLUp)],
    ),
    def(
        AppAction::PageUp,
        "page_up",
        STORY | MENUS,
        true,
        &[K(C::PageUp), S(V::StickRUp)],
    ),
    def(
        AppAction::PageDown,
        "page_down",
        STORY | MENUS,
        true,
        &[K(C::PageDown), S(V::StickRDown)],
    ),
    def(
        AppAction::ToggleFullscreen,
        "fullscreen",
        STORY | MENUS,
        true,
        &[K(C::F11)],
    ),
    // menu navigation (and choices and the minigames in the story) stays on the arrows, d-pad and left stick
    def(
        AppAction::AnyUp,
        "up",
        MENUS,
        false,
        &[K(C::ArrowUp), Pad(P::Up), S(V::StickLUp)],
    ),
    def(
        AppAction::AnyDown,
        "down",
        MENUS,
        false,
        &[K(C::ArrowDown), Pad(P::Down), S(V::StickLDown)],
    ),
    def(
        AppAction::AnyLeft,
        "left",
        MENUS,
        false,
        &[K(C::ArrowLeft), Pad(P::Left), S(V::StickLLeft)],
    ),
    def(
        AppAction::AnyRight,
        "right",
        MENUS,
        false,
        &[K(C::ArrowRight), Pad(P::Right), S(V::StickLRight)],
    ),
    // `--dev` only, so they block nothing
    def(AppAction::DevOverlay, "dev_overlay", 0, false, &[K(C::F3)]),
    def(AppAction::DevPause, "dev_pause", 0, false, &[K(C::F6)]),
    def(AppAction::DevStep, "dev_step", 0, false, &[K(C::F7)]),
    def(AppAction::DevSlower, "dev_slower", 0, false, &[K(C::F8)]),
    def(AppAction::DevFaster, "dev_faster", 0, false, &[K(C::F9)]),
    def(
        AppAction::DevSpeedReset,
        "dev_speed_reset",
        0,
        false,
        &[K(C::F10)],
    ),
];

pub fn def_of(action: AppAction) -> &'static ActionDef {
    ACTIONS
        .iter()
        .find(|d| d.action == action)
        .expect("every action has a definition")
}

pub type Table = EnumMap<AppAction, Vec<Input>>;

pub fn defaults() -> Table {
    EnumMap::from_fn(|action| def_of(action).defaults.to_vec())
}

/// Another action that reads `input` where `action` is read too
pub fn conflict(table: &Table, action: AppAction, input: Input) -> Option<AppAction> {
    let contexts = def_of(action).contexts;
    ACTIONS
        .iter()
        .find(|d| {
            d.action != action && d.contexts & contexts != 0 && table[d.action].contains(&input)
        })
        .map(|d| d.action)
}

/// The defaults with `overrides` applied, and what was wrong with them. An unknown action or input name is skipped,
/// an action left without a valid input or clashing with another keeps its defaults.
pub fn resolve(overrides: &BTreeMap<String, Vec<String>>) -> (Table, Vec<String>) {
    let mut table = defaults();
    let mut warnings = Vec::new();
    let mut overridden = Vec::new();
    for (name, names) in overrides {
        let Some(def) = ACTIONS.iter().find(|d| d.name == name) else {
            warnings.push(format!("unknown action {name:?}"));
            continue;
        };
        if !def.remappable {
            warnings.push(format!("{name:?} cannot be rebound"));
            continue;
        }
        let mut inputs = Vec::new();
        for input in names {
            match Input::parse(input) {
                Some(parsed) if !inputs.contains(&parsed) => inputs.push(parsed),
                Some(_) => {}
                None => warnings.push(format!("{name:?}: unknown input {input:?}")),
            }
        }
        if inputs.is_empty() {
            warnings.push(format!("{name:?} has no valid input; using its defaults"));
            continue;
        }
        table[def.action] = inputs;
        overridden.push(def.action);
    }
    // an overridden action that clashes goes back to its defaults, until nothing clashes (the defaults never do)
    while let Some(index) = overridden
        .iter()
        .position(|&a| table[a].iter().any(|&i| conflict(&table, a, i).is_some()))
    {
        let action = overridden.remove(index);
        let def = def_of(action);
        warnings.push(format!(
            "{:?} clashes with another action; using its defaults",
            def.name
        ));
        table[action] = def.defaults.to_vec();
    }
    (table, warnings)
}

static TABLE: LazyLock<RwLock<Table>> = LazyLock::new(|| RwLock::new(defaults()));
/// Counts the changes, for whatever caches the bound keys' names
static GENERATION: AtomicU32 = AtomicU32::new(0);
/// The inputs held in the last frame, for the Controls screen
static HELD: Mutex<Vec<Input>> = Mutex::new(Vec::new());
/// The Controls screen waits for an input: the game sees none
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Makes `overrides` the bindings (from [`port_settings`], whenever its `bindings` change)
pub fn apply(overrides: &BTreeMap<String, Vec<String>>) {
    let (table, warnings) = resolve(overrides);
    for warning in warnings {
        warn!("settings.json bindings: {warning}");
    }
    *TABLE.write().unwrap() = table;
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn table() -> Table {
    TABLE.read().unwrap().clone()
}

pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// Every action's state from the raw input (`AppAction::lower`)
pub fn lower(raw: &RawInputState) -> EnumMap<AppAction, bool> {
    let table = TABLE.read().unwrap();
    EnumMap::from_fn(|action| table[action].iter().any(|input| input.held(raw)))
}

/// Remembers what is held this frame (from `App::filter_input`)
pub fn record_held(raw: &RawInputState) {
    let mut held: Vec<Input> = raw.keyboard.iter().map(|&key| Input::Key(key)).collect();
    held.extend(
        MOUSE_NAMES
            .iter()
            .filter(|(button, ..)| raw.mouse.buttons[*button])
            .map(|(button, ..)| Input::Mouse(*button)),
    );
    held.extend(
        PAD_NAMES
            .iter()
            .filter(|(button, _)| raw.gamepads.is_held(*button))
            .map(|(button, _)| Input::Pad(*button)),
    );
    held.extend(
        STICK_NAMES
            .iter()
            .filter(|(direction, _)| raw.gamepads.is_vheld(*direction))
            .map(|(direction, _)| Input::Stick(*direction)),
    );
    *HELD.lock().unwrap() = held;
}

pub fn held() -> Vec<Input> {
    HELD.lock().unwrap().clone()
}

pub fn set_capturing(capturing: bool) {
    CAPTURING.store(capturing, Ordering::Relaxed);
}

pub fn is_capturing() -> bool {
    CAPTURING.load(Ordering::Relaxed)
}

/// Binds `input` to `action` in place of its inputs of the same kind (keyboard and mouse, or gamepad), and saves it;
/// refused with the action already reading it in the same place
pub fn rebind(action: AppAction, input: Input) -> Result<(), AppAction> {
    let table = table();
    if let Some(other) = conflict(&table, action, input) {
        return Err(other);
    }
    let mut inputs: Vec<Input> = table[action]
        .iter()
        .copied()
        .filter(|i| i.is_pad() != input.is_pad())
        .collect();
    // keyboard first, then the pad, as the defaults are listed
    let at = if input.is_pad() { inputs.len() } else { 0 };
    inputs.insert(at, input);
    let def = def_of(action);
    port_settings::update(|s| {
        if inputs == def.defaults {
            s.bindings.remove(def.name);
        } else {
            s.bindings.insert(
                def.name.to_owned(),
                inputs.iter().map(|i| i.name()).collect(),
            );
        }
    });
    Ok(())
}

/// Every action back to its defaults
pub fn reset() {
    port_settings::update(|s| s.bindings.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for name in [
            "KeyS",
            "F5",
            "Space",
            "Escape",
            "ControlLeft",
            "MouseRight",
            "WheelDown",
            "PadSouth",
            "PadR2",
            "RStickUp",
        ] {
            assert_eq!(Input::parse(name).unwrap().name(), name);
        }
        assert_eq!(Input::parse("KeyS"), Some(Input::Key(KeyCode::KeyS)));
        assert_eq!(Input::parse("PadSouth"), Some(Input::Pad(GamepadButton::B)));
        assert_eq!(Input::parse("MouseLeft"), None);
        assert_eq!(Input::parse("Nonsense"), None);
        assert_eq!(Input::Key(KeyCode::KeyS).label(), "S");
        assert_eq!(Input::Key(KeyCode::Digit4).label(), "4");
        assert_eq!(Input::Key(KeyCode::Backspace).label(), "BkSp");
    }

    #[test]
    fn defaults_are_the_old_bindings_and_do_not_clash() {
        let table = defaults();
        assert_eq!(table[AppAction::SkipToggle], [K(C::KeyS), Pad(P::R)]);
        assert_eq!(
            table[AppAction::Cancel],
            [
                K(C::Backspace),
                K(C::Escape),
                M(MouseButton::Right),
                Pad(P::B)
            ]
        );
        assert_eq!(table[AppAction::Save], [K(C::F5), K(C::KeyP), Pad(P::ZR)]);
        for def in ACTIONS {
            for &input in def.defaults {
                assert_eq!(
                    conflict(&table, def.action, input),
                    None,
                    "{} {input:?}",
                    def.name
                );
            }
        }
        // roll back and menu up share the up arrow: one reads it in the story, the other in menus
        assert!(table[AppAction::AnyUp].contains(&K(C::ArrowUp)));
    }

    #[test]
    fn overrides_replace_and_bad_ones_fall_back() {
        let overrides = |pairs: &[(&str, &[&str])]| -> BTreeMap<String, Vec<String>> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
                .collect()
        };
        let (table, warnings) =
            resolve(&overrides(&[("system_menu", &["KeyM", "Bogus", "PadL3"])]));
        assert_eq!(table[AppAction::ButtonY], [K(C::KeyM), Pad(P::StickL)]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        // PadNorth is the backlog's: the system menu keeps its defaults
        let (table, warnings) = resolve(&overrides(&[("system_menu", &["KeyM", "PadNorth"])]));
        assert_eq!(table[AppAction::ButtonY], defaults()[AppAction::ButtonY]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");

        let (table, warnings) = resolve(&overrides(&[
            ("nonexistent", &["KeyQ"]),
            ("auto", &["Bogus"]),
            ("up", &["KeyW"]),
        ]));
        assert_eq!(table, defaults());
        assert_eq!(warnings.len(), 4, "{warnings:?}");

        // a swap is fine
        let (table, warnings) = resolve(&overrides(&[
            ("confirm", &["Space", "Enter", "PadSouth"]),
            ("cancel", &["Backspace", "Escape", "MouseRight", "PadEast"]),
        ]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(table[AppAction::Enter][2], Pad(P::B));
        // hold-to-skip is story-only, the menus' down arrow is not read there
        let (table, warnings) = resolve(&overrides(&[("hold_skip", &["ArrowDown"])]));
        assert!(warnings.is_empty());
        assert_eq!(table[AppAction::HoldSkip], [K(C::ArrowDown)]);
        // confirm on the down arrow would break the menus
        let (table, _) = resolve(&overrides(&[("confirm", &["ArrowDown"])]));
        assert_eq!(table[AppAction::Enter], defaults()[AppAction::Enter]);
    }
}
