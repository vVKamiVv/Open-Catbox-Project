//! The game's executable, for the texts the engine keeps in it (the culprit game's testimonies and evidence, the
//! system dialogs). It is optional: without it those texts stay empty.
//!
//! It is looked for in a folder holding `main` (the game's `exefs/main`, or a translation's replacement) and the
//! translation's `.ips` patches: `--exefs`, then `SHIN_EXEFS`, then an `exefs` folder next to the assets.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result};
use shin_core::format::executable::Executable;
use tracing::{info, warn};

static EXECUTABLE: OnceLock<Option<Executable>> = OnceLock::new();

/// The executable loaded at startup, if one was found
pub fn executable() -> Option<&'static Executable> {
    EXECUTABLE.get().and_then(Option::as_ref)
}

/// Loads `main` and its patches from the first folder that has them; call once at startup
pub fn load(candidates: &[PathBuf]) {
    let loaded = candidates
        .iter()
        .find(|dir| dir.join("main").is_file())
        .and_then(|dir| match load_from(dir) {
            Ok(executable) => {
                info!("Loaded the game's executable from {}", dir.display());
                Some(executable)
            }
            Err(e) => {
                warn!("Could not read the game's executable in {}: {e:?}", dir.display());
                None
            }
        });
    if loaded.is_none() {
        info!("No game executable found (tried {candidates:?}); its texts stay empty");
    }
    let _ = EXECUTABLE.set(loaded);
}

fn load_from(dir: &Path) -> Result<Executable> {
    let nso = std::fs::read(dir.join("main")).context("reading main")?;
    let mut patches = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("ips")) {
            patches.push(std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?);
        }
    }
    Executable::load(&nso, &patches.iter().map(Vec::as_slice).collect::<Vec<_>>())
}

// the executable keeps its texts in UTF-8 (the Japanese release's too), unlike the scenario's Shift-JIS

/// The string at `address`
pub fn text_at(address: u64) -> Option<String> {
    std::str::from_utf8(executable()?.c_string(address)?).ok().map(str::to_owned)
}

/// The string a pointer at `address` points to
pub fn text_at_pointer(address: u64) -> Option<String> {
    std::str::from_utf8(executable()?.string_at_pointer(address)?).ok().map(str::to_owned)
}

/// A UI text the original reads from its executable, where a translation's `.ips` rewrites it in place: its address,
/// and the English patch's text for when no executable was found
pub struct EngineText(pub u64, pub &'static str);

impl EngineText {
    pub fn get(&self) -> String {
        text_at(self.0).unwrap_or_else(|| self.1.to_owned())
    }
}
