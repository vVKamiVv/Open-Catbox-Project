//! Linux and macOS: the package installed the launcher, and updates replace it in place (the updater plugin's own
//! install). There is no installer screen and no portable home; the same calls as on Windows say so.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

/// Never built in: the package carries `shin` next to the launcher
pub const SHIN: Option<&[u8]> = None;

/// What the command line asks for; only Windows' installer passes anything
#[derive(Debug, PartialEq)]
pub enum Mode {
    Normal,
    Uninstall,
    Update(Option<PathBuf>),
    UpdateFailed(String),
}

pub fn mode(_args: &[OsString]) -> Mode {
    Mode::Normal
}

/// `(step label, fraction 0..1)`
pub type Progress<'a> = &'a dyn Fn(&str, f64);

pub struct Choices {
    pub folder: PathBuf,
    pub menu: bool,
    pub desktop: bool,
}

/// The package's copy is not "installed" in the Windows sense: its home is the user's data folder, not beside it
pub fn is_installed() -> bool {
    false
}

pub fn default_folder() -> PathBuf {
    PathBuf::new()
}

pub fn install(_choices: &Choices, _progress: Progress) -> Result<PathBuf, String> {
    Err("On this system the launcher is installed by its package.".into())
}

pub fn run_update(_folder: Option<PathBuf>) {}

pub fn uninstall(_folder: &Path, _webview: &[PathBuf]) -> Result<(), String> {
    Err(
        "Remove Open Catbox the way it was installed (its package, or the app in Applications)."
            .into(),
    )
}

/// Starts `program` on its own, in its folder.
pub fn start(program: &Path, args: &[OsString]) -> Result<(), String> {
    Command::new(program)
        .args(args)
        .current_dir(program.parent().unwrap_or(Path::new("")))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot start {}: {e}", program.display()))
}
