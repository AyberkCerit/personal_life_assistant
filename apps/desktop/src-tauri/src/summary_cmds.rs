//! A daily note's summary box (FR-MEM-006, owner decision B): show it, or ask for it again.

use pla_core::summary::{self, DaySummary};
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;
use crate::worker::Command;

#[derive(Serialize)]
pub struct DayInfo {
    date: String,
    /// The day is over, so it can be summarised (today never is).
    past: bool,
    summary: Option<DaySummary>,
}

/// A daily note's day and its summary; `None` for any other note.
#[tauri::command]
pub fn day_summary(state: State<AppState>, path: String) -> Result<Option<DayInfo>, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let Some(date) = summary::daily_note_date(&session.vault.config.folders, &path) else { return Ok(None) };
    let cache = session.cache.lock().expect("cache lock");
    let found = summary::get(&cache, &session.vault.config.folders, date).map_err(|e| format!("io||{e}"))?;
    Ok(Some(DayInfo { date: date.format("%Y-%m-%d").to_string(), past: date < chrono::Local::now().date_naive(), summary: found }))
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
