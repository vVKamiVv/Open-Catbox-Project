//! The port's own settings, which the Switch original has no counterpart for: the window, the frame timing,
//! supersampling, the button prompts and the remapped controls. They live in `settings.json` next to the save file (`%APPDATA%\shin\settings.json` on Windows),
//! not in `SAVEDATA`, whose format stays the original's; the Config screen's "PC" tab edits them and every change is
//! written at once. The touch effects stay the save's own setting (`v17_showtoucheffect`, the Miscellaneous tab).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use shin_window::DisplaySettings;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    Windowed,
    Borderless,
    /// Exclusive fullscreen
    Exclusive,
}

/// Which buttons the key help shows
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonPrompts {
    /// The device used last: the keyboard, or the gamepad's own buttons
    #[default]
    Auto,
    Xbox,
    #[serde(rename = "playstation")]
    PlayStation,
    Switch,
    Keyboard,
}

/// The window sizes offered while windowed, in logical pixels
pub const WINDOW_SIZES: [[u32; 2]; 5] = [
    [1280, 720],
    [1600, 900],
    [1920, 1080],
    [2560, 1440],
    [3840, 2160],
];
/// The frame-rate caps offered; 0 is no cap
pub const FRAME_CAPS: [u32; 6] = [0, 30, 60, 120, 144, 240];
/// The supersampling factors offered
pub const SUPERSAMPLING: [f32; 3] = [1.0, 1.5, 2.0];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortSettings {
    pub window_mode: WindowMode,
    /// The window's size while windowed, in logical pixels
    pub window_size: [u32; 2],
    pub vsync: bool,
    /// Frames per second at most; 0 for no cap
    pub frame_cap: u32,
    /// The canvas is rendered this many times larger and filtered down to the window
    pub supersampling: f32,
    pub button_prompts: ButtonPrompts,
    /// A controller confirms with its bottom face button (Xbox A, PlayStation ✕) instead of the right one
    pub bottom_confirms: bool,
    /// The remapped controls: action name to inputs (see [`crate::bindings`]); actions not here keep their defaults
    pub bindings: BTreeMap<String, Vec<String>>,
}

impl PortSettings {
    pub const DEFAULT: Self = Self {
        window_mode: WindowMode::Windowed,
        window_size: [1920, 1080],
        vsync: true,
        frame_cap: 0,
        supersampling: 1.0,
        button_prompts: ButtonPrompts::Auto,
        bottom_confirms: false,
        bindings: BTreeMap::new(),
    };

    /// What the window is asked for
    pub fn display(&self) -> DisplaySettings {
        DisplaySettings {
            mode: match self.window_mode {
                WindowMode::Windowed => shin_window::WindowMode::Windowed,
                WindowMode::Borderless => shin_window::WindowMode::Borderless,
                WindowMode::Exclusive => shin_window::WindowMode::Exclusive,
            },
            size: (self.window_size[0], self.window_size[1]),
            vsync: self.vsync,
            frame_cap: (self.frame_cap > 0).then_some(self.frame_cap),
            // a hand-edited file cannot ask for absurd sizes
            render_scale: self.supersampling.clamp(1.0, 2.0),
        }
    }
}

impl Default for PortSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

static SETTINGS: Mutex<PortSettings> = Mutex::new(PortSettings::DEFAULT);
/// Where changes are written; `None` before `load` or without a data directory
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// `settings.json` next to the save file `savedata`
pub fn path_next_to(savedata: &Path) -> PathBuf {
    savedata.with_file_name("settings.json")
}

/// Reads the settings from `path` (defaults when there is no file or it does not parse) and writes later changes there
pub fn load(path: Option<PathBuf>) {
    let settings = match path.as_deref().map(std::fs::read_to_string) {
        Some(Ok(text)) => match serde_json::from_str(&text) {
            Ok(settings) => {
                info!(
                    "Loaded the PC settings {}",
                    path.as_ref().unwrap().display()
                );
                settings
            }
            Err(e) => {
                warn!("The PC settings file does not parse ({e}); using the defaults");
                PortSettings::default()
            }
        },
        _ => PortSettings::default(),
    };
    crate::bindings::apply(&settings.bindings);
    shin_input::inputs::set_bottom_confirms(settings.bottom_confirms);
    *SETTINGS.lock().unwrap() = settings;
    *PATH.lock().unwrap() = path;
}

/// The settings now
pub fn get() -> PortSettings {
    SETTINGS.lock().unwrap().clone()
}

/// Changes the settings, writing the file when anything changed
pub fn update(change: impl FnOnce(&mut PortSettings)) {
    let mut settings = SETTINGS.lock().unwrap();
    let old = settings.clone();
    change(&mut settings);
    if *settings == old {
        return;
    }
    if settings.bindings != old.bindings {
        crate::bindings::apply(&settings.bindings);
    }
    shin_input::inputs::set_bottom_confirms(settings.bottom_confirms);
    if let Some(path) = PATH.lock().unwrap().as_deref() {
        if let Err(e) = write(path, &settings) {
            warn!("Saving the PC settings failed: {e:?}");
        }
    }
}

/// Changes the settings for this run only (command-line overrides), without writing the file
pub fn override_for_run(change: impl FnOnce(&mut PortSettings)) {
    change(&mut SETTINGS.lock().unwrap());
}

/// Writes through a temporary file, so a failed write leaves the old file whole
fn write(path: &Path, settings: &PortSettings) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_string_pretty(settings)?)
        .with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))?;
    info!("Wrote the PC settings {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_fill_missing_fields() {
        let dir =
            std::env::temp_dir().join(format!("shin-port-settings-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        let settings = PortSettings {
            window_mode: WindowMode::Exclusive,
            frame_cap: 144,
            supersampling: 1.5,
            ..PortSettings::default()
        };
        write(&path, &settings).unwrap();
        let read: PortSettings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read, settings);
        // an older or hand-written file with fields missing takes the defaults for them
        let partial: PortSettings = serde_json::from_str(r#"{"vsync": false}"#).unwrap();
        assert_eq!(
            partial,
            PortSettings {
                vsync: false,
                ..PortSettings::default()
            }
        );
        assert_eq!(partial.display().frame_cap, None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
