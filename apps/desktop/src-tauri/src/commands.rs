//! Tauri commands: thin wrappers; all rules live in pla-core and the worker.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pla_core::db::open_databases;
use pla_core::files::{self, FileError, NoteFile, TreeEntry};
use pla_core::llm::{ModelHost, ServerConfig};
use pla_core::pipeline::Extractor;
use pla_core::settings::{load_settings, save_settings, AppSettings, Theme};
use pla_core::vault::{default_app_root, open_vault as open_vault_dir, resolve_data_dir, Vault};
use chrono::Local;
use pla_core::db::connect;
use pla_core::pipeline::items::{reject_item, ItemRef};
use pla_core::reminders;
use pla_core::pipeline::write_tx;
use pla_core::tasks::{self, ReviewView, TaskInput, TaskList, TaskView};
use rusqlite::Connection;

use crate::worker::{AddedItem, WorkerStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::notify::{show_info, strings, AppNotifier};
use crate::scheduler::{Pending, PendingLists, SchedCommand, Scheduler, SchedulerHandle};
use crate::system::WindowsProbe;
use crate::watcher::{watch, SelfWrites, WatchHandle};
use crate::worker::{Command, Worker, WorkerHandle};

/// One open vault. Fields drop in order: the scheduler stops first, then the watcher, then the
/// worker (which ends the model).
pub struct Session {
    pub vault: Vault,
    pub scheduler: SchedulerHandle,
    pub pending: Arc<Pending>,
    pub db: Mutex<Connection>,
    pub status: Arc<Mutex<WorkerStatus>>,
    pub _watch: Option<WatchHandle>,
    pub worker: WorkerHandle,
}

#[derive(Default)]
pub struct AppState {
    pub session: Mutex<Option<Session>>,
    pub self_writes: Arc<SelfWrites>,
    /// Background AI paused (FR-SCH-014); kept across vault switches.
    pub paused: AtomicBool,
}

#[derive(Serialize)]
pub struct VaultInfo {
    path: String,
    inbox: String,
}

fn vault_info_of(vault: &Vault) -> VaultInfo {
    VaultInfo { path: vault.root.to_string_lossy().into_owned(), inbox: vault.config.folders.inbox.clone() }
}

#[derive(Serialize)]
pub struct StartupInfo {
    vault_path: Option<String>,
    first_run: bool,
    settings_recovered: bool,
    theme: String,
    error: Option<String>,
    inbox: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SaveResult {
    Saved { hash: String },
    Conflict { current_hash: String },
    Missing,
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

fn start_session(app: &AppHandle, root: &Path, settings: &AppSettings, own: Arc<SelfWrites>, paused: bool) -> Result<Session, String> {
    let mut vault = open_vault_dir(root).map_err(|e| e.to_string())?;
    let dir = resolve_data_dir(&app_root()?, &mut vault).map_err(|e| e.to_string())?;
    let dbs = open_databases(&dir).map_err(|e| e.to_string())?;
    let ui_db = connect(&dir.join("pla.db")).map_err(|e| e.to_string())?;
    let status = Arc::new(Mutex::new(WorkerStatus { queued: 0, model: crate::worker::ModelState::Off, busy: false, last_error: None, added: 0, paused: false }));
    let (status_app, status_copy) = (app.clone(), Arc::clone(&status));
    let added_app = app.clone();
    let worker = Worker::new(
        vault.clone(),
        dbs.pla,
        extractor(settings),
        Box::new(move |s| {
            *status_copy.lock().expect("status lock") = s.clone();
            let _ = status_app.emit("worker-status", s);
        }),
    )
    .with_added_listener(Box::new(move |items: &[AddedItem]| {
        let _ = added_app.emit("items-added", items);
    }));
    let handle = worker.spawn();
    if paused {
        handle.set_paused(true);
    }
    let watch_handle = watch(vault.clone(), own, handle.sender(), app.clone()).ok();
    let sched_db = connect(&dir.join("pla.db")).map_err(|e| e.to_string())?;
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<SchedCommand>();
    let notifier = AppNotifier::new(app.clone(), cmd_tx.clone());
    let scheduler = Scheduler::new(vault.root.clone(), sched_db, Box::new(notifier), Box::new(WindowsProbe), Box::new(|| Local::now().fixed_offset()));
    let pending = scheduler.pending();
    let scheduler = scheduler.spawn(std::time::Duration::from_secs(30));
    // Toast buttons send to `cmd_tx`; forward them to the scheduler thread.
    let forward = scheduler.sender();
    std::thread::spawn(move || {
        for command in cmd_rx {
            if forward.send(command).is_err() {
                break;
            }
        }
    });
    Ok(Session { vault, scheduler, pending, db: Mutex::new(ui_db), status, _watch: watch_handle, worker: handle })
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
        inbox: None,
    };
    if let Some(running) = state.session.lock().expect("session lock").as_ref() {
        info.vault_path = Some(running.vault.root.to_string_lossy().into_owned());
        info.inbox = Some(running.vault.config.folders.inbox.clone());
        return Ok(info);
    }
    if let Some(path) = loaded.settings.vault_path.clone() {
        match start_session(&app, &path, &loaded.settings, Arc::clone(&state.self_writes), state.paused.load(Ordering::SeqCst)) {
            Ok(session) => {
                info.vault_path = Some(session.vault.root.to_string_lossy().into_owned());
                info.inbox = Some(session.vault.config.folders.inbox.clone());
                *state.session.lock().expect("session lock") = Some(session);
            }
            Err(e) => info.error = Some(format!("{}: {e}", path.display())),
        }
    }
    Ok(info)
}

#[tauri::command]
pub fn open_vault(app: AppHandle, state: State<AppState>, path: String) -> Result<VaultInfo, String> {
    let root = app_root()?;
    let mut settings = load_settings(&root).map_err(|e| e.to_string())?.settings;
    // Stop the previous vault before opening the next, outside the lock: one writer per pla.db.
    let previous = state.session.lock().expect("session lock").take();
    if let Some(old) = previous {
        if same_folder(&old.vault.root, Path::new(&path)) {
            let opened = vault_info_of(&old.vault);
            *state.session.lock().expect("session lock") = Some(old);
            return Ok(opened);
        }
        drop(old);
    }
    let session = start_session(&app, Path::new(&path), &settings, Arc::clone(&state.self_writes), state.paused.load(Ordering::SeqCst))?;
    let opened = vault_info_of(&session.vault);
    settings.vault_path = Some(session.vault.root.clone());
    save_settings(&root, &settings).map_err(|e| e.to_string())?;
    *state.session.lock().expect("session lock") = Some(session); // drops (and stops) the previous one
    Ok(opened)
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
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
    let own = Arc::clone(&state.self_writes);
    with_vault(&state, |v| match files::save_note(v, &path, &text, expected_hash.as_deref()) {
        Ok(hash) => {
            own.record(&path, &hash);
            Ok(SaveResult::Saved { hash })
        }
        Err(FileError::Conflict { current_hash }) => Ok(SaveResult::Conflict { current_hash }),
        Err(FileError::Missing) => Ok(SaveResult::Missing),
        Err(e) => Err(e),
    })
}

#[tauri::command]
pub fn save_copy(state: State<AppState>, path: String, text: String) -> Result<String, String> {
    let own = Arc::clone(&state.self_writes);
    with_vault(&state, |v| {
        let copy = files::save_copy(v, &path, &text)?;
        own.record_text(&copy, &text);
        Ok(copy)
    })
}

#[tauri::command]
pub fn create_note(state: State<AppState>, folder: String, title: String) -> Result<String, String> {
    let own = Arc::clone(&state.self_writes);
    with_vault(&state, |v| {
        let rel = files::create_note(v, &folder, &title)?;
        own.record_text(&rel, "");
        Ok(rel)
    })
}

#[tauri::command]
pub fn queue_note(state: State<AppState>, path: String) -> Result<(), String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("No vault is open.")?;
    files::resolve(&session.vault, &path).map_err(|e| e.to_string())?;
    session.worker.send(Command::Enqueue(path));
    Ok(())
}

fn with_db<T>(state: &State<AppState>, f: impl FnOnce(&mut Connection) -> Result<T, tasks::TaskError>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("No vault is open.")?;
    let mut db = session.db.lock().expect("db lock");
    f(&mut db).map_err(|e| e.to_string())
}

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    Local::now().fixed_offset()
}

#[tauri::command]
pub fn vault_info(state: State<AppState>) -> Result<VaultInfo, String> {
    let guard = state.session.lock().expect("session lock");
    guard.as_ref().map(|s| vault_info_of(&s.vault)).ok_or_else(|| "No vault is open.".to_owned())
}

#[tauri::command]
pub fn worker_status(state: State<AppState>) -> Result<WorkerStatus, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("No vault is open.")?;
    let status = session.status.lock().expect("status lock").clone();
    Ok(status)
}

#[tauri::command]
pub fn list_tasks(state: State<AppState>, list: TaskList) -> Result<Vec<TaskView>, String> {
    with_db(&state, |db| tasks::list_tasks(db, list, Local::now().date_naive()))
}

#[tauri::command]
pub fn add_task(state: State<AppState>, input: TaskInput) -> Result<String, String> {
    with_db(&state, |db| tasks::add_task(db, &input, now()))
}

#[tauri::command]
pub fn edit_task(state: State<AppState>, id: String, input: TaskInput) -> Result<(), String> {
    with_db(&state, |db| tasks::edit_task(db, &id, &input, now()))
}

#[tauri::command]
pub fn set_task_done(state: State<AppState>, id: String, done: bool) -> Result<(), String> {
    with_db(&state, |db| tasks::set_done(db, &id, done, now()))
}

#[tauri::command]
pub fn delete_task(state: State<AppState>, id: String) -> Result<(), String> {
    with_db(&state, |db| {
        let tx = write_tx(db)?;
        tasks::delete_task(&tx, &id, now())?;
        tx.commit()?;
        Ok(())
    })
}

/// Undo of an automatic addition (FR-EXT-015): removed and remembered as rejected (FR-EXT-018).
#[tauri::command]
pub fn undo_item(state: State<AppState>, kind: String, id: String) -> Result<(), String> {
    with_db(&state, |db| {
        let item = if kind == "metric" { ItemRef::Metric(id.clone()) } else { ItemRef::Task(id.clone()) };
        let tx = write_tx(db)?;
        reject_item(&tx, &item, now())?;
        tx.commit()?;
        Ok(())
    })
}

#[tauri::command]
pub fn list_review(state: State<AppState>) -> Result<Vec<ReviewView>, String> {
    with_db(&state, |db| tasks::list_review(db))
}

#[tauri::command]
pub fn accept_review(state: State<AppState>, id: String, input: TaskInput) -> Result<String, String> {
    with_db(&state, |db| {
        let tx = write_tx(db)?;
        let task_id = tasks::accept_review(&tx, &id, &input, now())?;
        tx.commit()?;
        Ok(task_id)
    })
}

#[tauri::command]
pub fn reject_review(state: State<AppState>, id: String) -> Result<(), String> {
    with_db(&state, |db| tasks::reject_review(db, &id))
}

#[tauri::command]
pub fn reminder_done(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    with_db(&state, |db| reminders::complete(db, &id, now()))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[tauri::command]
pub fn reminder_snooze(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    with_db(&state, |db| reminders::snooze(db, &id, now()))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[tauri::command]
pub fn backup_now(state: State<AppState>) -> Result<(), String> {
    let guard = state.session.lock().expect("session lock");
    guard.as_ref().ok_or("No vault is open.")?.scheduler.send(SchedCommand::BackupNow);
    Ok(())
}

/// Reminders shown but not answered yet, for the banner (FR-TSK-015/016).
#[tauri::command]
pub fn pending_reminders(state: State<AppState>) -> Result<PendingLists, String> {
    let guard = state.session.lock().expect("session lock");
    let Some(session) = guard.as_ref() else { return Ok(PendingLists::default()) };
    let db = session.db.lock().expect("db lock");
    Ok(session.pending.current(&db))
}

#[tauri::command]
pub fn dismiss_missed(state: State<AppState>) -> Result<(), String> {
    if let Some(session) = state.session.lock().expect("session lock").as_ref() {
        let db = session.db.lock().expect("db lock");
        session.pending.dismiss_missed(&db, now());
    }
    Ok(())
}

/// FR-SCH-014: the one path for pausing background AI, from the tray or the status bar, so the
/// tray check, the worker and the UI agree (final review I4).
pub fn apply_pause(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    state.paused.store(paused, Ordering::SeqCst);
    if let Some(session) = state.session.lock().expect("session lock").as_ref() {
        session.worker.set_paused(paused);
    }
    crate::tray::set_pause_checked(app, paused);
    let _ = app.emit("paused-changed", paused);
}

#[tauri::command]
pub fn set_paused(app: AppHandle, paused: bool) -> Result<(), String> {
    apply_pause(&app, paused);
    Ok(())
}

/// FR-SCH-001: closing the window keeps PLA in the tray; the first time, say so.
#[tauri::command]
pub fn hide_to_tray(window: tauri::WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())?;
    let root = app_root()?;
    let mut settings = load_settings(&root).map_err(|e| e.to_string())?.settings;
    if !settings.tray_hint_shown {
        let s = strings();
        show_info(s.tray_title, s.tray_body);
        settings.tray_hint_shown = true;
        save_settings(&root, &settings).map_err(|e| e.to_string())?;
    }
    Ok(())
}
