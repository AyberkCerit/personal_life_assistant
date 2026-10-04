//! Tauri commands: thin wrappers; all rules live in pla-core and the worker.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pla_core::db::open_databases;
use pla_core::files::{self, FileError, NoteFile, TreeEntry};
use pla_core::llm::ModelHost;
use pla_core::pipeline::Extractor;
use pla_core::settings::{load_settings, save_settings, AppSettings, Theme};
use pla_core::vault::{default_app_root, open_vault as open_vault_dir, resolve_data_dir, Vault};
use chrono::Local;
use pla_core::db::connect;
use pla_core::pipeline::items::{reject_item, ItemRef};
use pla_core::reminders;
use pla_core::models::{self, catalog, download as dl, local};
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
    /// The model download outlives vault switches (model-manager spec § 5).
    pub downloads: crate::model_download::ModelDownloads,
    /// Held for every load-modify-save of settings.json, and while a vault opens, so a finished
    /// download, a vault switch and the tray hint never overwrite each other (final review I5).
    pub settings_lock: Mutex<()>,
}

/// Load, change and save settings.json under `lock`.
fn update_settings(root: &Path, lock: &Mutex<()>, change: impl FnOnce(&mut AppSettings)) -> Result<AppSettings, String> {
    let _guard = lock.lock().expect("settings lock");
    let mut settings = load_settings(root).map_err(|e| e.to_string())?.settings;
    change(&mut settings);
    save_settings(root, &settings).map_err(|e| e.to_string())?;
    Ok(settings)
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
    /// FR-SET-001: show the first-run wizard instead of the app.
    show_wizard: bool,
    /// The language chosen in the wizard; `None` = follow the OS (FR-SET-003).
    language: Option<String>,
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

/// The llama-server shipped next to PLA (tauri.conf.json resources), if present.
pub fn bundled_server(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let from_resources = app.path().resource_dir().ok().map(|d| d.join("llama").join("llama-server.exe"));
    let next_to_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("llama").join("llama-server.exe")));
    crate::model_paths::first_existing([from_resources, next_to_exe])
}

/// The model host for these settings, or none while no model is installed (FR-SET-010).
fn extractor(app: &AppHandle, settings: &AppSettings) -> Option<Box<dyn Extractor + Send>> {
    let cfg = crate::model_paths::server_config(settings, &crate::model_paths::process_env, bundled_server(app))?;
    Some(Box::new(ModelHost::new(cfg, Duration::from_secs(60))))
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
        extractor(app, settings),
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
    let _settings_guard = state.settings_lock.lock().expect("settings lock");
    let loaded = load_settings(&root).map_err(|e| e.to_string())?;
    let theme = match loaded.settings.theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    let mut info = StartupInfo {
        vault_path: None,
        first_run: loaded.first_run,
        show_wizard: crate::wizard::show_wizard(loaded.first_run, &loaded.settings),
        language: loaded.settings.language.clone(),
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
    let _settings_guard = state.settings_lock.lock().expect("settings lock");
    open_vault_locked(&app, &state, Path::new(&path))
}

/// Opens `path` as the vault and remembers it; the caller holds the settings lock.
fn open_vault_locked(app: &AppHandle, state: &State<AppState>, path: &Path) -> Result<VaultInfo, String> {
    let root = app_root()?;
    let mut settings = load_settings(&root).map_err(|e| e.to_string())?.settings;
    // Stop the previous vault before opening the next, outside the lock: one writer per pla.db.
    let previous = state.session.lock().expect("session lock").take();
    if let Some(old) = previous {
        if same_folder(&old.vault.root, path) {
            let opened = vault_info_of(&old.vault);
            *state.session.lock().expect("session lock") = Some(old);
            return Ok(opened);
        }
        drop(old);
    }
    let session = start_session(app, path, &settings, Arc::clone(&state.self_writes), state.paused.load(Ordering::SeqCst))?;
    let opened = vault_info_of(&session.vault);
    settings.vault_path = Some(session.vault.root.clone());
    save_settings(&root, &settings).map_err(|e| e.to_string())?;
    *state.session.lock().expect("session lock") = Some(session); // drops (and stops) the previous one
    Ok(opened)
}

/// FR-SET-006: the note a new or empty vault starts with, saved as PLA's own write. An existing note
/// of that name is left alone.
fn write_welcome_note(vault: &Vault, own: &SelfWrites, lang: &str) -> Result<String, String> {
    let (title, body) = crate::wizard::welcome_note(lang);
    let rel = format!("{}/{title}.md", vault.config.folders.inbox);
    let path = files::resolve(vault, &rel).map_err(|e| e.to_string())?;
    if !path.exists() {
        own.record_text(&rel, body);
        pla_core::fs_atomic::write_atomic(&path, body.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(rel)
}

#[derive(Serialize)]
pub struct WizardDefaults {
    suggested_vault: Option<String>,
    in_onedrive: bool,
    os_language: &'static str,
}

/// FR-SET-003/004: what the wizard proposes before the user chooses.
#[tauri::command]
pub fn wizard_defaults() -> WizardDefaults {
    let suggested = crate::wizard::suggested_vault();
    WizardDefaults {
        in_onedrive: suggested.as_deref().is_some_and(|p| crate::wizard::in_onedrive(p, &crate::model_paths::process_env)),
        suggested_vault: suggested.map(|p| p.to_string_lossy().into_owned()),
        os_language: if crate::notify::strings().done == "Tamamlandı" { "tr" } else { "en" },
    }
}

#[derive(Serialize)]
pub struct FolderReport {
    check: pla_core::vault::FolderCheck,
    in_onedrive: bool,
}

/// The vault step shows what choosing this folder will do (FR-SET-005…007), without changing it.
#[tauri::command]
pub fn inspect_vault_folder(path: String) -> FolderReport {
    let path = Path::new(path.trim());
    FolderReport { check: pla_core::vault::inspect_folder(path), in_onedrive: crate::wizard::in_onedrive(path, &crate::model_paths::process_env) }
}

/// The wizard's vault step: prepare the folder (create, refuse network paths, check writing), open
/// it, and greet a new vault with a welcome note.
#[tauri::command]
pub fn setup_vault(app: AppHandle, state: State<AppState>, path: String, lang: String) -> Result<VaultInfo, String> {
    let _settings_guard = state.settings_lock.lock().expect("settings lock");
    let path = PathBuf::from(path.trim());
    let fresh = crate::wizard::prepare_folder(&path).map_err(|e| e.to_string())?;
    let info = open_vault_locked(&app, &state, &path)?;
    if fresh {
        if let Some(session) = state.session.lock().expect("session lock").as_ref() {
            write_welcome_note(&session.vault, &state.self_writes, &lang)?;
        }
    }
    Ok(info)
}

/// FR-SET-003: the language chosen in the wizard (and later in settings).
#[tauri::command]
pub fn set_language(state: State<AppState>, lang: String) -> Result<(), String> {
    if lang != "tr" && lang != "en" {
        return Err(format!("unsupported language {lang}"));
    }
    update_settings(&app_root()?, &state.settings_lock, |s| s.language = Some(lang)).map(|_| ())
}

#[tauri::command]
pub fn finish_setup(state: State<AppState>) -> Result<(), String> {
    update_settings(&app_root()?, &state.settings_lock, |s| s.setup_complete = true).map(|_| ())
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
pub fn hide_to_tray(window: tauri::WebviewWindow, state: State<AppState>) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())?;
    let mut first_time = false;
    update_settings(&app_root()?, &state.settings_lock, |s| {
        first_time = !s.tray_hint_shown;
        s.tray_hint_shown = true;
    })?;
    if first_time {
        let s = strings();
        show_info(s.tray_title, s.tray_body);
    }
    Ok(())
}


#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledModel {
    name: String,
    size: u64,
    path: String,
    local: bool,
}

#[derive(Serialize)]
pub struct ModelStatus {
    installed: Option<InstalledModel>,
    recommended: &'static catalog::CatalogEntry,
    download: Option<crate::model_download::DownloadState>,
    models_dir: Option<String>,
}

fn describe_model(path: &Path, id: Option<&str>) -> Option<InstalledModel> {
    let size = std::fs::metadata(path).ok().filter(|m| m.is_file())?.len();
    let entry = id.and_then(catalog::by_id);
    Some(InstalledModel {
        name: entry.map_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), |e| e.name.to_owned()),
        size,
        path: path.to_string_lossy().into_owned(),
        local: entry.is_none(),
    })
}

/// FR-MDL-008: a checked GGUF file becomes the active model, used where it is.
fn apply_local_model(settings: &mut AppSettings, path: &Path) -> Result<InstalledModel, String> {
    let model = local::validate_gguf(path).map_err(|e| match e {
        local::LocalModelError::NotFound(_) => "not_found".to_owned(),
        local::LocalModelError::NotGguf(_) => "not_gguf".to_owned(),
        local::LocalModelError::Incomplete { .. } => "incomplete".to_owned(),
        local::LocalModelError::Io(e) => format!("io:{e}"),
    })?;
    settings.model_path = Some(model.path.clone());
    settings.model_id = None;
    describe_model(&model.path, None).ok_or_else(|| "not_found".to_owned())
}

/// Saves the model in the settings, hands it to the running worker and tells the UI.
fn install_model(app: &AppHandle, model_path: &Path, model_id: Option<&str>, installed: &InstalledModel) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = update_settings(&app_root()?, &state.settings_lock, |s| {
        s.model_path = Some(model_path.to_path_buf());
        s.model_id = model_id.map(str::to_owned);
    })?;
    if let Some(cfg) = crate::model_paths::server_config(&settings, &crate::model_paths::process_env, bundled_server(app)) {
        if let Some(session) = app.state::<AppState>().session.lock().expect("session lock").as_ref() {
            session.worker.send(Command::UseModel(cfg));
        }
    }
    let _ = app.emit("model-changed", installed);
    Ok(())
}

#[tauri::command]
pub fn model_status(state: State<AppState>) -> Result<ModelStatus, String> {
    let settings = load_settings(&app_root()?).map_err(|e| e.to_string())?.settings;
    let path = crate::model_paths::first_existing([settings.model_path.clone(), crate::model_paths::process_env("PLA_MODEL")]);
    Ok(ModelStatus {
        installed: path.as_deref().and_then(|p| describe_model(p, settings.model_id.as_deref())),
        recommended: catalog::recommended(),
        download: state.downloads.current(),
        models_dir: models::models_dir().map(|d| d.to_string_lossy().into_owned()),
    })
}

/// The download source: the catalogue on Hugging Face, or, in debug builds only, the file at
/// `PLA_DEV_MODEL_URL` on this PC, so pause/resume/quit can be checked without 2.4 GB from the
/// internet (final review I6). Release builds ignore the variable.
fn download_source() -> (&'static catalog::CatalogEntry, Box<dyn models::policy::UrlPolicy>) {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("PLA_DEV_MODEL_URL") {
        let entry = catalog::CatalogEntry { url: Box::leak(url.into_boxed_str()), ..catalog::recommended().clone() };
        return (Box::leak(Box::new(entry)), Box::new(DevLocal));
    }
    (catalog::recommended(), Box::new(models::policy::HuggingFace))
}

/// Debug builds: the production allow-list plus `127.0.0.1`.
#[cfg(debug_assertions)]
struct DevLocal;

#[cfg(debug_assertions)]
impl models::policy::UrlPolicy for DevLocal {
    fn allows(&self, url: &str) -> Result<(), models::policy::PolicyError> {
        match models::policy::scheme_and_host(url)? {
            (_, host) if host == "127.0.0.1" => Ok(()),
            _ => models::policy::HuggingFace.allows(url),
        }
    }
}

#[tauri::command]
pub fn model_download_start(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let dir = models::models_dir().ok_or("LOCALAPPDATA is not set; PLA cannot store the model.")?;
    let (entry, policy) = download_source();
    let (events, done_app) = (app.clone(), app.clone());
    state
        .downloads
        .start(
            move |cancel, report| {
                let req = dl::Request { entry, dir: &dir, policy: policy.as_ref(), free_space: &dl::free_space, cancel, progress_every: Duration::from_secs(1), stall_after: Duration::from_secs(60) };
                dl::download(&req, report)
            },
            move |s| {
                let _ = events.emit("model-download", s);
            },
            move |path| {
                if let Some(installed) = describe_model(&path, Some(entry.id)) {
                    let _ = install_model(&done_app, &path, Some(entry.id), &installed);
                    crate::notify::model_ready(&done_app);
                }
            },
        )
        .map_err(|_| "A model download is already running.".to_owned())
}

#[tauri::command]
pub fn model_download_pause(state: State<AppState>) {
    state.downloads.pause();
}

#[tauri::command]
pub fn model_use_local(app: AppHandle, path: String) -> Result<InstalledModel, String> {
    // Checked first, so a wrong file changes nothing (Review Focus 5).
    let mut checked = AppSettings::default();
    let installed = apply_local_model(&mut checked, Path::new(&path))?;
    let model_path = checked.model_path.expect("set by apply_local_model");
    install_model(&app, &model_path, None, &installed)?;
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn using_a_non_gguf_file_changes_nothing() {
        // Review Focus 5
        let tmp = tempfile::tempdir().unwrap();
        let txt = tmp.path().join("notes.gguf");
        std::fs::write(&txt, "not a model").unwrap();
        let mut settings = AppSettings::default();
        assert!(apply_local_model(&mut settings, &txt).is_err());
        assert_eq!(settings, AppSettings::default());
    }

    #[test]
    fn a_valid_local_file_becomes_the_active_model() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("m.gguf");
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        std::fs::write(&p, &bytes).unwrap();
        let mut settings = AppSettings { model_id: Some("gemma-4-e2b-it-q3km".into()), ..AppSettings::default() };
        let installed = apply_local_model(&mut settings, &p).unwrap();
        assert_eq!((settings.model_path.as_deref(), settings.model_id.as_deref()), (Some(p.as_path()), None));
        assert!(installed.local);
        assert_eq!(installed.size, 8);
    }

    #[test]
    fn settings_written_from_two_places_keep_both_changes() {
        // Final review I5: a vault switch and a finished download must not overwrite each other
        let tmp = tempfile::tempdir().unwrap();
        let lock = Mutex::new(());
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for i in 0..30 {
                    update_settings(tmp.path(), &lock, |s| s.vault_path = Some(format!("C:/v{i}").into())).unwrap();
                }
            });
            scope.spawn(|| {
                for i in 0..30 {
                    update_settings(tmp.path(), &lock, |s| s.model_path = Some(format!("C:/m{i}.gguf").into())).unwrap();
                }
            });
        });
        let s = load_settings(tmp.path()).unwrap().settings;
        assert_eq!((s.vault_path, s.model_path), (Some("C:/v29".into()), Some("C:/m29.gguf".into())));
    }

    #[test]
    fn the_debug_download_hook_reaches_only_this_pc() {
        // Final review I6: lets the end-to-end checks use a local server instead of 2.4 GB from HF
        use pla_core::models::policy::{PolicyError, UrlPolicy};
        assert_eq!(DevLocal.allows("http://127.0.0.1:8765/model.gguf"), Ok(()));
        assert_eq!(DevLocal.allows("https://huggingface.co/x"), Ok(()));
        assert_eq!(DevLocal.allows("http://example.com/x"), Err(PolicyError::NotHttps));
        assert!(matches!(DevLocal.allows("https://evil.com/x"), Err(PolicyError::HostNotAllowed(_))));
    }

    #[test]
    fn a_new_vault_gets_a_welcome_note_in_its_inbox() {
        // FR-SET-006, written as PLA's own save so the watcher ignores it
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault_dir(tmp.path()).unwrap();
        let own = SelfWrites::default();
        let rel = write_welcome_note(&vault, &own, "tr").unwrap();
        assert_eq!(rel, "inbox/Hoş geldin.md");
        let text = std::fs::read_to_string(tmp.path().join("inbox/Hoş geldin.md")).unwrap();
        assert!(text.starts_with("# PLA'ya hoş geldin"));
        assert!(own.is_own(&rel, &pla_core::pipeline::blocks::content_hash(text.as_bytes())));
        assert_eq!(write_welcome_note(&vault, &own, "en").unwrap(), "inbox/Welcome.md");
    }
}
