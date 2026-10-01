//! Absolute dates from the model's relative fields (FR-EXT-012, -027, -028).
//! Mirrors `resolve_date` in research/f1-model-eval/run_eval.py; tests/parity.rs keeps them equal.

use chrono::{Datelike, NaiveDate, TimeDelta};

use super::types::{ItemType, RawItem, RawWhen, Which};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DateError {
    #[error("invalid calendar date: {0}")]
    InvalidDate(String),
    #[error("day offset out of range: {0}")]
    OffsetOutOfRange(i64),
}

/// `reference` is the note's date (daily notes) or the block's first-seen date (FR-EXT-009), never "today".
pub fn resolve_date(item: &RawItem, reference: NaiveDate) -> Result<Option<NaiveDate>, DateError> {
    let empty = RawWhen::default();
    let when = item.when.as_ref().unwrap_or(&empty);

    if let Some(s) = &when.date {
        return NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| DateError::InvalidDate(s.clone()));
    }
    if let Some(n) = when.day_offset {
        return add_days(reference, n).map(Some).ok_or(DateError::OffsetOutOfRange(n));
    }
    if let Some(weekday) = when.weekday {
        let target = i64::from(weekday.num_days_from_monday());
        let today = i64::from(reference.weekday().num_days_from_monday());
        let ahead = (target - today).rem_euclid(7) + if when.which == Some(Which::Next) { 7 } else { 0 };
        return add_days(reference, ahead).map(Some).ok_or(DateError::OffsetOutOfRange(ahead));
    }
    if item.kind == ItemType::Metric || when.time.is_some() {
        return Ok(Some(reference));
    }
    Ok(None)
}

fn add_days(date: NaiveDate, days: i64) -> Option<NaiveDate> {
    TimeDelta::try_days(days).and_then(|delta| date.checked_add_signed(delta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::parse_extraction;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn item(json: &str) -> RawItem {
        parse_extraction(&format!(r#"{{"items": [{json}]}}"#)).unwrap().items.remove(0)
    }

    const TUE: &str = "2026-10-06";

    #[test]
    fn explicit_date_wins() {
        assert_eq!(resolve_date(&item(r#"{"type":"task","title":"x","when":{"date":"2026-12-24"}}"#), d(TUE)), Ok(Some(d("2026-12-24"))));
    }

    #[test]
    fn day_offset_counts_from_the_reference_date() {
        assert_eq!(resolve_date(&item(r#"{"type":"task","title":"x","when":{"day_offset":1}}"#), d(TUE)), Ok(Some(d("2026-10-07"))));
        assert_eq!(resolve_date(&item(r#"{"type":"metric","when":{"day_offset":-1},"metric":{"kind":"sleep","value":7,"unit":"h"}}"#), d(TUE)), Ok(Some(d("2026-10-05"))));
    }

    #[test]
    fn weekday_this_next_and_default() {
        let thu = r#"{"type":"task","title":"x","when":{"weekday":"thu","which":"this"}}"#;
        assert_eq!(resolve_date(&item(thu), d(TUE)), Ok(Some(d("2026-10-08"))));
        let next_mon = r#"{"type":"task","title":"x","when":{"weekday":"mon","which":"next"}}"#;
        assert_eq!(resolve_date(&item(next_mon), d(TUE)), Ok(Some(d("2026-10-19"))));
        // FR-EXT-028: no `which` -> this
        let fri = r#"{"type":"task","title":"x","when":{"weekday":"fri"}}"#;
        assert_eq!(resolve_date(&item(fri), d(TUE)), Ok(Some(d("2026-10-09"))));
        // same weekday, this -> the reference day itself
        let tue = r#"{"type":"task","title":"x","when":{"weekday":"tue","which":"this"}}"#;
        assert_eq!(resolve_date(&item(tue), d(TUE)), Ok(Some(d(TUE))));
    }

    #[test]
    fn weekday_wraps_over_sunday_and_year_end() {
        let mon = r#"{"type":"task","title":"x","when":{"weekday":"mon"}}"#;
        assert_eq!(resolve_date(&item(mon), d("2026-12-31")), Ok(Some(d("2027-01-04"))));
    }

    #[test]
    fn time_only_task_falls_on_the_reference_date() {
        // FR-EXT-027
        assert_eq!(resolve_date(&item(r#"{"type":"task","title":"x","when":{"time":"21:00"}}"#), d(TUE)), Ok(Some(d(TUE))));
    }

    #[test]
    fn metric_without_when_is_for_the_reference_date() {
        assert_eq!(resolve_date(&item(r#"{"type":"metric","metric":{"kind":"steps","value":9000,"unit":"count"}}"#), d(TUE)), Ok(Some(d(TUE))));
    }

    #[test]
    fn undated_task_has_no_date() {
        assert_eq!(resolve_date(&item(r#"{"type":"task","title":"x"}"#), d(TUE)), Ok(None));
    }

    #[test]
    fn impossible_dates_are_errors_not_panics() {
        assert_eq!(
            resolve_date(&item(r#"{"type":"task","title":"x","when":{"date":"2026-02-30"}}"#), d(TUE)),
            Err(DateError::InvalidDate("2026-02-30".into()))
        );
        assert_eq!(
            resolve_date(&item(r#"{"type":"task","title":"x","when":{"day_offset":9223372036854775807}}"#), d(TUE)),
            Err(DateError::OffsetOutOfRange(i64::MAX))
        );
    }
}
