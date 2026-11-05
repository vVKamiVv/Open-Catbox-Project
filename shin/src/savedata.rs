//! The save file: one file holding everything (`savedata:/SAVEDATA` on the Switch, same format), read at startup and
//! written whole (`shin::SaveGame` `0x71000d3ce0`, `shin::LoadGame` `0x71000d4890`).
//!
//! By default it is `SAVEDATA` in the user's data directory (`%APPDATA%\shin` on Windows); a Switch save copied there
//! loads as is.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use shin_core::format::save::SaveData;
use tracing::{info, warn};

pub fn default_path() -> Option<PathBuf> {
    dirs_next::data_dir().map(|dir| dir.join("shin").join("SAVEDATA"))
}

/// The save file at `path`, or a fresh save when there is none. A file that does not decode is moved aside to
/// `SAVEDATA.unreadable` rather than overwritten later; one that cannot be read at all is an error, so it is not
/// overwritten either.
pub fn load(path: &Path) -> Result<SaveData> {
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!("No save file at {}; starting a fresh one", path.display());
            return Ok(SaveData::new());
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
            Ok(save)
        }
        Err(e) => {
            let aside = path.with_extension("unreadable");
            warn!(
                "The save file {} is unreadable ({e:#}); moving it to {} and starting a fresh one",
                path.display(),
                aside.display()
            );
            std::fs::rename(path, &aside).context("moving the unreadable save file aside")?;
            Ok(SaveData::new())
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
        assert_eq!(load(&path).unwrap().play_seconds, 1234);

        std::fs::write(&path, b"not a save").unwrap();
        assert_eq!(load(&path).unwrap().play_seconds, 0);
        assert!(path.with_extension("unreadable").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
