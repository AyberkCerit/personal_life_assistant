//! Tauri commands for the task panel, the Review box and reminders (FR-TSK); the rules live in
//! `pla_core::tasks` and `pla_core::reminders`.

use chrono::Local;
use pla_core::pipeline::items::{reject_item, ItemRef};
use pla_core::pipeline::write_tx;
use pla_core::reminders;
use pla_core::tasks::{self, ReviewView, TaskInput, TaskList, TaskView};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{AppState, NO_VAULT};
use crate::scheduler::PendingLists;
use crate::errors::ue;

/// A task error as a code the UI turns into a sentence in its language (architecture review A2).
pub fn task_code(e: tasks::TaskError) -> String {
    use tasks::TaskError;
    match e {
        TaskError::BadTitle => "bad_title||".into(),
        TaskError::BadDate(d) => format!("bad_date|{d}|"),
        TaskError::BadTime(t) => format!("bad_time|{t}|"),
        TaskError::NotFound => "not_found||".into(),
        TaskError::NotAnAction => "not_an_action||".into(),
        TaskError::Db(e) => ue(e),
    }
}

fn with_db<T>(state: &State<AppState>, f: impl FnOnce(&mut Connection) -> Result<T, tasks::TaskError>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or(NO_VAULT)?;
    let mut db = session.db.lock().expect("db lock");
    f(&mut db).map_err(task_code)
}

pub(crate) fn now() -> chrono::DateTime<chrono::FixedOffset> {
    Local::now().fixed_offset()
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
    with_db(&state, |db| undo(db, &kind, &id))
}

fn undo(db: &mut Connection, kind: &str, id: &str) -> Result<(), tasks::TaskError> {
    let item = match kind {
        "metric" => ItemRef::Metric(id.to_owned()),
        "routine" => ItemRef::Routine(id.to_owned()),
        _ => ItemRef::Task(id.to_owned()),
    };
    let tx = write_tx(db)?;
    reject_item(&tx, &item, now())?;
    tx.commit()?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_errors_are_codes_the_ui_translates() {
        // architecture review A2: the task panel showed these as English sentences
        use pla_core::tasks::TaskError;
        assert_eq!(task_code(TaskError::BadTitle), "bad_title||");
        assert_eq!(task_code(TaskError::BadDate("31.02".into())), "bad_date|31.02|");
        assert_eq!(task_code(TaskError::BadTime("25:00".into())), "bad_time|25:00|");
        assert_eq!(task_code(TaskError::NotFound), "not_found||");
        assert_eq!(task_code(TaskError::NotAnAction), "not_an_action||");
        // a database failure is a general one, said by `error.db` (M6 translations)
        assert!(task_code(TaskError::Db(rusqlite::Error::InvalidQuery)).starts_with("db||"));
        assert_eq!(NO_VAULT, "no_vault||", "one spelling in every command module");
    }

    #[test]
    fn undoing_a_routine_removes_it_and_reopens_the_copies_it_took_over() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = pla_core::db::open_databases(tmp.path()).unwrap().pla;
        let at = "2026-10-26T10:00:00+03:00";
        db.execute_batch(&format!(
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES ('b1', 'n.md', 0, 'Spor', 'h', '{at}', '{at}');
             INSERT INTO routine (routine_id, title, days_json, origin, block_id, item_signature, created_at, updated_at)
               VALUES ('r1', 'Spor', '[{{\"day\":\"mon\",\"time\":null}}]', 'extracted', 'b1', 'routine:spor', '{at}', '{at}');
             INSERT INTO task (task_id, title, date, status, origin, block_id, item_signature, superseded_by, created_at, updated_at)
               VALUES ('copy', 'Spor', '2026-10-26', 'cancelled', 'extracted', 'b1', 'action:spor', 'r1', '{at}', '{at}');"
        ))
        .unwrap();
        undo(&mut db, "routine", "r1").unwrap();
        let routines: i64 = db.query_row("SELECT count(*) FROM routine", [], |r| r.get(0)).unwrap();
        let status: String = db.query_row("SELECT status FROM task WHERE task_id = 'copy'", [], |r| r.get(0)).unwrap();
        assert_eq!((routines, status.as_str()), (0, "open"));
    }
}
