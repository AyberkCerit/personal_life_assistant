//! Keeping pla.db's `block` table in step with a note (FR-EXT-005…007, -009, -020).

use chrono::{DateTime, FixedOffset, NaiveDate};
use rusqlite::{params, Connection};

use super::matching::{match_blocks, OldBlock};
use crate::notes::Block;

#[derive(Debug, Clone, PartialEq)]
pub struct SyncedBlock {
    pub block_id: String,
    pub text: String,
    pub reference_date: NaiveDate,
    pub needs_extraction: bool,
}

struct Stored {
    block: OldBlock,
    extracted_hash: Option<String>,
    first_seen_at: String,
    missing: bool,
}

/// FNV-1a 64-bit — only answers "did this block change?" (FR-EXT-007), not a security hash.
pub fn text_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub fn sync_note_blocks(
    conn: &Connection,
    note_path: &str,
    blocks: &[Block],
    daily_date: Option<NaiveDate>,
    now: DateTime<FixedOffset>,
    threshold: f64,
) -> rusqlite::Result<Vec<SyncedBlock>> {
    let stored = load(conn, note_path)?;
    let old: Vec<OldBlock> = stored.iter().map(|s| s.block.clone()).collect();
    let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
    let m = match_blocks(&old, &texts, threshold);
    let now_s = now.to_rfc3339();
    let mut out: Vec<Option<SyncedBlock>> = vec![None; blocks.len()];

    for (i, id) in &m.matched {
        let s = stored.iter().find(|s| &s.block.id == id).expect("matched id comes from stored");
        let b = &blocks[*i];
        let hash = text_hash(&b.text);
        conn.execute(
            "UPDATE block SET position = ?1, text = ?2, text_hash = ?3, last_seen_at = ?4, missing = 0 WHERE block_id = ?5",
            params![b.position as i64, b.text, hash, now_s, id],
        )?;
        if s.missing {
            set_source_missing(conn, id, false)?;
        }
        let first_seen = DateTime::parse_from_rfc3339(&s.first_seen_at).map_or(now.date_naive(), |t| t.date_naive());
        out[*i] = Some(SyncedBlock {
            block_id: id.clone(),
            text: b.text.clone(),
            reference_date: daily_date.unwrap_or(first_seen),
            needs_extraction: s.extracted_hash.as_deref() != Some(hash.as_str()),
        });
    }
    for &i in &m.unmatched_new {
        let b = &blocks[i];
        let id = uuid::Uuid::new_v4().simple().to_string();
        conn.execute(
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, note_path, b.position as i64, b.text, text_hash(&b.text), now_s],
        )?;
        out[i] = Some(SyncedBlock {
            block_id: id,
            text: b.text.clone(),
            reference_date: daily_date.unwrap_or(now.date_naive()),
            needs_extraction: true,
        });
    }
    for id in &m.unmatched_old {
        let was_missing = stored.iter().any(|s| &s.block.id == id && s.missing);
        if !was_missing {
            conn.execute("UPDATE block SET missing = 1 WHERE block_id = ?1", [id])?;
            set_source_missing(conn, id, true)?;
        }
    }
    Ok(out.into_iter().map(|b| b.expect("every new block is matched or inserted")).collect())
}

pub fn mark_extracted(conn: &Connection, block_id: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE block SET extracted_hash = text_hash WHERE block_id = ?1", [block_id])?;
    Ok(())
}

/// FR-VLT-019: the note is gone; its items stay, marked as having lost their source.
pub fn mark_note_missing(conn: &Connection, note_path: &str) -> rusqlite::Result<usize> {
    let ids: Vec<String> = conn
        .prepare("SELECT block_id FROM block WHERE note_path = ?1 AND missing = 0")?
        .query_map([note_path], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in &ids {
        conn.execute("UPDATE block SET missing = 1 WHERE block_id = ?1", [id])?;
        set_source_missing(conn, id, true)?;
    }
    Ok(ids.len())
}

fn set_source_missing(conn: &Connection, block_id: &str, missing: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE task SET source_missing = ?1 WHERE block_id = ?2", params![missing, block_id])?;
    conn.execute("UPDATE metric_record SET source_missing = ?1 WHERE block_id = ?2", params![missing, block_id])?;
    Ok(())
}

fn load(conn: &Connection, note_path: &str) -> rusqlite::Result<Vec<Stored>> {
    conn.prepare(
        "SELECT block_id, position, text, extracted_hash, first_seen_at, missing FROM block WHERE note_path = ?1 ORDER BY position",
    )?
    .query_map([note_path], |r| {
        Ok(Stored {
            block: OldBlock { id: r.get(0)?, position: r.get::<_, i64>(1)? as usize, text: r.get(2)? },
            extracted_hash: r.get(3)?,
            first_seen_at: r.get(4)?,
            missing: r.get(5)?,
        })
    })?
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;
    use crate::notes::split_blocks;

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    const T1: &str = "2026-10-06T10:00:00+03:00";
    const T2: &str = "2026-10-09T18:30:00+03:00";

    fn sync(conn: &Connection, note: &str, text: &str, daily: Option<NaiveDate>, now: &str) -> Vec<SyncedBlock> {
        sync_note_blocks(conn, note, &split_blocks(text), daily, at(now), 0.6).unwrap()
    }

    fn insert_task(conn: &Connection, id: &str, block_id: &str) {
        conn.execute(
            "INSERT INTO task (task_id, title, origin, block_id, item_signature, created_at, updated_at) VALUES (?1, 'x', 'extracted', ?2, 'action:x', 'now', 'now')",
            [id, block_id],
        )
        .unwrap();
    }

    #[test]
    fn new_blocks_need_extraction_and_get_reference_dates() {
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        let daily = sync(&dbs.pla, "daily/2026/2026-10-01.md", "Yarın spor.", Some(d("2026-10-01")), T1);
        assert_eq!(daily[0].reference_date, d("2026-10-01"), "daily note: the note's date");
        assert!(daily[0].needs_extraction);
        let other = sync(&dbs.pla, "notes/x.md", "Yarın spor.", None, T1);
        assert_eq!(other[0].reference_date, d("2026-10-06"), "other notes: first seen");
    }

    #[test]
    fn unchanged_extracted_blocks_are_skipped_and_keep_first_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        let first = sync(&dbs.pla, "notes/x.md", "Yarın spor.\n\nCuma fatura.", None, T1);
        for b in &first {
            mark_extracted(&dbs.pla, &b.block_id).unwrap();
        }
        let again = sync(&dbs.pla, "notes/x.md", "Yarın spor.\n\nCuma fatura.", None, T2);
        assert_eq!(again.iter().map(|b| &b.block_id).collect::<Vec<_>>(), first.iter().map(|b| &b.block_id).collect::<Vec<_>>());
        assert!(again.iter().all(|b| !b.needs_extraction));
        assert_eq!(again[0].reference_date, d("2026-10-06"), "reference stays the first-seen date");
    }

    #[test]
    fn an_edited_block_keeps_its_id_and_needs_extraction() {
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        let first = sync(&dbs.pla, "notes/x.md", "Yarın 9da dişci var.", None, T1);
        mark_extracted(&dbs.pla, &first[0].block_id).unwrap();
        let edited = sync(&dbs.pla, "notes/x.md", "Yarın 9'da dişçi var.", None, T2);
        assert_eq!(edited[0].block_id, first[0].block_id);
        assert!(edited[0].needs_extraction);
    }

    #[test]
    fn removed_blocks_mark_their_items_and_come_back() {
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        let first = sync(&dbs.pla, "notes/x.md", "Yarın spor.\n\nCuma fatura öde.", None, T1);
        insert_task(&dbs.pla, "t1", &first[1].block_id);

        sync(&dbs.pla, "notes/x.md", "Yarın spor.", None, T2);
        let flag: i64 = dbs.pla.query_row("SELECT source_missing FROM task WHERE task_id='t1'", [], |r| r.get(0)).unwrap();
        assert_eq!(flag, 1, "FR-EXT-020: source deleted, item kept");

        let back = sync(&dbs.pla, "notes/x.md", "Yarın spor.\n\nCuma fatura öde.", None, T2);
        assert_eq!(back[1].block_id, first[1].block_id);
        let flag: i64 = dbs.pla.query_row("SELECT source_missing FROM task WHERE task_id='t1'", [], |r| r.get(0)).unwrap();
        assert_eq!(flag, 0);
    }

    #[test]
    fn deleted_note_marks_everything_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dbs = open_databases(tmp.path()).unwrap();
        let first = sync(&dbs.pla, "notes/x.md", "Cuma fatura öde.", None, T1);
        insert_task(&dbs.pla, "t1", &first[0].block_id);
        assert_eq!(mark_note_missing(&dbs.pla, "notes/x.md").unwrap(), 1);
        let (task, block): (i64, i64) = dbs
            .pla
            .query_row("SELECT t.source_missing, b.missing FROM task t JOIN block b USING (block_id)", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((task, block), (1, 1));
    }

    #[test]
    fn hash_is_stable_and_distinguishes_text() {
        assert_eq!(text_hash("abc"), text_hash("abc"));
        assert_ne!(text_hash("abc"), text_hash("abd"));
        assert_eq!(text_hash("").len(), 16);
    }
}
