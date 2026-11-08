//! The save manager: what the game's save holds (its slots, read with shin-core), and backups of it in a `backups`
//! folder beside it, one folder per backup named by when it was made (`2026-11-07 22-41-05`, `… auto` for the ones
//! the launcher makes by itself: before the game starts, and before a restore or an import replaces the save).
//!
//! There can be several saves (profiles), one picked before playing: the main one at `saves/SAVEDATA`, and others
//! at `saves/profiles/<name>/SAVEDATA`, each with its own backups. A new one starts the game from the beginning.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use shin_core::format::save::{GameData, SaveData};

/// The automatic backups kept; older ones are deleted as new ones are made. Backups the player makes stay.
const KEEP_AUTO: usize = 10;
const STAMP: &str = "%Y-%m-%d %H-%M-%S";

#[derive(Serialize)]
pub struct Slot {
    /// 1..=100 as the game numbers them; `None` for the automatic save
    pub number: Option<u8>,
    pub date: String,
}

/// A save file's contents, as the save manager shows them
#[derive(Serialize)]
pub struct Summary {
    pub slots: Vec<Slot>,
    pub play_seconds: u32,
}

#[derive(Serialize)]
pub struct Backup {
    /// The folder's name, which identifies it
    pub name: String,
    /// When it was made, `2026-11-07 22:41`
    pub made: String,
    pub auto: bool,
    /// `None` when the file inside cannot be read
    pub summary: Option<Summary>,
}

#[derive(Serialize)]
pub struct State {
    pub folder: PathBuf,
    /// Whether there is a save yet
    pub exists: bool,
    /// `None` when there is none, or it cannot be read
    pub current: Option<Summary>,
    pub backups: Vec<Backup>,
}

fn date(data: &GameData) -> String {
    data.date_time.format("%Y-%m-%d %H:%M").to_string()
}

pub fn summary(bytes: &[u8]) -> Option<Summary> {
    let save = SaveData::decode(bytes).ok()?;
    let auto = save.auto_save_slot.iter().map(|data| Slot {
        number: None,
        date: date(data),
    });
    let manual = save
        .manual_save_slots
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| slot.as_ref().map(|data| (i, data)))
        .map(|(i, data)| Slot {
            number: Some(i as u8 + 1),
            date: date(data),
        });
    Some(Summary {
        slots: auto.chain(manual).collect(),
        play_seconds: save.play_seconds,
    })
}

fn backups_dir(savedata: &Path) -> PathBuf {
    savedata.parent().unwrap().join("backups")
}

/// The backup folder `name` (one listed by [`state`]; anything else is refused, so a name cannot reach outside)
fn backup_dir(savedata: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = backups_dir(savedata).join(name);
    let known =
        chrono::NaiveDateTime::parse_from_str(name.trim_end_matches(" auto"), STAMP).is_ok();
    if !known || !dir.join("SAVEDATA").is_file() {
        return Err("This backup is gone.".into());
    }
    Ok(dir)
}

/// The backups, newest first
fn list(savedata: &Path) -> Vec<Backup> {
    let Ok(entries) = fs::read_dir(backups_dir(savedata)) else {
        return Vec::new();
    };
    let mut backups: Vec<Backup> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter_map(|name| {
            let stamp = name.strip_suffix(" auto").unwrap_or(&name);
            let made = chrono::NaiveDateTime::parse_from_str(stamp, STAMP).ok()?;
            let bytes = fs::read(backups_dir(savedata).join(&name).join("SAVEDATA")).ok()?;
            Some(Backup {
                made: made.format("%Y-%m-%d %H:%M").to_string(),
                auto: name.ends_with(" auto"),
                summary: summary(&bytes),
                name,
            })
        })
        .collect();
    // the names sort by time
    backups.sort_by(|a, b| b.name.cmp(&a.name));
    backups
}

pub fn state(savedata: &Path) -> State {
    let bytes = fs::read(savedata).ok();
    State {
        folder: savedata.parent().unwrap().to_path_buf(),
        exists: bytes.is_some(),
        current: bytes.and_then(|bytes| summary(&bytes)),
        backups: list(savedata),
    }
}

/// Copies the save into a new backup; `None` when there is no save, or it is the same as the newest backup.
/// Automatic backups beyond the newest [`KEEP_AUTO`] are deleted.
pub fn back_up(savedata: &Path, auto: bool) -> Result<Option<String>, String> {
    let Ok(bytes) = fs::read(savedata) else {
        return Ok(None);
    };
    let backups = list(savedata);
    let newest = backups
        .first()
        .map(|b| backups_dir(savedata).join(&b.name).join("SAVEDATA"));
    if newest.and_then(|path| fs::read(path).ok()).as_deref() == Some(&bytes[..]) {
        return Ok(None);
    }
    let mut name = chrono::Local::now().format(STAMP).to_string();
    if auto {
        name.push_str(" auto");
    }
    let dir = backups_dir(savedata).join(&name);
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    fs::write(dir.join("SAVEDATA"), &bytes)
        .map_err(|e| format!("Cannot back up your save: {e}"))?;
    for old in list(savedata).iter().filter(|b| b.auto).skip(KEEP_AUTO) {
        let _ = fs::remove_dir_all(backups_dir(savedata).join(&old.name));
    }
    Ok(Some(name))
}

/// Puts `bytes` in place as the save, after backing up the current one. Refuses what the game cannot read.
fn replace(savedata: &Path, bytes: &[u8]) -> Result<(), String> {
    if summary(bytes).is_none() {
        return Err("This is not a save file the game can read.".into());
    }
    back_up(savedata, true)?;
    let dir = savedata.parent().unwrap();
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    // written beside it and renamed over it, so a failure never leaves half a save
    let tmp = dir.join("SAVEDATA.launcher.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("Cannot write your save: {e}"))?;
    fs::rename(&tmp, savedata).map_err(|e| format!("Cannot write your save: {e}"))
}

pub fn restore(savedata: &Path, name: &str) -> Result<(), String> {
    let bytes =
        fs::read(backup_dir(savedata, name)?.join("SAVEDATA")).map_err(|e| e.to_string())?;
    replace(savedata, &bytes)
}

/// A save from elsewhere (a Switch SAVEDATA, or one exported before)
pub fn import(savedata: &Path, file: &Path) -> Result<(), String> {
    let len = fs::metadata(file)
        .map_err(|e| format!("Cannot read {}: {e}", file.display()))?
        .len();
    if len > 1 << 20 {
        return Err("This is not a save file the game can read.".into());
    }
    let bytes = fs::read(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?;
    replace(savedata, &bytes)
}

/// Copies the current save (`name: None`) or a backup to `to`
pub fn export(savedata: &Path, name: Option<&str>, to: &Path) -> Result<(), String> {
    let from = match name {
        Some(name) => backup_dir(savedata, name)?.join("SAVEDATA"),
        None => savedata.to_path_buf(),
    };
    fs::copy(&from, to).map_err(|e| format!("Cannot export the save: {e}"))?;
    Ok(())
}

pub fn delete(savedata: &Path, name: &str) -> Result<(), String> {
    fs::remove_dir_all(backup_dir(savedata, name)?)
        .map_err(|e| format!("Cannot delete the backup: {e}"))
}

// --- Profiles: `main` is the main save's path (`saves/SAVEDATA`); a profile is named by its folder ---

#[derive(Serialize)]
pub struct Profile {
    /// `None` for the main save
    pub name: Option<String>,
    pub summary: Option<Summary>,
}

fn profiles_dir(main: &Path) -> PathBuf {
    main.parent().unwrap().join("profiles")
}

/// The save file of `profile` (`None`: the main one); a named one must exist
pub fn profile_save(main: &Path, profile: Option<&str>) -> Result<PathBuf, String> {
    match profile {
        None => Ok(main.to_path_buf()),
        Some(name) => {
            let dir = profiles_dir(main).join(name);
            if valid_name(name).is_err() || !dir.is_dir() {
                return Err("This save is gone.".into());
            }
            Ok(dir.join("SAVEDATA"))
        }
    }
}

/// The main save, then the others by name
pub fn profiles(main: &Path) -> Vec<Profile> {
    let summary = |path: &Path| fs::read(path).ok().and_then(|bytes| summary(&bytes));
    let mut names: Vec<String> = fs::read_dir(profiles_dir(main))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    names.sort_by_key(|name| name.to_lowercase());
    std::iter::once(Profile {
        name: None,
        summary: summary(main),
    })
    .chain(names.into_iter().map(|name| Profile {
        summary: summary(&profiles_dir(main).join(&name).join("SAVEDATA")),
        name: Some(name),
    }))
    .collect()
}

/// A name that is also a valid folder name on every system
fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    let bad = name.is_empty()
        || name.chars().count() > 40
        || name.starts_with('.')
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || r#"/\:*?"<>|"#.contains(c));
    if bad {
        return Err("Use a name of up to 40 letters, without / \\ : * ? \" < > |.".into());
    }
    Ok(name)
}

fn free_name<'a>(main: &Path, name: &'a str) -> Result<&'a str, String> {
    let name = valid_name(name)?;
    let taken = profiles(main)
        .iter()
        .filter_map(|p| p.name.as_deref())
        .any(|other| other.eq_ignore_ascii_case(name));
    if taken {
        return Err("A save with this name already exists.".into());
    }
    Ok(name)
}

/// A new, empty save: the game starts from the beginning with it
pub fn create(main: &Path, name: &str) -> Result<String, String> {
    let name = free_name(main, name)?;
    let dir = profiles_dir(main).join(name);
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    Ok(name.to_string())
}

pub fn rename(main: &Path, old: &str, new: &str) -> Result<String, String> {
    let from = profile_save(main, Some(old))?;
    let new = if valid_name(new)?.eq_ignore_ascii_case(old) {
        valid_name(new)? // only its letter case changes
    } else {
        free_name(main, new)?
    };
    fs::rename(from.parent().unwrap(), profiles_dir(main).join(new))
        .map_err(|e| format!("Cannot rename the save: {e}"))?;
    Ok(new.to_string())
}

/// Deletes a save and its backups
pub fn remove(main: &Path, name: &str) -> Result<(), String> {
    let save = profile_save(main, Some(name))?;
    fs::remove_dir_all(save.parent().unwrap()).map_err(|e| format!("Cannot delete the save: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::save::{GameDataEntry, RouteData};

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("open catbox saves {name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A save with slots 1 and 5 and the automatic one
    fn save(play_seconds: u32) -> Vec<u8> {
        let mut save = SaveData::new();
        let data = GameData {
            date_time: chrono::NaiveDate::from_ymd_opt(2026, 11, 7)
                .unwrap()
                .and_hms_opt(9, 5, 0)
                .unwrap(),
            entry: GameDataEntry {
                scenario_id: 0,
                random_seed: 0,
                save_position: 0,
                route_data: RouteData::default(),
            },
        };
        save.manual_save_slots[0] = Some(data.clone());
        save.manual_save_slots[4] = Some(data.clone());
        save.auto_save_slot = Some(data);
        save.play_seconds = play_seconds;
        save.encode()
    }

    #[test]
    fn the_slots_are_read_from_the_save() {
        let s = summary(&save(42)).unwrap();
        let numbers: Vec<_> = s.slots.iter().map(|slot| slot.number).collect();
        assert_eq!(numbers, [None, Some(1), Some(5)]);
        assert_eq!(s.slots[1].date, "2026-11-07 09:05");
        assert_eq!(s.play_seconds, 42);
        assert!(summary(b"not a save").is_none());
    }

    #[test]
    fn backups_restore_and_import_keep_the_save_safe() {
        let dir = temp("backups");
        let savedata = dir.join("SAVEDATA");
        assert_eq!(
            back_up(&savedata, false).unwrap(),
            None,
            "no save, no backup"
        );

        fs::write(&savedata, save(1)).unwrap();
        let first = back_up(&savedata, false).unwrap().unwrap();
        assert_eq!(
            back_up(&savedata, true).unwrap(),
            None,
            "unchanged since the last backup"
        );

        // importing a file the game cannot read changes nothing
        let junk = dir.join("junk");
        fs::write(&junk, b"junk").unwrap();
        assert!(import(&savedata, &junk).is_err());
        assert_eq!(fs::read(&savedata).unwrap(), save(1));

        // the import replaces the save (already backed up, so no new backup)
        let other = dir.join("other");
        fs::write(&other, save(2)).unwrap();
        import(&savedata, &other).unwrap();
        assert_eq!(
            summary(&fs::read(&savedata).unwrap()).unwrap().play_seconds,
            2
        );

        // restoring the first backup backs up the imported save first
        std::thread::sleep(std::time::Duration::from_millis(1100)); // a new second, so a new name
        restore(&savedata, &first).unwrap();
        assert_eq!(
            summary(&fs::read(&savedata).unwrap()).unwrap().play_seconds,
            1
        );
        let s = state(&savedata);
        assert_eq!(s.backups.len(), 2);
        assert!(
            s.backups[0].auto,
            "the imported save, kept before the restore"
        );
        assert_eq!(s.backups[0].summary.as_ref().unwrap().play_seconds, 2);

        let out = dir.join("exported");
        export(&savedata, Some(&first), &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), save(1));

        delete(&savedata, &first).unwrap();
        assert_eq!(state(&savedata).backups.len(), 1);
        assert!(delete(&savedata, "..").is_err(), "only listed backups");
        assert!(restore(&savedata, "../../x").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_automatic_backups_are_kept() {
        let dir = temp("prune");
        let savedata = dir.join("SAVEDATA");
        let backups = dir.join("backups");
        // twelve old automatic backups and one the player made
        for i in 0..12 {
            let d = backups.join(format!("2026-01-01 00-00-{i:02} auto"));
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("SAVEDATA"), save(i)).unwrap();
        }
        let mine = backups.join("2026-01-01 00-00-30");
        fs::create_dir_all(&mine).unwrap();
        fs::write(mine.join("SAVEDATA"), save(30)).unwrap();

        fs::write(&savedata, save(99)).unwrap();
        back_up(&savedata, true).unwrap().unwrap();
        let s = state(&savedata);
        assert_eq!(s.backups.iter().filter(|b| b.auto).count(), KEEP_AUTO);
        assert!(
            s.backups.iter().any(|b| !b.auto),
            "the player's backup stays"
        );
        assert!(
            !backups.join("2026-01-01 00-00-00 auto").exists(),
            "the oldest went"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn profiles_are_separate_saves_with_their_own_backups() {
        let dir = temp("profiles");
        let main = dir.join("SAVEDATA");
        fs::write(&main, save(5)).unwrap();

        let fresh = create(&main, "  New run ").unwrap();
        assert_eq!(fresh, "New run");
        let fresh_save = profile_save(&main, Some(&fresh)).unwrap();
        assert!(!fresh_save.exists(), "a new save starts empty");
        assert!(create(&main, "new RUN").is_err(), "names are unique");
        for bad in ["", "a/b", "..", "x:y", &"n".repeat(41)] {
            assert!(create(&main, bad).is_err(), "{bad:?}");
        }
        assert!(profile_save(&main, Some("..")).is_err());
        assert!(profile_save(&main, Some("missing")).is_err());

        // its own backups, beside it
        fs::write(&fresh_save, save(1)).unwrap();
        back_up(&fresh_save, false).unwrap().unwrap();
        assert_eq!(state(&fresh_save).backups.len(), 1);
        assert!(state(&main).backups.is_empty());

        let list = profiles(&main);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, None);
        assert_eq!(list[0].summary.as_ref().unwrap().play_seconds, 5);
        assert_eq!(list[1].summary.as_ref().unwrap().play_seconds, 1);

        let renamed = rename(&main, "New run", "Second").unwrap();
        assert!(profile_save(&main, Some("New run")).is_err());
        let moved = profile_save(&main, Some(&renamed)).unwrap();
        assert_eq!(fs::read(&moved).unwrap(), save(1));
        assert_eq!(rename(&main, "Second", "second").unwrap(), "second");

        remove(&main, "second").unwrap();
        assert_eq!(profiles(&main).len(), 1);
        assert_eq!(
            fs::read(&main).unwrap(),
            save(5),
            "the main save is untouched"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
