//! A daily note's summary box (FR-MEM-006, owner decision B): show it, or ask for it again.

use pla_core::summary::{self, DaySummary};
use serde::Serialize;
use tauri::State;

use crate::commands::{with_session, AppState};
use crate::errors::{own_code, ue};
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
    with_session(&state, |session| {
        let Some(date) = pla_core::notes::daily_note_date(&path, &session.vault.config.folders) else { return Ok(None) };
        let found = summary::get(&session.cache.lock().expect("cache lock"), &session.vault.config.folders, date).map_err(ue)?;
        Ok(Some(DayInfo { date: date.format("%Y-%m-%d").to_string(), past: date < chrono::Local::now().date_naive(), summary: found }))
    })
}

/// "Summarise again": the day goes to the worker at once, not at the next maintenance.
#[tauri::command]
pub fn summary_regenerate(state: State<AppState>, path: String) -> Result<(), String> {
    with_session(&state, |session| {
        let date = pla_core::notes::daily_note_date(&path, &session.vault.config.folders).ok_or_else(|| own_code("bad_path", "", ""))?;
        session.worker.send(Command::Summarize { only: Some(date), cache_path: session.cache_path.clone() });
        Ok(())
    })
}
