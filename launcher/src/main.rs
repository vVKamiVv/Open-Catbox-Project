//! Open Catbox launcher: first-run setup from the player's own dump, translation downloads, and starting the game.

// no console window in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod home;
mod install;
mod langs;
mod nx;
mod setup;

use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// The workspace root, for finding shin.exe when running from a source checkout. Not in release builds, which
/// must not carry this PC's paths.
#[cfg(not(feature = "installer"))]
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// The data folder when it is not `<home>\data`; a relative path is relative to the home.
    pub data_dir: Option<PathBuf>,
    pub language: Option<String>,
    pub fullscreen: bool,
    /// Game log at debug level for shin's own crates instead of warnings only.
    pub debug_logging: bool,
    /// The `%APPDATA%\OpenCatbox` this portable home's settings, data and saves were brought in from (once).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migrated_from: Option<PathBuf>,
}

#[derive(Serialize)]
struct State {
    settings: Settings,
    data_dir: PathBuf,
    data_ready: bool,
    languages: std::collections::HashMap<String, Option<String>>,
    shin: Option<PathBuf>,
    program: Program,
    /// Where launcher.json, data\ and saves\ live (see `home.rs`).
    home: PathBuf,
    portable: bool,
    savedata: PathBuf,
    /// Settings, data and saves to bring in from `%APPDATA%` before anything else (first start of a portable home).
    migration: Option<home::Plan>,
}

/// The launcher's own installation (see `install.rs`).
#[derive(Serialize)]
struct Program {
    /// This is the installed copy, `Open Catbox.exe`.
    installed: bool,
    /// Not installed, but able to install itself (a release build with shin.exe inside).
    can_install: bool,
    /// Where it is installed, or would be.
    folder: PathBuf,
    /// Started with `--uninstall` (from Installed apps).
    uninstall: bool,
    /// Started by an update that could not finish, with the reason.
    update_error: Option<String>,
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    task: String,
    label: String,
    fraction: f64,
}

fn load_settings() -> Settings {
    fs::read_to_string(home::get().settings())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn store_settings(settings: &Settings) -> Result<(), String> {
    write_settings(&home::get().settings(), settings)
}

pub fn write_settings(path: &Path, settings: &Settings) -> Result<(), String> {
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(path, serde_json::to_string_pretty(settings).unwrap()).map_err(|e| e.to_string())
}

fn data_dir(settings: &Settings) -> PathBuf {
    home::get().data(settings)
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

fn find_shin() -> Option<PathBuf> {
    // a debug launcher sits in target/debug next to a debug shin.exe, which overflows its stack at startup
    let beside = (!cfg!(debug_assertions)).then(|| exe_dir().join("shin.exe"));
    #[cfg(not(feature = "installer"))]
    let beside = beside
        .into_iter()
        .chain([repo_root().join("target").join("release").join("shin.exe")]);
    beside.into_iter().find(|p| p.is_file())
}

fn start_mode() -> install::Mode {
    install::mode(&std::env::args_os().skip(1).collect::<Vec<OsString>>())
}

fn emit(app: &AppHandle, task: &str, label: &str, fraction: f64) {
    let _ = app.emit(
        "progress",
        ProgressEvent {
            task: task.into(),
            label: label.into(),
            fraction,
        },
    );
}

#[tauri::command]
fn get_state() -> State {
    let settings = load_settings();
    let data = data_dir(&settings);
    let home = home::get();
    let languages = ["en", "enx", "ja"]
        .iter()
        .map(|id| (id.to_string(), langs::installed(&data, id)))
        .collect();
    let installed = install::is_installed();
    let mode = start_mode();
    let program = Program {
        installed,
        can_install: install::SHIN.is_some(),
        folder: if installed {
            exe_dir()
        } else {
            install::default_folder()
        },
        uninstall: mode == install::Mode::Uninstall,
        update_error: match mode {
            install::Mode::UpdateFailed(e) => Some(e),
            _ => None,
        },
    };
    State {
        program,
        data_ready: data.join("data.rom").is_file(),
        shin: find_shin(),
        languages,
        data_dir: data,
        settings,
        home: home.dir.clone(),
        portable: home.portable,
        savedata: home.savedata(),
        migration: home::plan(home),
    }
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    store_settings(&settings)
}

/// Brings settings, data and saves in from `%APPDATA%` (see `home::plan`); returns what to tell the player.
#[tauri::command]
async fn migrate(app: AppHandle) -> Result<Vec<String>, String> {
    let home = home::get();
    let Some(plan) = home::plan(home) else {
        return Ok(Vec::new());
    };
    tauri::async_runtime::spawn_blocking(move || {
        home::migrate(home, plan, &|label, f| emit(&app, "migrate", label, f))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Checks a game folder chosen on the setup screen: true if its data.rom is the known dump.
#[tauri::command]
async fn check_game_folder(app: AppHandle, folder: PathBuf) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        home::check_game_folder(&folder, &|label, f| emit(&app, "setup", label, f))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Uses a checked game folder: copied into `<home>\data`, or as the data folder where it is.
#[tauri::command]
async fn use_game_folder(app: AppHandle, folder: PathBuf, copy: bool) -> Result<(), String> {
    let mut settings = load_settings();
    let home_data = home::get().dir.join("data");
    if copy && folder != home_data {
        let to = home_data.clone();
        let from = folder.clone();
        tauri::async_runtime::spawn_blocking(move || {
            home::copy_game_folder(&from, &to, &|label, f| emit(&app, "setup", label, f))
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    settings.data_dir = (!copy && folder != home_data).then_some(folder);
    store_settings(&settings)
}

/// A release's notes (markdown) as one string per bullet, hard-wrapped lines joined.
#[tauri::command]
fn release_notes(body: String) -> Vec<String> {
    history::notes(&body)
}

#[tauri::command]
async fn run_setup(
    app: AppHandle,
    base: PathBuf,
    update: Option<PathBuf>,
    prod_keys: PathBuf,
    title_keys: Option<PathBuf>,
) -> Result<Vec<String>, String> {
    let data = data_dir(&load_settings());
    fs::create_dir_all(&data)
        .map_err(|e| format!("Cannot create the data folder {}: {e}", data.display()))?;
    let input = setup::Input {
        base,
        update,
        prod_keys,
        title_keys,
    };
    tauri::async_runtime::spawn_blocking(move || {
        setup::run(&data, &input, &|label, f| emit(&app, "setup", label, f))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn install_language(app: AppHandle, id: String) -> Result<String, String> {
    let data = data_dir(&load_settings());
    tauri::async_runtime::spawn_blocking(move || {
        langs::install(&data, &id, &|f| emit(&app, &id, "Downloading", f))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Latest release of each translation; errors are reported per language.
#[tauri::command]
async fn check_updates() -> Vec<Result<langs::Release, String>> {
    tauri::async_runtime::spawn_blocking(|| {
        langs::PATCHES
            .iter()
            .map(|(id, _)| langs::latest(id))
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// This project's releases, from the embedded CHANGELOG.md and, if `online`, GitHub.
#[tauri::command]
async fn version_history(online: bool) -> history::History {
    tauri::async_runtime::spawn_blocking(move || history::history(online))
        .await
        .unwrap_or_else(|_| history::history(false))
}

#[tauri::command]
fn launch(lang: String) -> Result<u32, String> {
    let mut settings = load_settings();
    let data = data_dir(&settings);
    if !data.join("data.rom").is_file() {
        return Err("Game data missing. Run setup again.".into());
    }
    if langs::installed(&data, &lang).is_none() {
        return Err("This language is not installed yet.".into());
    }
    let shin = find_shin().ok_or("shin.exe was not found next to the launcher.")?;
    // shin.exe is a console app: it runs hidden, so its output goes to a log for diagnosing problems
    let logs = data.join("logs");
    fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let log = File::create(logs.join("shin.log")).map_err(|e| e.to_string())?;
    let rust_log = if settings.debug_logging {
        "warn,shin=debug,shin_core=debug,shin_video=debug,shin_audio=debug,shin_render=debug,shin_tasks=debug"
    } else {
        "warn"
    };
    let lang_dir = data.join("languages").join(&lang);
    let mut cmd = Command::new(&shin);
    cmd.arg("--assets-dir")
        .arg(&data)
        .arg("--patch-dir")
        .arg(&lang_dir)
        .arg("--exefs")
        .arg(lang_dir.join("exefs"))
        .arg("--savedata")
        .arg(home::get().savedata());
    if settings.fullscreen {
        cmd.arg("--fullscreen");
    }
    cmd.current_dir(shin.parent().unwrap())
        .env("RUST_LOG", rust_log)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    setup::no_window(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Cannot start the game: {e}"))?;
    settings.language = Some(lang);
    store_settings(&settings)?;
    Ok(child.id())
}

/// Moves the data folder (rename, or copy + check + delete across drives) and remembers the new place.
#[tauri::command]
async fn move_data_dir(app: AppHandle, target: PathBuf) -> Result<(), String> {
    let mut settings = load_settings();
    let old = data_dir(&settings);
    if target == old {
        return Ok(());
    }
    if !home::is_empty(&target) {
        return Err("Choose an empty folder.".into());
    }
    if old.is_dir() {
        let (from, to) = (old.clone(), target.clone());
        tauri::async_runtime::spawn_blocking(move || {
            home::move_dir(&from, &to, &|label, f| emit(&app, "move", label, f))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("The game data was not moved. {e}"))?;
    }
    settings.data_dir = (target != home::get().dir.join("data")).then_some(target);
    store_settings(&settings)
}

/// Installs the launcher (this program) and starts the installed copy, which takes over from here.
#[tauri::command]
async fn install_program(
    app: AppHandle,
    folder: PathBuf,
    menu: bool,
    desktop: bool,
) -> Result<(), String> {
    let choices = install::Choices {
        folder,
        menu,
        desktop,
    };
    let handle = app.clone();
    let exe = tauri::async_runtime::spawn_blocking(move || {
        install::install(&choices, &|label, f| emit(&handle, "install", label, f))
    })
    .await
    .map_err(|e| e.to_string())??;
    install::start(&exe, &[])?;
    app.exit(0);
    Ok(())
}

/// Removes the installed launcher (once it has exited) and, if asked, the game data, languages, saves and settings.
#[tauri::command]
async fn uninstall_program(app: AppHandle, delete_data: bool) -> Result<(), String> {
    if !install::is_installed() {
        return Err("Only the installed Open Catbox can uninstall itself.".into());
    }
    let home = home::get();
    if delete_data {
        home::delete_user_data(home, &data_dir(&load_settings()))?;
    }
    // the web view's data: in a portable home, and Tauri's default %LOCALAPPDATA%\<identifier>
    let webview: Vec<PathBuf> = home
        .webview()
        .into_iter()
        .chain(app.path().app_local_data_dir().ok())
        .collect();
    tauri::async_runtime::spawn_blocking(move || install::uninstall(&exe_dir(), &webview))
        .await
        .map_err(|e| e.to_string())?
}

/// Downloads the new version through the updater, which checks its signature, and starts it to replace this one:
/// the download is the new Setup, run with `--update <folder>`. It waits for this launcher to exit.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("You have the latest version.")?;
    let mut done = 0;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk;
                if let Some(total) = total {
                    emit(
                        &app,
                        "app-update",
                        "Downloading",
                        done as f64 / total.max(1) as f64,
                    );
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir().join(format!("OpenCatbox-update-{}", update.version));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let setup = dir.join("OpenCatbox-Setup.exe");
    fs::write(&setup, bytes).map_err(|e| format!("Cannot save the update: {e}"))?;
    // not installed (e.g. Setup run directly): the new Setup shows its install screen instead
    let args: Vec<OsString> = if install::is_installed() {
        vec!["--update".into(), exe_dir().into()]
    } else {
        vec![]
    };
    install::start(&setup, &args)?;
    app.exit(0);
    Ok(())
}

fn main() {
    if let install::Mode::Update(folder) = start_mode() {
        install::run_update(folder);
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // made here ("create": false in tauri.conf.json) so a portable home also holds the WebView2 data
            let config = app.config().app.windows[0].clone();
            let mut window = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?;
            if let Some(dir) = home::get().webview() {
                window = window.data_directory(dir);
            }
            window.build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_settings,
            migrate,
            check_game_folder,
            use_game_folder,
            release_notes,
            run_setup,
            install_language,
            check_updates,
            version_history,
            launch,
            move_data_dir,
            install_program,
            uninstall_program,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running the launcher");
}
