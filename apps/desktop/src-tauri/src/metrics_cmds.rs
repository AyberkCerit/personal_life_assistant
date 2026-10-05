//! Tauri commands for the metrics view and the quick entry (SRS §3.8); the rules live in
//! `pla_core::metrics`.

use chrono::{Local, NaiveDate};
use pla_core::extraction::{MetricKind, ValidationSettings};
use pla_core::metrics::{self, Card, MetricError, MetricInput, MetricRecord, Summary};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::commands::AppState;

/// The periods the detail view offers (FR-MET-009); anything else is refused.
pub fn period(days: i64) -> Result<i64, String> {
    if matches!(days, 7 | 30 | 90) {
        Ok(days)
    } else {
        Err(format!("invalid_period|{days}|"))
    }
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn with_db<T>(state: &State<AppState>, f: impl FnOnce(&Connection) -> Result<T, MetricError>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault")?;
    let db = session.db.lock().expect("db lock");
    f(&db).map_err(|e| e.to_string())
}

fn changed(app: &AppHandle) {
    let _ = app.emit("metrics-changed", ());
}

#[tauri::command]
pub fn metrics_overview(state: State<AppState>) -> Result<Vec<Card>, String> {
    with_db(&state, |db| metrics::overview(db, today()))
}

#[tauri::command]
pub fn metrics_summary(state: State<AppState>, kind: MetricKind, days: i64) -> Result<Summary, String> {
    let days = period(days)?;
    with_db(&state, |db| metrics::summary(db, kind, days, today()))
}

#[tauri::command]
pub fn metric_records(state: State<AppState>, kind: MetricKind, days: i64) -> Result<Vec<MetricRecord>, String> {
    let days = period(days)?;
    let to = today();
    with_db(&state, |db| metrics::records(db, kind, to - chrono::Duration::days(days - 1), to))
}

/// FR-MET-003: errors are `out_of_range|<value>` (ask to confirm), `missing_value`, `empty_workout`
/// or `invalid_date`.
#[tauri::command]
pub fn metric_log(app: AppHandle, state: State<AppState>, input: MetricInput) -> Result<String, String> {
    let id = with_db(&state, |db| metrics::log_metric(db, &input, &ValidationSettings::default(), Local::now().fixed_offset()))?;
    changed(&app);
    Ok(id)
}

#[tauri::command]
pub fn metric_edit(app: AppHandle, state: State<AppState>, id: String, input: MetricInput) -> Result<(), String> {
    with_db(&state, |db| metrics::edit_metric(db, &id, &input, &ValidationSettings::default()))?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn metric_delete(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    with_db(&state, |db| metrics::delete_metric(db, &id, Local::now().fixed_offset()))?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn metric_resolve(app: AppHandle, state: State<AppState>, keep_id: String) -> Result<(), String> {
    with_db(&state, |db| metrics::resolve_conflict(db, &keep_id, Local::now().fixed_offset()))?;
    changed(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_offered_periods_are_accepted() {
        assert_eq!(period(30), Ok(30));
        assert!(period(0).is_err());
        assert!(period(100_000).is_err(), "a huge range would read the whole table");
    }
}
