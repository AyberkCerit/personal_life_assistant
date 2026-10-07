//! Semantic memory in the app (FR-MEM-003/004): the embedding model's download (owner decision A:
//! after the language model, or by button), the shared embedding server, and its progress.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pla_core::llm::{ModelHost, ServerConfig};
use pla_core::memory::{self, Embedder};
use pla_core::models::{self, catalog, download as dl};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::{bundled_server, AppState};

/// The embedding server both the indexer (in the background) and the worker (for a question) use.
pub type Embeds = Arc<Mutex<Option<Box<dyn Embedder + Send>>>>;

/// FR-MDL-013: the embedding server stops after this long without requests, like the chat model.
const IDLE_AFTER: Duration = Duration::from_secs(60);

/// The downloaded embedding model, or `PLA_EMBED_MODEL` (development).
pub fn model_file() -> Option<PathBuf> {
    let env = std::env::var_os("PLA_EMBED_MODEL").map(PathBuf::from).filter(|p| p.is_file());
    env.or_else(|| models::models_dir().map(|d| d.join(catalog::embedding().file_name)).filter(|p| p.is_file()))
}

/// The embedding server for `model`, when llama-server is there to run it.
pub fn host(app: &AppHandle, model: PathBuf) -> Option<Box<dyn Embedder + Send>> {
    let settings = pla_core::settings::load_settings(&crate::commands::app_root().ok()?).ok()?.settings;
    let bin = crate::model_paths::first_existing([settings.llama_server.clone(), crate::model_paths::process_env("PLA_LLAMA_SERVER"), bundled_server(app)])?;
    Some(Box::new(ModelHost::new(ServerConfig::embedding(bin, model), IDLE_AFTER)))
}

#[derive(Serialize)]
pub struct MemoryStatus {
    installed: bool,
    entry: &'static catalog::CatalogEntry,
    download: Option<crate::model_download::DownloadState>,
    /// Chunks still to embed, and all chunks (FR-MEM-003).
    pending: usize,
    total: usize,
}

#[tauri::command]
pub fn memory_status(state: State<AppState>) -> Result<MemoryStatus, String> {
    let entry = catalog::embedding();
    let (mut pending, mut total) = (0, 0);
    if let Some(session) = state.session.lock().expect("session lock").as_ref() {
        let cache = session.cache.lock().expect("cache lock");
        let model = model_file().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        total = memory::total(&cache).unwrap_or(0);
        pending = if model.is_empty() { total } else { memory::pending(&cache, &model).unwrap_or(0) };
    }
    Ok(MemoryStatus {
        installed: model_file().is_some(),
        entry,
        download: state.memory_downloads.current().or_else(|| {
            let dir = models::models_dir()?;
            crate::model_download::leftover(&dl::part_path(&dir, entry), entry.size)
        }),
        pending,
        total,
    })
}

/// Starts (or resumes) the embedding model's download; done, the open session starts using it.
pub fn start_download(app: &AppHandle) -> Result<(), String> {
    let dir = models::models_dir().ok_or("LOCALAPPDATA is not set; PLA cannot store the model.")?;
    let entry = catalog::embedding();
    let (events, done_app) = (app.clone(), app.clone());
    app.state::<AppState>()
        .memory_downloads
        .start(
            move |cancel, report| {
                let req = dl::Request {
                    entry,
                    dir: &dir,
                    policy: &models::policy::HuggingFace,
                    free_space: &dl::free_space,
                    cancel,
                    progress_every: Duration::from_secs(1),
                    stall_after: Duration::from_secs(60),
                };
                dl::download(&req, report)
            },
            move |s| {
                let _ = events.emit("memory-download", s);
            },
            move |path| {
                let state = done_app.state::<AppState>();
                if let Some(session) = state.session.lock().expect("session lock").as_ref() {
                    *session.embeds.lock().expect("embeds lock") = host(&done_app, path);
                }
                let _ = done_app.emit("memory-changed", ());
            },
        )
        .map_err(|_| "download_running".to_owned())
}

#[tauri::command]
pub fn memory_download_start(app: AppHandle) -> Result<(), String> {
    start_download(&app)
}

#[tauri::command]
pub fn memory_download_pause(state: State<AppState>) {
    state.memory_downloads.pause();
}
