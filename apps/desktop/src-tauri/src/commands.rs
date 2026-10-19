//! The open vault's session (its threads and connections, `start_session`) and the commands for the
//! vault, the first-run wizard, settings and notes. Tasks are in `task_cmds`, the model in
//! `model_cmds`; the rules themselves live in pla-core and the worker.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Local;
use pla_core::db::connect;
use pla_core::db::open_databases;
use pla_core::files::{self, FileError, NoteFile, TreeEntry};
use pla_core::llm::ModelHost;
use pla_core::pipeline::Extractor;
use pla_core::settings::{load_settings, save_settings, AppSettings, Theme};
use pla_core::vault::{default_app_root, open_vault as open_vault_dir, resolve_data_dir, Vault};
use rusqlite::Connection;

use crate::worker::{AddedItem, WorkerStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::errors::ue;
use crate::notify::{show_info, strings, AppNotifier};
use crate::scheduler::{Pending, SchedCommand, Scheduler, SchedulerHandle};
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
    /// After the watcher, which feeds it (M3).
    pub indexer: crate::indexer::IndexerHandle,
    /// The UI's reading connection to `cache.db` (search, backlinks, tags).
    pub cache: Mutex<Connection>,
    /// For the worker's own reading connection while it answers a question (FR-QA-003).
    pub cache_path: std::path::PathBuf,
    /// The embedding server, when its model is there (FR-MEM-004); shared with the indexer.
    pub embeds: crate::memory_cmds::Embeds,
    /// Background AI paused (FR-SCH-014): the indexer stops embedding too.
    pub memory_pause: Arc<AtomicBool>,
    pub worker: WorkerHandle,
    /// FR-SCH-016: when the weekly report is due; the settings screen changes it live.
    pub weekly: Arc<Mutex<pla_core::report::WeeklySchedule>>,
    /// Where this vault's pla.db and cache.db live (`<app root>/vaults/<vault_id>`).
    pub data_dir: std::path::PathBuf,
    /// FR-BKP-004/008, FR-LOG-007: a backup offered when the vault opened; background AI waits for
    /// the answer (it would only extract into data about to be replaced).
    pub restore_offer: Mutex<Option<pla_core::recovery::RestoreOffer>>,
    /// Last, so it drops after the worker, scheduler and indexer have closed pla.db.
    pub _running: RunningGuard,
}

/// FR-LOG-006: a session that ends normally tells the next start not to check pla.db; dropped last
/// in `Session`, so a stop during the shutdown itself still counts as unclean (final review).
pub struct RunningGuard(std::path::PathBuf);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        pla_core::recovery::closed_cleanly(&self.0);
    }
}

#[derive(Default)]
pub struct AppState {
    pub session: Mutex<Option<Session>>,
    pub self_writes: Arc<SelfWrites>,
    /// Background AI paused (FR-SCH-014); kept across vault switches.
    pub paused: AtomicBool,
    /// A restore is swapping pla.db: closing or quitting waits for it (final review I2).
    pub restoring: AtomicBool,
    /// The model download outlives vault switches (model-manager spec § 5).
    pub downloads: crate::model_download::ModelDownloads,
    /// The embedding model's download, next to the language model's.
    pub memory_downloads: crate::model_download::ModelDownloads,
    /// Held for every load-modify-save of settings.json, and while a vault opens, so a finished
    /// download, a vault switch and the tray hint never overwrite each other (final review I5).
    pub settings_lock: Mutex<()>,
}

/// Load, change and save settings.json under `lock`.
pub(crate) fn update_settings(root: &Path, lock: &Mutex<()>, change: impl FnOnce(&mut AppSettings)) -> Result<AppSettings, String> {
    let _guard = lock.lock().expect("settings lock");
    let mut settings = load_settings(root).map_err(ue)?.settings;
    change(&mut settings);
    save_settings(root, &settings).map_err(ue)?;
    Ok(settings)
}

#[derive(Serialize)]
pub struct VaultInfo {
    path: String,
    inbox: String,
    /// FR-I18N-004: the tree names the vault's own folders in the interface language.
    folders: pla_core::vault::Folders,
    restore_offer: Option<pla_core::recovery::RestoreOffer>,
}

fn vault_info_of(vault: &Vault) -> VaultInfo {
    VaultInfo {
        path: vault.root.to_string_lossy().into_owned(),
        inbox: vault.config.folders.inbox.clone(),
        folders: vault.config.folders.clone(),
        restore_offer: None,
    }
}

fn session_info(session: &Session) -> VaultInfo {
    VaultInfo { restore_offer: session.restore_offer.lock().expect("offer lock").clone(), ..vault_info_of(&session.vault) }
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
    folders: Option<pla_core::vault::Folders>,
    /// The remembered vault lies in PLA's own data folders (from before PLA refused that): it still
    /// opens, but the user is told to move it before uninstalling (NFR-SEC-010).
    vault_in_app_data: bool,
    /// FR-BKP-004/008, FR-LOG-007: offer a backup before the vault is used.
    restore_offer: Option<pla_core::recovery::RestoreOffer>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SaveResult {
    Saved { hash: String },
    Conflict { current_hash: String },
    Missing,
}

pub(crate) fn app_root() -> Result<PathBuf, String> {
    default_app_root().ok_or_else(|| ue(crate::errors::UserError::new("no_app_data", "")))
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
    let mut vault = open_vault_dir(root).map_err(ue)?;
    let dir = resolve_data_dir(&app_root()?, &mut vault).map_err(ue)?;
    // before the databases open: a damaged pla.db goes aside, a missing one may come from a backup
    let offer = pla_core::recovery::before_open(&dir, &vault.root, Local::now().fixed_offset()).map_err(ue)?;
    let paused = paused || offer.is_some();
    let dbs = open_databases(&dir).map_err(ue)?;
    let ui_db = connect(&dir.join("pla.db")).map_err(ue)?;
    let status =
        Arc::new(Mutex::new(WorkerStatus { queued: 0, model: crate::worker::ModelState::Off, busy: false, last_error: None, added: 0, paused: false }));
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
    }))
    .with_summary_listener({
        let summary_app = app.clone();
        Box::new(move |date: &str, error: Option<&str>| {
            let _ = summary_app.emit("summary-changed", serde_json::json!({ "date": date, "error": error }));
        })
    })
    .with_report_listener({
        let (report_app, report_vault) = (app.clone(), vault.root.clone());
        Box::new(move |week, outcome| match outcome {
            crate::worker::ReportOutcome::Written(rel) => crate::notify::report_ready(&report_app, &report_vault, week, rel),
            // FR-SCH-015: the second failure of a day is shown; the first is tried again
            crate::worker::ReportOutcome::Failed { attempts, reason } if *attempts >= pla_core::jobs::MAX_ATTEMPTS_PER_DAY => {
                crate::notify::job_failed(&report_app, "report", reason);
            }
            _ => {}
        })
    });
    let handle = worker.spawn();
    if paused {
        handle.set_paused(true);
    }
    let index_app = app.clone();
    let embeds: crate::memory_cmds::Embeds = Arc::new(Mutex::new(crate::memory_cmds::model_file().and_then(|m| crate::memory_cmds::host(app, m))));
    let memory_pause = Arc::new(AtomicBool::new(paused));
    let (progress_app, worker_status) = (app.clone(), Arc::clone(&status));
    let memory = crate::indexer::MemoryJob {
        embeds: Arc::clone(&embeds),
        pause: Arc::clone(&memory_pause),
        // two models at once would fight for the CPU: embedding waits while the worker is busy
        busy: Box::new(move || worker_status.lock().expect("status lock").busy),
        progress: Box::new(move |pending, total| {
            let _ = progress_app.emit("memory-progress", serde_json::json!({ "pending": pending, "total": total }));
        }),
    };
    let indexer = crate::indexer::spawn(
        vault.root.clone(),
        dir.join("cache.db"),
        Box::new(move || {
            let _ = index_app.emit("index-changed", ());
        }),
        Some(memory),
    );
    let cache = dbs.cache;
    let watch_handle = watch(vault.clone(), own, handle.sender(), indexer.sender(), app.clone()).ok();
    let sched_db = connect(&dir.join("pla.db")).map_err(ue)?;
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<SchedCommand>();
    let notifier = AppNotifier::new(app.clone(), cmd_tx.clone());
    let weekly = Arc::new(Mutex::new(settings.weekly_report.clone()));
    let reports = vault.root.join(&vault.config.folders.reports).join("weekly");
    let scheduler = Scheduler::new(vault.root.clone(), sched_db, Box::new(notifier), Box::new(WindowsProbe), Box::new(|| Local::now().fixed_offset()))
        .with_weekly_report(reports, Arc::clone(&weekly));
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
    Ok(Session {
        vault,
        scheduler,
        pending,
        db: Mutex::new(ui_db),
        status,
        _watch: watch_handle,
        indexer,
        cache: Mutex::new(cache),
        cache_path: dir.join("cache.db"),
        embeds,
        memory_pause,
        worker: handle,
        weekly,
        _running: RunningGuard(dir.clone()),
        data_dir: dir,
        restore_offer: Mutex::new(offer),
    })
}

/// The open session, or the `no_vault` error every command gives without one.
pub(crate) fn with_session<T>(state: &AppState, f: impl FnOnce(&Session) -> Result<T, String>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    f(guard.as_ref().ok_or(NO_VAULT)?)
}

/// The window's pla.db connection; `code` says the command's own error the way the page reads it.
pub(crate) fn with_db<T, E>(state: &AppState, code: impl FnOnce(E) -> String, f: impl FnOnce(&mut Connection) -> Result<T, E>) -> Result<T, String> {
    with_session(state, |session| f(&mut session.db.lock().expect("db lock")).map_err(code))
}

/// The window's cache.db connection (search, backlinks, tags, summaries).
pub(crate) fn with_cache<T>(state: &AppState, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
    with_session(state, |session| f(&session.cache.lock().expect("cache lock")).map_err(ue))
}

/// Runs a write in one immediate transaction (the write lock taken up front, `write_tx`) and commits it.
pub(crate) fn in_write_tx<T, E: From<rusqlite::Error>>(db: &mut Connection, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, E> {
    let tx = pla_core::pipeline::write_tx(db)?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

fn with_vault<T>(state: &AppState, f: impl FnOnce(&Vault) -> Result<T, FileError>) -> Result<T, String> {
    with_session(state, |session| f(&session.vault).map_err(ue))
}

#[tauri::command]
pub fn startup(app: AppHandle, state: State<AppState>) -> Result<StartupInfo, String> {
    begin(&app, &state)
}

/// Opens the vault of the settings once (the session: worker, scheduler, watcher, reminders) and
/// says what the page needs. The page calls it; a start in the tray (`--hidden`) calls it at
/// setup, as no page loads there until the window opens (M6 final review C1).
pub fn begin(app: &AppHandle, state: &AppState) -> Result<StartupInfo, String> {
    let root = app_root()?;
    let _settings_guard = state.settings_lock.lock().expect("settings lock");
    let loaded = load_settings(&root).map_err(ue)?;
    let theme = match loaded.settings.theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    let mut info = StartupInfo {
        vault_path: None,
        first_run: loaded.first_run,
        show_wizard: crate::wizard::show_wizard(loaded.first_run, &loaded.settings),
        // only languages the UI has; a hand-edited "de" would leave it blank (final review M5)
        language: loaded.settings.language.clone().filter(|l| l == "tr" || l == "en"),
        settings_recovered: loaded.recovered_from_broken,
        theme: theme.to_owned(),
        error: None,
        inbox: None,
        folders: None,
        vault_in_app_data: false,
        restore_offer: None,
    };
    if let Some(running) = state.session.lock().expect("session lock").as_ref() {
        info.vault_path = Some(running.vault.root.to_string_lossy().into_owned());
        info.inbox = Some(running.vault.config.folders.inbox.clone());
        info.folders = Some(running.vault.config.folders.clone());
        info.vault_in_app_data = pla_core::vault::inside_app_data(&running.vault.root, &pla_core::vault::app_data_roots());
        info.restore_offer = running.restore_offer.lock().expect("offer lock").clone();
        return Ok(info);
    }
    if let Some(path) = loaded.settings.vault_path.clone() {
        match start_session(app, &path, &loaded.settings, Arc::clone(&state.self_writes), state.paused.load(Ordering::SeqCst)) {
            Ok(session) => {
                info.vault_path = Some(session.vault.root.to_string_lossy().into_owned());
                info.inbox = Some(session.vault.config.folders.inbox.clone());
                info.folders = Some(session.vault.config.folders.clone());
                info.vault_in_app_data = pla_core::vault::inside_app_data(&session.vault.root, &pla_core::vault::app_data_roots());
                info.restore_offer = session.restore_offer.lock().expect("offer lock").clone();
                *state.session.lock().expect("session lock") = Some(session);
            }
            Err(e) => info.error = Some(crate::errors::naming(&e, &path.display().to_string())),
        }
    }
    Ok(info)
}

#[tauri::command]
pub fn open_vault(app: AppHandle, state: State<AppState>, path: String) -> Result<VaultInfo, String> {
    let _settings_guard = state.settings_lock.lock().expect("settings lock");
    open_vault_locked(&app, &state, &crate::wizard::clean_path(&path))
}

/// Opens `path` as the vault and remembers it; the caller holds the settings lock.
fn open_vault_locked(app: &AppHandle, state: &State<AppState>, path: &Path) -> Result<VaultInfo, String> {
    // NFR-SEC-010: the uninstaller may delete PLA's own folders, so a vault never lives there
    if pla_core::vault::inside_app_data(path, &pla_core::vault::app_data_roots()) {
        return Err(format!("app_data|{}", path.display()));
    }
    let root = app_root()?;
    let settings = load_settings(&root).map_err(ue)?.settings;
    // Stop the previous vault before opening the next, outside the lock: one writer per pla.db.
    let previous = state.session.lock().expect("session lock").take();
    let mut old_root = None;
    if let Some(old) = previous {
        if same_folder(&old.vault.root, path) {
            let opened = session_info(&old);
            *state.session.lock().expect("session lock") = Some(old);
            return Ok(opened);
        }
        old_root = Some(old.vault.root.clone());
        drop(old);
    }
    let paused = state.paused.load(Ordering::SeqCst);
    // The new folder could not be opened (or remembered): go back to the previous vault (settings
    // Review Focus 3). If even that fails, say so: the UI must not keep showing a closed vault.
    let restore = |error: String| -> String {
        let Some(old) = &old_root else { return error };
        match start_session(app, old, &settings, Arc::clone(&state.self_writes), paused) {
            Ok(back) => {
                *state.session.lock().expect("session lock") = Some(back);
                error
            }
            Err(_) => format!("vault_lost|{}|{error}", old.display()),
        }
    };
    let session = match start_session(app, path, &settings, Arc::clone(&state.self_writes), paused) {
        Ok(session) => session,
        Err(e) => return Err(restore(e)),
    };
    let opened = session_info(&session);
    let mut remembered = settings.clone();
    remembered.vault_path = Some(session.vault.root.clone());
    if let Err(e) = save_settings(&root, &remembered) {
        drop(session); // stop it before the old one opens again
        return Err(restore(crate::errors::ue(e)));
    }
    *state.session.lock().expect("session lock") = Some(session); // drops (and stops) the previous one
    Ok(opened)
}

/// FR-SET-006: the note a new or empty vault starts with, saved as PLA's own write. An existing note
/// of that name is left alone.
fn write_welcome_note(vault: &Vault, own: &SelfWrites, lang: &str) -> Result<String, String> {
    let (title, body) = crate::wizard::welcome_note(lang);
    let rel = format!("{}/{title}.md", vault.config.folders.inbox);
    let path = files::resolve(vault, &rel).map_err(ue)?;
    if !path.exists() {
        own.record_text(&rel, body);
        pla_core::fs_atomic::write_atomic(&path, body.as_bytes()).map_err(ue)?;
    }
    Ok(rel)
}

#[derive(Serialize)]
pub struct WizardDefaults {
    suggested_vault: Option<String>,
}

/// FR-SET-004: the folder the wizard proposes before the user chooses.
#[tauri::command]
pub fn wizard_defaults() -> WizardDefaults {
    WizardDefaults { suggested_vault: crate::wizard::suggested_vault().map(|p| p.to_string_lossy().into_owned()) }
}

#[derive(Serialize)]
pub struct FolderReport {
    check: pla_core::vault::FolderCheck,
    in_onedrive: bool,
}

/// The vault step shows what choosing this folder will do (FR-SET-005…007), without changing it.
/// Runs off the main thread: counting notes in a big folder takes a moment (final review I3).
#[tauri::command(async)]
pub fn inspect_vault_folder(path: String) -> FolderReport {
    let path = crate::wizard::clean_path(&path);
    FolderReport { check: pla_core::vault::inspect_folder(&path), in_onedrive: crate::wizard::in_onedrive(&path, &crate::model_paths::process_env) }
}

/// The wizard's vault step: prepare the folder (create, refuse network paths, check writing), open
/// it, and greet a new vault with a welcome note.
#[tauri::command(async)]
pub fn setup_vault(app: AppHandle, state: State<AppState>, path: String, lang: String) -> Result<VaultInfo, String> {
    let _settings_guard = state.settings_lock.lock().expect("settings lock");
    let path = crate::wizard::clean_path(&path);
    let fresh = crate::wizard::prepare_folder(&path).map_err(String::from)?;
    let info = open_vault_locked(&app, &state, &path)?;
    if fresh {
        if let Some(session) = state.session.lock().expect("session lock").as_ref() {
            write_welcome_note(&session.vault, &state.self_writes, &lang)?;
        }
    }
    Ok(info)
}

/// FR-SET-003: the language chosen in the wizard.
#[tauri::command]
pub fn set_language(app: AppHandle, state: State<AppState>, lang: String) -> Result<(), String> {
    settings_set(app, state, "language".into(), serde_json::Value::String(lang)).map(|_| ())
}

/// The weekly report's settings and its last run (FR-SCH-011, -016).
#[derive(Serialize)]
pub struct WeeklyReportView {
    schedule: pla_core::report::WeeklySchedule,
    job: pla_core::jobs::JobStatus,
}

#[tauri::command]
pub fn weekly_report_settings(state: State<AppState>) -> Result<WeeklyReportView, String> {
    let settings = load_settings(&app_root()?).map_err(ue)?.settings;
    let guard = state.session.lock().expect("session lock");
    let job = match guard.as_ref() {
        Some(session) => pla_core::jobs::status(&session.db.lock().expect("db lock"), pla_core::jobs::WEEKLY_REPORT).map_err(ue)?,
        None => pla_core::jobs::JobStatus::default(),
    };
    Ok(WeeklyReportView { schedule: settings.weekly_report, job })
}

/// FR-SCH-016: the day and time of the weekly report, or off. The running scheduler hears it at once.
#[tauri::command]
pub fn set_weekly_report(state: State<AppState>, schedule: pla_core::report::WeeklySchedule) -> Result<(), String> {
    if schedule.weekday > 6 || chrono::NaiveTime::parse_from_str(&schedule.time, "%H:%M").is_err() {
        return Err(format!("invalid_setting|weekly_report|{} {}", schedule.weekday, schedule.time));
    }
    update_settings(&app_root()?, &state.settings_lock, |s| s.weekly_report = schedule.clone())?;
    if let Some(session) = state.session.lock().expect("session lock").as_ref() {
        *session.weekly.lock().expect("weekly schedule lock") = schedule;
    }
    Ok(())
}

/// What the settings screen shows (FR-SET-011).
#[derive(Serialize)]
pub struct SettingsView {
    language: Option<String>,
    theme: Theme,
    autostart: bool,
    paused: bool,
    vault_path: Option<String>,
}

/// One plain setting from the settings screen; anything else is refused and changes nothing.
pub fn apply_setting(s: &mut AppSettings, key: &str, value: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    match (key, value) {
        ("language", Value::Null) => s.language = None,
        ("language", Value::String(l)) if l == "tr" || l == "en" => s.language = Some(l.clone()),
        ("theme", Value::String(t)) if t == "dark" => s.theme = Theme::Dark,
        ("theme", Value::String(t)) if t == "light" => s.theme = Theme::Light,
        _ => return Err(format!("invalid_setting|{key}|{value}")),
    }
    Ok(())
}

fn settings_view(app: &AppHandle, state: &State<AppState>, settings: &AppSettings) -> SettingsView {
    use tauri_plugin_autostart::ManagerExt;
    SettingsView {
        language: settings.language.clone(),
        theme: settings.theme,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        paused: state.paused.load(Ordering::SeqCst),
        vault_path: settings.vault_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
pub fn settings_get(app: AppHandle, state: State<AppState>) -> Result<SettingsView, String> {
    let settings = load_settings(&app_root()?).map_err(ue)?.settings;
    Ok(settings_view(&app, &state, &settings))
}

/// Changes one setting at once (no Save button); a language change also re-labels the tray and
/// the notifications (FR-SET-012).
#[tauri::command]
pub fn settings_set(app: AppHandle, state: State<AppState>, key: String, value: serde_json::Value) -> Result<SettingsView, String> {
    let mut refused = None;
    let settings = update_settings(&app_root()?, &state.settings_lock, |s| {
        let mut changed = s.clone();
        match apply_setting(&mut changed, &key, &value) {
            Ok(()) => *s = changed,
            Err(e) => refused = Some(e),
        }
    })?;
    if let Some(e) = refused {
        return Err(e);
    }
    if key == "language" {
        crate::notify::set_ui_language(settings.language.as_deref());
        crate::tray::refresh(&app);
    }
    Ok(settings_view(&app, &state, &settings))
}

/// The Notifications section's "send a test notification".
#[tauri::command]
pub fn send_test_notification() {
    let s = strings();
    show_info(s.test_title, s.test_body);
}

/// FR-SET-016, from the settings screen; the tray check follows.
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    crate::tray::apply_autostart(&app, enabled)
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
    with_session(&state, |session| {
        files::resolve(&session.vault, &path).map_err(ue)?;
        session.worker.send(Command::Enqueue(path));
        Ok(())
    })
}

/// Every command's "no vault is open" error, in the `<code>|<detail>|<reason>` form the UI reads.
pub const NO_VAULT: &str = "no_vault||";

#[tauri::command]
pub fn vault_info(state: State<AppState>) -> Result<VaultInfo, String> {
    with_session(&state, |s| Ok(vault_info_of(&s.vault)))
}

#[tauri::command]
pub fn worker_status(state: State<AppState>) -> Result<WorkerStatus, String> {
    with_session(&state, |s| Ok(s.status.lock().expect("status lock").clone()))
}

#[tauri::command]
pub fn backup_now(state: State<AppState>) -> Result<(), String> {
    with_session(&state, |s| {
        s.scheduler.send(SchedCommand::BackupNow);
        Ok(())
    })
}

/// FR-BKP-005: the vault's backups, newest first (`pla-<YYYY-MM-DD>.db`).
#[tauri::command]
pub fn backup_list(state: State<AppState>) -> Result<Vec<String>, String> {
    with_session(&state, |s| Ok(pla_core::jobs::backups(&s.vault.root)))
}

/// FR-BKP-004/005/006/008, FR-LOG-007: puts backup `name` in place of this vault's pla.db. The
/// session stops first (nothing may hold pla.db), and starts again whatever happened, so a refused
/// backup leaves PLA as it was. The page reloads afterwards: every view reads the new data.
#[tauri::command(async)]
pub fn restore_backup(app: AppHandle, state: State<AppState>, name: String) -> Result<(), String> {
    let _settings_guard = state.settings_lock.lock().expect("settings lock");
    let root = app_root()?;
    let settings = load_settings(&root).map_err(ue)?.settings;
    let session = state.session.lock().expect("session lock").take().ok_or(NO_VAULT)?;
    let (vault_root, data_dir) = (session.vault.root.clone(), session.data_dir.clone());
    state.restoring.store(true, Ordering::SeqCst);
    drop(session); // waits for the worker, scheduler and indexer: their connections close
    let restored = pla_core::jobs::restore(&data_dir, &vault_root, &name, Local::now().fixed_offset()).map_err(|e| match e {
        pla_core::jobs::RestoreError::Unknown(n) => ue(crate::errors::UserError::new("missing", n)),
        pla_core::jobs::RestoreError::Damaged(n) => ue(crate::errors::UserError::new("backup_damaged", n)),
        pla_core::jobs::RestoreError::NewerSchema(n) => ue(crate::errors::UserError::new("newer_schema", n)),
        pla_core::jobs::RestoreError::Db(e) => ue(e),
        pla_core::jobs::RestoreError::Io(e) => ue(e),
    });
    let again = start_session(&app, &vault_root, &settings, Arc::clone(&state.self_writes), state.paused.load(Ordering::SeqCst));
    state.restoring.store(false, Ordering::SeqCst);
    // the page reloads next: tray requests wait for the new page (final review)
    app.state::<crate::tray::UiQueue>().not_listening();
    match again {
        Ok(session) => *state.session.lock().expect("session lock") = Some(session),
        Err(e) => return Err(restored.err().unwrap_or(e)), // the restore's own failure is the one to say
    }
    restored.map(drop)
}

/// The user starts with no data instead of the offered backup: background AI goes on as it was.
#[tauri::command]
pub fn keep_empty(app: AppHandle, state: State<AppState>) {
    if let Some(session) = state.session.lock().expect("session lock").as_ref() {
        *session.restore_offer.lock().expect("offer lock") = None;
        pla_core::recovery::answered(&session.data_dir);
    }
    apply_pause(&app, state.paused.load(Ordering::SeqCst));
}

/// FR-SCH-014: the one path for pausing background AI, from the tray or the status bar, so the
/// tray check, the worker and the UI agree (final review I4).
pub fn apply_pause(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    state.paused.store(paused, Ordering::SeqCst);
    if let Some(session) = state.session.lock().expect("session lock").as_ref() {
        session.worker.set_paused(paused);
        session.memory_pause.store(paused, Ordering::SeqCst);
    }
    crate::tray::set_pause_checked(app, paused);
    let _ = app.emit("paused-changed", paused);
}

#[tauri::command]
pub fn set_paused(app: AppHandle, paused: bool) -> Result<(), String> {
    apply_pause(&app, paused);
    Ok(())
}

/// What the tray or a notification asked of the window while it was being made (`tray::to_ui`).
#[tauri::command]
pub fn take_ui_events(queue: State<crate::tray::UiQueue>) -> Vec<(String, serde_json::Value)> {
    queue.take()
}

/// FR-SCH-001: closing the window keeps PLA in the tray; the first time, say so.
#[tauri::command]
pub fn hide_to_tray(window: tauri::WebviewWindow, state: State<AppState>, queue: State<crate::tray::UiQueue>) -> Result<(), String> {
    // the WebView goes with the window: in the tray PLA keeps only its core (NFR-PERF-006, M6
    // decision a); the page saved its note before asking (App.svelte onCloseRequested)
    queue.not_listening();
    window.destroy().map_err(ue)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_screen_can_change_only_known_values() {
        // FR-SET-011, settings_set
        let mut s = AppSettings::default();
        apply_setting(&mut s, "language", &serde_json::json!("en")).unwrap();
        assert_eq!(s.language.as_deref(), Some("en"));
        apply_setting(&mut s, "language", &serde_json::Value::Null).unwrap();
        assert_eq!(s.language, None, "null = follow Windows again");
        apply_setting(&mut s, "theme", &serde_json::json!("light")).unwrap();
        assert_eq!(s.theme, Theme::Light);
        let before = s.clone();
        for (key, value) in [
            ("language", serde_json::json!("de")),
            ("theme", serde_json::json!("blue")),
            ("vault_path", serde_json::json!("C:/x")),
            ("theme", serde_json::json!(1)),
        ] {
            assert!(apply_setting(&mut s, key, &value).is_err(), "{key} = {value}");
        }
        assert_eq!(s, before, "a refused value changes nothing");
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
    fn a_new_vault_gets_a_welcome_note_in_its_inbox() {
        // FR-SET-006, written as PLA's own save so the watcher ignores it
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault_dir(tmp.path()).unwrap();
        let own = SelfWrites::default();
        let rel = write_welcome_note(&vault, &own, "tr").unwrap();
        assert_eq!(rel, "inbox/Hoş geldin.md");
        let text = std::fs::read_to_string(tmp.path().join("inbox/Hoş geldin.md")).unwrap();
        assert!(text.contains("# PLA'ya hoş geldin"));
        assert!(own.is_own(&rel, &pla_core::util::content_hash(text.as_bytes())));
        assert_eq!(write_welcome_note(&vault, &own, "en").unwrap(), "inbox/Welcome.md");
    }
}
