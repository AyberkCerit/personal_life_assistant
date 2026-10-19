//! Weekly routines (spec 2026-10-18): a routine repeats on chosen weekdays and each due day is one task row.
//! Weeks are ISO weeks, Monday to Sunday, as in `report.rs`. A routine the user edits is theirs (`user_modified`).

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::atomically;
use crate::extraction::Weekday;
use crate::pipeline::items::{reject_item, ItemRef};
use crate::util::{new_id, MAX_TITLE_CHARS};

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

fn fmt_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// The dates a routine keeps open: today when it is a routine day, plus the first routine day after today (at most 7 days ahead). Ascending.
pub fn occurrence_dates(days: &[Weekday], today: NaiveDate) -> Vec<NaiveDate> {
    let on = |date: NaiveDate| days.iter().any(|d| d.num_days_from_monday() == date.weekday().num_days_from_monday());
    let mut dates = Vec::new();
    if on(today) {
        dates.push(today);
    }
    if let Some(next) = (1..=7).map(|n| today + Duration::days(n)).find(|&date| on(date)) {
        dates.push(next);
    }
    dates
}

/// Brings the open occurrences of one routine in line with it; returns whether anything changed.
pub(crate) fn roll_one(conn: &Connection, routine_id: &str, today: NaiveDate, now: DateTime<FixedOffset>) -> rusqlite::Result<bool> {
    let found: Option<(String, String, bool, Option<String>)> = conn
        .query_row("SELECT title, days_json, paused, block_id FROM routine WHERE routine_id = ?1", [routine_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    let Some((title, days_json, paused, block_id)) = found else { return Ok(false) };
    let Ok(days) = serde_json::from_str::<Vec<RoutineDay>>(&days_json) else { return Ok(false) };
    let wanted: Vec<(String, Option<String>)> = if paused {
        Vec::new()
    } else {
        let weekdays: Vec<Weekday> = days.iter().map(|d| d.day).collect();
        occurrence_dates(&weekdays, today)
            .into_iter()
            .map(|date| {
                let time = days.iter().find(|d| d.day.num_days_from_monday() == date.weekday().num_days_from_monday()).and_then(|d| d.time.clone());
                (fmt_date(date), time)
            })
            .collect()
    };
    let today_s = fmt_date(today);
    let now_s = now.to_rfc3339();

    atomically(conn, "roll_routine", || {
        let mut changed = 0;
        // Rows the user touched (`user_modified = 1`) are never changed or deleted here.
        let open: Vec<(String, String)> = conn
            .prepare(
                "SELECT task_id, COALESCE(routine_date, date) FROM task
                 WHERE routine_id = ?1 AND status = 'open' AND user_modified = 0 AND date >= ?2",
            )?
            .query_map(params![routine_id, today_s], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (task_id, slot) in &open {
            match wanted.iter().find(|(d, _)| d == slot) {
                None => changed += conn.execute("DELETE FROM task WHERE task_id = ?1", [task_id])?,
                Some((_, time)) => {
                    let notify_at = time.as_ref().map(|t| format!("{slot}T{t}"));
                    changed += conn.execute(
                        "UPDATE task SET title = ?1, time = ?2,
                                notified_at = CASE WHEN notify_at IS ?3 THEN notified_at ELSE NULL END, notify_at = ?3, updated_at = ?4
                         WHERE task_id = ?5 AND (title IS NOT ?1 OR time IS NOT ?2 OR notify_at IS NOT ?3)",
                        params![title, time, notify_at, now_s, task_id],
                    )?;
                }
            }
        }
        for (date, time) in &wanted {
            let exists: Option<i64> = conn
                .query_row("SELECT 1 FROM task WHERE routine_id = ?1 AND COALESCE(routine_date, date) = ?2", params![routine_id, date], |r| r.get(0))
                .optional()?;
            if exists.is_some() {
                continue;
            }
            let notify_at = time.as_ref().map(|t| format!("{date}T{t}"));
            changed += conn.execute(
                "INSERT INTO task (task_id, title, date, time, notify_at, status, origin, block_id, item_signature, routine_id, routine_date, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'open', 'extracted', ?6, NULL, ?7, ?3, ?8, ?8)",
                params![new_id(), title, date, time, notify_at, block_id, routine_id, now_s],
            )?;
        }
        Ok(changed > 0)
    })
}

/// Daily upkeep: open occurrences of past days become `cancelled`, then every routine is rolled. Returns whether anything changed.
pub fn roll(conn: &Connection, today: NaiveDate, now: DateTime<FixedOffset>) -> rusqlite::Result<bool> {
    atomically(conn, "roll_routines", || {
        let mut changed = conn.execute(
            "UPDATE task SET status = 'cancelled', updated_at = ?1 WHERE routine_id IS NOT NULL AND status = 'open' AND date < ?2",
            params![now.to_rfc3339(), fmt_date(today)],
        )? > 0;
        let ids: Vec<String> = conn.prepare("SELECT routine_id FROM routine")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for id in ids {
            changed |= roll_one(conn, &id, today, now)?;
        }
        Ok(changed)
    })
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
    atomically(conn, "edit_routine", || {
        let changed = conn.execute(
            "UPDATE routine SET title = ?1, days_json = ?2, user_modified = 1, updated_at = ?3 WHERE routine_id = ?4",
            params![title, serde_json::to_string(&days).expect("days serialize"), now.to_rfc3339(), id],
        )?;
        if changed == 0 {
            return Err(RoutineError::NotFound);
        }
        roll_one(conn, id, today, now)?;
        Ok(())
    })
}

pub fn set_paused(conn: &Connection, id: &str, paused: bool, today: NaiveDate, now: DateTime<FixedOffset>) -> Result<(), RoutineError> {
    atomically(conn, "set_paused", || {
        let changed =
            conn.execute("UPDATE routine SET paused = ?1, user_modified = 1, updated_at = ?2 WHERE routine_id = ?3", params![paused, now.to_rfc3339(), id])?;
        if changed == 0 {
            return Err(RoutineError::NotFound);
        }
        roll_one(conn, id, today, now)?;
        Ok(())
    })
}

/// The UI confirms first. The routine is remembered as rejected so extraction does not bring it back.
/// The one-off copies it took over stay cancelled: only Undo (`reject_item` alone) reopens them.
pub fn delete_routine(conn: &Connection, id: &str, now: DateTime<FixedOffset>) -> Result<(), RoutineError> {
    atomically(conn, "delete_routine", || {
        conn.execute("UPDATE task SET superseded_by = NULL WHERE superseded_by = ?1", [id])?;
        if !reject_item(conn, &ItemRef::Routine(id.to_owned()), now)? {
            return Err(RoutineError::NotFound);
        }
        Ok(())
    })
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

    use crate::tasks::{TaskInput, TaskList};

    /// Monday/Wednesday 20:00 routine "Spor" tied to block b1.
    fn mon_wed(conn: &Connection) -> String {
        insert_routine(conn, "Spor", &[day(Weekday::Mon, "20:00"), day(Weekday::Wed, "20:00")], Some("b1"), Some("routine:spor"), NOW).unwrap()
    }
    fn open(conn: &Connection) -> Vec<(String, Option<String>)> {
        conn.prepare("SELECT date, notify_at FROM task WHERE status = 'open' ORDER BY date")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
    fn open_dates(conn: &Connection) -> Vec<String> {
        open(conn).into_iter().map(|(d, _)| d).collect()
    }
    fn occurrence(conn: &Connection, date: &str) -> String {
        conn.query_row("SELECT task_id FROM task WHERE date = ?1", [date], |r| r.get(0)).unwrap()
    }
    fn status_on(conn: &Connection, date: &str) -> String {
        conn.query_row("SELECT status FROM task WHERE date = ?1", [date], |r| r.get(0)).unwrap()
    }
    fn time_on(conn: &Connection, date: &str) -> String {
        conn.query_row("SELECT time FROM task WHERE date = ?1", [date], |r| r.get(0)).unwrap()
    }
    fn some(date: &str, at: &str) -> (String, Option<String>) {
        (date.into(), Some(at.into()))
    }

    #[test]
    fn today_and_the_next_routine_day_are_open() {
        let (_t, conn) = setup();
        mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap(); // Monday
        assert_eq!(open(&conn), [some("2026-10-26", "2026-10-26T20:00"), some("2026-10-28", "2026-10-28T20:00")]);
        let (origin, block, sig): (String, Option<String>, Option<String>) = conn
            .query_row("SELECT origin, block_id, item_signature FROM task WHERE date = '2026-10-28'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((origin.as_str(), block.as_deref(), sig), ("extracted", Some("b1"), None));
    }

    #[test]
    fn rolling_twice_on_one_day_changes_nothing() {
        let (_t, conn) = setup();
        mon_wed(&conn);
        assert!(roll(&conn, d("2026-10-26"), now()).unwrap());
        assert!(!roll(&conn, d("2026-10-26"), now()).unwrap());
        assert_eq!(count(&conn, "SELECT count(*) FROM task"), 2);
    }

    #[test]
    fn a_ticked_day_is_not_recreated_and_the_next_one_stays() {
        let (_t, conn) = setup();
        mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        crate::tasks::set_done(&conn, &occurrence(&conn, "2026-10-26"), true, now()).unwrap();
        roll(&conn, d("2026-10-26"), now()).unwrap();
        assert_eq!(open_dates(&conn), ["2026-10-28"]);
        roll(&conn, d("2026-10-27"), now()).unwrap(); // Tuesday: not a routine day
        assert_eq!(open_dates(&conn), ["2026-10-28"]);
        roll(&conn, d("2026-10-28"), now()).unwrap();
        assert_eq!(open_dates(&conn), ["2026-10-28", "2026-11-02"]);
    }

    #[test]
    fn a_missed_day_is_cancelled_not_left_open() {
        let (_t, conn) = setup();
        mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        roll(&conn, d("2026-10-27"), now()).unwrap();
        assert_eq!(status_on(&conn, "2026-10-26"), "cancelled");
        assert!(crate::tasks::list_tasks(&conn, TaskList::Today, d("2026-10-27")).unwrap().is_empty());
    }

    #[test]
    fn editing_the_days_moves_future_occurrences_only() {
        let (_t, conn) = setup();
        let id = mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        crate::tasks::set_done(&conn, &occurrence(&conn, "2026-10-26"), true, now()).unwrap();
        edit_routine(&conn, &id, &input("Spor", &[("mon", Some("21:00")), ("thu", Some("21:00"))]), d("2026-10-26"), now()).unwrap();
        assert_eq!(open(&conn), [some("2026-10-29", "2026-10-29T21:00")]);
        assert_eq!(status_on(&conn, "2026-10-26"), "done");
    }

    #[test]
    fn a_paused_routine_has_no_open_occurrences_until_resumed() {
        let (_t, conn) = setup();
        let id = mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        set_paused(&conn, &id, true, d("2026-10-26"), now()).unwrap();
        assert!(open_dates(&conn).is_empty());
        roll(&conn, d("2026-10-26"), now()).unwrap();
        assert!(open_dates(&conn).is_empty(), "rolling does not add occurrences of a paused routine");
        set_paused(&conn, &id, false, d("2026-10-26"), now()).unwrap();
        assert_eq!(open_dates(&conn), ["2026-10-26", "2026-10-28"]);
    }

    #[test]
    fn an_occurrence_the_user_edited_keeps_its_time() {
        let (_t, conn) = setup();
        let id = mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        let occ = occurrence(&conn, "2026-10-28");
        let moved = TaskInput { title: "Spor".into(), date: Some("2026-10-28".into()), time: Some("18:00".into()), ..Default::default() };
        crate::tasks::edit_task(&conn, &occ, &moved, now()).unwrap();
        roll(&conn, d("2026-10-26"), now()).unwrap();
        assert_eq!(time_on(&conn, "2026-10-28"), "18:00");
        // Even a routine edit leaves it alone while its day stays a routine day.
        edit_routine(&conn, &id, &input("Spor", &[("mon", Some("21:00")), ("wed", Some("21:00"))]), d("2026-10-26"), now()).unwrap();
        assert_eq!(time_on(&conn, "2026-10-28"), "18:00");
        assert_eq!(time_on(&conn, "2026-10-26"), "21:00");
    }

    #[test]
    fn an_occurrence_the_user_moved_is_not_deleted_or_recreated() {
        let (_t, conn) = setup();
        mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        let occ = occurrence(&conn, "2026-10-28");
        let moved = TaskInput { title: "Spor".into(), date: Some("2026-10-29".into()), time: Some("20:00".into()), ..Default::default() };
        crate::tasks::edit_task(&conn, &occ, &moved, now()).unwrap();
        for day in ["2026-10-26", "2026-10-27"] {
            roll(&conn, d(day), now()).unwrap();
            assert_eq!(count(&conn, "SELECT count(*) FROM task WHERE date = '2026-10-28'"), 0);
            assert_eq!(count(&conn, "SELECT count(*) FROM task WHERE date = '2026-10-29' AND status = 'open'"), 1);
        }
    }

    #[test]
    fn a_user_modified_occurrence_survives_a_routine_edit_that_drops_its_weekday() {
        let (_t, conn) = setup();
        let id = mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        let occ = occurrence(&conn, "2026-10-28");
        let kept = TaskInput { title: "Spor (özel)".into(), date: Some("2026-10-28".into()), time: Some("18:00".into()), ..Default::default() };
        crate::tasks::edit_task(&conn, &occ, &kept, now()).unwrap();
        edit_routine(&conn, &id, &input("Spor", &[("mon", Some("20:00")), ("fri", Some("20:00"))]), d("2026-10-26"), now()).unwrap();
        assert_eq!(open_dates(&conn), ["2026-10-26", "2026-10-28", "2026-10-30"]);
        assert_eq!(time_on(&conn, "2026-10-28"), "18:00");
    }

    #[test]
    fn a_changed_time_re_arms_the_reminder_of_an_untouched_occurrence() {
        let (_t, conn) = setup();
        let id = mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        conn.execute("UPDATE task SET notified_at = '2026-10-26T20:00'", []).unwrap();
        edit_routine(&conn, &id, &input("Spor", &[("mon", Some("20:00")), ("wed", Some("21:00"))]), d("2026-10-26"), now()).unwrap();
        let notified = |date: &str| -> Option<String> { conn.query_row("SELECT notified_at FROM task WHERE date = ?1", [date], |r| r.get(0)).unwrap() };
        assert!(notified("2026-10-26").is_some(), "same time: stays notified");
        assert!(notified("2026-10-28").is_none());
    }

    #[test]
    fn a_failing_roll_leaves_an_edit_unapplied() {
        let (_t, conn) = setup();
        let id = mon_wed(&conn);
        conn.execute_batch("CREATE TRIGGER boom BEFORE INSERT ON task BEGIN SELECT RAISE(ABORT, 'boom'); END").unwrap();
        let err = edit_routine(&conn, &id, &input("Koşu", &[("mon", Some("21:00"))]), d("2026-10-26"), now());
        assert!(matches!(err, Err(RoutineError::Db(_))));
        let v = &list_routines(&conn, d("2026-10-26")).unwrap()[0];
        assert_eq!((v.title.as_str(), v.days.len(), v.user_modified), ("Spor", 2, false));
    }

    #[test]
    fn the_next_day_wraps_into_next_week() {
        assert_eq!(occurrence_dates(&[Weekday::Mon], d("2026-11-01")), [d("2026-11-02")]); // Sunday -> Monday
        assert_eq!(occurrence_dates(&[Weekday::Sun], d("2026-11-01")), [d("2026-11-01"), d("2026-11-08")]);
    }

    #[test]
    fn a_routine_without_a_time_is_no_reminder() {
        let (_t, conn) = setup();
        insert_routine(&conn, "Su", &[RoutineDay { day: Weekday::Mon, time: None }], None, None, NOW).unwrap();
        roll(&conn, d("2026-10-26"), now()).unwrap();
        assert_eq!(open(&conn)[0].1, None);
    }

    #[test]
    fn a_routine_edit_keeps_a_cancelled_past_occurrence_and_opens_only_future_days() {
        let (_t, conn) = setup();
        let id = mon_wed(&conn);
        roll(&conn, d("2026-10-26"), now()).unwrap();
        roll(&conn, d("2026-10-27"), now()).unwrap(); // Monday's occurrence is cancelled
        edit_routine(&conn, &id, &input("Spor", &[("fri", None)]), d("2026-10-27"), now()).unwrap();
        assert_eq!(status_on(&conn, "2026-10-26"), "cancelled");
        assert_eq!(open_dates(&conn), ["2026-10-30"]);
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
