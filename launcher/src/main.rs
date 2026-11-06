//! Open Catbox launcher: first-run setup from the player's own dump, translation downloads, and starting the game.

// no console window in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod langs;
mod nx;
mod setup;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// The workspace root, for finding shin.exe when running from a source checkout.
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Settings {
    data_dir: Option<PathBuf>,
    language: Option<String>,
    fullscreen: bool,
    /// Game log at debug level for shin's own crates instead of warnings only.
    debug_logging: bool,
}

#[derive(Serialize)]
struct State {
    settings: Settings,
    data_dir: PathBuf,
    data_ready: bool,
    languages: std::collections::HashMap<String, Option<String>>,
    shin: Option<PathBuf>,
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    task: String,
    label: String,
    fraction: f64,
}

fn base_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("OpenCatbox")
}

fn settings_path(app: &AppHandle) -> PathBuf {
    base_dir(app).join("launcher.json")
}

fn load_settings(app: &AppHandle) -> Settings {
    fs::read_to_string(settings_path(app))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn store_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app);
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(path, serde_json::to_string_pretty(settings).unwrap()).map_err(|e| e.to_string())
}

fn data_dir(app: &AppHandle, settings: &Settings) -> PathBuf {
    settings
        .data_dir
        .clone()
        .unwrap_or_else(|| base_dir(app).join("data"))
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

fn find_shin() -> Option<PathBuf> {
    // a debug launcher sits in target/debug next to a debug shin.exe, which overflows its stack at startup
    let beside = (!cfg!(debug_assertions)).then(|| exe_dir().join("shin.exe"));
    beside
        .into_iter()
        .chain([repo_root().join("target").join("release").join("shin.exe")])
        .find(|p| p.is_file())
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
fn get_state(app: AppHandle) -> State {
    let settings = load_settings(&app);
    let data = data_dir(&app, &settings);
    let languages = ["en", "enx", "ja"]
        .iter()
        .map(|id| (id.to_string(), langs::installed(&data, id)))
        .collect();
    State {
        data_ready: data.join("data.rom").is_file(),
        shin: find_shin(),
        languages,
        data_dir: data,
        settings,
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    store_settings(&app, &settings)
}

#[tauri::command]
async fn run_setup(
    app: AppHandle,
    base: PathBuf,
    update: Option<PathBuf>,
    prod_keys: PathBuf,
    title_keys: Option<PathBuf>,
) -> Result<Vec<String>, String> {
    let data = data_dir(&app, &load_settings(&app));
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
    let data = data_dir(&app, &load_settings(&app));
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
fn launch(app: AppHandle, lang: String) -> Result<u32, String> {
    let mut settings = load_settings(&app);
    let data = data_dir(&app, &settings);
    if !data.join("data.rom").is_file() {
        return Err("The game data is missing. Please run the setup again.".into());
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
        .arg(lang_dir.join("exefs"));
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
    store_settings(&app, &settings)?;
    Ok(child.id())
}

/// Moves the data folder (rename, or copy + delete across drives) and remembers the new place.
#[tauri::command]
async fn move_data_dir(app: AppHandle, target: PathBuf) -> Result<(), String> {
    let mut settings = load_settings(&app);
    let old = data_dir(&app, &settings);
    if target == old {
        return Ok(());
    }
    if fs::read_dir(&target).is_ok_and(|mut d| d.next().is_some()) {
        return Err("Please choose an empty folder.".into());
    }
    if old.is_dir() {
        let (from, to) = (old.clone(), target.clone());
        tauri::async_runtime::spawn_blocking(move || move_dir(&from, &to))
            .await
            .map_err(|e| e.to_string())??;
    }
    settings.data_dir = Some(target);
    store_settings(&app, &settings)
}

fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    let _ = fs::remove_dir(to); // an empty target blocks rename on Windows
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_dir(from, to).map_err(|e| format!("Cannot copy the data folder: {e}"))?;
    fs::remove_dir_all(from)
        .map_err(|e| format!("Copied, but could not delete the old folder: {e}"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_settings,
            run_setup,
            install_language,
            check_updates,
            version_history,
            launch,
            move_data_dir
        ])
        .run(tauri::generate_context!())
        .expect("error while running the launcher");
}
