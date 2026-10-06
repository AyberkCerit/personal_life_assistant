//! Per-vault databases: pla.db (user data, backed up) and cache.db (rebuildable) — SRS Appendix C, E-C7.

use std::path::Path;

use rusqlite::Connection;

const PLA_MIGRATIONS: &[&str] = &[
    include_str!("../migrations/pla/001_init.sql"),
    include_str!("../migrations/pla/002_blocks_queue.sql"),
    include_str!("../migrations/pla/003_reminders.sql"),
    include_str!("../migrations/pla/004_reminder_seen.sql"),
];
const CACHE_MIGRATIONS: &[&str] = &[include_str!("../migrations/cache/001_init.sql"), include_str!("../migrations/cache/002_link_line.sql"), include_str!("../migrations/cache/003_tag_keys.sql"), include_str!("../migrations/cache/004_yaml_frontmatter.sql")];

pub struct Databases {
    pub pla: Connection,
    pub cache: Connection,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("database schema version {found} is newer than this app supports ({supported})")]
    NewerSchema { found: i64, supported: i64 },
}

/// Opens (creating if needed) `dir/pla.db` and `dir/cache.db` and brings both to the current schema.
pub fn open_databases(dir: &Path) -> Result<Databases, DbError> {
    std::fs::create_dir_all(dir)?;
    let mut pla = connect(&dir.join("pla.db"))?;
    migrate(&mut pla, PLA_MIGRATIONS, Some(&dir.join("pla.before-upgrade.db")))?;
    let mut cache = connect(&dir.join("cache.db"))?;
    migrate(&mut cache, CACHE_MIGRATIONS, None)?;
    Ok(Databases { pla, cache })
}

/// A connection with PLA's pragmas (busy timeout, WAL, foreign keys); no migration.
pub fn connect(path: &Path) -> Result<Connection, DbError> {
    let conn = Connection::open(path)?;
    // A second writer (another window, a restarting worker) waits instead of failing.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

/// Applies `migrations[current..]`, one transaction each. `backup_to` receives a consistent copy
/// of the database before an existing one is upgraded (E-C3).
pub fn migrate(conn: &mut Connection, migrations: &[&str], backup_to: Option<&Path>) -> Result<(), DbError> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)")?;
    let current: i64 = conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0))?;
    let supported = migrations.len() as i64;
    if current > supported {
        return Err(DbError::NewerSchema { found: current, supported });
    }
    if current == supported {
        return Ok(());
    }
    if let (true, Some(backup)) = (current > 0, backup_to) {
        if backup.exists() {
            std::fs::remove_file(backup)?;
        }
        conn.execute("VACUUM INTO ?1", [backup.to_string_lossy()])?;
    }
    for (index, sql) in migrations.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))", [index as i64 + 1])?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(conn: &Connection) -> Vec<String> {
        let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type IN ('table') ORDER BY name").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn creates_both_databases_with_the_appendix_c_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vaults").join("0123456789abcdef0123456789abcdef");
        let dbs = open_databases(&dir).unwrap();
        let pla = tables(&dbs.pla);
        for t in ["block", "task", "metric_record", "rejection", "review_item", "job_run", "qa_turn", "schema_version"] {
            assert!(pla.contains(&t.to_string()), "pla.db missing {t}");
        }
        let cache = tables(&dbs.cache);
        for t in ["note_index", "note_fts", "link", "chunk", "daily_summary"] {
            assert!(cache.contains(&t.to_string()), "cache.db missing {t}");
        }
        assert!(dir.join("pla.db").is_file() && dir.join("cache.db").is_file());
    }

    #[test]
    fn uses_wal_and_foreign_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        let mode: String = dbs.pla.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let fk: i64 = dbs.pla.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        let err = dbs.pla.execute(
            "INSERT INTO task (task_id, title, origin, block_id, created_at, updated_at) VALUES ('t', 'x', 'extracted', 'no-such-block', 'now', 'now')",
            [],
        );
        assert!(err.is_err(), "foreign key must be enforced");
    }

    #[test]
    fn reopening_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        drop(open_databases(tmp.path()).unwrap());
        let dbs = open_databases(tmp.path()).unwrap();
        let n: i64 = dbs.pla.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
        assert_eq!(n, PLA_MIGRATIONS.len() as i64);
    }

    #[test]
    fn full_text_search_finds_turkish_words() {
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        dbs.cache.execute("INSERT INTO note_fts (note_path, title, body) VALUES ('a.md', 'Randevu', 'yarın dişçi var')", []).unwrap();
        let hit: String = dbs.cache.query_row("SELECT note_path FROM note_fts WHERE note_fts MATCH 'dişçi'", [], |r| r.get(0)).unwrap();
        assert_eq!(hit, "a.md");
    }

    #[test]
    fn upgrade_backs_up_first_and_newer_schema_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.db");
        let backup = tmp.path().join("x.before-upgrade.db");
        let v1 = ["CREATE TABLE a (id INTEGER);"];
        let v2 = ["CREATE TABLE a (id INTEGER);", "CREATE TABLE b (id INTEGER);"];

        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn, &v1, Some(&backup)).unwrap();
        assert!(!backup.exists(), "fresh database needs no backup");
        migrate(&mut conn, &v2, Some(&backup)).unwrap();
        assert!(backup.is_file(), "backup must exist before upgrading");

        // Review Focus 5: an older app opening a newer database
        let err = migrate(&mut conn, &v1, Some(&backup)).unwrap_err();
        assert!(matches!(err, DbError::NewerSchema { found: 2, supported: 1 }));
    }

    #[test]
    fn writers_wait_instead_of_failing_when_busy() {
        // Final review I6
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        let ms: i64 = dbs.pla.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).unwrap();
        assert!(ms >= 5000, "busy_timeout {ms}");
    }
}
