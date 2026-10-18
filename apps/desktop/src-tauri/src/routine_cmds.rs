//! Tauri commands for the weekly routines (Rutinler); the rules live in `pla_core::routines`.

use chrono::Local;
use pla_core::routines::{self, RoutineError, RoutineInput, RoutineView};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{AppState, NO_VAULT};
use crate::errors::ue;

/// A routine error as a code the UI turns into a sentence in its language (like `task_code`).
pub fn routine_code(e: RoutineError) -> String {
    match e {
        RoutineError::BadTitle => "bad_title||".into(),
        RoutineError::NoDays => "no_days||".into(),
        RoutineError::BadTime(t) => format!("bad_time|{t}|"),
        RoutineError::NotFound => "not_found||".into(),
        RoutineError::Db(e) => ue(e),
    }
}

fn with_db<T>(state: &State<AppState>, f: impl FnOnce(&mut Connection) -> Result<T, RoutineError>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or(NO_VAULT)?;
    let mut db = session.db.lock().expect("db lock");
    f(&mut db).map_err(routine_code)
}

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    Local::now().fixed_offset()
}

#[tauri::command]
pub fn list_routines(state: State<AppState>) -> Result<Vec<RoutineView>, String> {
    with_db(&state, |db| routines::list_routines(db, Local::now().date_naive()))
}

#[tauri::command]
pub fn edit_routine(app: AppHandle, state: State<AppState>, id: String, input: RoutineInput) -> Result<(), String> {
    with_db(&state, |db| routines::edit_routine(db, &id, &input, Local::now().date_naive(), now()))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[tauri::command]
pub fn set_routine_paused(app: AppHandle, state: State<AppState>, id: String, paused: bool) -> Result<(), String> {
    with_db(&state, |db| routines::set_paused(db, &id, paused, Local::now().date_naive(), now()))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[tauri::command]
pub fn delete_routine(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    with_db(&state, |db| routines::delete_routine(db, &id, now()))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_errors_are_codes_the_ui_translates() {
        assert_eq!(routine_code(RoutineError::BadTitle), "bad_title||");
        assert_eq!(routine_code(RoutineError::NoDays), "no_days||");
        assert_eq!(routine_code(RoutineError::BadTime("25:00".into())), "bad_time|25:00|");
        assert_eq!(routine_code(RoutineError::NotFound), "not_found||");
        assert!(routine_code(RoutineError::Db(rusqlite::Error::InvalidQuery)).starts_with("db||"));
    }
}
