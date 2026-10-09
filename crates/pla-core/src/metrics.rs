//! Health and productivity metrics (SRS §3.8): daily values by the merge rules, statistics and
//! manual entry. All numbers the UI or the model show are computed here (FR-MET-010).

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::extraction::{canonical_value, MetricKind, RawMetric, Unit, ValidationSettings};
use crate::pipeline::items::{metric_kind_name, reject_item, ItemRef};

/// The order of the overview cards.
pub const KINDS: [MetricKind; 5] = [MetricKind::Sleep, MetricKind::Weight, MetricKind::Steps, MetricKind::Water, MetricKind::Workout];

#[derive(Debug, thiserror::Error)]
pub enum MetricError {
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error("not_found")]
    NotFound,
    #[error("invalid_date")]
    InvalidDate,
    #[error("missing_value")]
    MissingValue,
    #[error("empty_workout")]
    EmptyWorkout,
    /// FR-MET-008: outside the plausible range; the user must confirm. The field (`value`, `sets`,
    /// `reps`, `kg`) and its value in canonical units.
    #[error("out_of_range|{0}|{1}")]
    OutOfRange(&'static str, f64),
    /// Negative or not a number: no confirmation makes it a measurement.
    #[error("invalid_value")]
    InvalidValue,
}

/// One stored record, as the detail list shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricRecord {
    pub metric_id: String,
    pub kind: MetricKind,
    pub date: String,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub exercise: Option<String>,
    pub sets: Option<i64>,
    pub reps: Option<i64>,
    pub origin: String,
    pub user_modified: bool,
    pub note_path: Option<String>,
    pub block_text: Option<String>,
    /// The note it came from no longer says it (SRS C.1): kept, marked "source deleted".
    pub source_missing: bool,
    pub created_at: String,
}

/// A day's value by the merge rules (FR-MET-004…006). For a workout `value` is the number of
/// sessions and `sets` their total.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayValue {
    pub date: String,
    pub value: f64,
    pub sets: Option<i64>,
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeekAverage {
    /// The Monday the week starts on.
    pub week_start: String,
    pub average: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub kind: MetricKind,
    pub days: Vec<DayValue>,
    pub average: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// This period's average minus the previous period's of the same length.
    pub trend: Option<f64>,
    pub weekly: Vec<WeekAverage>,
    pub conflicts: usize,
}

/// One overview card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Card {
    pub kind: MetricKind,
    pub last: Option<DayValue>,
    pub average7: Option<f64>,
    /// The last 14 days, oldest first; `None` where nothing was recorded.
    pub spark: Vec<Option<f64>>,
    pub conflicts: usize,
}

/// A manual record from the quick entry or an edit (FR-MET-003).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MetricInput {
    pub kind: MetricKind,
    pub date: String,
    pub value: Option<f64>,
    pub unit: Option<Unit>,
    pub exercise: Option<String>,
    pub sets: Option<i64>,
    pub reps: Option<i64>,
    /// The user confirmed a value outside the plausible range (FR-MET-008).
    #[serde(default)]
    pub confirmed: bool,
}

const DATE: &str = "%Y-%m-%d";

fn kind_of(name: &str) -> MetricKind {
    match name {
        "weight" => MetricKind::Weight,
        "steps" => MetricKind::Steps,
        "water" => MetricKind::Water,
        "workout" => MetricKind::Workout,
        _ => MetricKind::Sleep,
    }
}

/// The unit a kind is stored in (E-D7): sleep h, water ml, steps count, weight and workout kg.
fn canonical_unit_name(kind: MetricKind) -> &'static str {
    match kind {
        MetricKind::Sleep => "h",
        MetricKind::Water => "ml",
        MetricKind::Steps => "count",
        MetricKind::Weight | MetricKind::Workout => "kg",
    }
}

pub fn records(conn: &Connection, kind: MetricKind, from: NaiveDate, to: NaiveDate) -> Result<Vec<MetricRecord>, MetricError> {
    let rows = conn
        .prepare(
            "SELECT m.metric_id, m.type, m.date, m.value_json, m.unit, m.origin, m.user_modified, b.note_path, b.text, m.created_at, m.source_missing
             FROM metric_record m LEFT JOIN block b ON b.block_id = m.block_id
             WHERE m.type = ?1 AND m.date BETWEEN ?2 AND ?3
             ORDER BY m.date DESC, m.created_at DESC, m.rowid DESC",
        )?
        .query_map(params![metric_kind_name(kind), from.format(DATE).to_string(), to.format(DATE).to_string()], |r| {
            let json: serde_json::Value = serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default();
            Ok(MetricRecord {
                metric_id: r.get(0)?,
                kind: kind_of(&r.get::<_, String>(1)?),
                date: r.get(2)?,
                value: json.get("value").and_then(|v| v.as_f64()),
                unit: r.get(4)?,
                exercise: json.get("exercise").and_then(|v| v.as_str()).map(str::to_owned),
                sets: json.get("sets").and_then(|v| v.as_i64()),
                reps: json.get("reps").and_then(|v| v.as_i64()),
                origin: r.get(5)?,
                user_modified: r.get(6)?,
                note_path: r.get(7)?,
                block_text: r.get(8)?,
                created_at: r.get(9)?,
                source_missing: r.get(10)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// A record counts as the user's when they entered it, the assistant did on their word, or they
/// edited an extracted one (FR-MET-005).
fn is_users(r: &MetricRecord) -> bool {
    r.origin != "extracted" || r.user_modified
}

pub fn daily_values(conn: &Connection, kind: MetricKind, from: NaiveDate, to: NaiveDate) -> Result<Vec<DayValue>, MetricError> {
    let mut all = records(conn, kind, from, to)?;
    all.reverse(); // oldest first: the last record of a day is its newest
    let mut days: Vec<DayValue> = Vec::new();
    let mut start = 0;
    while start < all.len() {
        let date = all[start].date.clone();
        let end = all[start..].iter().position(|r| r.date != date).map_or(all.len(), |n| start + n);
        let day = &all[start..end];
        start = end;
        let value = match kind {
            MetricKind::Water => Some(DayValue { date, value: day.iter().filter_map(|r| r.value).sum(), sets: None, conflict: false }),
            MetricKind::Workout => {
                let sets = day.iter().filter_map(|r| r.sets).fold(None, |acc: Option<i64>, s| Some(acc.unwrap_or(0) + s));
                // a session is a note (all its exercises), and what was entered in PLA that day is one more
                let mut notes: Vec<&str> = day.iter().filter_map(|r| r.note_path.as_deref()).collect();
                notes.sort_unstable();
                notes.dedup();
                let entered = usize::from(day.iter().any(|r| r.note_path.is_none()));
                Some(DayValue { date, value: (notes.len() + entered) as f64, sets, conflict: false })
            }
            _ => {
                let users: Vec<f64> = day.iter().filter(|r| is_users(r)).filter_map(|r| r.value).collect();
                let extracted: Vec<f64> = day.iter().filter(|r| !is_users(r)).filter_map(|r| r.value).collect();
                match users.last() {
                    Some(v) => Some(DayValue { date, value: *v, sets: None, conflict: false }),
                    None => extracted.last().map(|v| DayValue { date, value: *v, sets: None, conflict: extracted.iter().any(|x| (x - v).abs() > 1e-9) }),
                }
            }
        };
        days.extend(value);
    }
    Ok(days)
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / n as f64)
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// FR-MET-009: the last `days` days up to `today`.
pub fn summary(conn: &Connection, kind: MetricKind, days: i64, today: NaiveDate) -> Result<Summary, MetricError> {
    let from = today - Duration::days(days - 1);
    let values = daily_values(conn, kind, from, today)?;
    let before = daily_values(conn, kind, from - Duration::days(days), from - Duration::days(1))?;
    let average = mean(values.iter().map(|v| v.value));
    let mut weeks: Vec<(NaiveDate, Vec<f64>)> = Vec::new();
    for v in &values {
        let monday = monday_of(NaiveDate::parse_from_str(&v.date, DATE).map_err(|_| MetricError::InvalidDate)?);
        match weeks.last_mut() {
            Some((m, list)) if *m == monday => list.push(v.value),
            _ => weeks.push((monday, vec![v.value])),
        }
    }
    let weekly = weeks
        .into_iter()
        .map(|(monday, list)| WeekAverage { week_start: monday.format(DATE).to_string(), average: mean(list.into_iter()).unwrap_or_default() })
        .collect();
    Ok(Summary {
        kind,
        average,
        min: values.iter().map(|v| v.value).reduce(f64::min),
        max: values.iter().map(|v| v.value).reduce(f64::max),
        trend: average.zip(mean(before.iter().map(|v| v.value))).map(|(now, then)| now - then),
        weekly,
        conflicts: values.iter().filter(|v| v.conflict).count(),
        days: values,
    })
}

pub fn overview(conn: &Connection, today: NaiveDate) -> Result<Vec<Card>, MetricError> {
    KINDS
        .iter()
        .map(|&kind| {
            let from = today - Duration::days(13);
            let values = daily_values(conn, kind, from, today)?;
            let spark = (0..14)
                .map(|i| {
                    let date = (from + Duration::days(i)).format(DATE).to_string();
                    values.iter().find(|v| v.date == date).map(|v| v.value)
                })
                .collect();
            let week_start = (today - Duration::days(6)).format(DATE).to_string();
            let last = match values.last() {
                Some(v) => Some(v.clone()),
                None => last_older_day(conn, kind, from)?,
            };
            Ok(Card {
                kind,
                last,
                average7: mean(values.iter().filter(|v| v.date >= week_start).map(|v| v.value)),
                spark,
                conflicts: values.iter().filter(|v| v.conflict).count(),
            })
        })
        .collect()
}

/// The latest day with a record before `before`, for a card whose last two weeks are empty.
fn last_older_day(conn: &Connection, kind: MetricKind, before: NaiveDate) -> Result<Option<DayValue>, MetricError> {
    let date: Option<String> = conn.query_row(
        "SELECT max(date) FROM metric_record WHERE type = ?1 AND date < ?2",
        params![metric_kind_name(kind), before.format(DATE).to_string()],
        |r| r.get(0),
    )?;
    let Some(date) = date.and_then(|d| NaiveDate::parse_from_str(&d, DATE).ok()) else { return Ok(None) };
    Ok(daily_values(conn, kind, date, date)?.pop())
}

/// A checked record in canonical units: value, exercise, sets, reps.
type Checked = (Option<f64>, Option<String>, Option<i64>, Option<i64>);

fn check(input: &MetricInput, settings: &ValidationSettings, today: NaiveDate) -> Result<(NaiveDate, Checked), MetricError> {
    let date = NaiveDate::parse_from_str(input.date.trim(), DATE).map_err(|_| MetricError::InvalidDate)?;
    if date > today {
        return Err(MetricError::InvalidDate); // a future record would sit unseen until that day
    }
    let impossible = |v: f64| !v.is_finite() || v < 0.0;
    if input.value.is_some_and(impossible) || [input.sets, input.reps].iter().flatten().any(|n| *n < 0) {
        return Err(MetricError::InvalidValue);
    }
    let exercise = input.exercise.as_deref().map(str::trim).filter(|e| !e.is_empty()).map(str::to_owned);
    let raw = RawMetric { kind: Some(input.kind), exercise: exercise.clone(), sets: input.sets, reps: input.reps, value: input.value, unit: input.unit };
    let value = canonical_value(&raw, settings);
    if value.is_some_and(impossible) {
        return Err(MetricError::InvalidValue);
    }
    let out = |field: &'static str, v: f64| if input.confirmed { Ok(()) } else { Err(MetricError::OutOfRange(field, v)) };
    if input.kind == MetricKind::Workout {
        if input.sets.is_none() && input.reps.is_none() && value.is_none() {
            return Err(MetricError::EmptyWorkout);
        }
        // SRS §3.8.1: sets 1-50, reps 1-500, kg 0-500
        for (field, v, lo, hi) in [("sets", input.sets.map(|s| s as f64), 1.0, 50.0), ("reps", input.reps.map(|r| r as f64), 1.0, 500.0), ("kg", value, 0.0, 500.0)] {
            if let Some(v) = v.filter(|v| !(lo..=hi).contains(v)) {
                out(field, v)?;
            }
        }
    } else {
        let v = value.ok_or(MetricError::MissingValue)?;
        let (lo, hi) = crate::extraction::plausible_range(input.kind);
        if !(lo..=hi).contains(&v) {
            out("value", v)?;
        }
    }
    Ok((date, (value, exercise, input.sets, input.reps)))
}

pub fn log_metric(conn: &Connection, input: &MetricInput, settings: &ValidationSettings, now: DateTime<FixedOffset>) -> Result<String, MetricError> {
    log_metric_as(conn, input, settings, "manual", now)
}

/// `log_metric` with its origin: `manual`, or `assistant` for the Q&A panel's log_metric (FR-QA-008).
pub fn log_metric_as(conn: &Connection, input: &MetricInput, settings: &ValidationSettings, origin: &str, now: DateTime<FixedOffset>) -> Result<String, MetricError> {
    let (date, (value, exercise, sets, reps)) = check(input, settings, now.date_naive())?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    conn.execute(
        "INSERT INTO metric_record (metric_id, type, value_json, unit, date, origin, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?7, ?6)",
        params![
            id,
            metric_kind_name(input.kind),
            crate::pipeline::items::metric_value_json(value, &exercise, sets, reps),
            canonical_unit_name(input.kind),
            date.format(DATE).to_string(),
            now.to_rfc3339(),
            origin
        ],
    )?;
    Ok(id)
}

/// FR-MET-011: an edited record is the user's from now on; the pipeline leaves it alone.
pub fn edit_metric(conn: &Connection, id: &str, input: &MetricInput, settings: &ValidationSettings, now: DateTime<FixedOffset>) -> Result<(), MetricError> {
    let (date, (value, exercise, sets, reps)) = check(input, settings, now.date_naive())?;
    let changed = conn.execute(
        "UPDATE metric_record SET type = ?1, value_json = ?2, unit = ?3, date = ?4, user_modified = 1 WHERE metric_id = ?5",
        params![
            metric_kind_name(input.kind),
            crate::pipeline::items::metric_value_json(value, &exercise, sets, reps),
            canonical_unit_name(input.kind),
            date.format(DATE).to_string(),
            id
        ],
    )?;
    if changed == 0 {
        return Err(MetricError::NotFound);
    }
    Ok(())
}

/// FR-MET-011: a deleted extraction is remembered as rejected, so its note does not bring it back.
pub fn delete_metric(conn: &Connection, id: &str, now: DateTime<FixedOffset>) -> Result<(), MetricError> {
    let origin: Option<String> = conn.query_row("SELECT origin FROM metric_record WHERE metric_id = ?1", [id], |r| r.get(0)).optional()?;
    match origin.as_deref() {
        None => Err(MetricError::NotFound),
        Some("extracted") => {
            reject_item(conn, &ItemRef::Metric(id.to_owned()), now)?;
            Ok(())
        }
        Some(_) => {
            conn.execute("DELETE FROM metric_record WHERE metric_id = ?1", [id])?;
            Ok(())
        }
    }
}

/// FR-MET-006: the user picks the right value; the other extractions of that day are rejected.
pub fn resolve_conflict(conn: &Connection, keep_id: &str, now: DateTime<FixedOffset>) -> Result<(), MetricError> {
    let found: Option<(String, String)> =
        conn.query_row("SELECT type, date FROM metric_record WHERE metric_id = ?1", [keep_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let (kind, date) = found.ok_or(MetricError::NotFound)?;
    if kind == "water" || kind == "workout" {
        return Err(MetricError::NotFound); // these add up per day: there is nothing to choose between
    }
    let tx = conn.unchecked_transaction()?; // the worker writes on its own connection meanwhile
    let others: Vec<String> = tx
        .prepare("SELECT metric_id FROM metric_record WHERE type = ?1 AND date = ?2 AND metric_id <> ?3 AND origin = 'extracted' AND user_modified = 0")?
        .query_map(params![kind, date, keep_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in others {
        reject_item(&tx, &ItemRef::Metric(id), now)?;
    }
    tx.execute("UPDATE metric_record SET user_modified = 1 WHERE metric_id = ?1", [keep_id])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;

    const NOW: &str = "2026-10-06T10:00:00+03:00";

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
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES ('b1', 'daily/2026/2026-10-05.md', 0, 'Dün 6 saat uyudum.', 'h', ?1, ?1)",
            [NOW],
        )
        .unwrap();
        (tmp, conn)
    }

    /// A record as the pipeline or an earlier entry left it.
    #[allow(clippy::too_many_arguments)]
    fn put(conn: &Connection, id: &str, kind: &str, date: &str, value_json: &str, origin: &str, created: &str, user_modified: bool) {
        let (block, sig) = if origin == "extracted" { (Some("b1"), Some(format!("sig-{id}"))) } else { (None, None) };
        conn.execute(
            "INSERT INTO metric_record (metric_id, type, value_json, unit, date, origin, block_id, item_signature, user_modified, created_at) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, kind, value_json, date, origin, block, sig, user_modified, created],
        )
        .unwrap();
    }

    fn input(kind: MetricKind, value: Option<f64>, unit: Option<Unit>) -> MetricInput {
        MetricInput { kind, date: "2026-10-06".into(), value, unit, exercise: None, sets: None, reps: None, confirmed: false }
    }

    #[test]
    fn a_single_value_day_prefers_the_users_value_and_flags_disagreeing_extractions() {
        // FR-MET-004/005/006, Review Focus 1
        let (_tmp, conn) = setup();
        put(&conn, "m1", "sleep", "2026-10-01", r#"{"value":7}"#, "manual", "2026-10-01T08:00:00+03:00", false);
        put(&conn, "m2", "sleep", "2026-10-01", r#"{"value":6}"#, "extracted", "2026-10-01T09:00:00+03:00", false);
        put(&conn, "m3", "sleep", "2026-10-02", r#"{"value":6}"#, "extracted", "2026-10-02T08:00:00+03:00", false);
        put(&conn, "m4", "sleep", "2026-10-02", r#"{"value":8}"#, "extracted", "2026-10-02T09:00:00+03:00", false);
        put(&conn, "m5", "sleep", "2026-10-03", r#"{"value":6.5}"#, "extracted", "2026-10-03T08:00:00+03:00", true);
        put(&conn, "m6", "sleep", "2026-10-03", r#"{"value":5}"#, "extracted", "2026-10-03T09:00:00+03:00", false);
        put(&conn, "m7", "sleep", "2026-10-04", r#"{"value":7}"#, "extracted", "2026-10-04T08:00:00+03:00", false);
        put(&conn, "m8", "sleep", "2026-10-04", r#"{"value":7}"#, "extracted", "2026-10-04T09:00:00+03:00", false);
        let days = daily_values(&conn, MetricKind::Sleep, d("2026-10-01"), d("2026-10-06")).unwrap();
        let view: Vec<(&str, f64, bool)> = days.iter().map(|v| (v.date.as_str(), v.value, v.conflict)).collect();
        assert_eq!(
            view,
            vec![
                ("2026-10-01", 7.0, false), // manual wins even though older
                ("2026-10-02", 8.0, true),  // newest extraction, but they disagree
                ("2026-10-03", 6.5, false), // an edited extraction is the user's value
                ("2026-10-04", 7.0, false), // two extractions that agree are no conflict
            ]
        );
    }

    #[test]
    fn water_adds_up_and_workouts_count_sessions_and_sets() {
        // FR-MET-004
        let (_tmp, conn) = setup();
        put(&conn, "w1", "water", "2026-10-05", r#"{"value":750}"#, "extracted", "2026-10-05T08:00:00+03:00", false);
        put(&conn, "w2", "water", "2026-10-05", r#"{"value":500}"#, "manual", "2026-10-05T12:00:00+03:00", false);
        put(&conn, "x1", "workout", "2026-10-05", r#"{"exercise":"şınav","sets":3,"reps":10}"#, "extracted", "2026-10-05T08:00:00+03:00", false);
        put(&conn, "x2", "workout", "2026-10-05", r#"{"exercise":"squat","sets":4,"reps":8,"value":60}"#, "manual", "2026-10-05T18:00:00+03:00", false);
        // nl-quality: four exercises in one note were "4 sessions" in the owner's window; one note is one session
        put(&conn, "x3", "workout", "2026-10-05", r#"{"exercise":"row","sets":2,"reps":8}"#, "extracted", "2026-10-05T08:00:00+03:00", false);
        put(&conn, "x4", "workout", "2026-10-05", r#"{"exercise":"curl","sets":3,"reps":8}"#, "manual", "2026-10-05T18:05:00+03:00", false);
        let water = daily_values(&conn, MetricKind::Water, d("2026-10-01"), d("2026-10-06")).unwrap();
        assert_eq!(water, vec![DayValue { date: "2026-10-05".into(), value: 1250.0, sets: None, conflict: false }]);
        let workout = daily_values(&conn, MetricKind::Workout, d("2026-10-01"), d("2026-10-06")).unwrap();
        assert_eq!(workout, vec![DayValue { date: "2026-10-05".into(), value: 2.0, sets: Some(12), conflict: false }]);
    }

    #[test]
    fn a_summary_has_averages_extremes_trend_and_monday_weeks() {
        // FR-MET-009, Review Focus 5
        let (_tmp, conn) = setup();
        // previous 7 days (2026-09-23..29): 6 h; this period (2026-09-30..10-06): 7, 8, 9
        put(&conn, "p1", "sleep", "2026-09-25", r#"{"value":6}"#, "manual", NOW, false);
        put(&conn, "a1", "sleep", "2026-10-04", r#"{"value":7}"#, "manual", NOW, false); // Sunday
        put(&conn, "a2", "sleep", "2026-10-05", r#"{"value":8}"#, "manual", NOW, false); // Monday
        put(&conn, "a3", "sleep", "2026-10-06", r#"{"value":9}"#, "manual", NOW, false);
        let s = summary(&conn, MetricKind::Sleep, 7, d("2026-10-06")).unwrap();
        assert_eq!(s.days.len(), 3);
        assert_eq!((s.average, s.min, s.max), (Some(8.0), Some(7.0), Some(9.0)));
        assert_eq!(s.trend, Some(2.0));
        assert_eq!(
            s.weekly,
            vec![WeekAverage { week_start: "2026-09-28".into(), average: 7.0 }, WeekAverage { week_start: "2026-10-05".into(), average: 8.5 }]
        );
        assert_eq!(summary(&conn, MetricKind::Weight, 7, d("2026-10-06")).unwrap().average, None, "nothing recorded");
    }

    #[test]
    fn the_overview_has_five_cards_with_two_weeks_of_sparkline() {
        let (_tmp, conn) = setup();
        put(&conn, "a1", "sleep", "2026-10-05", r#"{"value":7}"#, "manual", NOW, false);
        put(&conn, "a2", "sleep", "2026-10-02", r#"{"value":6}"#, "extracted", NOW, false);
        put(&conn, "a3", "sleep", "2026-10-02", r#"{"value":5}"#, "extracted", "2026-10-06T11:00:00+03:00", false);
        let cards = overview(&conn, d("2026-10-06")).unwrap();
        assert_eq!(cards.iter().map(|c| c.kind).collect::<Vec<_>>(), KINDS.to_vec());
        let sleep = &cards[0];
        assert_eq!(sleep.spark.len(), 14);
        assert_eq!(sleep.spark[13], None, "today: nothing yet");
        assert_eq!(sleep.spark[12], Some(7.0));
        assert_eq!(sleep.last.as_ref().map(|v| v.value), Some(7.0));
        assert_eq!(sleep.average7, Some(6.0)); // (7 + 5) / 2
        assert_eq!(sleep.conflicts, 1);
        assert_eq!(cards[1].last, None);
    }

    #[test]
    fn manual_entries_are_converted_and_checked() {
        // FR-MET-003/007/008, Review Focus 3
        let (_tmp, conn) = setup();
        let s = ValidationSettings::default();
        let id = log_metric(&conn, &input(MetricKind::Water, Some(3.0), Some(Unit::Glass)), &s, now()).unwrap();
        let rec = records(&conn, MetricKind::Water, d("2026-10-06"), d("2026-10-06")).unwrap();
        assert_eq!((rec[0].metric_id.as_str(), rec[0].value, rec[0].origin.as_str()), (id.as_str(), Some(750.0), "manual"));
        log_metric(&conn, &input(MetricKind::Weight, Some(176.0), Some(Unit::Lb)), &s, now()).unwrap();
        let kg = records(&conn, MetricKind::Weight, d("2026-10-06"), d("2026-10-06")).unwrap()[0].value.unwrap();
        assert!((kg - 79.83).abs() < 0.01, "{kg}");

        let heavy = input(MetricKind::Weight, Some(450.0), Some(Unit::Kg));
        assert!(matches!(log_metric(&conn, &heavy, &s, now()), Err(MetricError::OutOfRange("value", v)) if v == 450.0));
        assert_eq!(records(&conn, MetricKind::Weight, d("2026-10-06"), d("2026-10-06")).unwrap().len(), 1, "not stored");
        log_metric(&conn, &MetricInput { confirmed: true, ..heavy }, &s, now()).unwrap();
        assert_eq!(records(&conn, MetricKind::Weight, d("2026-10-06"), d("2026-10-06")).unwrap().len(), 2);

        assert!(matches!(log_metric(&conn, &input(MetricKind::Sleep, None, None), &s, now()), Err(MetricError::MissingValue)));
        assert!(matches!(log_metric(&conn, &input(MetricKind::Workout, None, None), &s, now()), Err(MetricError::EmptyWorkout)));
        let bad_date = MetricInput { date: "06.10.2026".into(), ..input(MetricKind::Sleep, Some(7.0), None) };
        assert!(matches!(log_metric(&conn, &bad_date, &s, now()), Err(MetricError::InvalidDate)));
        let workout = MetricInput { exercise: Some("  şınav ".into()), sets: Some(3), reps: Some(12), ..input(MetricKind::Workout, None, None) };
        log_metric(&conn, &workout, &s, now()).unwrap();
        let w = &records(&conn, MetricKind::Workout, d("2026-10-06"), d("2026-10-06")).unwrap()[0];
        assert_eq!((w.exercise.as_deref(), w.sets, w.reps), (Some("şınav"), Some(3), Some(12)));
    }

    #[test]
    fn editing_marks_the_record_the_users_and_deleting_an_extraction_rejects_it() {
        // FR-MET-011, Review Focus 2
        let (_tmp, conn) = setup();
        let s = ValidationSettings::default();
        put(&conn, "e1", "sleep", "2026-10-05", r#"{"value":6}"#, "extracted", NOW, false);
        edit_metric(&conn, "e1", &MetricInput { date: "2026-10-05".into(), ..input(MetricKind::Sleep, Some(6.5), None) }, &s, now()).unwrap();
        let r = &records(&conn, MetricKind::Sleep, d("2026-10-05"), d("2026-10-05")).unwrap()[0];
        assert_eq!((r.value, r.user_modified), (Some(6.5), true));
        assert!(matches!(edit_metric(&conn, "e1", &input(MetricKind::Sleep, Some(30.0), None), &s, now()), Err(MetricError::OutOfRange(..))));

        put(&conn, "e2", "water", "2026-10-05", r#"{"value":500}"#, "extracted", NOW, false);
        delete_metric(&conn, "e2", now()).unwrap();
        let rejected: i64 = conn.query_row("SELECT COUNT(*) FROM rejection WHERE item_signature = 'sig-e2'", [], |r| r.get(0)).unwrap();
        assert_eq!(rejected, 1, "the note will not bring it back");
        put(&conn, "e3", "water", "2026-10-05", r#"{"value":500}"#, "manual", NOW, false);
        delete_metric(&conn, "e3", now()).unwrap();
        assert!(records(&conn, MetricKind::Water, d("2026-10-05"), d("2026-10-05")).unwrap().is_empty());
        assert!(matches!(delete_metric(&conn, "nope", now()), Err(MetricError::NotFound)));
    }

    #[test]
    fn impossible_values_and_future_days_are_refused_even_when_confirmed() {
        // metrics final review: confirmation is for unusual values, not impossible ones
        let (_tmp, conn) = setup();
        let s = ValidationSettings::default();
        let yes = |i: MetricInput| MetricInput { confirmed: true, ..i };
        assert!(matches!(log_metric(&conn, &yes(input(MetricKind::Sleep, Some(-2.0), None)), &s, now()), Err(MetricError::InvalidValue)));
        assert!(matches!(log_metric(&conn, &yes(input(MetricKind::Water, Some(1e308), Some(Unit::L))), &s, now()), Err(MetricError::InvalidValue)));
        let tomorrow = MetricInput { date: "2026-10-07".into(), ..input(MetricKind::Sleep, Some(7.0), None) };
        assert!(matches!(log_metric(&conn, &tomorrow, &s, now()), Err(MetricError::InvalidDate)));
        assert!(records(&conn, MetricKind::Sleep, d("2026-10-01"), d("2026-10-10")).unwrap().is_empty());
    }

    #[test]
    fn a_workout_warning_names_the_field_that_is_unusual() {
        // metrics final review I3
        let (_tmp, conn) = setup();
        let s = ValidationSettings::default();
        let w = |sets, reps, kg| MetricInput { sets, reps, ..input(MetricKind::Workout, kg, Some(Unit::Kg)) };
        assert!(matches!(log_metric(&conn, &w(Some(3), Some(600), None), &s, now()), Err(MetricError::OutOfRange("reps", v)) if v == 600.0));
        assert!(matches!(log_metric(&conn, &w(Some(60), Some(10), None), &s, now()), Err(MetricError::OutOfRange("sets", _))));
        assert!(matches!(log_metric(&conn, &w(Some(3), Some(10), Some(600.0)), &s, now()), Err(MetricError::OutOfRange("kg", _))));
        assert_eq!(MetricError::OutOfRange("reps", 600.0).to_string(), "out_of_range|reps|600");
        // sleep in minutes and a workout weight in pounds arrive in canonical units
        log_metric(&conn, &input(MetricKind::Sleep, Some(450.0), Some(Unit::Min)), &s, now()).unwrap();
        assert_eq!(records(&conn, MetricKind::Sleep, d("2026-10-06"), d("2026-10-06")).unwrap()[0].value, Some(7.5));
        log_metric(&conn, &w(Some(3), Some(8), Some(100.0)).clone_with_unit(Unit::Lb), &s, now()).unwrap();
        let kg = records(&conn, MetricKind::Workout, d("2026-10-06"), d("2026-10-06")).unwrap()[0].value.unwrap();
        assert!((kg - 45.36).abs() < 0.01, "{kg}");
    }

    impl MetricInput {
        fn clone_with_unit(&self, unit: Unit) -> Self {
            Self { unit: Some(unit), ..self.clone() }
        }
    }

    #[test]
    fn only_single_value_kinds_have_conflicts_to_resolve() {
        let (_tmp, conn) = setup();
        put(&conn, "w1", "water", "2026-10-05", r#"{"value":500}"#, "extracted", NOW, false);
        put(&conn, "w2", "water", "2026-10-05", r#"{"value":750}"#, "extracted", NOW, false);
        assert!(matches!(resolve_conflict(&conn, "w1", now()), Err(MetricError::NotFound)));
        assert_eq!(records(&conn, MetricKind::Water, d("2026-10-05"), d("2026-10-05")).unwrap().len(), 2, "both waters stay");
    }

    #[test]
    fn a_record_whose_note_is_gone_says_so_and_the_overview_finds_old_values() {
        let (_tmp, conn) = setup();
        put(&conn, "s1", "weight", "2026-08-01", r#"{"value":81}"#, "extracted", NOW, false);
        conn.execute("UPDATE metric_record SET source_missing = 1 WHERE metric_id = 's1'", []).unwrap();
        assert!(records(&conn, MetricKind::Weight, d("2026-08-01"), d("2026-08-01")).unwrap()[0].source_missing);
        let weight = &overview(&conn, d("2026-10-06")).unwrap()[1];
        assert_eq!(weight.last.as_ref().map(|v| (v.date.as_str(), v.value)), Some(("2026-08-01", 81.0)), "older than two weeks");
        assert_eq!(weight.average7, None);
    }

    fn extract_again(conn: &Connection, json: &str) {
        let ex = crate::extraction::parse_extraction(json).unwrap();
        crate::pipeline::items::apply_block_items(conn, "b1", d("2026-10-05"), &ex, &ValidationSettings::default(), None, now()).unwrap();
    }

    #[test]
    fn a_deleted_extraction_stays_deleted_when_its_note_is_read_again() {
        // metrics final review I1, Review Focus 2: two values of one kind in one paragraph
        let (_tmp, conn) = setup();
        let water = r#"{"items": [{"type": "metric", "metric": {"kind": "water", "value": 2, "unit": "glass"}},
                                  {"type": "metric", "metric": {"kind": "water", "value": 3, "unit": "glass"}}]}"#;
        extract_again(&conn, water);
        let day = |c: &Connection| records(c, MetricKind::Water, d("2026-10-05"), d("2026-10-05")).unwrap();
        let first = day(&conn).into_iter().find(|r| r.value == Some(500.0)).unwrap();
        delete_metric(&conn, &first.metric_id, now()).unwrap();
        extract_again(&conn, water);
        assert_eq!(day(&conn).iter().map(|r| r.value).collect::<Vec<_>>(), vec![Some(750.0)], "the 500 ml stays deleted, the 750 ml stays");

        let sleep = r#"{"items": [{"type": "metric", "when": {"day_offset": -1}, "metric": {"kind": "sleep", "value": 6, "unit": "h"}},
                                  {"type": "metric", "when": {"day_offset": 0}, "metric": {"kind": "sleep", "value": 7, "unit": "h"}}]}"#;
        extract_again(&conn, sleep);
        let monday = records(&conn, MetricKind::Sleep, d("2026-10-04"), d("2026-10-04")).unwrap();
        delete_metric(&conn, &monday[0].metric_id, now()).unwrap();
        extract_again(&conn, sleep);
        let left = records(&conn, MetricKind::Sleep, d("2026-10-01"), d("2026-10-06")).unwrap();
        assert_eq!(left.iter().map(|r| (r.date.as_str(), r.value)).collect::<Vec<_>>(), vec![("2026-10-05", Some(7.0))]);
    }

    #[test]
    fn resolving_a_conflict_keeps_one_value_and_rejects_the_others() {
        // FR-MET-006
        let (_tmp, conn) = setup();
        put(&conn, "c1", "weight", "2026-10-05", r#"{"value":80}"#, "extracted", "2026-10-05T08:00:00+03:00", false);
        put(&conn, "c2", "weight", "2026-10-05", r#"{"value":82}"#, "extracted", "2026-10-05T09:00:00+03:00", false);
        resolve_conflict(&conn, "c1", now()).unwrap();
        let days = daily_values(&conn, MetricKind::Weight, d("2026-10-05"), d("2026-10-05")).unwrap();
        assert_eq!((days[0].value, days[0].conflict), (80.0, false));
        assert_eq!(records(&conn, MetricKind::Weight, d("2026-10-05"), d("2026-10-05")).unwrap().len(), 1);
        let rejected: i64 = conn.query_row("SELECT COUNT(*) FROM rejection WHERE item_signature = 'sig-c2'", [], |r| r.get(0)).unwrap();
        assert_eq!(rejected, 1);
    }
}
