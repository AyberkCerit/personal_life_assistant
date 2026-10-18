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

fn now() -> chrono::DateTime<chrono::FixedOffset> {
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
    with_db(&state, |db| {
        let item = match kind.as_str() {
            "metric" => ItemRef::Metric(id.clone()),
            "routine" => ItemRef::Routine(id.clone()),
            _ => ItemRef::Task(id.clone()),
        };
        let tx = write_tx(db)?;
        reject_item(&tx, &item, now())?;
        tx.commit()?;
        Ok(())
    })
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
}
