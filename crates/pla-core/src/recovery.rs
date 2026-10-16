//! What opening a vault's data needs to know first (M6 acceptance): whether the last run ended
//! cleanly, whether `pla.db` survived it (FR-LOG-006/007), and whether a backup should be offered
//! because there is no `pla.db` at all, as on another computer (FR-BKP-004/008).

use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use rusqlite::{Connection, OpenFlags};

/// Present while a session has the data open; left behind by a crash or a forced stop.
const RUNNING: &str = "pla.running";

/// A backup to offer when the vault opens.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RestoreOffer {
    /// The newest backup's file name (`jobs::backups`); `None` when a damaged pla.db had none to
    /// come back from (the user still hears it was moved aside).
    pub backup: Option<String>,
    /// `pla.db` failed its integrity check after an unclean stop and was moved aside (FR-LOG-007);
    /// otherwise there was none.
    pub damaged: bool,
}

/// Run before the databases open. After an unclean stop a damaged `pla.db` (with its WAL) is moved
/// aside as `pla-damaged-<time>.db`, never deleted; then, when there is no `pla.db` and the vault
/// has a backup, the newest intact one is offered. Marks the data as open until `closed_cleanly`.
pub fn before_open(data_dir: &Path, vault_root: &Path, now: DateTime<FixedOffset>) -> std::io::Result<Option<RestoreOffer>> {
    std::fs::create_dir_all(data_dir)?;
    let db = data_dir.join("pla.db");
    let mut damaged = false;
    if data_dir.join(RUNNING).exists() && db.exists() && !intact(&db) {
        let base = format!("pla-damaged-{}.db", now.format("%Y%m%d-%H%M%S"));
        for suffix in ["", "-wal", "-shm"] {
            let file = data_dir.join(format!("pla.db{suffix}"));
            if file.exists() {
                std::fs::rename(&file, data_dir.join(format!("{base}{suffix}")))?;
            }
        }
        damaged = true;
    }
    std::fs::write(data_dir.join(RUNNING), now.to_rfc3339())?;
    if db.exists() {
        return Ok(None);
    }
    // the newest that passes the check (FR-BKP-006): a cut-off copy is never the one offered
    let folder = vault_root.join(".pla").join("backup");
    let backup = crate::jobs::backups(vault_root).into_iter().find(|name| intact(&folder.join(name)));
    Ok((backup.is_some() || damaged).then_some(RestoreOffer { backup, damaged }))
}

/// The session closed: the next start need not check `pla.db`.
pub fn closed_cleanly(data_dir: &Path) {
    let _ = std::fs::remove_file(data_dir.join(RUNNING));
}

/// SQLite's full integrity check (FR-LOG-006); a file it cannot even open fails it.
pub fn intact(db: &Path) -> bool {
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))
        .is_ok_and(|check| check == "ok")
}

/// The files `before_open` moved aside, newest first (for the restore dialog to name).
pub fn damaged_files(data_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(data_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("pla-damaged-") && n.to_string_lossy().ends_with(".db")))
        .collect();
    files.sort();
    files.reverse();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;
    use chrono::NaiveDate;

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn vault_with_backup(tmp: &Path) -> PathBuf {
        let vault = tmp.join("kasa");
        let conn = open_databases(&tmp.join("elsewhere")).unwrap().pla;
        crate::jobs::backup(&conn, &vault, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()).unwrap();
        vault
    }

    #[test]
    fn a_vault_with_no_data_here_offers_its_newest_backup() {
        // FR-BKP-004/008: the vault was copied to another computer
        let tmp = tempfile::tempdir().unwrap();
        let vault = vault_with_backup(tmp.path());
        let offer = before_open(&tmp.path().join("pc2"), &vault, at("2026-10-06T10:00:00+03:00")).unwrap();
        assert_eq!(offer, Some(RestoreOffer { backup: Some("pla-2026-10-05.db".into()), damaged: false }));
        assert_eq!(before_open(&tmp.path().join("pc3"), &tmp.path().join("no-backups"), at("2026-10-06T10:00:00+03:00")).unwrap(), None);
    }

    #[test]
    fn after_a_crash_a_damaged_pla_db_is_kept_aside_and_a_backup_offered() {
        // FR-LOG-006/007 (M6 decision: notes stay usable, the data waits for a restore)
        let tmp = tempfile::tempdir().unwrap();
        let vault = vault_with_backup(tmp.path());
        let data = tmp.path().join("data");
        drop(open_databases(&data).unwrap());
        assert_eq!(before_open(&data, &vault, at("2026-10-06T09:00:00+03:00")).unwrap(), None, "a healthy start");
        // the process is killed (no closed_cleanly), and the file is damaged meanwhile
        std::fs::write(data.join("pla.db"), b"half written".repeat(500)).unwrap();
        let offer = before_open(&data, &vault, at("2026-10-06T10:00:00+03:00")).unwrap();
        assert_eq!(offer, Some(RestoreOffer { backup: Some("pla-2026-10-05.db".into()), damaged: true }));
        assert!(!data.join("pla.db").exists());
        assert_eq!(damaged_files(&data), [data.join("pla-damaged-20261006-100000.db")], "kept, never deleted");
    }

    #[test]
    fn a_damaged_backup_is_never_the_one_offered() {
        // FR-BKP-006: the newest backup that passes the check, not just the newest
        let tmp = tempfile::tempdir().unwrap();
        let vault = vault_with_backup(tmp.path());
        std::fs::write(vault.join(".pla/backup/pla-2026-10-06.db"), b"cut off".repeat(400)).unwrap();
        let offer = before_open(&tmp.path().join("pc2"), &vault, at("2026-10-07T10:00:00+03:00")).unwrap();
        assert_eq!(offer, Some(RestoreOffer { backup: Some("pla-2026-10-05.db".into()), damaged: false }));
    }

    #[test]
    fn a_damaged_pla_db_with_no_backup_is_still_told() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        drop(open_databases(&data).unwrap());
        let no_backups = tmp.path().join("kasa");
        before_open(&data, &no_backups, at("2026-10-06T09:00:00+03:00")).unwrap();
        std::fs::write(data.join("pla.db"), b"half written".repeat(500)).unwrap();
        assert_eq!(before_open(&data, &no_backups, at("2026-10-06T10:00:00+03:00")).unwrap(), Some(RestoreOffer { backup: None, damaged: true }));
    }

    #[test]
    fn a_healthy_database_after_a_crash_is_left_alone_and_a_clean_close_skips_the_check() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = vault_with_backup(tmp.path());
        let data = tmp.path().join("data");
        drop(open_databases(&data).unwrap());
        before_open(&data, &vault, at("2026-10-06T09:00:00+03:00")).unwrap();
        assert_eq!(before_open(&data, &vault, at("2026-10-06T10:00:00+03:00")).unwrap(), None, "crashed, but intact");
        assert!(data.join("pla.db").exists());
        closed_cleanly(&data);
        assert!(!data.join(RUNNING).exists());
    }
}
