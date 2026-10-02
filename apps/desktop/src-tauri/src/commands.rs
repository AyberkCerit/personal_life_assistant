//! Tauri commands: thin wrappers; all rules live in pla-core and the worker.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::Duration;

use pla_core::db::open_databases;
use pla_core::files::{self, FileError, NoteFile, TreeEntry};
use pla_core::llm::{ModelHost, ServerConfig};
use pla_core::pipeline::Extractor;
use pla_core::settings::{load_settings, save_settings, AppSettings, Theme};
use pla_core::vault::{default_app_root, open_vault as open_vault_dir, resolve_data_dir, Vault};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::worker::{Command, Worker};

pub struct Session {
    pub vault: Vault,
    pub worker: Sender<Command>,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.worker.send(Command::Shutdown);
    }
}

#[derive(Default)]
pub struct AppState {
    pub session: Mutex<Option<Session>>,
}

#[derive(Serialize)]
pub struct StartupInfo {
    vault_path: Option<String>,
    first_run: bool,
    settings_recovered: bool,
    theme: String,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SaveResult {
    Saved { hash: String },
    Conflict { current_hash: String },
}

fn app_root() -> Result<PathBuf, String> {
    default_app_root().ok_or_else(|| "APPDATA is not set; PLA cannot store its data.".to_owned())
}

/// Model paths from settings, else from the developer environment (decision 6).
fn extractor(settings: &AppSettings) -> Option<Box<dyn Extractor + Send>> {
    let bin = settings.llama_server.clone().or_else(|| std::env::var_os("PLA_LLAMA_SERVER").map(PathBuf::from))?;
    let model = settings.model_path.clone().or_else(|| std::env::var_os("PLA_MODEL").map(PathBuf::from))?;
    (bin.is_file() && model.is_file())
        .then(|| Box::new(ModelHost::new(ServerConfig::new(bin, model), Duration::from_secs(60))) as Box<dyn Extractor + Send>)
}

fn start_session(app: &AppHandle, root: &Path, settings: &AppSettings) -> Result<Session, String> {
    let mut vault = open_vault_dir(root).map_err(|e| e.to_string())?;
    let dir = resolve_data_dir(&app_root()?, &mut vault).map_err(|e| e.to_string())?;
    let dbs = open_databases(&dir).map_err(|e| e.to_string())?;
    let handle = app.clone();
    let worker = Worker::new(
        vault.clone(),
        dbs.pla,
        extractor(settings),
        Box::new(move |status| {
            let _ = handle.emit("worker-status", status);
        }),
    );
    let (tx, _join) = worker.spawn();
    Ok(Session { vault, worker: tx })
}

fn with_vault<T>(state: &State<AppState>, f: impl FnOnce(&Vault) -> Result<T, FileError>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("No vault is open.")?;
    f(&session.vault).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn startup(app: AppHandle, state: State<AppState>) -> Result<StartupInfo, String> {
    let root = app_root()?;
    let loaded = load_settings(&root).map_err(|e| e.to_string())?;
    let theme = match loaded.settings.theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    let mut info = StartupInfo {
        vault_path: None,
        first_run: loaded.first_run,
        settings_recovered: loaded.recovered_from_broken,
        theme: theme.to_owned(),
        error: None,
    };
    if let Some(path) = loaded.settings.vault_path.clone() {
        match start_session(&app, &path, &loaded.settings) {
            Ok(session) => {
                info.vault_path = Some(session.vault.root.to_string_lossy().into_owned());
                *state.session.lock().expect("session lock") = Some(session);
            }
            Err(e) => info.error = Some(format!("{}: {e}", path.display())),
        }
    }
    Ok(info)
}

#[tauri::command]
pub fn open_vault(app: AppHandle, state: State<AppState>, path: String) -> Result<String, String> {
    let root = app_root()?;
    let mut settings = load_settings(&root).map_err(|e| e.to_string())?.settings;
    let session = start_session(&app, Path::new(&path), &settings)?;
    let opened = session.vault.root.to_string_lossy().into_owned();
    settings.vault_path = Some(session.vault.root.clone());
    save_settings(&root, &settings).map_err(|e| e.to_string())?;
    *state.session.lock().expect("session lock") = Some(session); // drops (and stops) the previous one
    Ok(opened)
}

#[tauri::command]
pub fn list_tree(state: State<AppState>) -> Result<Vec<TreeEntry>, String> {
    with_vault(&state, |v| files::list_tree(v).map_err(FileError::from))
}

#[tauri::command]
pub fn read_note(state: State<AppState>, path: String) -> Result<NoteFile, String> {
    with_vault(&state, |v| files::read_note(v, &path))
}

#[tauri::command]
pub fn save_note(state: State<AppState>, path: String, text: String, expected_hash: Option<String>) -> Result<SaveResult, String> {
    with_vault(&state, |v| match files::save_note(v, &path, &text, expected_hash.as_deref()) {
        Ok(hash) => Ok(SaveResult::Saved { hash }),
        Err(FileError::Conflict { current_hash }) => Ok(SaveResult::Conflict { current_hash }),
        Err(e) => Err(e),
    })
}

#[tauri::command]
pub fn save_copy(state: State<AppState>, path: String, text: String) -> Result<String, String> {
    with_vault(&state, |v| files::save_copy(v, &path, &text))
}

#[tauri::command]
pub fn create_note(state: State<AppState>, folder: String, title: String) -> Result<String, String> {
    with_vault(&state, |v| files::create_note(v, &folder, &title))
}

#[tauri::command]
pub fn queue_note(state: State<AppState>, path: String) -> Result<(), String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("No vault is open.")?;
    files::resolve(&session.vault, &path).map_err(|e| e.to_string())?;
    session.worker.send(Command::Enqueue(path)).map_err(|e| e.to_string())
}
