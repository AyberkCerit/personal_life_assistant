//! Tauri commands for the task panel, the Review box and reminders (FR-TSK); the rules live in
//! `pla_core::tasks` and `pla_core::reminders`.

use chrono::Local;
use pla_core::pipeline::items::{reject_item, ItemRef};
use pla_core::reminders;
use pla_core::tasks::{self, ReviewView, TaskInput, TaskList, TaskView};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{in_write_tx, AppState};
use crate::errors::{own_code, ue};
use crate::scheduler::PendingLists;

/// A task error as a code the UI turns into a sentence in its language (architecture review A2).
pub fn task_code(e: tasks::TaskError) -> String {
    use tasks::TaskError;
    match e {
        TaskError::BadTitle => own_code("bad_title", "", ""),
        TaskError::BadDate(d) => own_code("bad_date", d, ""),
        TaskError::BadTime(t) => own_code("bad_time", t, ""),
        TaskError::NotFound => own_code("not_found", "", ""),
        TaskError::NotAnAction => own_code("not_an_action", "", ""),
        TaskError::Db(e) => ue(e),
    }
}

fn with_db<T>(state: &State<AppState>, f: impl FnOnce(&mut Connection) -> Result<T, tasks::TaskError>) -> Result<T, String> {
    crate::commands::with_db(state, task_code, f)
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
    with_db(&state, |db| in_write_tx(db, |tx| tasks::delete_task(tx, &id, now())))
}

/// Undo of an automatic addition (FR-EXT-015): removed and remembered as rejected (FR-EXT-018).
/// A memory line goes to the archive as turned down (assistant memory).
#[tauri::command]
pub fn undo_item(state: State<AppState>, kind: String, id: String) -> Result<(), String> {
    if kind == "memory" {
        let guard = state.session.lock().expect("session lock");
        let Some(session) = guard.as_ref() else { return Err(crate::commands::NO_VAULT.into()) };
        let db = session.db.lock().expect("db lock");
        return undo_memory(&session.vault, &db, &id, &state.self_writes).map_err(ue);
    }
    with_db(&state, |db| undo(db, &kind, &id))
}

fn undo_memory(vault: &pla_core::vault::Vault, db: &Connection, id: &str, own: &crate::watcher::SelfWrites) -> Result<(), pla_core::recall::RecallError> {
    let wrote = |rel: &str| {
        if let Ok(text) = std::fs::read_to_string(vault.root.join(rel)) {
            own.record_text(rel, &text);
        }
    };
    pla_core::recall::undo(vault, db, id, now(), crate::notify::ui_language(), &wrote).map(drop)
}

fn undo(db: &mut Connection, kind: &str, id: &str) -> Result<(), tasks::TaskError> {
    let item = match kind {
        "metric" => ItemRef::Metric(id.to_owned()),
        "routine" => ItemRef::Routine(id.to_owned()),
        _ => ItemRef::Task(id.to_owned()),
    };
    in_write_tx(db, |tx| reject_item(tx, &item, now()).map(drop).map_err(tasks::TaskError::from))
}

#[tauri::command]
pub fn list_review(state: State<AppState>) -> Result<Vec<ReviewView>, String> {
    with_db(&state, |db| tasks::list_review(db))
}

#[tauri::command]
pub fn accept_review(state: State<AppState>, id: String, input: TaskInput) -> Result<String, String> {
    with_db(&state, |db| in_write_tx(db, |tx| tasks::accept_review(tx, &id, &input, now())))
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
    use crate::commands::NO_VAULT;

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

    #[test]
    fn undoing_memory_rejects_it() {
        // assistant memory Task 9: the toast's Undo turns the line down, with its reason
        use pla_core::recall::{apply, Change, Kind, Proposal, Role};
        let tmp = tempfile::tempdir().unwrap();
        let vault = pla_core::vault::open_vault(tmp.path()).unwrap();
        let db = pla_core::db::open_databases(&tmp.path().join(".data")).unwrap().pla;
        let lang = crate::notify::ui_language();
        let said = Proposal { text: "Kahveyi sütsüz içer.".into(), topic: "kahve".into(), role: Role::About, person: None, kind: Kind::Said, source: None };
        let id = apply(&vault, &db, &[Change::Add(said)], now(), lang, &|_| {}).unwrap()[0].id.clone();
        let own = crate::watcher::SelfWrites::new();
        undo_memory(&vault, &db, &id, &own).unwrap();
        let (state, reason): (String, String) = db.query_row("SELECT state, reason FROM memory_entry WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((state.as_str(), reason.as_str()), ("rejected", "undone"));
        let memory = tmp.path().join("memory");
        let archive = std::fs::read_to_string(memory.join(Role::Archive.file_name(lang))).unwrap();
        let rejected = archive.split(&format!("## {}\n", if lang == pla_core::lang::Lang::Tr { "Reddedilenler" } else { "Rejected" })).nth(1).unwrap_or("");
        assert!(rejected.contains("Kahveyi sütsüz içer."), "{archive}");
        assert!(!std::fs::read_to_string(memory.join(Role::About.file_name(lang))).unwrap().contains("Kahveyi"));
        let rel = format!("memory/{}", Role::Archive.file_name(lang));
        assert!(own.is_own(&rel, &pla_core::util::content_hash(archive.as_bytes())), "the watcher hears PLA's own write");
    }
}
