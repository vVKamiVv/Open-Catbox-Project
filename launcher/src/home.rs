//! Where the launcher keeps things. The installed launcher (`Open Catbox.exe`) is portable: its home is its own
//! folder, holding `launcher.json`, `data\` (data.rom, languages, logs), `saves\` (the game's SAVEDATA and
//! settings.json) and `webview\` (the window's WebView2 data). When that folder is not writable (e.g. Program Files),
//! and for builds from source, the home is `%APPDATA%\OpenCatbox` and the saves stay in shin's own `%APPDATA%\shin`.
//!
//! On the first start of a portable home, settings, data and saves from `%APPDATA%` are brought in once (see
//! [`plan`]). `OPEN_CATBOX_APPDATA` replaces `%APPDATA%` for all of this, so tests never touch the real one.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::Settings;

/// `(step label, fraction 0..1)`
pub type Progress<'a> = &'a dyn Fn(&str, f64);

pub struct Home {
    pub dir: PathBuf,
    /// The launcher's own folder rather than `%APPDATA%\OpenCatbox`.
    pub portable: bool,
    /// `%APPDATA%`, or `OPEN_CATBOX_APPDATA`.
    pub appdata: PathBuf,
}

impl Home {
    pub fn choose(exe_dir: &Path, installed: bool, appdata: PathBuf) -> Home {
        if installed && writable(exe_dir) {
            Home {
                dir: exe_dir.to_path_buf(),
                portable: true,
                appdata,
            }
        } else {
            Home {
                dir: appdata.join("OpenCatbox"),
                portable: false,
                appdata,
            }
        }
    }

    pub fn settings(&self) -> PathBuf {
        self.dir.join("launcher.json")
    }

    /// The data folder: `launcher.json`'s `data_dir` (relative to the home), else `<home>\data`.
    pub fn data(&self, settings: &Settings) -> PathBuf {
        match &settings.data_dir {
            Some(dir) => self.dir.join(dir),
            None => self.dir.join("data"),
        }
    }

    /// The game's save file, passed to shin as `--savedata`; its settings.json sits next to it.
    pub fn savedata(&self) -> PathBuf {
        if self.portable {
            self.dir.join("saves").join("SAVEDATA")
        } else {
            self.appdata.join("shin").join("SAVEDATA")
        }
    }

    /// The WebView2 data folder, when portable (else Tauri's default under `%LOCALAPPDATA%`).
    pub fn webview(&self) -> Option<PathBuf> {
        self.portable.then(|| self.dir.join("webview"))
    }
}

pub fn get() -> &'static Home {
    static HOME: OnceLock<Home> = OnceLock::new();
    HOME.get_or_init(|| {
        let appdata = std::env::var_os("OPEN_CATBOX_APPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from)
            .unwrap_or_default();
        Home::choose(&crate::exe_dir(), crate::install::is_installed(), appdata)
    })
}

/// Whether a file can be created in `dir`.
pub fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    let ok = File::create_new(&probe).is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// What the first start of a portable home brings in from `%APPDATA%`.
#[derive(Serialize, Debug, PartialEq)]
pub struct Plan {
    /// To write as the home's `launcher.json`: the old settings, `data_dir` kept if it was a folder of the player's.
    pub settings: Settings,
    /// The old default data folder, to move into `<home>\data`.
    pub move_data: Option<PathBuf>,
    /// The game's old save folder (`%APPDATA%\shin`), to copy into `<home>\saves`.
    pub copy_saves: Option<PathBuf>,
}

/// `None` unless this is a portable home without `launcher.json` and `%APPDATA%` has something to bring in.
pub fn plan(home: &Home) -> Option<Plan> {
    if !home.portable || home.settings().exists() {
        return None;
    }
    let old = home.appdata.join("OpenCatbox");
    let old_settings: Option<Settings> = fs::read_to_string(old.join("launcher.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let old_data = match old_settings.as_ref().and_then(|s| s.data_dir.clone()) {
        Some(dir) => old.join(dir),
        None => old.join("data"),
    };
    let mut settings = old_settings.clone().unwrap_or_default();
    let mut move_data = None;
    if old_data == old.join("data") {
        settings.data_dir = None;
        if old_data.join("data.rom").is_file() && is_empty(&home.dir.join("data")) {
            move_data = Some(old_data);
        }
    } else {
        settings.data_dir = Some(old_data); // the player's own folder: keep using it where it is
    }
    let saves = home.appdata.join("shin");
    let copy_saves =
        (saves.join("SAVEDATA").is_file() && !home.savedata().exists()).then_some(saves);
    if old_settings.is_none() && move_data.is_none() && copy_saves.is_none() {
        return None;
    }
    settings.migrated_from = Some(old);
    Some(Plan {
        settings,
        move_data,
        copy_saves,
    })
}

/// Carries out `plan` and writes the home's `launcher.json`, so it runs once. Returns what to tell the player.
pub fn migrate(home: &Home, mut plan: Plan, progress: Progress) -> Result<Vec<String>, String> {
    let mut said = Vec::new();
    if let Some(from) = plan.move_data.take() {
        let to = home.dir.join("data");
        match move_dir(&from, &to, progress) {
            Ok(()) => said.push("Your game data was moved here.".into()),
            Err(e) => {
                said.push(format!("Your game data stays in {}. {e}", from.display()));
                plan.settings.data_dir = Some(from);
            }
        }
    } else if let Some(dir) = &plan.settings.data_dir {
        said.push(format!("Your game data stays in {}.", dir.display()));
    }
    if let Some(from) = &plan.copy_saves {
        let to = home.savedata();
        let to = to.parent().unwrap();
        fs::create_dir_all(to).map_err(|e| format!("Cannot create {}: {e}", to.display()))?;
        for name in ["SAVEDATA", "settings.json"] {
            if from.join(name).is_file() {
                fs::copy(from.join(name), to.join(name))
                    .map_err(|e| format!("Cannot copy your saves: {e}"))?;
            }
        }
        said.push(format!(
            "Your saves were copied here. The originals stay in {}.",
            from.display()
        ));
    }
    crate::write_settings(&home.settings(), &plan.settings)?;
    Ok(said)
}

/// Puts `file` in place as the save at `to`, keeping the current one as `SAVEDATA.bak`.
// shortcut: only the size is checked (a Switch SAVEDATA is ~15 KB); decoding it would need shin-core here
pub fn import_save(file: &Path, to: &Path) -> Result<(), String> {
    let len = fs::metadata(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?.len();
    if len == 0 || len > 1 << 20 {
        return Err("This is not a save file.".into());
    }
    let dir = to.parent().unwrap();
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    if to.is_file() {
        fs::copy(to, dir.join("SAVEDATA.bak")).map_err(|e| format!("Cannot back up your save: {e}"))?;
    }
    fs::copy(file, to).map_err(|e| format!("Cannot import the save: {e}"))?;
    Ok(())
}

pub fn is_empty(dir: &Path) -> bool {
    fs::read_dir(dir).map_or(true, |mut d| d.next().is_none())
}

pub fn tree_size(path: &Path) -> u64 {
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| tree_size(&e.path()))
            .sum(),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// Fails unless the drive of `dir` (or of its nearest existing parent) has `needed` bytes free.
pub fn ensure_space(dir: &Path, needed: u64) -> Result<(), String> {
    let existing = dir.ancestors().find(|d| d.exists()).unwrap_or(dir);
    match fs4::available_space(existing) {
        Ok(free) if free < needed => {
            Err(format!("Not enough free space (needs {}).", human(needed)))
        }
        _ => Ok(()),
    }
}

pub fn human(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// Moves a folder: a rename on the same drive, else copy, check the copy, then delete the original. On failure the
/// original is untouched and a partial copy removed. `to` must not exist or be empty.
pub fn move_dir(from: &Path, to: &Path, progress: Progress) -> Result<(), String> {
    let _ = fs::remove_dir(to); // an empty target blocks rename on Windows
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let total = tree_size(from);
    ensure_space(to, total)?;
    if let Err(e) = copy_checked(from, to, total, "Moving the game data", progress) {
        let _ = fs::remove_dir_all(to);
        return Err(e);
    }
    fs::remove_dir_all(from)
        .map_err(|e| format!("Copied, but the old folder could not be deleted: {e}"))
}

/// Copies `from` (a file or folder) to `to` and reads the copy back, comparing SHA-256s. `total` is the size of
/// `from`, for the progress (copy and check each take half).
pub fn copy_checked(
    from: &Path,
    to: &Path,
    total: u64,
    label: &str,
    progress: Progress,
) -> Result<(), String> {
    let mut done = 0;
    copy_tree(from, to, &mut |n| {
        done += n;
        progress(label, done as f64 / total.max(1) as f64 / 2.0);
    })
    .map_err(|e| format!("The copy failed: {e}"))
}

fn copy_tree(from: &Path, to: &Path, step: &mut dyn FnMut(u64)) -> std::io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()), step)?;
        }
        return Ok(());
    }
    let mut src = File::open(from)?;
    let mut dst = File::create(to)?;
    let mut written = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])?;
        written.update(&buf[..n]);
        step(n as u64);
    }
    dst.sync_all()?;
    drop(dst);
    let read_back = hash_file(to, &mut |n| step(n))?;
    if read_back != hex(&written.finalize()) {
        return Err(std::io::Error::other(format!(
            "{} differs from the original",
            to.display()
        )));
    }
    Ok(())
}

/// SHA-256 of a file as lowercase hex, reporting bytes read.
pub fn hash_file(path: &Path, step: &mut dyn FnMut(u64)) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        step(n as u64);
    }
    Ok(hex(&hash.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks a folder chosen on the setup screen: whether its `data.rom` is the known dump (by SHA-256).
pub fn check_game_folder(dir: &Path, progress: Progress) -> Result<bool, String> {
    let rom = dir.join("data.rom");
    let size = fs::metadata(&rom)
        .ok()
        .filter(|m| m.is_file())
        .ok_or("This folder has no data.rom.")?
        .len();
    let mut done = 0;
    let sha256 = hash_file(&rom, &mut |n| {
        done += n;
        progress("Checking data.rom", done as f64 / size.max(1) as f64);
    })
    .map_err(|e| format!("Cannot read data.rom: {e}"))?;
    Ok(sha256 == crate::setup::DATA_ROM_SHA256)
}

/// Copies `data.rom` and `languages\` of a game folder into `to`.
pub fn copy_game_folder(from: &Path, to: &Path, progress: Progress) -> Result<(), String> {
    let parts = [from.join("data.rom"), from.join("languages")];
    let total = parts.iter().map(|p| tree_size(p)).sum();
    // shortcut: files it would replace are not counted as free, so re-copying over a full data folder needs room twice
    ensure_space(to, total)?;
    fs::create_dir_all(to).map_err(|e| format!("Cannot create {}: {e}", to.display()))?;
    let mut done = 0;
    for part in parts.iter().filter(|p| p.exists()) {
        let size = tree_size(part);
        copy_checked(
            part,
            &to.join(part.file_name().unwrap()),
            size,
            "Copying the game data",
            &|label, f| progress(label, (done as f64 + f * size as f64) / total.max(1) as f64),
        )?;
        done += size;
    }
    Ok(())
}

/// Deletes what the launcher and the game put in the data folder and the save folder, then those folders if that
/// leaves them empty. Nothing else in them is touched: the data folder may be one of the player's.
pub fn delete_user_data(home: &Home, data: &Path) -> Result<(), String> {
    let saves = home.savedata();
    let saves = saves.parent().unwrap();
    for (dir, names) in [
        (data, &["data.rom", "languages", "logs", ".setup"][..]),
        (
            saves,
            &[
                "SAVEDATA",
                "SAVEDATA.tmp",
                "SAVEDATA.unreadable",
                "settings.json",
            ][..],
        ),
        (&home.dir, &["launcher.json"][..]),
    ] {
        for path in names.iter().map(|n| dir.join(n)).filter(|p| p.exists()) {
            let result = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            result.map_err(|e| format!("Cannot delete {}: {e}", path.display()))?;
        }
        if dir != home.dir {
            let _ = fs::remove_dir(dir); // only if empty
        }
    }
    if !home.portable {
        let _ = fs::remove_dir(&home.dir);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("open catbox home {name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// A portable home in `<root>\Open Catbox` with `<root>\AppData` as `%APPDATA%`.
    fn portable(root: &Path) -> Home {
        let exe_dir = root.join("Open Catbox");
        fs::create_dir_all(&exe_dir).unwrap();
        Home::choose(&exe_dir, true, root.join("AppData"))
    }

    #[test]
    fn the_home_is_the_exe_folder_only_when_installed_and_writable() {
        let root = temp("choice");
        let home = portable(&root);
        assert!(home.portable);
        assert_eq!(home.dir, root.join("Open Catbox"));
        assert_eq!(home.savedata(), root.join(r"Open Catbox\saves\SAVEDATA"));
        assert_eq!(home.webview(), Some(root.join(r"Open Catbox\webview")));
        assert_eq!(
            fs::read_dir(&home.dir).unwrap().count(),
            0,
            "the write test leaves nothing"
        );

        // a build from source, and a folder that cannot be written (here: a path under a file)
        let file = root.join("file");
        put(&file, "");
        for (dir, installed) in [(home.dir.clone(), false), (file.join("sub"), true)] {
            let fallback = Home::choose(&dir, installed, root.join("AppData"));
            assert!(!fallback.portable);
            assert_eq!(fallback.dir, root.join(r"AppData\OpenCatbox"));
            assert_eq!(fallback.savedata(), root.join(r"AppData\shin\SAVEDATA"));
            assert_eq!(fallback.webview(), None);
        }
        let mut s = Settings::default();
        assert_eq!(home.data(&s), home.dir.join("data"));
        s.data_dir = Some("elsewhere".into());
        assert_eq!(
            home.data(&s),
            home.dir.join("elsewhere"),
            "relative to the home"
        );
        s.data_dir = Some(root.join("abs"));
        assert_eq!(home.data(&s), root.join("abs"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_default_data_folder_moves_in_and_saves_are_copied_once() {
        let root = temp("default");
        let home = portable(&root);
        assert_eq!(plan(&home), None, "nothing in AppData");
        let old = root.join(r"AppData\OpenCatbox");
        put(
            &old.join("launcher.json"),
            r#"{"language":"enx","fullscreen":true}"#,
        );
        put(&old.join(r"data\data.rom"), "rom");
        put(&old.join(r"data\languages\enx\version.txt"), "v1");
        put(&root.join(r"AppData\shin\SAVEDATA"), "save");
        put(&root.join(r"AppData\shin\settings.json"), "{}");

        let p = plan(&home).unwrap();
        assert_eq!(p.move_data, Some(old.join("data")));
        assert_eq!(p.copy_saves, Some(root.join(r"AppData\shin")));
        assert_eq!(p.settings.language.as_deref(), Some("enx"));
        assert!(p.settings.fullscreen);
        assert_eq!(p.settings.data_dir, None);

        let said = migrate(&home, p, &|_, _| {}).unwrap();
        assert_eq!(said.len(), 2);
        assert_eq!(
            fs::read_to_string(home.dir.join(r"data\data.rom")).unwrap(),
            "rom"
        );
        assert_eq!(
            fs::read_to_string(home.dir.join(r"data\languages\enx\version.txt")).unwrap(),
            "v1"
        );
        assert!(!old.join("data").exists(), "moved, not copied");
        assert_eq!(fs::read_to_string(home.savedata()).unwrap(), "save");
        assert!(home.dir.join(r"saves\settings.json").is_file());
        assert!(
            root.join(r"AppData\shin\SAVEDATA").is_file(),
            "the original save is kept"
        );
        let written: Settings =
            serde_json::from_str(&fs::read_to_string(home.settings()).unwrap()).unwrap();
        assert_eq!(written.migrated_from, Some(old));
        assert_eq!(written.language.as_deref(), Some("enx"));
        assert_eq!(plan(&home), None, "runs once");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_custom_data_folder_is_kept_where_it_is() {
        let root = temp("custom");
        let home = portable(&root);
        let mine = root.join("My data");
        put(&mine.join("data.rom"), "rom");
        let json = serde_json::json!({ "data_dir": mine }).to_string();
        put(&root.join(r"AppData\OpenCatbox\launcher.json"), &json);
        let p = plan(&home).unwrap();
        assert_eq!(p.move_data, None);
        assert_eq!(p.copy_saves, None);
        assert_eq!(p.settings.data_dir.as_deref(), Some(mine.as_path()));
        migrate(&home, p, &|_, _| {}).unwrap();
        assert!(mine.join("data.rom").is_file());
        assert!(!home.dir.join("data").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn data_already_in_the_home_and_saves_already_there_are_kept() {
        let root = temp("kept");
        let home = portable(&root);
        put(&home.dir.join(r"data\data.rom"), "mine");
        put(&home.savedata(), "newer");
        put(&root.join(r"AppData\OpenCatbox\data\data.rom"), "old");
        put(&root.join(r"AppData\shin\SAVEDATA"), "old");
        assert_eq!(plan(&home), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_checked_copy_matches_and_a_move_leaves_nothing_behind() {
        let root = temp("copy");
        let from = root.join("from");
        put(&from.join("a.bin"), &"x".repeat(3 << 20));
        put(&from.join(r"sub\b.txt"), "b");
        let last = std::cell::Cell::new(0.0);
        copy_checked(
            &from,
            &root.join("to"),
            tree_size(&from),
            "Copying",
            &|_, f| last.set(f),
        )
        .unwrap();
        assert_eq!(last.get(), 1.0);
        assert_eq!(tree_size(&root.join("to")), tree_size(&from));
        move_dir(&from, &root.join(r"moved\here"), &|_, _| {}).unwrap();
        assert!(!from.exists());
        assert_eq!(
            fs::read_to_string(root.join(r"moved\here\sub\b.txt")).unwrap(),
            "b"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_existing_game_folder_is_verified_and_copied() {
        let root = temp("existing");
        let game = root.join("game");
        assert!(check_game_folder(&game, &|_, _| {}).is_err());
        put(&game.join("data.rom"), "not the real one");
        put(&game.join(r"languages\en\exefs\main"), "m");
        put(&game.join(r"languages\en\patch.rom"), "p");
        put(&game.join("other.nsp"), "not ours");
        assert!(
            !check_game_folder(&game, &|_, _| {}).unwrap(),
            "not the known dump"
        );
        put(&root.join("abc"), "abc");
        assert_eq!(
            hash_file(&root.join("abc"), &mut |_| {}).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let to = root.join("home data");
        copy_game_folder(&game, &to, &|_, _| {}).unwrap();
        assert!(to.join(r"languages\en\patch.rom").is_file());
        assert!(to.join("data.rom").is_file());
        assert!(!to.join("other.nsp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_user_data_leaves_other_files_alone() {
        let root = temp("delete");
        let home = portable(&root);
        let data = root.join("My data");
        put(&data.join("data.rom"), "rom");
        put(&data.join(r"languages\en\patch.rom"), "p");
        put(&data.join("my notes.txt"), "keep");
        put(&home.savedata(), "save");
        put(&home.settings(), "{}");
        delete_user_data(&home, &data).unwrap();
        assert_eq!(
            fs::read_dir(&data).unwrap().count(),
            1,
            "only the player's own file is left"
        );
        assert!(!home.dir.join("saves").exists());
        assert!(!home.settings().exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn importing_a_save_keeps_the_old_one() {
        let dir = std::env::temp_dir().join(format!("oc-import-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (new, to) = (dir.join("new"), dir.join("saves").join("SAVEDATA"));
        fs::write(&new, b"new").unwrap();
        import_save(&new, &to).unwrap();
        fs::write(&new, b"newer").unwrap();
        import_save(&new, &to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"newer");
        assert_eq!(fs::read(dir.join("saves").join("SAVEDATA.bak")).unwrap(), b"new");
        fs::write(&new, b"").unwrap();
        assert!(import_save(&new, &to).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

}
