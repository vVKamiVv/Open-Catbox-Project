//! Installing the launcher for this Windows user. A release is one `OpenCatbox-Setup.exe`: the launcher itself with
//! `shin.exe` inside. It copies itself into a folder of the user's as `Open Catbox.exe`, writes `shin.exe` next to it,
//! adds shortcuts and an entry in Installed apps (under HKCU: no administrator rights), replaces itself on updates and
//! removes itself on uninstall. Nothing here opens a console window.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use windows_registry::CURRENT_USER;

pub const EXE: &str = "Open Catbox.exe";
const SHIN_EXE: &str = "shin.exe";
const SHORTCUT: &str = "Open Catbox.lnk";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Open Catbox";
/// v0.1.0 was an NSIS install (`%LOCALAPPDATA%\Open Catbox`, same uninstall key): its binary had another name, it had
/// its own uninstaller, and it kept its folder under this key.
const NSIS_FILES: [&str; 2] = ["open-catbox-launcher.exe", "uninstall.exe"];
const NSIS_KEY: &str = r"Software\github\Open Catbox";

/// `shin.exe`, built into release builds by `scripts/release.ps1`.
#[cfg(feature = "installer")]
pub const SHIN: Option<&[u8]> = Some(include_bytes!(env!("OPEN_CATBOX_SHIN")));
#[cfg(not(feature = "installer"))]
pub const SHIN: Option<&[u8]> = None;

/// What the command line asks for.
#[derive(Debug, PartialEq)]
pub enum Mode {
    Normal,
    /// `--uninstall`, from Installed apps: show the uninstall screen.
    Uninstall,
    /// `--update [folder]`: replace the installation there (default: the registered one), start it, exit.
    Update(Option<PathBuf>),
    /// `--update-failed <reason>`: started by an update that could not finish.
    UpdateFailed(String),
}

pub fn mode(args: &[OsString]) -> Mode {
    let arg = |i: usize| args.get(i).map(|a| a.to_string_lossy().into_owned());
    match arg(0).as_deref() {
        Some("--uninstall") => Mode::Uninstall,
        Some("--update") => Mode::Update(args.get(1).map(PathBuf::from)),
        Some("--update-failed") => Mode::UpdateFailed(arg(1).unwrap_or_default()),
        // v0.1.0's updater starts what it downloaded as an NSIS installer: `/P /UPDATE /R /ARGS`
        Some(a) if a.starts_with('/') => Mode::Update(None),
        _ => Mode::Normal,
    }
}

/// `(step label, fraction 0..1)`
pub type Progress<'a> = &'a dyn Fn(&str, f64);

pub struct Choices {
    pub folder: PathBuf,
    pub menu: bool,
    pub desktop: bool,
}

/// Whether this process is the installed copy.
pub fn is_installed() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.eq_ignore_ascii_case(EXE)))
        .unwrap_or(false)
}

/// The registered installation, else `%LOCALAPPDATA%\Programs\Open Catbox`.
pub fn default_folder() -> PathBuf {
    registered_folder().unwrap_or_else(|| local_app_data().join("Programs").join("Open Catbox"))
}

fn local_app_data() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
}

fn registered_folder() -> Option<PathBuf> {
    location(UNINSTALL_KEY)
}

fn location(key: &str) -> Option<PathBuf> {
    let text = CURRENT_USER
        .open(key)
        .ok()?
        .get_string("InstallLocation")
        .ok()?;
    let text = text.trim().trim_matches('"'); // NSIS wrote it in quotes
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// Copies the launcher and `shin.exe` into `choices.folder`, adds the chosen shortcuts and the Installed apps entry.
/// Returns the installed launcher.
pub fn install(choices: &Choices, progress: Progress) -> Result<PathBuf, String> {
    progress("Copying the launcher and the game engine", 0.1);
    let exe = put_files(&choices.folder, Duration::from_secs(3))?;
    progress("Creating shortcuts", 0.7);
    for (wanted, place) in [
        (choices.menu, Place::Menu),
        (choices.desktop, Place::Desktop),
    ] {
        if wanted {
            add_shortcut(&place, &exe, false)?;
        }
    }
    progress("Adding Open Catbox to Installed apps", 0.9);
    register(UNINSTALL_KEY, &choices.folder)?;
    progress("Installed", 1.0);
    Ok(exe)
}

/// Replaces the installation in `folder` with this program, points its existing shortcuts at it and updates its
/// entry. Also turns a v0.1.0 NSIS install into one of ours.
fn update(folder: &Path) -> Result<PathBuf, String> {
    // the old launcher exits right after starting this, so its file frees up within moments
    let exe = put_files(folder, Duration::from_secs(60))?;
    for name in NSIS_FILES {
        remove_retrying(&folder.join(name), Duration::from_secs(10));
    }
    for place in [Place::Menu, Place::Desktop] {
        add_shortcut(&place, &exe, true)?;
    }
    register(UNINSTALL_KEY, folder)?;
    let _ = CURRENT_USER.remove_tree(NSIS_KEY);
    Ok(exe)
}

/// `--update`: replaces the installation, starts it (told the reason if the update failed), and has this downloaded
/// copy deleted once it has exited.
pub fn run_update(folder: Option<PathBuf>) {
    let folder = folder
        .or_else(registered_folder)
        .unwrap_or_else(|| local_app_data().join("Open Catbox"));
    let result = update(&folder);
    let new = folder.join(EXE);
    let program = if new.is_file() {
        new
    } else {
        folder.join(NSIS_FILES[0])
    };
    let args: Vec<OsString> = match &result {
        Ok(_) => vec![],
        Err(e) => vec!["--update-failed".into(), e.into()],
    };
    let _ = start(&program, &args);
    if let Ok(me) = std::env::current_exe() {
        if me.starts_with(std::env::temp_dir()) && !me.starts_with(&folder) {
            let _ = after_exit(std::slice::from_ref(&me), me.parent());
        }
    }
}

/// Writes `shin.exe`, then this program as `Open Catbox.exe`, into `folder`.
fn put_files(folder: &Path, wait: Duration) -> Result<PathBuf, String> {
    let shin = SHIN.ok_or(
        "This launcher was built without shin.exe inside, so it cannot install itself (see launcher/scripts/release.ps1).",
    )?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let me = fs::read(&me).map_err(|e| format!("Cannot read {}: {e}", me.display()))?;
    fs::create_dir_all(folder).map_err(|e| format!("Cannot create {}: {e}", folder.display()))?;
    // shin.exe first: if the game is running and it cannot be replaced, nothing has changed yet
    place(&folder.join(SHIN_EXE), shin, wait)?;
    let exe = folder.join(EXE);
    place(&exe, &me, wait)?;
    Ok(exe)
}

/// Writes `bytes` beside `to` and swaps it in whole, retrying for `wait` while the old file is still in use.
fn place(to: &Path, bytes: &[u8], wait: Duration) -> Result<(), String> {
    let part = to.with_extension("part");
    fs::write(&part, bytes).map_err(|e| format!("Cannot write {}: {e}", part.display()))?;
    let start = Instant::now();
    loop {
        match fs::rename(&part, to) {
            Ok(()) => return Ok(()),
            Err(e) if start.elapsed() >= wait => {
                let _ = fs::remove_file(&part);
                return Err(format!(
                    "Cannot replace {}: {e}. If Open Catbox or the game is running, close it and try again.",
                    to.display()
                ));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

fn remove_retrying(path: &Path, wait: Duration) {
    let start = Instant::now();
    while path.exists() && fs::remove_file(path).is_err() && start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn register(key: &str, folder: &Path) -> Result<(), String> {
    let exe = folder.join(EXE);
    let size: u64 = [EXE, SHIN_EXE]
        .iter()
        .filter_map(|name| fs::metadata(folder.join(name)).ok())
        .map(|m| m.len())
        .sum();
    fn err(e: impl std::fmt::Display) -> String {
        format!("Cannot add Open Catbox to Installed apps: {e}")
    }
    let k = CURRENT_USER.create(key).map_err(err)?;
    for (name, value) in [
        ("DisplayName", "Open Catbox".to_string()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
        ("Publisher", "Open Catbox Project".to_string()),
        ("DisplayIcon", exe.display().to_string()),
        ("InstallLocation", folder.display().to_string()),
        (
            "UninstallString",
            format!("\"{}\" --uninstall", exe.display()),
        ),
        ("URLInfoAbout", env!("CARGO_PKG_REPOSITORY").to_string()),
    ] {
        k.set_string(name, value).map_err(err)?;
    }
    k.set_u32("EstimatedSize", (size / 1024) as u32)
        .map_err(err)?;
    k.set_u32("NoModify", 1).map_err(err)?;
    k.set_u32("NoRepair", 1).map_err(err)?;
    let _ = k.remove_value("MainBinaryName"); // v0.1.0's NSIS
    Ok(())
}

/// Deletes `data` (the game data and settings, only when the player asked), the shortcuts and the entry; the program
/// files in `folder` and the web view's cache in `webview` go once this process has exited, as a running program
/// cannot delete itself. Only our own files are deleted, and `folder` only if that leaves it empty.
pub fn uninstall(folder: &Path, data: &[PathBuf], webview: &Path) -> Result<(), String> {
    for dir in data.iter().filter(|d| d.exists()) {
        fs::remove_dir_all(dir).map_err(|e| format!("Cannot delete {}: {e}", dir.display()))?;
    }
    for place in [Place::Menu, Place::Desktop] {
        remove_shortcut(&place)?;
    }
    let _ = CURRENT_USER.remove_tree(UNINSTALL_KEY);
    let _ = CURRENT_USER.remove_tree(NSIS_KEY);
    let files: Vec<PathBuf> = [EXE, SHIN_EXE]
        .into_iter()
        .chain(NSIS_FILES)
        .map(|name| folder.join(name))
        .chain([webview.to_path_buf()])
        .collect();
    after_exit(&files, Some(folder))
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

/// A hidden PowerShell that waits for this process to exit, deletes `paths` (retrying while files are still in use,
/// e.g. by the web view) and then `empty_dir` if nothing else is in it.
fn after_exit(paths: &[PathBuf], empty_dir: Option<&Path>) -> Result<(), String> {
    let list = paths
        .iter()
        .map(|p| quoted(p))
        .collect::<Vec<_>>()
        .join(",");
    let mut script = format!(
        "Wait-Process -Id {} -EA 0;$f=@({list});foreach($i in 1..30){{Remove-Item -LiteralPath $f -Recurse -Force -EA 0;if(-not($f|?{{Test-Path -LiteralPath $_}})){{break}};Start-Sleep 1}}",
        std::process::id()
    );
    if let Some(dir) = empty_dir {
        script += &format!(";try{{[IO.Directory]::Delete({})}}catch{{}}", quoted(dir));
    }
    let mut c = powershell(&script);
    c.current_dir(std::env::temp_dir()); // a process's own folder cannot be deleted
    c.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// A Windows tool by its full path, never one that happens to sit in the folder Setup runs from.
fn system(name: &str) -> Command {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let mut c = Command::new(PathBuf::from(root).join("System32").join(name));
    crate::setup::no_window(&mut c);
    c
}

fn powershell(script: &str) -> Command {
    let mut c = system(r"WindowsPowerShell\v1.0\powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    c
}

fn run(mut c: Command) -> Result<(), String> {
    let out = c.output().map_err(|e| e.to_string())?;
    let said = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{} {}", said(&out.stderr), said(&out.stdout))
            .trim()
            .to_string())
    }
}

/// A PowerShell single-quoted string.
fn quoted(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

/// Where a shortcut goes, as Windows says: the desktop can live in OneDrive.
enum Place<'a> {
    Menu,
    Desktop,
    #[cfg_attr(not(test), allow(dead_code))]
    Folder(&'a Path),
}

impl Place<'_> {
    fn script(&self) -> String {
        match self {
            Place::Menu => {
                format!("(Join-Path ([Environment]::GetFolderPath('Programs')) '{SHORTCUT}')")
            }
            Place::Desktop => {
                format!("(Join-Path ([Environment]::GetFolderPath('Desktop')) '{SHORTCUT}')")
            }
            Place::Folder(p) => format!("(Join-Path {} '{SHORTCUT}')", quoted(p)),
        }
    }
}

/// Creates the shortcut, or with `only_existing` re-points one that is already there (v0.1.0's went to another name).
fn add_shortcut(place: &Place, program: &Path, only_existing: bool) -> Result<(), String> {
    let folder = quoted(program.parent().unwrap_or(Path::new("")));
    let guard = if only_existing {
        "if(Test-Path -LiteralPath $p)"
    } else {
        "if($true)"
    };
    run(powershell(&format!(
        "$p={};{guard}{{$s=(New-Object -ComObject WScript.Shell).CreateShortcut($p);$s.TargetPath={};$s.WorkingDirectory={folder};$s.Save()}}",
        place.script(),
        quoted(program)
    )))
    .map_err(|e| format!("Cannot create the shortcut: {e}"))
}

fn remove_shortcut(place: &Place) -> Result<(), String> {
    run(powershell(&format!(
        "$p={};if(Test-Path -LiteralPath $p){{Remove-Item -LiteralPath $p}}",
        place.script()
    )))
    .map_err(|e| format!("Cannot remove the shortcut: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "open catbox install's {name}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn modes_from_the_command_line() {
        assert_eq!(mode(&args(&[])), Mode::Normal);
        assert_eq!(mode(&args(&["--uninstall"])), Mode::Uninstall);
        assert_eq!(
            mode(&args(&["--update", r"D:\Games\Open Catbox"])),
            Mode::Update(Some(r"D:\Games\Open Catbox".into()))
        );
        assert_eq!(
            mode(&args(&["--update-failed", "busy"])),
            Mode::UpdateFailed("busy".into())
        );
        // what v0.1.0's updater passes to the NSIS installer it thinks it downloaded
        assert_eq!(
            mode(&args(&["/P", "/UPDATE", "/R", "/ARGS"])),
            Mode::Update(None)
        );
        assert_eq!(mode(&args(&["/UPDATE"])), Mode::Update(None));
    }

    #[test]
    fn a_file_is_swapped_in_whole_and_replaced_by_the_next_version() {
        let dir = temp("place");
        let to = dir.join(EXE);
        place(&to, b"first", Duration::ZERO).unwrap();
        place(&to, b"second", Duration::ZERO).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"second");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "no .part left over");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_in_use_is_reported_and_no_part_is_left() {
        let dir = temp("in use");
        let to = dir.join("busy");
        fs::create_dir(&to).unwrap(); // a folder of that name stands in for a locked file
        assert!(place(&to, b"x", Duration::from_millis(300))
            .unwrap_err()
            .contains("close it"));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_shortcut_survives_a_folder_with_a_quote_and_japanese_in_its_name() {
        let dir = temp("ショート'カット");
        let program = dir.join(EXE);
        fs::write(&program, "x").unwrap();
        add_shortcut(&Place::Folder(&dir), &program, true).unwrap();
        assert!(
            !dir.join(SHORTCUT).exists(),
            "only_existing creates nothing"
        );
        add_shortcut(&Place::Folder(&dir), &program, false).unwrap();
        assert!(dir.join(SHORTCUT).is_file());
        add_shortcut(&Place::Folder(&dir), &program, true).unwrap();
        remove_shortcut(&Place::Folder(&dir)).unwrap();
        assert!(!dir.join(SHORTCUT).exists());
        remove_shortcut(&Place::Folder(&dir)).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_entry_says_how_to_uninstall() {
        let dir = temp("エントリ");
        fs::write(dir.join(EXE), vec![0; 4096]).unwrap();
        fs::write(dir.join(SHIN_EXE), vec![0; 4096]).unwrap();
        let key = format!(r"Software\Open Catbox test {}", std::process::id());
        CURRENT_USER
            .create(&key)
            .unwrap()
            .set_string("MainBinaryName", "old.exe")
            .unwrap();
        register(&key, &dir).unwrap();
        let k = CURRENT_USER.open(&key).unwrap();
        let uninstall = k.get_string("UninstallString").unwrap();
        let size = k.get_u32("EstimatedSize").unwrap();
        let old = k.get_string("MainBinaryName");
        let found = location(&key);
        CURRENT_USER.remove_tree(&key).unwrap();
        assert_eq!(found.as_deref(), Some(dir.as_path()));
        assert_eq!(location(&key), None, "gone with the entry");
        assert_eq!(
            uninstall,
            format!("\"{}\" --uninstall", dir.join(EXE).display())
        );
        assert_eq!(size, 8);
        assert!(old.is_err(), "v0.1.0's value removed");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nsis_quotes_around_the_location_are_dropped() {
        let key = format!(r"Software\Open Catbox test quotes {}", std::process::id());
        CURRENT_USER
            .create(&key)
            .unwrap()
            .set_string(
                "InstallLocation",
                r#""C:\Users\x\AppData\Local\Open Catbox""#,
            )
            .unwrap();
        let found = location(&key);
        CURRENT_USER.remove_tree(&key).unwrap();
        assert_eq!(found, Some(r"C:\Users\x\AppData\Local\Open Catbox".into()));
    }
}
