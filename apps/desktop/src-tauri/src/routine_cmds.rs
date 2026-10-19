//! Tauri commands for the weekly routines (Rutinler); the rules live in `pla_core::routines`.

use chrono::Local;
use pla_core::routines::{self, RoutineError, RoutineInput, RoutineView};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{in_write_tx, AppState};
use crate::errors::{own_code, ue};
use crate::task_cmds::now;

/// A routine error as a code the UI turns into a sentence in its language (like `task_code`).
pub fn routine_code(e: RoutineError) -> String {
    match e {
        RoutineError::BadTitle => own_code("bad_title", "", ""),
        RoutineError::NoDays => own_code("no_days", "", ""),
        RoutineError::BadTime(t) => own_code("bad_time", t, ""),
        RoutineError::NotFound => own_code("not_found", "", ""),
        RoutineError::Db(e) => ue(e),
    }
}

fn with_db<T>(state: &State<AppState>, f: impl FnOnce(&mut Connection) -> Result<T, RoutineError>) -> Result<T, String> {
    crate::commands::with_db(state, routine_code, f)
}

#[tauri::command]
pub fn list_routines(state: State<AppState>) -> Result<Vec<RoutineView>, String> {
    with_db(&state, |db| routines::list_routines(db, Local::now().date_naive()))
}

#[tauri::command]
pub fn edit_routine(app: AppHandle, state: State<AppState>, id: String, input: RoutineInput) -> Result<(), String> {
    with_db(&state, |db| in_write_tx(db, |tx| routines::edit_routine(tx, &id, &input, Local::now().date_naive(), now())))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[tauri::command]
pub fn set_routine_paused(app: AppHandle, state: State<AppState>, id: String, paused: bool) -> Result<(), String> {
    with_db(&state, |db| in_write_tx(db, |tx| routines::set_paused(tx, &id, paused, Local::now().date_naive(), now())))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[tauri::command]
pub fn delete_routine(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    with_db(&state, |db| in_write_tx(db, |tx| routines::delete_routine(tx, &id, now())))?;
    let _ = app.emit("tasks-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pla_core::db::open_databases;

    #[test]
    fn routine_errors_are_codes_the_ui_translates() {
        assert_eq!(routine_code(RoutineError::BadTitle), "bad_title||");
        assert_eq!(routine_code(RoutineError::NoDays), "no_days||");
        assert_eq!(routine_code(RoutineError::BadTime("25:00".into())), "bad_time|25:00|");
        assert_eq!(routine_code(RoutineError::NotFound), "not_found||");
        assert!(routine_code(RoutineError::Db(rusqlite::Error::InvalidQuery)).starts_with("db||"));
    }

    #[test]
    fn a_write_is_committed_and_visible_from_another_connection() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = open_databases(tmp.path()).unwrap().pla;
        db.execute("INSERT INTO routine (routine_id, title, days_json, origin, created_at, updated_at) VALUES ('r1', 'Spor', '[{\"day\":\"mon\"}]', 'extracted', 't', 't')", [])
            .unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        in_write_tx(&mut db, |tx| routines::set_paused(tx, "r1", true, today, now())).unwrap();
        let other = Connection::open(tmp.path().join("pla.db")).unwrap();
        let paused: bool = other.query_row("SELECT paused FROM routine WHERE routine_id = 'r1'", [], |r| r.get(0)).unwrap();
        assert!(paused, "committed, not left in an open transaction");
        let missing = in_write_tx(&mut db, |tx| routines::delete_routine(tx, "nope", now()));
        assert!(matches!(missing, Err(RoutineError::NotFound)));
    }
}
