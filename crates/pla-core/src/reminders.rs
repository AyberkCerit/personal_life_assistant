//! Reminders = tasks with a `notify_at` (FR-TSK-010…013, -016). The scheduler asks what is due,
//! shows it once, and the user completes or snoozes it.

use chrono::{DateTime, FixedOffset, TimeDelta};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::tasks::{set_done, TaskError};

const SNOOZE_MINUTES: i64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DueReminder {
    pub task_id: String,
    pub title: String,
    pub notify_at: String,
}

/// The local wall-clock minute, in the same text form as `notify_at`.
pub fn local_minute(now: DateTime<FixedOffset>) -> String {
    now.naive_local().format("%Y-%m-%dT%H:%M").to_string()
}

pub fn due_reminders(conn: &Connection, now: DateTime<FixedOffset>) -> rusqlite::Result<Vec<DueReminder>> {
    conn.prepare(
        "SELECT task_id, title, notify_at FROM task
         WHERE status = 'open' AND notify_at IS NOT NULL AND notified_at IS NULL AND notify_at <= ?1
         ORDER BY notify_at, created_at",
    )?
    .query_map([local_minute(now)], |r| Ok(DueReminder { task_id: r.get(0)?, title: r.get(1)?, notify_at: r.get(2)? }))?
    .collect()
}

pub fn mark_notified(conn: &Connection, task_id: &str, now: DateTime<FixedOffset>) -> rusqlite::Result<()> {
    conn.execute("UPDATE task SET notified_at = ?1 WHERE task_id = ?2", params![local_minute(now), task_id])?;
    Ok(())
}

/// FR-TSK-013: show it again 10 minutes from now.
pub fn snooze(conn: &Connection, task_id: &str, now: DateTime<FixedOffset>) -> Result<(), TaskError> {
    let later = local_minute(now + TimeDelta::minutes(SNOOZE_MINUTES));
    let changed = conn.execute(
        "UPDATE task SET notify_at = ?1, notified_at = NULL, user_modified = 1, updated_at = ?2 WHERE task_id = ?3",
        params![later, now.to_rfc3339(), task_id],
    )?;
    if changed == 0 {
        return Err(TaskError::NotFound);
    }
    Ok(())
}

pub fn complete(conn: &Connection, task_id: &str, now: DateTime<FixedOffset>) -> Result<(), TaskError> {
    set_done(conn, task_id, true, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;
    use crate::tasks::{add_task, edit_task, list_tasks, TaskInput, TaskList};
    use chrono::NaiveDate;

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        (tmp, conn)
    }
    fn reminder(conn: &Connection, title: &str, date: &str, time: Option<&str>) -> String {
        let input = TaskInput { title: title.into(), date: Some(date.into()), time: time.map(Into::into), remind: Some(true), ..Default::default() };
        add_task(conn, &input, at("2026-10-06T08:00:00+03:00")).unwrap()
    }

    #[test]
    fn a_reminder_is_due_once_at_its_time() {
        // Review Focus 1
        let (_t, conn) = setup();
        let id = reminder(&conn, "İlaç", "2026-10-06", Some("21:30"));
        assert!(due_reminders(&conn, at("2026-10-06T21:29:59+03:00")).unwrap().is_empty());
        let due = due_reminders(&conn, at("2026-10-06T21:30:00+03:00")).unwrap();
        assert_eq!(due, vec![DueReminder { task_id: id.clone(), title: "İlaç".into(), notify_at: "2026-10-06T21:30".into() }]);
        mark_notified(&conn, &id, at("2026-10-06T21:30:00+03:00")).unwrap();
        assert!(due_reminders(&conn, at("2026-10-06T23:00:00+03:00")).unwrap().is_empty());
    }

    #[test]
    fn snooze_brings_it_back_ten_minutes_later() {
        let (_t, conn) = setup();
        let id = reminder(&conn, "İlaç", "2026-10-06", Some("21:30"));
        mark_notified(&conn, &id, at("2026-10-06T21:30:00+03:00")).unwrap();
        snooze(&conn, &id, at("2026-10-06T21:31:00+03:00")).unwrap();
        assert!(due_reminders(&conn, at("2026-10-06T21:40:00+03:00")).unwrap().is_empty());
        let due = due_reminders(&conn, at("2026-10-06T21:41:00+03:00")).unwrap();
        assert_eq!(due[0].notify_at, "2026-10-06T21:41");
        let t = &list_tasks(&conn, TaskList::Today, NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()).unwrap()[0];
        assert!(t.user_modified, "snooze is a user action");
    }

    #[test]
    fn editing_the_time_re_arms_a_fired_reminder() {
        // Review Focus 5
        let (_t, conn) = setup();
        let id = reminder(&conn, "Toplantı", "2026-10-06", Some("10:00"));
        mark_notified(&conn, &id, at("2026-10-06T10:00:00+03:00")).unwrap();
        let moved = TaskInput { title: "Toplantı".into(), date: Some("2026-10-06".into()), time: Some("15:00".into()), remind: None, ..Default::default() };
        edit_task(&conn, &id, &moved, at("2026-10-06T10:05:00+03:00")).unwrap();
        assert_eq!(due_reminders(&conn, at("2026-10-06T15:00:00+03:00")).unwrap().len(), 1);
    }

    #[test]
    fn completed_and_plain_tasks_never_fire() {
        let (_t, conn) = setup();
        let id = reminder(&conn, "Bitti", "2026-10-06", Some("09:00"));
        complete(&conn, &id, at("2026-10-06T08:30:00+03:00")).unwrap();
        let plain = TaskInput { title: "Düz".into(), date: Some("2026-10-06".into()), time: Some("09:00".into()), ..Default::default() };
        add_task(&conn, &plain, at("2026-10-06T08:00:00+03:00")).unwrap();
        assert!(due_reminders(&conn, at("2026-10-06T12:00:00+03:00")).unwrap().is_empty());
    }

    #[test]
    fn reminders_can_be_switched_on_and_off_from_the_form() {
        // Decision 4
        let (_t, conn) = setup();
        let id = reminder(&conn, "Fatura", "2026-10-07", None);
        let t = list_tasks(&conn, TaskList::Upcoming, NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()).unwrap();
        assert_eq!(t[0].notify_at.as_deref(), Some("2026-10-07T09:00"));
        let off = TaskInput { title: "Fatura".into(), date: Some("2026-10-07".into()), remind: Some(false), ..Default::default() };
        edit_task(&conn, &id, &off, at("2026-10-06T08:00:00+03:00")).unwrap();
        let t = list_tasks(&conn, TaskList::Upcoming, NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()).unwrap();
        assert_eq!(t[0].notify_at, None);
    }

    #[test]
    fn local_minute_uses_the_wall_clock() {
        assert_eq!(local_minute(at("2026-10-06T21:30:59+03:00")), "2026-10-06T21:30");
    }
}
