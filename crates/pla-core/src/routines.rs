//! Weekly routines (spec 2026-10-18): a routine repeats on chosen weekdays and each due day is one task row.
//! Weeks are ISO weeks, Monday to Sunday, as in `report.rs`. A routine the user edits is theirs (`user_modified`).

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::extraction::Weekday;
use crate::pipeline::items::{reject_item, ItemRef};
use crate::util::new_id;

const MAX_TITLE_CHARS: usize = 200;

/// One day of a routine, optionally with a time of day (`HH:MM`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineDay {
    pub day: Weekday,
    #[serde(default)]
    pub time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoutineInput {
    pub title: String,
    pub days: Vec<RoutineDay>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoutineView {
    pub routine_id: String,
    pub title: String,
    pub days: Vec<RoutineDay>,
    pub paused: bool,
    pub origin: String,
    pub note_path: Option<String>,
    pub block_text: Option<String>,
    pub source_missing: bool,
    pub user_modified: bool,
    /// Occurrences done in the ISO week of `today`.
    pub week_done: u32,
    /// Days the routine has per week.
    pub week_due: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum RoutineError {
    #[error("a routine needs a title of 1 to 200 characters")]
    BadTitle,
    #[error("a routine needs at least one day")]
    NoDays,
    #[error("invalid time {0:?} (expected HH:MM)")]
    BadTime(String),
    #[error("this routine no longer exists")]
    NotFound,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// Trims the title and puts the days in week order with canonical times; a repeated day keeps its last entry.
fn clean(input: &RoutineInput) -> Result<(String, Vec<RoutineDay>), RoutineError> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(RoutineError::BadTitle);
    }
    if input.days.is_empty() {
        return Err(RoutineError::NoDays);
    }
    let mut days: Vec<RoutineDay> = Vec::new();
    for d in &input.days {
        let time = d
            .time
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| NaiveTime::parse_from_str(t, "%H:%M").map(|p| p.format("%H:%M").to_string()).map_err(|_| RoutineError::BadTime(t.to_owned())))
            .transpose()?;
        days.retain(|e| e.day != d.day);
        days.push(RoutineDay { day: d.day, time });
    }
    days.sort_by_key(|d| d.day.num_days_from_monday());
    Ok((title.to_owned(), days))
}

/// Inserts an extracted routine; the caller has validated `days` (see `clean`).
#[cfg_attr(not(test), allow(dead_code))] // used by extraction once routines are extracted
pub(crate) fn insert_routine(
    conn: &Connection,
    title: &str,
    days: &[RoutineDay],
    block_id: Option<&str>,
    signature: Option<&str>,
    now: &str,
) -> rusqlite::Result<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO routine (routine_id, title, days_json, origin, block_id, item_signature, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'extracted', ?4, ?5, ?6, ?6)",
        params![id, title.chars().take(MAX_TITLE_CHARS).collect::<String>(), serde_json::to_string(days).expect("days serialize"), block_id, signature, now],
    )?;
    Ok(id)
}

/// Creates or tops up the open occurrence of a routine (implemented with occurrence rolling).
pub(crate) fn roll_one(_conn: &Connection, _routine_id: &str, _today: NaiveDate, _now: DateTime<FixedOffset>) -> rusqlite::Result<bool> {
    Ok(false)
}

pub fn list_routines(conn: &Connection, today: NaiveDate) -> Result<Vec<RoutineView>, RoutineError> {
    let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
    let (from, to) = (monday.format("%Y-%m-%d").to_string(), (monday + Duration::days(6)).format("%Y-%m-%d").to_string());
    let rows: Vec<(RoutineView, String)> = conn
        .prepare(
            "SELECT r.routine_id, r.title, r.days_json, r.paused, r.origin, b.note_path, b.text, r.source_missing, r.user_modified,
                    (SELECT count(*) FROM task t WHERE t.routine_id = r.routine_id AND t.status = 'done' AND t.date BETWEEN ?1 AND ?2)
             FROM routine r LEFT JOIN block b ON b.block_id = r.block_id
             ORDER BY r.title, r.routine_id",
        )?
        .query_map([&from, &to], |r| {
            Ok((
                RoutineView {
                    routine_id: r.get(0)?,
                    title: r.get(1)?,
                    days: Vec::new(),
                    paused: r.get(3)?,
                    origin: r.get(4)?,
                    note_path: r.get(5)?,
                    block_text: r.get(6)?,
                    source_missing: r.get(7)?,
                    user_modified: r.get(8)?,
                    week_done: r.get(9)?,
                    week_due: 0,
                },
                r.get(2)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(mut view, days_json)| {
            view.days = serde_json::from_str(&days_json).unwrap_or_default();
            view.week_due = view.days.len() as u32;
            view
        })
        .collect())
}

pub fn edit_routine(conn: &Connection, id: &str, input: &RoutineInput, today: NaiveDate, now: DateTime<FixedOffset>) -> Result<(), RoutineError> {
    let (title, days) = clean(input)?;
    let changed = conn.execute(
        "UPDATE routine SET title = ?1, days_json = ?2, user_modified = 1, updated_at = ?3 WHERE routine_id = ?4",
        params![title, serde_json::to_string(&days).expect("days serialize"), now.to_rfc3339(), id],
    )?;
    if changed == 0 {
        return Err(RoutineError::NotFound);
    }
    roll_one(conn, id, today, now)?;
    Ok(())
}

pub fn set_paused(conn: &Connection, id: &str, paused: bool, today: NaiveDate, now: DateTime<FixedOffset>) -> Result<(), RoutineError> {
    let changed =
        conn.execute("UPDATE routine SET paused = ?1, user_modified = 1, updated_at = ?2 WHERE routine_id = ?3", params![paused, now.to_rfc3339(), id])?;
    if changed == 0 {
        return Err(RoutineError::NotFound);
    }
    roll_one(conn, id, today, now)?;
    Ok(())
}

/// The UI confirms first. The routine is remembered as rejected so extraction does not bring it back.
pub fn delete_routine(conn: &Connection, id: &str, now: DateTime<FixedOffset>) -> Result<(), RoutineError> {
    let known: Option<i64> = conn.query_row("SELECT 1 FROM routine WHERE routine_id = ?1", [id], |r| r.get(0)).optional()?;
    if known.is_none() || !reject_item(conn, &ItemRef::Routine(id.to_owned()), now)? {
        return Err(RoutineError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;

    const NOW: &str = "2026-10-26T10:00:00+03:00";
    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(NOW).unwrap()
    }
    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        conn.execute(
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES ('b1', 'n.md', 0, 'Spor', 'h', ?1, ?1)",
            [NOW],
        )
        .unwrap();
        (tmp, conn)
    }
    fn day(day: Weekday, time: &str) -> RoutineDay {
        RoutineDay { day, time: Some(time.into()) }
    }
    fn spor_days() -> Vec<RoutineDay> {
        use Weekday::*;
        vec![day(Mon, "20:00"), day(Tue, "20:00"), day(Wed, "20:00"), day(Thu, "20:00"), day(Fri, "20:00")]
    }
    fn input(title: &str, days: &[(&str, Option<&str>)]) -> RoutineInput {
        let days = days.iter().map(|(d, t)| serde_json::from_value(serde_json::json!({"day": d, "time": t})).unwrap()).collect();
        RoutineInput { title: title.into(), days }
    }
    fn done_occurrence(conn: &Connection, routine_id: &str, date: &str) {
        conn.execute(
            "INSERT INTO task (task_id, title, date, status, origin, routine_id, created_at, updated_at)
             VALUES (?1, 'Spor', ?2, 'done', 'extracted', ?3, ?4, ?4)",
            params![new_id(), date, routine_id, NOW],
        )
        .unwrap();
    }
    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_routine_lists_with_this_weeks_count() {
        // 2026-10-26 is a Monday; the ISO week is 26 Oct – 1 Nov.
        let (_t, conn) = setup();
        let id = insert_routine(&conn, "Spor", &spor_days(), None, None, NOW).unwrap();
        done_occurrence(&conn, &id, "2026-10-26");
        done_occurrence(&conn, &id, "2026-10-25"); // previous week: not counted
        let v = &list_routines(&conn, d("2026-10-26")).unwrap()[0];
        assert_eq!((v.title.as_str(), v.week_done, v.week_due, v.paused), ("Spor", 1, 5, false));
        assert_eq!(v.days[0], RoutineDay { day: Weekday::Mon, time: Some("20:00".into()) });
    }

    #[test]
    fn editing_validates_and_marks_the_routine_as_the_users() {
        let (_t, conn) = setup();
        let id = insert_routine(&conn, "Spor", &spor_days(), None, None, NOW).unwrap();
        let today = d("2026-10-26");
        assert!(matches!(edit_routine(&conn, &id, &input("", &[("mon", Some("20:00"))]), today, now()), Err(RoutineError::BadTitle)));
        assert!(matches!(edit_routine(&conn, &id, &input("Spor", &[]), today, now()), Err(RoutineError::NoDays)));
        assert!(matches!(edit_routine(&conn, &id, &input("Spor", &[("mon", Some("25:00"))]), today, now()), Err(RoutineError::BadTime(_))));
        assert!(matches!(edit_routine(&conn, "yok", &input("Spor", &[("mon", None)]), today, now()), Err(RoutineError::NotFound)));
        edit_routine(&conn, &id, &input("Koşu", &[("fri", Some("18:00")), ("mon", Some("8:05"))]), today, now()).unwrap();
        let v = &list_routines(&conn, today).unwrap()[0];
        assert_eq!(v.title, "Koşu");
        assert!(v.user_modified);
        // Days are stored in week order with canonical times; a repeated day keeps the last one.
        assert_eq!(
            v.days.iter().map(|d| (d.day, d.time.clone().unwrap())).collect::<Vec<_>>(),
            [(Weekday::Mon, "08:05".into()), (Weekday::Fri, "18:00".into())]
        );
        edit_routine(&conn, &id, &input("Koşu", &[("mon", Some("07:00")), ("mon", None)]), today, now()).unwrap();
        assert_eq!(list_routines(&conn, today).unwrap()[0].days, [RoutineDay { day: Weekday::Mon, time: None }]);
    }

    #[test]
    fn pausing_marks_the_routine_as_the_users() {
        let (_t, conn) = setup();
        let id = insert_routine(&conn, "Spor", &spor_days(), None, None, NOW).unwrap();
        set_paused(&conn, &id, true, d("2026-10-26"), now()).unwrap();
        let v = &list_routines(&conn, d("2026-10-26")).unwrap()[0];
        assert!(v.paused && v.user_modified);
        assert!(matches!(set_paused(&conn, "yok", true, d("2026-10-26"), now()), Err(RoutineError::NotFound)));
    }

    #[test]
    fn deleting_an_extracted_routine_remembers_the_rejection_and_keeps_history() {
        let (_t, conn) = setup();
        let id = insert_routine(&conn, "Spor", &spor_days(), Some("b1"), Some("routine:spor"), NOW).unwrap();
        done_occurrence(&conn, &id, "2026-10-26");
        conn.execute(
            "INSERT INTO task (task_id, title, date, status, origin, routine_id, created_at, updated_at) VALUES ('open1', 'Spor', '2026-10-27', 'open', 'extracted', ?1, ?2, ?2)",
            params![id, NOW],
        )
        .unwrap();
        delete_routine(&conn, &id, now()).unwrap();
        assert_eq!(count(&conn, "SELECT count(*) FROM routine"), 0);
        assert_eq!(count(&conn, "SELECT count(*) FROM rejection WHERE item_signature = 'routine:spor'"), 1);
        assert_eq!(count(&conn, "SELECT count(*) FROM task WHERE status = 'open'"), 0);
        assert_eq!(count(&conn, "SELECT count(*) FROM task WHERE status = 'done' AND routine_id IS NULL"), 1);
        assert!(matches!(delete_routine(&conn, &id, now()), Err(RoutineError::NotFound)));
    }
}
