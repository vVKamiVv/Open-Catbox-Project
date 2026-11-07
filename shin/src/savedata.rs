//! The save file: one file holding everything (`savedata:/SAVEDATA` on the Switch, same format), read at startup and
//! written whole (`shin::SaveGame` `0x71000d3ce0`, `shin::LoadGame` `0x71000d4890`).
//!
//! By default it is `SAVEDATA` in the user's data directory (`%APPDATA%\shin` on Windows); a Switch save copied there
//! loads as is.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use shin_core::format::save::SaveData;
use tracing::{info, warn};

use crate::asset::executable::EngineText;

/// Why the save file could not be used, for the dialog `LoadGame` shows (`0x71000d4c80`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadProblem {
    /// Case 1: it does not decode; a fresh save replaces it
    Corrupt,
    /// Case 2: written by a newer version; a fresh save replaces it
    Newer,
    /// Any other case: it cannot be read at all, so nothing is saved this session
    Unreadable,
}

impl LoadProblem {
    /// The dialog's text: the executable's, which the English patch leaves in Japanese, so English players get the
    /// port's own wording
    pub fn message(self) -> String {
        let (text, english) = match self {
            Self::Corrupt => (
                EngineText(0x7100161ace, ""),
                "The save data is damaged and can't be loaded.@rThe game will start from the beginning.",
            ),
            Self::Newer => (
                EngineText(0x710015c930, ""),
                "This save data is from a newer version of the game.@rThe game will start with a new save.",
            ),
            Self::Unreadable => (
                EngineText(0x710015f8dc, ""),
                "The save data could not be read.@rNothing will be saved this time.",
            ),
        };
        if crate::adv::menu::config_menu::japanese() {
            text.get()
        } else {
            english.to_owned()
        }
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs_next::data_dir().map(|dir| dir.join("shin").join("SAVEDATA"))
}

/// The save file at `path`, or a fresh save when there is none, with the problem to tell the player about. A file
/// that does not decode (or is from a newer version) is moved aside to `SAVEDATA.unreadable` rather than overwritten
/// later; one that cannot be read at all is an error, so it is not overwritten either.
pub fn load(path: &Path) -> Result<(SaveData, Option<LoadProblem>)> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!("No save file at {}; starting a fresh one", path.display());
            return Ok((SaveData::new(), None));
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    match SaveData::decode(&data) {
        Ok(save) => {
            info!(
                "Loaded the save file {} ({} h of play)",
                path.display(),
                save.play_seconds / 3600
            );
            Ok((save, None))
        }
        Err(e) => {
            let aside = path.with_extension("unreadable");
            warn!(
                "The save file {} is unreadable ({e:#}); moving it to {} and starting a fresh one",
                path.display(),
                aside.display()
            );
            std::fs::rename(path, &aside).context("moving the unreadable save file aside")?;
            // the leading version byte (once deobfuscated) above 1 is a newer version's save
            let newer = SaveData::deobfuscate(&data)
                .is_ok_and(|plain| plain.first().is_some_and(|&v| v > 1));
            let problem = if newer {
                LoadProblem::Newer
            } else {
                LoadProblem::Corrupt
            };
            Ok((SaveData::new(), Some(problem)))
        }
    }
}

/// Writes `save` to `path`, through a temporary file so a failed write leaves the old file whole
pub fn write(path: &Path, save: &SaveData) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, save.encode())
        .with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))?;
    info!("Wrote the save file {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_save_loads_back() {
        let dir = std::env::temp_dir().join(format!("shin-savedata-test-{}", std::process::id()));
        let path = dir.join("SAVEDATA");
        let mut save = SaveData::new();
        save.play_seconds = 1234;
        write(&path, &save).unwrap();
        assert_eq!(load(&path).unwrap().0.play_seconds, 1234);

        std::fs::write(&path, b"not a save").unwrap();
        let (save, problem) = load(&path).unwrap();
        assert_eq!(
            (save.play_seconds, problem),
            (0, Some(LoadProblem::Corrupt))
        );
        assert!(path.with_extension("unreadable").exists());

        // a well-formed file whose version byte is 2
        std::fs::write(&path, SaveData::obfuscate(&[2u8; 64])).unwrap();
        assert_eq!(load(&path).unwrap().1, Some(LoadProblem::Newer));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
