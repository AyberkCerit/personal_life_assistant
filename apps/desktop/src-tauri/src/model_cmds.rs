//! Tauri commands for the language model (FR-MDL): its status, download, a file the user has, and
//! removing it; the rules live in `pla_core::models` and `pla_core::llm`.

use std::path::Path;
use std::time::Duration;

use pla_core::models::{self, catalog, download as dl, local};
use pla_core::settings::{load_settings, AppSettings};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{app_root, bundled_server, update_settings, AppState};
use crate::worker::Command;
use crate::errors::{own_code, ue, UserError};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledModel {
    pub name: String,
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

pub(crate) fn describe_model(path: &Path, id: Option<&str>) -> Option<InstalledModel> {
    let size = std::fs::metadata(path).ok().filter(|m| m.is_file())?.len();
    let entry = id.and_then(catalog::by_id);
    Some(InstalledModel {
        name: entry.map_or_else(|| local_name(path), |e| e.name.to_owned()),
        size,
        path: path.to_string_lossy().into_owned(),
        local: entry.is_none(),
    })
}

/// A local file by its name; a nameless one (an Ollama blob, `sha256-…`) by the model's own name.
fn local_name(path: &Path) -> String {
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("gguf")) {
        return file;
    }
    models::scan::read_header(path).ok().and_then(|h| h.name).unwrap_or(file)
}

/// FR-MDL-008: a checked GGUF file becomes the active model, used where it is.
fn apply_local_model(settings: &mut AppSettings, path: &Path) -> Result<InstalledModel, String> {
    let model = local::validate_gguf(path).map_err(|e| match e {
        local::LocalModelError::NotFound(p) => own_code("not_found", p.display(), ""),
        local::LocalModelError::NotGguf(p) => own_code("not_gguf", p.display(), ""),
        local::LocalModelError::Incomplete { expected, actual } => own_code("incomplete", "", format!("{actual} of {expected} bytes")),
        local::LocalModelError::Io(e) => format!("io:{e}"),
    })?;
    settings.model_path = Some(model.path.clone());
    settings.model_id = None;
    describe_model(&model.path, None).ok_or_else(|| own_code("not_found", "", ""))
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

/// FR-MDL-019: stops the model, then deletes a model PLA downloaded or only forgets a file the
/// user chose (settings Review Focus 2). AI features turn off; notes keep queueing.
#[tauri::command(async)]
pub fn model_remove(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    crate::qa_cmds::qa_stop(); // a question in progress would hold the model (final review)
    let root = app_root()?;
    let settings = load_settings(&root).map_err(ue)?.settings;
    let models_dir = models::models_dir();
    let Some(plan) = crate::model_paths::removal(&settings, models_dir.as_deref()) else { return Ok(()) };
    let (ack, done) = std::sync::mpsc::channel();
    let asked = state.session.lock().expect("session lock").as_ref().map(|s| s.worker.drop_model(ack)).is_some();
    if asked {
        // The run stops before its next model call and the call in flight within a second
        // (Extractor::set_interrupt); only a model still starting can hold it, two tries of 30 s.
        done.recv_timeout(Duration::from_secs(90)).map_err(|_| own_code("model_busy", "", ""))?;
    }
    if let crate::model_paths::Removal::Delete(path) = &plan {
        // llama-server may need a moment to release the file after it exits
        let mut result = std::fs::remove_file(path);
        for _ in 0..20 {
            match &result {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    std::thread::sleep(Duration::from_millis(250));
                    result = std::fs::remove_file(path);
                }
                _ => break,
            }
        }
        match result {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(own_code("not_removed", path.display(), crate::wizard::write_failure(&e))),
        }
    }
    // A model chosen or downloaded meanwhile stays (settings final review M1).
    update_settings(&root, &state.settings_lock, |s| {
        if s.model_path == settings.model_path {
            s.model_path = None;
            s.model_id = None;
        }
    })?;
    let _ = app.emit("model-removed", ());
    Ok(())
}

#[tauri::command]
pub fn model_status(state: State<AppState>) -> Result<ModelStatus, String> {
    let settings = load_settings(&app_root()?).map_err(ue)?.settings;
    let path = crate::model_paths::first_existing([settings.model_path.clone(), crate::model_paths::process_env("PLA_MODEL")]);
    Ok(ModelStatus {
        installed: path.as_deref().and_then(|p| describe_model(p, settings.model_id.as_deref())),
        recommended: catalog::recommended(),
        download: state.downloads.current().or_else(|| {
            let (entry, dir) = (catalog::recommended(), models::models_dir()?);
            crate::model_download::leftover(&dl::part_path(&dir, entry), entry.size)
        }),
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
    let dir = models::models_dir().ok_or_else(|| ue(UserError::new("no_app_data", "")))?;
    let (entry, policy) = download_source();
    let (events, done_app) = (app.clone(), app.clone());
    state
        .downloads
        .start(
            move |cancel, report| crate::model_download::download(entry, &dir, policy.as_ref(), cancel, report),
            move |s| {
                let _ = events.emit("model-download", s);
            },
            move |path| {
                if let Some(installed) = describe_model(&path, Some(entry.id)) {
                    let _ = install_model(&done_app, &path, Some(entry.id), &installed);
                    crate::notify::model_ready(&done_app);
                }
                // owner decision A: the embedding model follows the language model
                if crate::memory_cmds::model_file().is_none() {
                    let _ = crate::memory_cmds::start_download(&done_app);
                }
            },
        )
        .map_err(|_| own_code("download_running", "", ""))
}

#[tauri::command]
pub fn model_download_pause(state: State<AppState>) {
    state.downloads.pause();
}

#[tauri::command]
pub fn model_use_local(app: AppHandle, path: String) -> Result<InstalledModel, String> {
    // Checked first, so a wrong file changes nothing (Review Focus 5).
    let mut checked = AppSettings::default();
    let mut installed = apply_local_model(&mut checked, Path::new(&path))?;
    let model_path = checked.model_path.expect("set by apply_local_model");
    // the catalogue model already in PLA's folder (a reinstall, a download from before) is PLA's own
    let id = catalogue_id(&model_path, models::models_dir().as_deref());
    if let Some(id) = id {
        installed = describe_model(&model_path, Some(id)).ok_or_else(|| own_code("not_found", "", ""))?;
    }
    install_model(&app, &model_path, id, &installed)?;
    Ok(installed)
}

/// The catalogue id of a whole catalogue file inside PLA's models folder (its size is checked by
/// `validate_gguf`); anything else is the user's own file.
fn catalogue_id(path: &Path, models_dir: Option<&Path>) -> Option<&'static str> {
    let entry = catalog::recommended();
    let name = path.file_name()?.to_string_lossy();
    let inside = models_dir.zip(path.parent()).is_some_and(|(dir, parent)| crate::model_paths::same_dir(dir, parent));
    (inside && name.eq_ignore_ascii_case(entry.file_name)).then_some(entry.id)
}

/// FR-MDL-008: language models other tools (or an earlier PLA) left on this PC, from their known
/// folders only; the one in use is left out.
#[tauri::command(async)]
pub fn model_scan() -> Result<Vec<models::scan::FoundModel>, String> {
    let current = load_settings(&app_root()?).map_err(ue)?.settings.model_path;
    Ok(models::scan::scan(&models::scan::this_pc()).into_iter().filter(|m| Some(&m.path) != current.as_ref()).collect())
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
    fn a_nameless_blob_is_shown_by_the_models_own_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes()); // one key: general.name = "Gemma 3 4b It"
        bytes.extend_from_slice(&12u64.to_le_bytes());
        bytes.extend_from_slice(b"general.name");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&13u64.to_le_bytes());
        bytes.extend_from_slice(b"Gemma 3 4b It");
        let blob = tmp.path().join(format!("sha256-{}", "a".repeat(64)));
        std::fs::write(&blob, &bytes).unwrap();
        assert_eq!(describe_model(&blob, None).unwrap().name, "Gemma 3 4b It");
        let named = tmp.path().join("mine.gguf");
        std::fs::write(&named, &bytes).unwrap();
        assert_eq!(describe_model(&named, None).unwrap().name, "mine.gguf", "a file the user named keeps its name");
    }

    #[test]
    fn only_the_catalogue_file_in_plas_folder_is_plas_own() {
        let tmp = tempfile::tempdir().unwrap();
        let ours = tmp.path().join("models");
        std::fs::create_dir(&ours).unwrap();
        let name = catalog::recommended().file_name;
        assert_eq!(catalogue_id(&ours.join(name), Some(&ours)), Some(catalog::recommended().id));
        assert_eq!(catalogue_id(&tmp.path().join(name), Some(&ours)), None, "a copy elsewhere is the user's");
        assert_eq!(catalogue_id(&ours.join("other.gguf"), Some(&ours)), None);
        assert_eq!(catalogue_id(&ours.join(name), None), None);
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
}
