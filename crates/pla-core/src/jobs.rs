//! Daily maintenance bookkeeping (FR-SCH-004, -011, -012, -015) and `pla.db` backups (FR-BKP-001/002).
//! For now the daily job is the backup; summaries and embeddings join it with the memory slice.

use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};

pub const DAILY: &str = "daily_maintenance";
pub const MAX_ATTEMPTS_PER_DAY: i64 = 2;
const KEEP_BACKUPS: usize = 7;

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

struct JobRow {
    last_success: Option<String>,
    last_attempt: Option<String>,
    attempts: i64,
}

fn row(conn: &Connection, job: &str) -> rusqlite::Result<Option<JobRow>> {
    conn.query_row(
        "SELECT last_success_at, last_attempt_at, attempts FROM job_run WHERE job_type = ?1",
        [job],
        |r| Ok(JobRow { last_success: r.get(0)?, last_attempt: r.get(1)?, attempts: r.get(2)? }),
    )
    .optional()
}

fn day_of(stamp: &Option<String>) -> Option<NaiveDate> {
    stamp.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.date_naive())
}

/// Due when it has not succeeded today and has not failed twice today.
pub fn maintenance_due(conn: &Connection, today: NaiveDate) -> rusqlite::Result<bool> {
    let Some(job) = row(conn, DAILY)? else { return Ok(true) };
    if day_of(&job.last_success) == Some(today) {
        return Ok(false);
    }
    let attempts_today = if day_of(&job.last_attempt) == Some(today) { job.attempts } else { 0 };
    Ok(attempts_today < MAX_ATTEMPTS_PER_DAY)
}

pub fn record_success(conn: &Connection, job: &str, now: DateTime<FixedOffset>) -> rusqlite::Result<()> {
    let now = now.to_rfc3339();
    conn.execute(
        "INSERT INTO job_run (job_type, last_success_at, last_attempt_at, last_error, attempts) VALUES (?1, ?2, ?2, NULL, 0)
         ON CONFLICT (job_type) DO UPDATE SET last_success_at = ?2, last_attempt_at = ?2, last_error = NULL, attempts = 0",
        params![job, now],
    )?;
    Ok(())
}

/// Returns how many times the job has failed today (attempts restart each day).
pub fn record_failure(conn: &Connection, job: &str, now: DateTime<FixedOffset>, error: &str) -> rusqlite::Result<i64> {
    let today = now.date_naive();
    let previous = row(conn, job)?;
    let attempts = match &previous {
        Some(p) if day_of(&p.last_attempt) == Some(today) => p.attempts + 1,
        _ => 1,
    };
    conn.execute(
        "INSERT INTO job_run (job_type, last_attempt_at, last_error, attempts) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (job_type) DO UPDATE SET last_attempt_at = ?2, last_error = ?3, attempts = ?4",
        params![job, now.to_rfc3339(), error, attempts],
    )?;
    Ok(attempts)
}

fn is_backup_name(name: &str) -> bool {
    name.len() == "pla-YYYY-MM-DD.db".len()
        && name.starts_with("pla-")
        && name.ends_with(".db")
        && NaiveDate::parse_from_str(&name[4..14], "%Y-%m-%d").is_ok()
}

/// A consistent copy of pla.db in the vault, so copying the vault folder is a full backup (FR-BKP-001).
pub fn backup(conn: &Connection, vault_root: &Path, today: NaiveDate) -> Result<PathBuf, JobError> {
    let dir = vault_root.join(".pla").join("backup");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("pla-{}.db", today.format("%Y-%m-%d")));
    let partial = dir.join(format!("pla-{}.db.partial", today.format("%Y-%m-%d")));
    let _ = std::fs::remove_file(&partial);
    conn.backup(rusqlite::MAIN_DB, &partial, None)?;
    std::fs::rename(&partial, &target)?;

    // FR-BKP-002: keep the newest 7 of PLA's own backup files; nothing else in the folder is touched.
    let mut backups: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_backup_name(n))
        .collect();
    backups.sort();
    let excess = backups.len().saturating_sub(KEEP_BACKUPS);
    for name in &backups[..excess] {
        std::fs::remove_file(dir.join(name))?;
    }
    Ok(target)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn maintenance_runs_once_a_day() {
        // Review Focus 3: FR-SCH-004
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        assert!(maintenance_due(&conn, d("2026-10-06")).unwrap());
        record_success(&conn, DAILY, at("2026-10-06T23:10:00+03:00")).unwrap();
        assert!(!maintenance_due(&conn, d("2026-10-06")).unwrap());
        assert!(maintenance_due(&conn, d("2026-10-07")).unwrap(), "next day it is due again");
    }

    #[test]
    fn a_failure_is_retried_once_then_given_up_for_the_day() {
        // Review Focus 3: FR-SCH-015
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        assert_eq!(record_failure(&conn, DAILY, at("2026-10-06T10:00:00+03:00"), "disk full").unwrap(), 1);
        assert!(maintenance_due(&conn, d("2026-10-06")).unwrap(), "one retry");
        assert_eq!(record_failure(&conn, DAILY, at("2026-10-06T11:00:00+03:00"), "disk full").unwrap(), 2);
        assert!(!maintenance_due(&conn, d("2026-10-06")).unwrap(), "given up for today");
        assert!(maintenance_due(&conn, d("2026-10-07")).unwrap());
        assert_eq!(record_failure(&conn, DAILY, at("2026-10-07T10:00:00+03:00"), "x").unwrap(), 1, "attempts count per day");
    }

    #[test]
    fn backups_are_consistent_copies_and_only_seven_are_kept() {
        // FR-BKP-001/002
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("kasa");
        std::fs::create_dir_all(vault.join(".pla/backup")).unwrap();
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        conn.execute("INSERT INTO task (task_id, title, origin, created_at, updated_at) VALUES ('t', 'Yedekte olmalı', 'manual', 'n', 'n')", []).unwrap();
        for day in 1..=8 {
            backup(&conn, &vault, d(&format!("2026-10-{day:02}"))).unwrap();
        }
        std::fs::write(vault.join(".pla/backup/notlarım.txt"), "kullanıcının dosyası").unwrap();
        let path = backup(&conn, &vault, d("2026-10-09")).unwrap();
        assert_eq!(path, vault.join(".pla/backup/pla-2026-10-09.db"));
        let mut names: Vec<String> = std::fs::read_dir(vault.join(".pla/backup")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), 8, "7 backups + the user's own file: {names:?}");
        assert!(names.contains(&"notlarım.txt".to_owned()), "never touches other files");
        assert!(!names.contains(&"pla-2026-10-02.db".to_owned()), "oldest backups removed");
        let copy = rusqlite::Connection::open(&path).unwrap();
        let title: String = copy.query_row("SELECT title FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Yedekte olmalı");
    }
}
