//! Daily maintenance bookkeeping (FR-SCH-004, -011, -012, -015) and `pla.db` backups (FR-BKP-001/002).
//! The daily job is the backup, then pruning old missing blocks; the worker summarises the days.

use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};

pub const DAILY: &str = "daily_maintenance";
/// FR-SCH-007…011: the weekly report's bookkeeping.
pub const WEEKLY_REPORT: &str = "weekly_report";
pub const MAX_ATTEMPTS_PER_DAY: i64 = 2;
const KEEP_BACKUPS: usize = 7;

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A backup waits to be restored (`recovery::pending`): copying this database now could
    /// replace it with an empty one (final review C1). Not a failure; the job waits.
    #[error("a backup waits to be restored")]
    Held,
}

struct JobRow {
    last_success: Option<String>,
    last_attempt: Option<String>,
    attempts: i64,
}

fn row(conn: &Connection, job: &str) -> rusqlite::Result<Option<JobRow>> {
    conn.query_row("SELECT last_success_at, last_attempt_at, attempts FROM job_run WHERE job_type = ?1", [job], |r| {
        Ok(JobRow { last_success: r.get(0)?, last_attempt: r.get(1)?, attempts: r.get(2)? })
    })
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

/// How many times `job` failed today (FR-SCH-015: a second failure stops it until tomorrow).
pub fn failures_today(conn: &Connection, job: &str, today: NaiveDate) -> rusqlite::Result<i64> {
    Ok(row(conn, job)?.filter(|j| day_of(&j.last_attempt) == Some(today) && j.attempts > 0).map_or(0, |j| j.attempts))
}

/// What the Scheduler settings show about a job.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct JobStatus {
    pub last_success_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_error: Option<String>,
}

pub fn status(conn: &Connection, job: &str) -> rusqlite::Result<JobStatus> {
    Ok(conn
        .query_row("SELECT last_success_at, last_attempt_at, last_error FROM job_run WHERE job_type = ?1", [job], |r| {
            Ok(JobStatus { last_success_at: r.get(0)?, last_attempt_at: r.get(1)?, last_error: r.get(2)? })
        })
        .optional()?
        .unwrap_or_default())
}

/// PLA's backups of this vault, newest first.
pub fn backups(vault_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(vault_root.join(".pla").join("backup")) else { return Vec::new() };
    let mut names: Vec<String> = entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_backup_name(n)).collect();
    names.sort_by(|a, b| b.cmp(a));
    names
}

fn is_backup_name(name: &str) -> bool {
    name.len() == "pla-YYYY-MM-DD.db".len() && name.starts_with("pla-") && name.ends_with(".db") && NaiveDate::parse_from_str(&name[4..14], "%Y-%m-%d").is_ok()
}

/// A consistent copy of pla.db in the vault, so copying the vault folder is a full backup (FR-BKP-001).
pub fn backup(conn: &Connection, vault_root: &Path, today: NaiveDate) -> Result<PathBuf, JobError> {
    if conn.path().and_then(|p| Path::new(p).parent()).is_some_and(crate::recovery::pending) {
        return Err(JobError::Held);
    }
    let dir = vault_root.join(".pla").join("backup");
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("pla-{}.db", today.format("%Y-%m-%d")));
    let partial = dir.join(format!("pla-{}.db.partial", today.format("%Y-%m-%d")));
    let _ = std::fs::remove_file(&partial);
    conn.backup(rusqlite::MAIN_DB, &partial, None)?;
    std::fs::rename(&partial, &target)?;

    // FR-BKP-002: keep the newest 7 of PLA's own backup files; nothing else in the folder is touched.
    let names: Vec<String> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    for stale in names.iter().filter(|n| n.strip_suffix(".partial").is_some_and(is_backup_name)) {
        let _ = std::fs::remove_file(dir.join(stale)); // an earlier run was interrupted
    }
    let mut backups: Vec<String> = names.into_iter().filter(|n| is_backup_name(n)).collect();
    backups.sort();
    let excess = backups.len().saturating_sub(KEEP_BACKUPS);
    for name in &backups[..excess] {
        std::fs::remove_file(dir.join(name))?;
    }
    Ok(target)
}
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    /// Not one of PLA's backup names in the vault's backup folder (no path ever comes from outside).
    #[error("no such backup: {0}")]
    Unknown(String),
    /// FR-BKP-006: the file fails SQLite's integrity check; nothing was changed.
    #[error("the backup {0} is damaged")]
    Damaged(String),
    /// Written by a newer PLA: this one cannot read it (final review C2); nothing was changed.
    #[error("the backup {0} is from a newer PLA")]
    NewerSchema(String),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// FR-BKP-005/008: puts the vault's backup `name` in place of `data_dir/pla.db`. The backup is
/// checked first (FR-BKP-006); the current pla.db, with its WAL, is moved aside as
/// `pla-before-restore-<time>.db` (returned; `None` where there was none, as on another computer);
/// the copy goes in under a temporary name and is renamed, then brought to the current schema.
/// No session may have pla.db open meanwhile.
pub fn restore(data_dir: &Path, vault_root: &Path, name: &str, now: DateTime<FixedOffset>) -> Result<Option<PathBuf>, RestoreError> {
    if !is_backup_name(name) || !backups(vault_root).iter().any(|b| b == name) {
        return Err(RestoreError::Unknown(name.to_owned()));
    }
    let source = vault_root.join(".pla").join("backup").join(name);
    if !crate::recovery::intact(&source) {
        return Err(RestoreError::Damaged(name.to_owned()));
    }
    // a newer PLA's schema is refused before anything moves (final review C2)
    let version: i64 = Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0)))
        .unwrap_or(0);
    if version > crate::db::pla_schema_supported() {
        return Err(RestoreError::NewerSchema(name.to_owned()));
    }
    std::fs::create_dir_all(data_dir)?;
    let current = data_dir.join("pla.db");
    let kept = if current.exists() { Some(crate::recovery::move_aside(data_dir, "pla-before-restore", now)?) } else { None };
    let swapped = (|| -> Result<(), RestoreError> {
        let partial = data_dir.join("pla.db.restoring");
        std::fs::copy(&source, &partial)?;
        std::fs::rename(&partial, &current)?;
        drop(crate::db::open_databases(data_dir)?); // an older backup gets the current schema
        Ok(())
    })();
    if let Err(e) = swapped {
        // what was there comes back: the copy of the backup goes (the backup itself stays)
        for leftover in ["pla.db.restoring", "pla.db", "pla.db-wal", "pla.db-shm"] {
            let _ = std::fs::remove_file(data_dir.join(leftover));
        }
        if let Some(kept) = &kept {
            crate::recovery::put_back(data_dir, kept)?;
        }
        return Err(e);
    }
    crate::recovery::answered(data_dir);
    Ok(kept)
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
    fn failures_are_counted_per_day_and_cleared_by_success() {
        // FR-SCH-015
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        assert_eq!(failures_today(&conn, WEEKLY_REPORT, d("2026-10-11")).unwrap(), 0);
        record_failure(&conn, WEEKLY_REPORT, at("2026-10-11T20:01:00+03:00"), "disk").unwrap();
        record_failure(&conn, WEEKLY_REPORT, at("2026-10-11T20:31:00+03:00"), "disk").unwrap();
        assert_eq!(failures_today(&conn, WEEKLY_REPORT, d("2026-10-11")).unwrap(), 2);
        assert_eq!(failures_today(&conn, WEEKLY_REPORT, d("2026-10-12")).unwrap(), 0, "a new day");
        record_success(&conn, WEEKLY_REPORT, at("2026-10-11T21:00:00+03:00")).unwrap();
        assert_eq!(failures_today(&conn, WEEKLY_REPORT, d("2026-10-11")).unwrap(), 0);
    }

    #[test]
    fn the_settings_screen_sees_the_last_backup_and_its_error() {
        // Scheduler section: last backup or error, and the backups on disk
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(&tmp.path().join("data")).unwrap().pla;
        assert_eq!(status(&conn, DAILY).unwrap(), JobStatus::default());
        record_failure(&conn, DAILY, at("2026-10-06T23:00:00+03:00"), "disk full").unwrap();
        let failed = status(&conn, DAILY).unwrap();
        assert_eq!((failed.last_success_at, failed.last_error.as_deref()), (None, Some("disk full")));
        record_success(&conn, DAILY, at("2026-10-06T23:10:00+03:00")).unwrap();
        let ok = status(&conn, DAILY).unwrap();
        assert_eq!((ok.last_success_at.as_deref(), ok.last_error), (Some("2026-10-06T23:10:00+03:00"), None));

        let vault = tmp.path().join("kasa");
        assert_eq!(backups(&vault), Vec::<String>::new());
        backup(&conn, &vault, d("2026-10-05")).unwrap();
        backup(&conn, &vault, d("2026-10-06")).unwrap();
        std::fs::write(vault.join(".pla/backup/notlarim.txt"), "x").unwrap();
        assert_eq!(backups(&vault), vec!["pla-2026-10-06.db".to_owned(), "pla-2026-10-05.db".to_owned()], "newest first, only PLA's own");
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
        let mut names: Vec<String> =
            std::fs::read_dir(vault.join(".pla/backup")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), 8, "7 backups + the user's own file: {names:?}");
        assert!(names.contains(&"notlarım.txt".to_owned()), "never touches other files");
        assert!(!names.contains(&"pla-2026-10-02.db".to_owned()), "oldest backups removed");
        let copy = rusqlite::Connection::open(&path).unwrap();
        let title: String = copy.query_row("SELECT title FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Yedekte olmalı");
    }

    #[test]
    fn an_interrupted_backup_from_an_earlier_day_is_cleaned_up() {
        // Final review M8
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("kasa");
        std::fs::create_dir_all(vault.join(".pla/backup")).unwrap();
        std::fs::write(vault.join(".pla/backup/pla-2026-10-01.db.partial"), "yarım").unwrap();
        std::fs::write(vault.join(".pla/backup/benim.db.partial"), "kullanıcının").unwrap();
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        backup(&conn, &vault, d("2026-10-02")).unwrap();
        assert!(!vault.join(".pla/backup/pla-2026-10-01.db.partial").exists());
        assert!(vault.join(".pla/backup/benim.db.partial").exists(), "only PLA's own names");
    }

    fn titles(dir: &Path) -> Vec<String> {
        let conn = Connection::open(dir.join("pla.db")).unwrap();
        let mut t: Vec<String> = conn.prepare("SELECT title FROM task ORDER BY title").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        t.sort();
        t
    }

    fn add(conn: &Connection, title: &str) {
        crate::tasks::add_task(conn, &crate::tasks::TaskInput { title: title.into(), ..Default::default() }, at("2026-10-05T09:00:00+03:00")).unwrap();
    }

    #[test]
    fn a_backup_comes_back_and_what_was_there_is_kept() {
        // FR-BKP-005, M6 acceptance: restore, keeping the current pla.db beside it
        let tmp = tempfile::tempdir().unwrap();
        let (data, vault) = (tmp.path().join("data"), tmp.path().join("kasa"));
        {
            let conn = open_databases(&data).unwrap().pla;
            add(&conn, "Dişçi");
            backup(&conn, &vault, d("2026-10-05")).unwrap();
            add(&conn, "Sonradan eklenen");
        }
        let kept = restore(&data, &vault, "pla-2026-10-05.db", at("2026-10-06T10:00:00+03:00")).unwrap();
        assert_eq!(titles(&data), ["Dişçi"]);
        let kept = kept.expect("the current pla.db is kept");
        assert_eq!(kept.file_name().unwrap(), "pla-before-restore-20261006-100000.db");
        let before = Connection::open(&kept).unwrap();
        let n: i64 = before.query_row("SELECT count(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "nothing of the replaced data is lost");
    }

    #[test]
    fn a_vault_copied_alone_brings_its_tasks_to_another_computer() {
        // FR-BKP-008: on the other computer there is no pla.db at all; the vault's backup restores it
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("kasa");
        {
            let conn = open_databases(&tmp.path().join("pc1")).unwrap().pla;
            add(&conn, "Fatura öde");
            backup(&conn, &vault, d("2026-10-05")).unwrap();
        }
        let pc2 = tmp.path().join("pc2");
        assert_eq!(restore(&pc2, &vault, &backups(&vault)[0], at("2026-10-06T10:00:00+03:00")).unwrap(), None, "nothing to keep there");
        assert_eq!(titles(&pc2), ["Fatura öde"]);
    }

    #[test]
    fn a_damaged_or_foreign_backup_is_refused_and_nothing_changes() {
        // FR-BKP-006
        let tmp = tempfile::tempdir().unwrap();
        let (data, vault) = (tmp.path().join("data"), tmp.path().join("kasa"));
        {
            let conn = open_databases(&data).unwrap().pla;
            add(&conn, "Dişçi");
        }
        std::fs::create_dir_all(vault.join(".pla/backup")).unwrap();
        std::fs::write(vault.join(".pla/backup/pla-2026-10-04.db"), b"not a database at all".repeat(100)).unwrap();
        let err = restore(&data, &vault, "pla-2026-10-04.db", at("2026-10-06T10:00:00+03:00")).unwrap_err();
        assert!(matches!(&err, RestoreError::Damaged(name) if name == "pla-2026-10-04.db"), "{err:?}");
        for bad in ["../../data/pla.db", "pla-2026-10-99.db", "benim.db"] {
            assert!(matches!(restore(&data, &vault, bad, at("2026-10-06T10:00:00+03:00")), Err(RestoreError::Unknown(_))), "{bad}");
        }
        assert_eq!(titles(&data), ["Dişçi"], "the current data is untouched");
        assert!(std::fs::read_dir(&data).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("before-restore")));
    }

    #[test]
    fn a_backup_from_a_newer_pla_is_refused_and_nothing_changes() {
        // final review C2: it used to be swapped in, and then the vault would not open at all
        let tmp = tempfile::tempdir().unwrap();
        let (data, vault) = (tmp.path().join("data"), tmp.path().join("kasa"));
        {
            let conn = open_databases(&data).unwrap().pla;
            add(&conn, "Dişçi");
            conn.execute("INSERT INTO schema_version (version, applied_at) VALUES (999, 'x')", []).unwrap();
            backup(&conn, &vault, d("2026-10-05")).unwrap();
            conn.execute("DELETE FROM schema_version WHERE version = 999", []).unwrap();
            add(&conn, "Sonra");
        }
        let err = restore(&data, &vault, "pla-2026-10-05.db", at("2026-10-06T10:00:00+03:00")).unwrap_err();
        assert!(matches!(err, RestoreError::NewerSchema(_)), "{err:?}");
        assert_eq!(titles(&data), ["Dişçi", "Sonra"], "pla.db is where it was");
    }

    #[test]
    fn no_backup_is_taken_while_one_waits_to_be_restored() {
        // final review C1: the empty database replaced the very backup the dialog offered
        let tmp = tempfile::tempdir().unwrap();
        let (data, vault) = (tmp.path().join("data"), tmp.path().join("kasa"));
        let conn = open_databases(&data).unwrap().pla;
        add(&conn, "Dişçi");
        backup(&conn, &vault, d("2026-10-05")).unwrap();
        let offer = crate::recovery::RestoreOffer { backup: Some("pla-2026-10-05.db".into()), damaged: false };
        std::fs::write(data.join("pla.restore-offer"), serde_json::to_string(&offer).unwrap()).unwrap();
        let empty = open_databases(&tmp.path().join("empty")).unwrap().pla;
        drop(empty);
        assert!(matches!(backup(&conn, &vault, d("2026-10-05")), Err(JobError::Held)));
        crate::recovery::answered(&data);
        backup(&conn, &vault, d("2026-10-05")).unwrap();
    }
}
