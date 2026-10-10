//! The Q&A panel (FR-QA-001…016): the model answers from the user's notes and may call a small set
//! of tools; every turn is kept in pla.db (FR-QA-014) and never indexed (FR-MEM-014).

pub mod context;
pub mod engine;
pub mod examples;
pub mod lang;
pub mod tools;

use std::sync::atomic::AtomicBool;

use chrono::{DateTime, FixedOffset};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::llm::LlmError;

/// The model as the Q&A panel uses it: one whole (grammar-bound) answer, or an answer streamed
/// token by token. Both stop with `LlmError::Cancelled` soon after `cancel` is set (FR-QA-013).
pub trait ChatModel {
    fn complete(&mut self, body: &Value, cancel: &AtomicBool) -> Result<String, LlmError>;
    fn stream(&mut self, body: &Value, cancel: &AtomicBool, on_token: &mut dyn FnMut(&str)) -> Result<String, LlmError>;
}

/// What undoes one write of the assistant (FR-QA-008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Undo {
    /// `task` (delete it), `task_done` (open it again), `metric` (delete it) or `note` (to the bin).
    pub kind: String,
    pub id: String,
}

/// One tool call as the panel shows it and the history keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct ToolRecord {
    pub tool: String,
    pub args: Value,
    pub ok: bool,
    /// What the tool gave back (shown to the model and, for writes, to the user).
    pub result: Value,
    /// Why the call was refused (FR-QA-016), when it was.
    pub error: Option<String>,
    pub undo: Option<Undo>,
}

/// One stored turn (FR-QA-014).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Turn {
    pub turn_id: String,
    pub question: String,
    pub answer: String,
    pub tools: Vec<ToolRecord>,
    pub created_at: String,
    /// `done`, `stopped` or `failed`.
    pub status: String,
    pub new_topic: bool,
}

fn turn_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Turn> {
    let tools: Option<String> = r.get(3)?;
    Ok(Turn {
        turn_id: r.get(0)?,
        question: r.get(1)?,
        answer: r.get(2)?,
        tools: tools.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default(),
        created_at: r.get(4)?,
        status: r.get(5)?,
        new_topic: r.get(6)?,
    })
}

const TURN: &str = "SELECT turn_id, question, answer, tool_calls_json, created_at, status, new_topic FROM qa_turn";

/// FR-QA-014: keeps a finished (or stopped, or failed) turn.
pub fn save_turn(conn: &Connection, turn: &Turn) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO qa_turn (turn_id, question, answer, tool_calls_json, created_at, status, new_topic) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            turn.turn_id,
            turn.question,
            turn.answer,
            serde_json::to_string(&turn.tools).unwrap_or_else(|_| "[]".into()),
            turn.created_at,
            turn.status,
            turn.new_topic
        ],
    )?;
    Ok(())
}

/// The last `limit` turns, oldest first (the panel's history).
pub fn history(conn: &Connection, limit: usize) -> rusqlite::Result<Vec<Turn>> {
    let mut turns: Vec<Turn> =
        conn.prepare(&format!("{TURN} ORDER BY created_at DESC, rowid DESC LIMIT ?1"))?.query_map([limit as i64], turn_from_row)?.collect::<Result<_, _>>()?;
    turns.reverse();
    Ok(turns)
}

/// The turns the model sees again (decision A: up to 3), from the latest new topic on, oldest
/// first. Only finished answers: a stopped or failed one says nothing to build on.
pub fn follow_up(conn: &Connection, asking_new_topic: bool) -> rusqlite::Result<Vec<Turn>> {
    if asking_new_topic {
        return Ok(Vec::new());
    }
    let start: Option<String> =
        conn.query_row("SELECT max(created_at) FROM qa_turn WHERE new_topic = 1", [], |r| r.get(0)).optional()?.flatten();
    let mut turns: Vec<Turn> = conn
        .prepare(&format!("{TURN} WHERE status = 'done' AND created_at >= ?1 ORDER BY created_at DESC, rowid DESC LIMIT 3"))?
        .query_map([start.unwrap_or_default()], turn_from_row)?
        .collect::<Result<_, _>>()?;
    turns.reverse();
    Ok(turns)
}

/// The undo of tool call `index` in turn `turn_id`, still to be done. Once taken it is gone from the
/// stored turn, so a reopened panel does not offer it again (FR-QA-008).
pub fn take_undo(conn: &Connection, turn_id: &str, index: usize) -> rusqlite::Result<Option<Undo>> {
    let Some(mut turn) = conn.query_row(&format!("{TURN} WHERE turn_id = ?1"), [turn_id], turn_from_row).optional()? else { return Ok(None) };
    let Some(record) = turn.tools.get_mut(index) else { return Ok(None) };
    let Some(undo) = record.undo.take() else { return Ok(None) };
    record.result["undone"] = Value::Bool(true);
    conn.execute("UPDATE qa_turn SET tool_calls_json = ?2 WHERE turn_id = ?1", params![turn_id, serde_json::to_string(&turn.tools).unwrap_or_default()])?;
    Ok(Some(undo))
}

/// Puts an undo back when carrying it out failed, so the user can try again.
pub fn restore_undo(conn: &Connection, turn_id: &str, index: usize, undo: Undo) -> rusqlite::Result<()> {
    let Some(mut turn) = conn.query_row(&format!("{TURN} WHERE turn_id = ?1"), [turn_id], turn_from_row).optional()? else { return Ok(()) };
    if let Some(record) = turn.tools.get_mut(index) {
        record.undo = Some(undo);
        if let Some(o) = record.result.as_object_mut() {
            o.remove("undone");
        }
    }
    conn.execute("UPDATE qa_turn SET tool_calls_json = ?2 WHERE turn_id = ?1", params![turn_id, serde_json::to_string(&turn.tools).unwrap_or_default()])?;
    Ok(())
}

/// The owner's decision a: a suggested note is written only when the user presses Save. Writes it
/// into the notes once and keeps that in the stored turn, with its Undo. `Ok(None)`: no such
/// suggestion, or saved already.
pub fn save_suggestion(conn: &Connection, vault: &crate::vault::Vault, turn_id: &str, index: usize) -> Result<Option<String>, String> {
    let db = |e: rusqlite::Error| e.to_string();
    let Some(mut turn) = conn.query_row(&format!("{TURN} WHERE turn_id = ?1"), [turn_id], turn_from_row).optional().map_err(db)? else { return Ok(None) };
    let Some(record) = turn.tools.get_mut(index).filter(|r| r.tool == "suggest_note" && r.result["saved"].is_null()) else { return Ok(None) };
    let title = record.result["title"].as_str().unwrap_or_default().to_owned();
    let body = record.result["body"].as_str().unwrap_or_default().to_owned();
    let rel = tools::write_note(vault, &title, &body)?;
    record.result["saved"] = Value::String(rel.clone());
    record.undo = Some(Undo { kind: "note".into(), id: rel.clone() });
    conn.execute("UPDATE qa_turn SET tool_calls_json = ?2 WHERE turn_id = ?1", params![turn_id, serde_json::to_string(&turn.tools).unwrap_or_default()]).map_err(db)?;
    Ok(Some(rel))
}

/// FR-QA-015 (the UI asks first): every turn goes.
pub fn clear_history(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM qa_turn", [])
}

/// FR-QA-008: takes back one write of the assistant. A note is the caller's (it goes to the
/// Recycle Bin, which only the desktop shell can reach).
pub fn undo(conn: &Connection, undo: &Undo, now: DateTime<FixedOffset>) -> Result<(), String> {
    match undo.kind.as_str() {
        "task" => crate::tasks::delete_task(conn, &undo.id, now).map_err(|e| e.to_string()),
        "task_done" => crate::tasks::set_done(conn, &undo.id, false, now).map_err(|e| e.to_string()),
        "metric" => crate::metrics::delete_metric(conn, &undo.id, now).map_err(|e| e.to_string()),
        other => Err(format!("cannot undo {other} here")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = crate::db::open_databases(tmp.path()).unwrap().pla;
        (tmp, conn)
    }

    fn turn(id: &str, at: &str, status: &str, new_topic: bool) -> Turn {
        Turn {
            turn_id: id.into(),
            question: format!("soru {id}"),
            answer: format!("cevap {id}"),
            tools: Vec::new(),
            created_at: at.into(),
            status: status.into(),
            new_topic,
        }
    }

    #[test]
    fn turns_are_kept_and_cleared() {
        // FR-QA-014/015
        let (_t, conn) = db();
        save_turn(&conn, &turn("a", "2026-10-06T10:00:00+03:00", "done", false)).unwrap();
        save_turn(&conn, &turn("b", "2026-10-06T10:01:00+03:00", "stopped", false)).unwrap();
        let all = history(&conn, 50).unwrap();
        assert_eq!(all.iter().map(|t| t.turn_id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        // an undo is offered once
        let mut with_tool = turn("c", "2026-10-06T10:02:00+03:00", "done", false);
        with_tool.tools.push(ToolRecord {
            tool: "add_task".into(),
            args: Value::Null,
            ok: true,
            result: serde_json::json!({ "task_id": "x" }),
            error: None,
            undo: Some(Undo { kind: "task".into(), id: "x".into() }),
        });
        save_turn(&conn, &with_tool).unwrap();
        let undo = take_undo(&conn, "c", 0).unwrap().unwrap();
        assert_eq!(undo.id, "x");
        assert_eq!(take_undo(&conn, "c", 0).unwrap(), None);
        assert_eq!(history(&conn, 50).unwrap()[2].tools[0].result["undone"], true);
        restore_undo(&conn, "c", 0, undo).unwrap();
        assert!(take_undo(&conn, "c", 0).unwrap().is_some(), "a failed undo can be tried again");
        assert_eq!(clear_history(&conn).unwrap(), 3);
        assert!(history(&conn, 50).unwrap().is_empty());
    }

    #[test]
    fn follow_ups_see_the_last_three_answers_of_the_topic() {
        // decision A
        let (_t, conn) = db();
        for (i, (status, topic)) in [("done", false), ("done", true), ("done", false), ("failed", false), ("done", false), ("done", false)].iter().enumerate() {
            save_turn(&conn, &turn(&i.to_string(), &format!("2026-10-06T10:0{i}:00+03:00"), status, *topic)).unwrap();
        }
        let seen: Vec<String> = follow_up(&conn, false).unwrap().into_iter().map(|t| t.turn_id).collect();
        assert_eq!(seen, ["2", "4", "5"], "from the new topic on, finished ones only, at most three");
        assert!(follow_up(&conn, true).unwrap().is_empty(), "a new topic starts empty");
    }

    #[test]
    fn a_suggested_note_is_written_once_on_save() {
        // the owner's decision a: nothing is written until Save
        let (tmp, conn) = db();
        std::fs::create_dir_all(tmp.path().join("kasa")).unwrap();
        let vault = crate::vault::open_vault(&tmp.path().join("kasa")).unwrap();
        let mut t = turn("1", "2026-10-09T10:00:00+03:00", "done", false);
        t.tools = vec![ToolRecord {
            tool: "suggest_note".into(),
            args: serde_json::json!({}),
            ok: true,
            result: serde_json::json!({ "suggested": true, "title": "İşletim Sistemleri", "body": "Chapter 1" }),
            error: None,
            undo: None,
        }];
        save_turn(&conn, &t).unwrap();
        let rel = save_suggestion(&conn, &vault, "1", 0).unwrap().unwrap();
        assert_eq!(rel, "notes/İşletim Sistemleri.md");
        assert!(std::fs::read_to_string(vault.root.join(&rel)).unwrap().contains("Chapter 1"));
        assert_eq!(save_suggestion(&conn, &vault, "1", 0).unwrap(), None, "once");
        let stored = &history(&conn, 5).unwrap()[0].tools[0];
        assert_eq!((stored.result["saved"].as_str(), stored.undo.as_ref().map(|u| u.kind.as_str())), (Some(rel.as_str()), Some("note")));
    }
}
