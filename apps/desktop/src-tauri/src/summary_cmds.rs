//! A daily note's summary box (FR-MEM-006, owner decision B): show it, or ask for it again.

use pla_core::summary::{self, DaySummary};
use tauri::State;

use crate::commands::AppState;
use crate::worker::Command;

/// The summary of the day a daily note is for; `None` for other notes or a day not summarised yet.
#[tauri::command]
pub fn day_summary(state: State<AppState>, path: String) -> Result<Option<DaySummary>, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let Some(date) = summary::daily_note_date(&session.vault.config.folders, &path) else { return Ok(None) };
    let cache = session.cache.lock().expect("cache lock");
    summary::get(&cache, &session.vault.config.folders, date).map_err(|e| format!("io||{e}"))
}

/// "Summarise again": the day goes to the worker at once, not at the next maintenance.
#[tauri::command]
pub fn summary_regenerate(state: State<AppState>, path: String) -> Result<(), String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let date = summary::daily_note_date(&session.vault.config.folders, &path).ok_or("bad_path||")?;
    session.worker.send(Command::Summarize { only: Some(date), cache_path: session.cache_path.clone() });
    Ok(())
}
