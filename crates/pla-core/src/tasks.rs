//! The task panel's view of pla.db (FR-TSK-001…009, -017; FR-EXT-018, -023). Every change the user
//! makes marks the task as theirs, so extraction never changes or recreates it (FR-EXT-017).

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::extraction::{ItemType, RawItem};
use crate::pipeline::items::{action_signature, reject_item, ItemRef};

const MAX_TITLE_CHARS: usize = 200;
const COMPLETED_LIMIT: i64 = 200;
const DEFAULT_REMINDER_TIME: &str = "09:00";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskList {
    Today,
    Upcoming,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskView {
    pub task_id: String,
    pub title: String,
    pub details: Option<String>,
    pub date: Option<String>,
    pub time: Option<String>,
    pub notify_at: Option<String>,
    pub status: String,
    pub origin: String,
    pub note_path: Option<String>,
    pub block_text: Option<String>,
    pub source_missing: bool,
    pub user_modified: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct TaskInput {
    pub title: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    /// None keeps the current kind; Some(true) makes a reminder, Some(false) a plain task (decision 4).
    #[serde(default)]
    pub remind: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewView {
    pub review_id: String,
    pub reason: String,
    pub title: Option<String>,
    pub is_action: bool,
    pub payload_json: String,
    pub note_path: Option<String>,
    pub block_text: Option<String>,
    pub created_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("a task needs a title of 1 to 200 characters")]
    BadTitle,
    #[error("invalid date {0:?} (expected YYYY-MM-DD)")]
    BadDate(String),
    #[error("invalid time {0:?} (expected HH:MM)")]
    BadTime(String),
    #[error("this item no longer exists")]
    NotFound,
    #[error("only tasks and reminders can be accepted from the Review box")]
    NotAnAction,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

struct Clean {
    title: String,
    details: Option<String>,
    date: Option<String>,
    time: Option<String>,
    remind: Option<bool>,
}

fn clean(input: &TaskInput) -> Result<Clean, TaskError> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(TaskError::BadTitle);
    }
    let present = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    // Stored in canonical form: dates are compared as text and notify_at is parsed later.
    let date = present(&input.date)
        .map(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").map(|p| p.format("%Y-%m-%d").to_string()).map_err(|_| TaskError::BadDate(d)))
        .transpose()?;
    let time = present(&input.time)
        .map(|t| NaiveTime::parse_from_str(&t, "%H:%M").map(|p| p.format("%H:%M").to_string()).map_err(|_| TaskError::BadTime(t)))
        .transpose()?;
    Ok(Clean { title: title.to_owned(), details: present(&input.details), date, time, remind: input.remind })
}

/// Reminder time for a date and optional time (F3 rule: 09:00 when there is no time).
fn notify_for(date: &Option<String>, time: &Option<String>) -> Option<String> {
    date.as_ref().map(|d| format!("{d}T{}", time.as_deref().unwrap_or(DEFAULT_REMINDER_TIME)))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

const SELECT: &str = "SELECT t.task_id, t.title, t.details, t.date, t.time, t.notify_at, t.status, t.origin,
                             b.note_path, b.text, t.source_missing, t.user_modified
                      FROM task t LEFT JOIN block b ON b.block_id = t.block_id";

fn task_row(r: &rusqlite::Row) -> rusqlite::Result<TaskView> {
    Ok(TaskView {
        task_id: r.get(0)?,
        title: r.get(1)?,
        details: r.get(2)?,
        date: r.get(3)?,
        time: r.get(4)?,
        notify_at: r.get(5)?,
        status: r.get(6)?,
        origin: r.get(7)?,
        note_path: r.get(8)?,
        block_text: r.get(9)?,
        source_missing: r.get(10)?,
        user_modified: r.get(11)?,
    })
}

pub fn list_tasks(conn: &Connection, list: TaskList, today: NaiveDate) -> Result<Vec<TaskView>, TaskError> {
    let today = today.format("%Y-%m-%d").to_string();
    let tasks = match list {
        TaskList::Today => conn
            .prepare(&format!(
                "{SELECT} WHERE t.status = 'open' AND t.date IS NOT NULL AND t.date <= ?1
                 ORDER BY t.date, t.time IS NULL, t.time, t.created_at"
            ))?
            .query_map([today], task_row)?
            .collect::<Result<_, _>>()?,
        TaskList::Upcoming => conn
            .prepare(&format!(
                "{SELECT} WHERE t.status = 'open' AND (t.date IS NULL OR t.date > ?1)
                 ORDER BY t.date IS NULL, t.date, t.time IS NULL, t.time, t.created_at"
            ))?
            .query_map([today], task_row)?
            .collect::<Result<_, _>>()?,
        TaskList::Completed => conn
            .prepare(&format!("{SELECT} WHERE t.status = 'done' ORDER BY t.completed_at DESC LIMIT {COMPLETED_LIMIT}"))?
            .query_map([], task_row)?
            .collect::<Result<_, _>>()?,
    };
    Ok(tasks)
}

pub fn add_task(conn: &Connection, input: &TaskInput, now: DateTime<FixedOffset>) -> Result<String, TaskError> {
    add_task_as(conn, input, "manual", now)
}

/// `add_task` with its origin: `manual`, or `assistant` for the Q&A panel's add_task (FR-QA-008).
pub fn add_task_as(conn: &Connection, input: &TaskInput, origin: &str, now: DateTime<FixedOffset>) -> Result<String, TaskError> {
    let c = clean(input)?;
    let id = new_id();
    let notify_at = if c.remind == Some(true) { notify_for(&c.date, &c.time) } else { None };
    conn.execute(
        "INSERT INTO task (task_id, title, details, date, time, notify_at, status, origin, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', ?8, ?7, ?7)",
        params![id, c.title, c.details, c.date, c.time, notify_at, now.to_rfc3339(), origin],
    )?;
    Ok(id)
}

pub fn edit_task(conn: &Connection, id: &str, input: &TaskInput, now: DateTime<FixedOffset>) -> Result<(), TaskError> {
    let c = clean(input)?;
    type Stored = (Option<String>, Option<String>, Option<String>);
    let current: Option<Stored> = conn
        .query_row("SELECT notify_at, date, time FROM task WHERE task_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let Some((stored, date, time)) = current else { return Err(TaskError::NotFound) };
    let is_reminder = c.remind.unwrap_or(stored.is_some());
    let notify_at = match stored {
        _ if !is_reminder => None,
        // Same day and time: keep the stored moment, which may be a snooze (final review I2).
        Some(at) if date == c.date && time == c.time => Some(at),
        _ => notify_for(&c.date, &c.time),
    };
    conn.execute(
        "UPDATE task SET title = ?1, details = ?2, date = ?3, time = ?4,
                notified_at = CASE WHEN notify_at IS ?5 THEN notified_at ELSE NULL END, notify_at = ?5, user_modified = 1, updated_at = ?6
         WHERE task_id = ?7",
        params![c.title, c.details, c.date, c.time, notify_at, now.to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn set_done(conn: &Connection, id: &str, done: bool, now: DateTime<FixedOffset>) -> Result<(), TaskError> {
    let now = now.to_rfc3339();
    let changed = conn.execute(
        "UPDATE task SET status = ?1, completed_at = ?2, user_modified = 1, updated_at = ?3 WHERE task_id = ?4",
        params![if done { "done" } else { "open" }, done.then(|| now.clone()), now, id],
    )?;
    if changed == 0 {
        return Err(TaskError::NotFound);
    }
    Ok(())
}

/// FR-TSK-006 (the UI confirms first). An extracted task is rejected so it never comes back (FR-EXT-018).
pub fn delete_task(conn: &Connection, id: &str, now: DateTime<FixedOffset>) -> Result<(), TaskError> {
    let origin: Option<String> = conn.query_row("SELECT origin FROM task WHERE task_id = ?1", [id], |r| r.get(0)).optional()?;
    match origin.as_deref() {
        None => Err(TaskError::NotFound),
        Some("extracted") => {
            reject_item(conn, &ItemRef::Task(id.to_owned()), now)?;
            Ok(())
        }
        Some(_) => {
            conn.execute("DELETE FROM task WHERE task_id = ?1", [id])?;
            Ok(())
        }
    }
}

pub fn list_review(conn: &Connection) -> Result<Vec<ReviewView>, TaskError> {
    let items = conn
        .prepare(
            "SELECT r.review_id, r.reason, r.payload_json, r.created_at, b.note_path, b.text
             FROM review_item r LEFT JOIN block b ON b.block_id = r.block_id
             WHERE r.resolved = 0 ORDER BY r.created_at DESC, r.review_id",
        )?
        .query_map([], |r| {
            let payload: String = r.get(2)?;
            let raw: Option<RawItem> = serde_json::from_str(&payload).ok();
            Ok(ReviewView {
                review_id: r.get(0)?,
                reason: r.get(1)?,
                title: raw.as_ref().and_then(|i| i.title.clone()),
                is_action: raw.as_ref().is_some_and(|i| i.kind != ItemType::Metric),
                payload_json: payload,
                created_at: r.get(3)?,
                note_path: r.get(4)?,
                block_text: r.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(items)
}

/// FR-TSK-009: the user corrected the item; it becomes their task, linked to its source block.
pub fn accept_review(conn: &Connection, review_id: &str, input: &TaskInput, now: DateTime<FixedOffset>) -> Result<String, TaskError> {
    let c = clean(input)?;
    let found: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT block_id, payload_json FROM review_item WHERE review_id = ?1 AND resolved = 0",
            [review_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((block_id, payload)) = found else { return Err(TaskError::NotFound) };
    let raw = serde_json::from_str::<RawItem>(&payload).map_err(|_| TaskError::NotAnAction)?;
    let kind = raw.kind;
    if kind == ItemType::Metric {
        return Err(TaskError::NotAnAction);
    }
    let notify_at = if kind == ItemType::Reminder { notify_for(&c.date, &c.time) } else { None };
    let id = new_id();
    conn.execute(
        "INSERT INTO task (task_id, title, details, date, time, notify_at, status, origin, block_id, item_signature,
                           user_modified, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', 'extracted', ?7, ?8, 1, ?9, ?9)",
        params![
            id,
            c.title,
            c.details,
            c.date,
            c.time,
            notify_at,
            block_id,
            // the model's wording, so a later valid extraction of the same item is recognised
            action_signature(raw.title.as_deref().unwrap_or(&c.title)),
            now.to_rfc3339()
        ],
    )?;
    conn.execute("UPDATE review_item SET resolved = 1 WHERE review_id = ?1", [review_id])?;
    Ok(id)
}

pub fn reject_review(conn: &Connection, review_id: &str) -> Result<(), TaskError> {
    let changed = conn.execute("UPDATE review_item SET resolved = 1 WHERE review_id = ?1 AND resolved = 0", [review_id])?;
    if changed == 0 {
        return Err(TaskError::NotFound);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;
    use crate::extraction::{parse_extraction, ValidationSettings};
    use crate::pipeline::items::{apply_block_items, Outcome};

    const NOW: &str = "2026-10-06T10:00:00+03:00";
    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(NOW).unwrap()
    }
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()
    }
    fn input(title: &str, date: Option<&str>, time: Option<&str>) -> TaskInput {
        TaskInput { title: title.into(), details: None, date: date.map(Into::into), time: time.map(Into::into), remind: None }
    }
    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        conn.execute(
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES ('b1', 'daily/2026/2026-10-06.md', 0, 'Yarın 9da dişçi', 'h', ?1, ?1)",
            [NOW],
        )
        .unwrap();
        (tmp, conn)
    }
    fn extract(conn: &Connection, json: &str) -> Vec<Outcome> {
        let ex = parse_extraction(&format!(r#"{{"items": [{json}]}}"#)).unwrap();
        apply_block_items(conn, "b1", today(), &ex, &ValidationSettings::default(), None, now()).unwrap()
    }
    fn titles(list: &[TaskView]) -> Vec<&str> {
        list.iter().map(|t| t.title.as_str()).collect()
    }

    #[test]
    fn lists_today_with_overdue_upcoming_with_undated_last_and_completed() {
        let (_t, conn) = setup();
        add_task(&conn, &input("Dün kalan", Some("2026-10-05"), None), now()).unwrap();
        add_task(&conn, &input("Bugün öğlen", Some("2026-10-06"), Some("12:00")), now()).unwrap();
        add_task(&conn, &input("Bugün sabah", Some("2026-10-06"), Some("08:00")), now()).unwrap();
        add_task(&conn, &input("Tarihsiz", None, None), now()).unwrap();
        add_task(&conn, &input("Cuma", Some("2026-10-09"), None), now()).unwrap();
        let done = add_task(&conn, &input("Bitti", Some("2026-10-06"), None), now()).unwrap();
        set_done(&conn, &done, true, now()).unwrap();

        assert_eq!(titles(&list_tasks(&conn, TaskList::Today, today()).unwrap()), vec!["Dün kalan", "Bugün sabah", "Bugün öğlen"]);
        assert_eq!(titles(&list_tasks(&conn, TaskList::Upcoming, today()).unwrap()), vec!["Cuma", "Tarihsiz"]);
        assert_eq!(titles(&list_tasks(&conn, TaskList::Completed, today()).unwrap()), vec!["Bitti"]);
    }

    #[test]
    fn invalid_input_is_refused_and_changes_nothing() {
        // Review Focus 4
        let (_t, conn) = setup();
        assert!(matches!(add_task(&conn, &input("   ", None, None), now()), Err(TaskError::BadTitle)));
        assert!(matches!(add_task(&conn, &input(&"x".repeat(201), None, None), now()), Err(TaskError::BadTitle)));
        assert!(matches!(add_task(&conn, &input("a", Some("2026-02-30"), None), now()), Err(TaskError::BadDate(_))));
        assert!(matches!(add_task(&conn, &input("a", None, Some("25:00")), now()), Err(TaskError::BadTime(_))));
        assert!(list_tasks(&conn, TaskList::Upcoming, today()).unwrap().is_empty());
        let id = add_task(&conn, &input("Doğru", Some(""), Some("")), now()).unwrap();
        assert!(matches!(edit_task(&conn, &id, &input("", None, None), now()), Err(TaskError::BadTitle)));
        assert_eq!(titles(&list_tasks(&conn, TaskList::Upcoming, today()).unwrap()), vec!["Doğru"]);
        assert!(matches!(edit_task(&conn, "yok", &input("a", None, None), now()), Err(TaskError::NotFound)));
    }

    #[test]
    fn user_changes_make_the_task_theirs() {
        // Review Focus 1: FR-EXT-017
        let (_t, conn) = setup();
        let Outcome::Added(ItemRef::Task(id)) = extract(&conn, r#"{"type":"task","title":"Dişçi","when":{"day_offset":1}}"#).remove(0) else {
            panic!("expected an added task")
        };
        edit_task(&conn, &id, &input("Dişçi randevusu", Some("2026-10-08"), Some("10:00")), now()).unwrap();
        assert!(matches!(&extract(&conn, r#"{"type":"task","title":"Dişçi","when":{"day_offset":1}}"#)[..], [Outcome::KeptUserVersion(_)]));
        let t = &list_tasks(&conn, TaskList::Upcoming, today()).unwrap()[0];
        assert_eq!((t.title.as_str(), t.date.as_deref(), t.user_modified), ("Dişçi randevusu", Some("2026-10-08"), true));
        assert_eq!(t.note_path.as_deref(), Some("daily/2026/2026-10-06.md"), "FR-EXT-023 source link");
        assert_eq!(t.block_text.as_deref(), Some("Yarın 9da dişçi"));
    }

    #[test]
    fn completing_and_reopening() {
        let (_t, conn) = setup();
        let id = add_task(&conn, &input("Fatura", Some("2026-10-06"), None), now()).unwrap();
        set_done(&conn, &id, true, now()).unwrap();
        let done = &list_tasks(&conn, TaskList::Completed, today()).unwrap()[0];
        assert_eq!(done.status, "done");
        set_done(&conn, &id, false, now()).unwrap();
        assert_eq!(titles(&list_tasks(&conn, TaskList::Today, today()).unwrap()), vec!["Fatura"]);
        assert!(matches!(set_done(&conn, "yok", true, now()), Err(TaskError::NotFound)));
    }

    #[test]
    fn editing_a_reminder_moves_its_notification() {
        let (_t, conn) = setup();
        extract(&conn, r#"{"type":"reminder","title":"İlaç","when":{"day_offset":0,"time":"21:30"}}"#);
        let id = list_tasks(&conn, TaskList::Today, today()).unwrap()[0].task_id.clone();
        edit_task(&conn, &id, &input("İlaç", Some("2026-10-07"), None), now()).unwrap();
        let t = &list_tasks(&conn, TaskList::Upcoming, today()).unwrap()[0];
        assert_eq!(t.notify_at.as_deref(), Some("2026-10-07T09:00"));
        let manual = add_task(&conn, &input("Normal", Some("2026-10-07"), Some("10:00")), now()).unwrap();
        edit_task(&conn, &manual, &input("Normal", Some("2026-10-08"), Some("10:00")), now()).unwrap();
        let normal = list_tasks(&conn, TaskList::Upcoming, today()).unwrap().into_iter().find(|t| t.title == "Normal").unwrap();
        assert_eq!(normal.notify_at, None, "a plain task does not become a reminder");
    }

    #[test]
    fn deleting_an_extracted_task_remembers_the_rejection() {
        // Review Focus 1/2: FR-EXT-018
        let (_t, conn) = setup();
        extract(&conn, r#"{"type":"task","title":"Dişçi","when":{"day_offset":1}}"#);
        let id = list_tasks(&conn, TaskList::Upcoming, today()).unwrap()[0].task_id.clone();
        delete_task(&conn, &id, now()).unwrap();
        assert!(matches!(&extract(&conn, r#"{"type":"task","title":"Dişçi","when":{"day_offset":1}}"#)[..], [Outcome::Rejected { .. }]));
        assert!(list_tasks(&conn, TaskList::Upcoming, today()).unwrap().is_empty());
        let manual = add_task(&conn, &input("Elle", None, None), now()).unwrap();
        delete_task(&conn, &manual, now()).unwrap();
        assert!(matches!(delete_task(&conn, &manual, now()), Err(TaskError::NotFound)));
    }

    #[test]
    fn review_items_can_be_corrected_and_accepted_or_dismissed() {
        // Review Focus 3, decision 3
        let (_t, conn) = setup();
        extract(&conn, r#"{"type":"reminder","title":"Annemi ara"}"#); // reminder without a date → Review
        extract(&conn, r#"{"type":"metric","metric":{"kind":"sleep","value":30,"unit":"h"}}"#);
        let review = list_review(&conn).unwrap();
        assert_eq!(review.len(), 2);
        let call = review.iter().find(|r| r.is_action).unwrap();
        assert_eq!(call.title.as_deref(), Some("Annemi ara"));
        assert_eq!(call.note_path.as_deref(), Some("daily/2026/2026-10-06.md"));
        let sleep = review.iter().find(|r| !r.is_action).unwrap();
        assert!(matches!(accept_review(&conn, &sleep.review_id, &input("x", None, None), now()), Err(TaskError::NotAnAction)));

        let id = accept_review(&conn, &call.review_id, &input("Annemi ara", Some("2026-10-07"), Some("19:00")), now()).unwrap();
        let t = list_tasks(&conn, TaskList::Upcoming, today()).unwrap().into_iter().find(|t| t.task_id == id).unwrap();
        assert_eq!((t.origin.as_str(), t.user_modified, t.notify_at.as_deref()), ("extracted", true, Some("2026-10-07T19:00")));
        reject_review(&conn, &sleep.review_id).unwrap();
        assert!(list_review(&conn).unwrap().is_empty());
        assert!(matches!(&extract(&conn, r#"{"type":"reminder","title":"Annemi ara"}"#)[..], [Outcome::AlreadyReviewed]));
        assert!(matches!(reject_review(&conn, &sleep.review_id), Err(TaskError::NotFound)));
    }

    #[test]
    fn an_accepted_review_item_with_a_new_title_is_not_extracted_again() {
        // Final review I1: the signature must follow the model's title, not the corrected one
        let (_t, conn) = setup();
        extract(&conn, r#"{"type":"reminder","title":"Annemi ara"}"#);
        let review = list_review(&conn).unwrap();
        accept_review(&conn, &review[0].review_id, &input("Doğum günü araması", Some("2026-10-07"), None), now()).unwrap();
        let out = extract(&conn, r#"{"type":"task","title":"Annemi ara","when":{"day_offset":1}}"#);
        assert!(matches!(&out[..], [Outcome::KeptUserVersion(_)]), "{out:?}");
        let all: i64 = conn.query_row("SELECT COUNT(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(all, 1);
    }

    #[test]
    fn dates_and_times_are_stored_in_canonical_form() {
        // Final review I2: text ordering and notify_at parsing rely on YYYY-MM-DD / HH:MM
        let (_t, conn) = setup();
        let id = add_task(&conn, &input("Kısa", Some("2026-1-5"), Some("9:5")), now()).unwrap();
        let (date, time): (String, String) =
            conn.query_row("SELECT date, time FROM task WHERE task_id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((date.as_str(), time.as_str()), ("2026-01-05", "09:05"));
    }
}
