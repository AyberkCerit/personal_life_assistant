//! Merging one block's extraction into pla.db (FR-EXT-013…019, -021; FR-TSK-011; FR-MET-002, -007).
//! Extraction adds and updates; it never deletes, and never touches what the user changed.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

use crate::extraction::{validate, MetricKind, RawExtraction, RawItem, Unit, ValidItem, ValidationSettings};

const MAX_TITLE_CHARS: usize = 200;
const DEFAULT_REMINDER_TIME: (u32, u32) = (9, 0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemRef {
    Task(String),
    Metric(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Added(ItemRef),
    Updated(ItemRef),
    KeptUserVersion(ItemRef),
    Rejected { signature: String },
    SkippedPast,
    Review { review_id: String, reason: String },
}

struct Existing {
    item: ItemRef,
    signature: String,
    user_owned: bool,
    claimed: bool,
}

pub fn metric_kind_name(kind: MetricKind) -> &'static str {
    match kind {
        MetricKind::Sleep => "sleep",
        MetricKind::Water => "water",
        MetricKind::Steps => "steps",
        MetricKind::Weight => "weight",
        MetricKind::Workout => "workout",
    }
}

fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::H => "h",
        Unit::Min => "min",
        Unit::Ml => "ml",
        Unit::L => "l",
        Unit::Glass => "glass",
        Unit::Count => "count",
        Unit::Kg => "kg",
        Unit::Lb => "lb",
    }
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// "The same item" within a block (SRS C.1): type plus normalized title, metric kind, or workout exercise.
pub fn item_signature(item: &ValidItem) -> String {
    match item {
        ValidItem::Action { title, .. } => format!("action:{}", normalize(title)),
        ValidItem::Metric { kind: MetricKind::Workout, exercise, .. } => {
            format!("metric:workout:{}", normalize(exercise.as_deref().unwrap_or_default()))
        }
        ValidItem::Metric { kind, .. } => format!("metric:{}", metric_kind_name(*kind)),
    }
}

/// Titles (or workout exercises) at least this similar (Jaro-Winkler) are the same item, reworded.
const SAME_ITEM_SIMILARITY: f64 = 0.8;

/// Splits a signature into its group (action, metric kind) and its label (title or exercise).
fn split_signature(signature: &str) -> (&str, &str) {
    if let Some(rest) = signature.strip_prefix("action:") {
        ("action", rest)
    } else if let Some(rest) = signature.strip_prefix("metric:workout:") {
        ("metric:workout", rest)
    } else {
        (signature, "")
    }
}

/// How alike two signatures are: 0 across groups, otherwise the similarity of their labels.
fn signature_similarity(a: &str, b: &str) -> f64 {
    let ((ga, la), (gb, lb)) = (split_signature(a), split_signature(b));
    if ga == gb {
        strsim::jaro_winkler(la, lb)
    } else {
        0.0
    }
}

pub fn apply_block_items(
    conn: &Connection,
    block_id: &str,
    reference: NaiveDate,
    extraction: &RawExtraction,
    settings: &ValidationSettings,
    skip_past_before: Option<NaiveDate>,
    now: DateTime<FixedOffset>,
) -> rusqlite::Result<Vec<Outcome>> {
    let now_s = now.to_rfc3339();
    let mut outcomes = Vec::new();
    let mut accepted: Vec<(ValidItem, String)> = Vec::new();

    for raw in &extraction.items {
        match validate(raw, reference, settings) {
            Err(reason) => outcomes.push(review(conn, block_id, raw, &reason.to_string(), &now_s)?),
            Ok(ValidItem::Action { reminder: true, date: None, .. }) => {
                outcomes.push(review(conn, block_id, raw, "reminder without a date", &now_s)?)
            }
            Ok(ValidItem::Action { date: Some(d), .. }) if skip_past_before.is_some_and(|today| d < today) => {
                outcomes.push(Outcome::SkippedPast)
            }
            Ok(item) => {
                let signature = item_signature(&item);
                accepted.push((item, signature));
            }
        }
    }

    let rejections = load_rejections(conn, block_id)?;
    let mut existing = load_existing(conn, block_id, &now_s)?;
    let mut pairing: Vec<Option<usize>> = vec![None; accepted.len()];
    let mut rejected = vec![false; accepted.len()];

    // 1. the same item as before (identical signature)
    for (i, (_, signature)) in accepted.iter().enumerate() {
        if let Some(j) = existing.iter().position(|e| !e.claimed && &e.signature == signature) {
            existing[j].claimed = true;
            pairing[i] = Some(j);
        }
    }
    // 2. what the user rejected stays rejected, even when the model words it differently (FR-EXT-019)
    for (i, (_, signature)) in accepted.iter().enumerate() {
        if pairing[i].is_none() {
            rejected[i] = rejections.iter().any(|r| r == signature || signature_similarity(r, signature) >= SAME_ITEM_SIMILARITY);
        }
    }
    // 3. reworded items: most similar pair first (FR-EXT-016)
    let mut candidates = Vec::new();
    for (i, (_, signature)) in accepted.iter().enumerate().filter(|(i, _)| pairing[*i].is_none() && !rejected[*i]) {
        for (j, e) in existing.iter().enumerate().filter(|(_, e)| !e.claimed) {
            let score = signature_similarity(signature, &e.signature);
            if score >= SAME_ITEM_SIMILARITY {
                candidates.push((score, i, j));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (_, i, j) in candidates {
        if pairing[i].is_none() && !existing[j].claimed {
            existing[j].claimed = true;
            pairing[i] = Some(j);
        }
    }

    for (i, (item, signature)) in accepted.iter().enumerate() {
        outcomes.push(match pairing[i] {
            _ if rejected[i] => Outcome::Rejected { signature: signature.clone() },
            Some(j) if existing[j].user_owned => {
                adopt(conn, &existing[j].item, block_id)?;
                Outcome::KeptUserVersion(existing[j].item.clone())
            }
            Some(j) => {
                update_item(conn, &existing[j].item, block_id, item, signature, &now_s)?;
                Outcome::Updated(existing[j].item.clone())
            }
            None => Outcome::Added(insert_item(conn, block_id, item, signature, &now_s)?),
        });
    }
    Ok(outcomes)
}

pub fn record_unreadable_answer(
    conn: &Connection,
    block_id: &str,
    answer: &str,
    reason: &str,
    now: DateTime<FixedOffset>,
) -> rusqlite::Result<Outcome> {
    let payload = serde_json::json!({ "raw_answer": answer }).to_string();
    insert_review(conn, block_id, &payload, reason, &now.to_rfc3339())
}

/// A block the model could not handle (too long, refused, timed out): kept for the user to look at.
pub fn record_block_problem(conn: &Connection, block_id: &str, problem: &str, now: DateTime<FixedOffset>) -> rusqlite::Result<Outcome> {
    let payload = serde_json::json!({ "problem": problem }).to_string();
    insert_review(conn, block_id, &payload, problem, &now.to_rfc3339())
}

/// The user undid or deleted an extracted item: remember the rejection, then remove the item.
pub fn reject_item(conn: &Connection, item: &ItemRef, now: DateTime<FixedOffset>) -> rusqlite::Result<bool> {
    let (table, key, id) = match item {
        ItemRef::Task(id) => ("task", "task_id", id),
        ItemRef::Metric(id) => ("metric_record", "metric_id", id),
    };
    let found: Option<(Option<String>, Option<String>)> = conn
        .query_row(&format!("SELECT block_id, item_signature FROM {table} WHERE {key} = ?1"), [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((block_id, signature)) = found else { return Ok(false) };
    if let (Some(block_id), Some(signature)) = (block_id, signature) {
        conn.execute(
            "INSERT OR IGNORE INTO rejection (rejection_id, block_id, item_signature, rejected_at) VALUES (?1, ?2, ?3, ?4)",
            params![new_id(), block_id, signature, now.to_rfc3339()],
        )?;
    }
    conn.execute(&format!("DELETE FROM {table} WHERE {key} = ?1"), [id])?;
    Ok(true)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn load_rejections(conn: &Connection, block_id: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT item_signature FROM rejection WHERE block_id = ?1")?
        .query_map([block_id], |r| r.get(0))?
        .collect()
}

fn review(conn: &Connection, block_id: &str, raw: &RawItem, reason: &str, now: &str) -> rusqlite::Result<Outcome> {
    let payload = serde_json::to_string(raw).expect("raw items serialize");
    insert_review(conn, block_id, &payload, reason, now)
}

fn insert_review(conn: &Connection, block_id: &str, payload: &str, reason: &str, now: &str) -> rusqlite::Result<Outcome> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT review_id FROM review_item WHERE block_id = ?1 AND payload_json = ?2 AND resolved = 0",
            [block_id, payload],
            |r| r.get(0),
        )
        .optional()?;
    let review_id = match existing {
        Some(id) => id,
        None => {
            let id = new_id();
            conn.execute(
                "INSERT INTO review_item (review_id, block_id, payload_json, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, block_id, payload, reason, now],
            )?;
            id
        }
    };
    Ok(Outcome::Review { review_id, reason: reason.to_owned() })
}

/// This block's extracted items, plus those of blocks of the same note that disappeared in this
/// same sync — so splitting or merging paragraphs moves items instead of duplicating them.
fn load_existing(conn: &Connection, block_id: &str, now: &str) -> rusqlite::Result<Vec<Existing>> {
    const SOURCES: &str = "(block_id = ?1 OR block_id IN (SELECT o.block_id FROM block o JOIN block b ON o.note_path = b.note_path
                           WHERE b.block_id = ?1 AND o.missing = 1 AND o.missing_since = ?2))";
    let mut out: Vec<Existing> = conn
        .prepare(&format!(
            "SELECT task_id, COALESCE(item_signature, ''), user_modified, status FROM task WHERE origin = 'extracted' AND {SOURCES}"
        ))?
        .query_map([block_id, now], |r| {
            let status: String = r.get(3)?;
            Ok(Existing {
                item: ItemRef::Task(r.get(0)?),
                signature: r.get(1)?,
                user_owned: r.get::<_, bool>(2)? || status != "open",
                claimed: false,
            })
        })?
        .collect::<Result<_, _>>()?;
    let metrics: Vec<Existing> = conn
        .prepare(&format!(
            "SELECT metric_id, COALESCE(item_signature, ''), user_modified FROM metric_record WHERE origin = 'extracted' AND {SOURCES}"
        ))?
        .query_map([block_id, now], |r| {
            Ok(Existing { item: ItemRef::Metric(r.get(0)?), signature: r.get(1)?, user_owned: r.get(2)?, claimed: false })
        })?
        .collect::<Result<_, _>>()?;
    out.extend(metrics);
    Ok(out)
}

/// A user-owned item found in a moved block: keep it as the user left it, but follow its source.
fn adopt(conn: &Connection, item: &ItemRef, block_id: &str) -> rusqlite::Result<()> {
    match item {
        ItemRef::Task(id) => conn.execute("UPDATE task SET block_id = ?1, source_missing = 0 WHERE task_id = ?2", [block_id, id])?,
        ItemRef::Metric(id) => {
            conn.execute("UPDATE metric_record SET block_id = ?1, source_missing = 0 WHERE metric_id = ?2", [block_id, id])?
        }
    };
    Ok(())
}

struct TaskFields {
    title: String,
    date: Option<String>,
    time: Option<String>,
    notify_at: Option<String>,
}

fn task_fields(reminder: bool, title: &str, date: Option<NaiveDate>, time: Option<NaiveTime>) -> TaskFields {
    let default_time = NaiveTime::from_hms_opt(DEFAULT_REMINDER_TIME.0, DEFAULT_REMINDER_TIME.1, 0).expect("valid time");
    TaskFields {
        title: title.chars().take(MAX_TITLE_CHARS).collect(),
        date: date.map(|d| d.format("%Y-%m-%d").to_string()),
        time: time.map(|t| t.format("%H:%M").to_string()),
        notify_at: match (reminder, date) {
            (true, Some(d)) => Some(format!("{}T{}", d.format("%Y-%m-%d"), time.unwrap_or(default_time).format("%H:%M"))),
            _ => None,
        },
    }
}

fn metric_value_json(value: Option<f64>, exercise: &Option<String>, sets: Option<i64>, reps: Option<i64>) -> String {
    let mut map = Map::new();
    if let Some(v) = value {
        map.insert("value".into(), Value::from(v));
    }
    if let Some(e) = exercise {
        map.insert("exercise".into(), Value::from(e.clone()));
    }
    if let Some(s) = sets {
        map.insert("sets".into(), Value::from(s));
    }
    if let Some(r) = reps {
        map.insert("reps".into(), Value::from(r));
    }
    Value::Object(map).to_string()
}

fn insert_item(conn: &Connection, block_id: &str, item: &ValidItem, signature: &str, now: &str) -> rusqlite::Result<ItemRef> {
    let id = new_id();
    match item {
        ValidItem::Action { reminder, title, date, time } => {
            let f = task_fields(*reminder, title, *date, *time);
            conn.execute(
                "INSERT INTO task (task_id, title, date, time, notify_at, status, origin, block_id, item_signature, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'open', 'extracted', ?6, ?7, ?8, ?8)",
                params![id, f.title, f.date, f.time, f.notify_at, block_id, signature, now],
            )?;
            Ok(ItemRef::Task(id))
        }
        ValidItem::Metric { kind, date, value, unit, exercise, sets, reps } => {
            conn.execute(
                "INSERT INTO metric_record (metric_id, type, value_json, unit, date, origin, block_id, item_signature, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'extracted', ?6, ?7, ?8)",
                params![
                    id,
                    metric_kind_name(*kind),
                    metric_value_json(*value, exercise, *sets, *reps),
                    unit.map(unit_name),
                    date.format("%Y-%m-%d").to_string(),
                    block_id,
                    signature,
                    now
                ],
            )?;
            Ok(ItemRef::Metric(id))
        }
    }
}

fn update_item(conn: &Connection, target: &ItemRef, block_id: &str, item: &ValidItem, signature: &str, now: &str) -> rusqlite::Result<()> {
    match (target, item) {
        (ItemRef::Task(id), ValidItem::Action { reminder, title, date, time }) => {
            let f = task_fields(*reminder, title, *date, *time);
            conn.execute(
                "UPDATE task SET title = ?1, date = ?2, time = ?3, notify_at = ?4, item_signature = ?5, source_missing = 0, updated_at = ?6, block_id = ?8 WHERE task_id = ?7",
                params![f.title, f.date, f.time, f.notify_at, signature, now, id, block_id],
            )?;
        }
        (ItemRef::Metric(id), ValidItem::Metric { kind, date, value, unit, exercise, sets, reps }) => {
            conn.execute(
                "UPDATE metric_record SET type = ?1, value_json = ?2, unit = ?3, date = ?4, item_signature = ?5, source_missing = 0, block_id = ?7 WHERE metric_id = ?6",
                params![
                    metric_kind_name(*kind),
                    metric_value_json(*value, exercise, *sets, *reps),
                    unit.map(unit_name),
                    date.format("%Y-%m-%d").to_string(),
                    signature,
                    id,
                    block_id
                ],
            )?;
        }
        _ => unreachable!("pairing only joins items of the same group"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;
    use crate::extraction::parse_extraction;

    const NOW: &str = "2026-10-06T10:00:00+03:00";
    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(NOW).unwrap()
    }
    fn reference() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()
    }
    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_databases(tmp.path()).unwrap().pla;
        conn.execute(
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES ('b1', 'n.md', 0, 'x', 'h', ?1, ?1)",
            [NOW],
        )
        .unwrap();
        (tmp, conn)
    }
    fn apply(conn: &Connection, json: &str) -> Vec<Outcome> {
        let ex = parse_extraction(&format!(r#"{{"items": [{json}]}}"#)).unwrap();
        apply_block_items(conn, "b1", reference(), &ex, &ValidationSettings::default(), None, now()).unwrap()
    }
    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }
    fn task_row(conn: &Connection) -> (String, Option<String>, Option<String>, Option<String>) {
        conn.query_row("SELECT title, date, time, notify_at FROM task", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap()
    }

    const DENTIST: &str = r#"{"type":"task","title":"Dişçi","when":{"day_offset":1,"time":"09:00"}}"#;

    #[test]
    fn adds_a_dated_task_with_its_source() {
        let (_t, conn) = setup();
        let out = apply(&conn, DENTIST);
        assert!(matches!(&out[..], [Outcome::Added(ItemRef::Task(_))]));
        assert_eq!(task_row(&conn), ("Dişçi".into(), Some("2026-10-07".into()), Some("09:00".into()), None));
        let (origin, block, sig): (String, String, String) =
            conn.query_row("SELECT origin, block_id, item_signature FROM task", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((origin.as_str(), block.as_str(), sig.as_str()), ("extracted", "b1", "action:dişçi"));
    }

    #[test]
    fn reapplying_updates_instead_of_duplicating() {
        let (_t, conn) = setup();
        apply(&conn, DENTIST);
        let out = apply(&conn, DENTIST);
        assert!(matches!(&out[..], [Outcome::Updated(ItemRef::Task(_))]));
        assert_eq!(count(&conn, "task"), 1);
    }

    #[test]
    fn a_reworded_single_item_updates_the_existing_one() {
        // Review Focus 1: FR-EXT-016 when the user fixes the wording
        let (_t, conn) = setup();
        apply(&conn, DENTIST);
        apply(&conn, r#"{"type":"task","title":"Dişçi randevusu","when":{"day_offset":2,"time":"10:00"}}"#);
        assert_eq!(count(&conn, "task"), 1);
        assert_eq!(task_row(&conn), ("Dişçi randevusu".into(), Some("2026-10-08".into()), Some("10:00".into()), None));
    }

    #[test]
    fn user_edited_or_completed_items_win() {
        // Review Focus 2: FR-EXT-017
        let (_t, conn) = setup();
        apply(&conn, DENTIST);
        conn.execute("UPDATE task SET title = 'Benim başlığım', user_modified = 1", []).unwrap();
        let out = apply(&conn, r#"{"type":"task","title":"Dişçi","when":{"day_offset":3}}"#);
        assert!(matches!(&out[..], [Outcome::KeptUserVersion(_)]));
        assert_eq!(task_row(&conn).0, "Benim başlığım");

        conn.execute("UPDATE task SET user_modified = 0, status = 'done'", []).unwrap();
        let out = apply(&conn, DENTIST);
        assert!(matches!(&out[..], [Outcome::KeptUserVersion(_)]));
        assert_eq!(count(&conn, "task"), 1);
    }

    #[test]
    fn rejected_items_are_not_recreated() {
        // Review Focus 2: FR-EXT-018/019
        let (_t, conn) = setup();
        let Outcome::Added(item) = apply(&conn, DENTIST).remove(0) else { panic!("expected Added") };
        assert!(reject_item(&conn, &item, now()).unwrap());
        assert_eq!(count(&conn, "task"), 0);
        assert_eq!(count(&conn, "rejection"), 1);
        let out = apply(&conn, DENTIST);
        assert_eq!(out, vec![Outcome::Rejected { signature: "action:dişçi".into() }]);
        assert_eq!(count(&conn, "task"), 0);
        assert!(!reject_item(&conn, &item, now()).unwrap(), "unknown item");
    }

    #[test]
    fn reminders_get_a_notification_time() {
        let (_t, conn) = setup();
        apply(&conn, r#"{"type":"reminder","title":"İlaç","when":{"day_offset":0,"time":"21:30"}}"#);
        assert_eq!(task_row(&conn).3, Some("2026-10-06T21:30".into()));
        conn.execute("DELETE FROM task", []).unwrap();
        apply(&conn, r#"{"type":"reminder","title":"Fatura","when":{"weekday":"fri"}}"#);
        assert_eq!(task_row(&conn).3, Some("2026-10-09T09:00".into()), "date without time notifies at 09:00");
    }

    #[test]
    fn invalid_items_go_to_review_once() {
        let (_t, conn) = setup();
        let out = apply(&conn, r#"{"type":"metric","metric":{"kind":"sleep","value":30,"unit":"h"}}"#);
        assert!(matches!(&out[..], [Outcome::Review { reason, .. }] if reason.contains("plausible")), "{out:?}");
        let again = apply(&conn, r#"{"type":"metric","metric":{"kind":"sleep","value":30,"unit":"h"}}"#);
        assert_eq!(count(&conn, "review_item"), 1, "identical unresolved review item is reused");
        assert!(matches!((&out[0], &again[0]), (Outcome::Review { review_id: a, .. }, Outcome::Review { review_id: b, .. }) if a == b));
        let dateless = apply(&conn, r#"{"type":"reminder","title":"Ara"}"#);
        assert!(matches!(&dateless[..], [Outcome::Review { reason, .. }] if reason == "reminder without a date"));
        assert_eq!(count(&conn, "task"), 0);
    }

    #[test]
    fn metrics_are_stored_canonically_and_workouts_per_exercise() {
        let (_t, conn) = setup();
        let ex = parse_extraction(
            r#"{"items": [
                {"type":"metric","when":{"day_offset":-1},"metric":{"kind":"water","value":4,"unit":"glass"}},
                {"type":"metric","metric":{"kind":"workout","exercise":"Squat","sets":3,"reps":8,"value":80,"unit":"kg"}},
                {"type":"metric","metric":{"kind":"workout","exercise":"Bench","sets":3,"reps":10,"value":60,"unit":"kg"}}
            ]}"#,
        )
        .unwrap();
        apply_block_items(&conn, "b1", reference(), &ex, &ValidationSettings::default(), None, now()).unwrap();
        let rows: Vec<(String, String, Option<String>, String, String)> = conn
            .prepare("SELECT type, value_json, unit, date, item_signature FROM metric_record ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows[0], ("water".into(), r#"{"value":1000.0}"#.into(), Some("ml".into()), "2026-10-05".into(), "metric:water".into()));
        assert_eq!(rows[1].1, r#"{"value":80.0,"exercise":"Squat","sets":3,"reps":8}"#);
        assert_eq!(rows[1].4, "metric:workout:squat");
        assert_eq!(rows[2].4, "metric:workout:bench");
    }

    #[test]
    fn unreadable_answers_are_kept_for_review() {
        let (_t, conn) = setup();
        let out = record_unreadable_answer(&conn, "b1", r#"{"items": [{"type": "ta"#, "EOF while parsing", now()).unwrap();
        assert!(matches!(out, Outcome::Review { .. }));
        let payload: String = conn.query_row("SELECT payload_json FROM review_item", [], |r| r.get(0)).unwrap();
        assert!(payload.contains("raw_answer"));
    }

    #[test]
    fn several_reworded_items_update_their_own_counterparts() {
        // Final review I2(a)
        let (_t, conn) = setup();
        apply(&conn, r#"{"type":"task","title":"Dişçi"},{"type":"task","title":"Market"}"#);
        apply(&conn, r#"{"type":"task","title":"Markete git"},{"type":"task","title":"Dişçiye git"}"#);
        let titles: Vec<String> = conn.prepare("SELECT title FROM task ORDER BY title").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(titles, vec!["Dişçiye git", "Markete git"]);
    }

    #[test]
    fn a_reworded_rejected_item_stays_rejected() {
        // Final review I2(b): FR-EXT-017/019
        let (_t, conn) = setup();
        let Outcome::Added(item) = apply(&conn, DENTIST).remove(0) else { panic!("expected Added") };
        reject_item(&conn, &item, now()).unwrap();
        let out = apply(&conn, r#"{"type":"task","title":"Dişçiye git","when":{"day_offset":1}}"#);
        assert!(matches!(&out[..], [Outcome::Rejected { .. }]), "{out:?}");
        assert_eq!(count(&conn, "task"), 0);
    }

    #[test]
    fn an_unrelated_item_is_not_taken_for_the_old_one() {
        // Final review I2(c): the user finished one appointment and reused the line for another
        let (_t, conn) = setup();
        apply(&conn, r#"{"type":"task","title":"Dişçi randevusu"}"#);
        conn.execute("UPDATE task SET status = 'done'", []).unwrap();
        let out = apply(&conn, r#"{"type":"task","title":"Kuaför randevusu"}"#);
        assert!(matches!(&out[..], [Outcome::Added(_)]), "{out:?}");
        assert_eq!(count(&conn, "task"), 2);
    }

    #[test]
    fn first_sight_skips_past_actions_but_keeps_metrics() {
        // Decision 3 (2026-10-02)
        let (_t, conn) = setup();
        let ex = parse_extraction(
            r#"{"items": [
                {"type":"task","title":"Eski iş","when":{"day_offset":-3}},
                {"type":"task","title":"Yeni iş","when":{"day_offset":2}},
                {"type":"metric","when":{"day_offset":-3},"metric":{"kind":"sleep","value":7,"unit":"h"}}
            ]}"#,
        )
        .unwrap();
        let out = apply_block_items(&conn, "b1", reference(), &ex, &ValidationSettings::default(), Some(reference()), now()).unwrap();
        assert_eq!(out[0], Outcome::SkippedPast);
        assert_eq!(count(&conn, "task"), 1);
        assert_eq!(count(&conn, "metric_record"), 1);
    }

    #[test]
    fn signatures_ignore_case_and_spacing() {
        let a = ValidItem::Action { reminder: false, title: "  Dişçi   Randevusu ".into(), date: None, time: None };
        let b = ValidItem::Action { reminder: true, title: "dişçi randevusu".into(), date: None, time: None };
        assert_eq!(item_signature(&a), item_signature(&b));
    }
}
