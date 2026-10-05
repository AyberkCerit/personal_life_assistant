//! Semantic validation of one extracted item (FR-EXT-013) and canonical units (E-D7, E-D8).
//! Mirrors `validate` / `canonical_value` in research/f1-model-eval/run_eval.py.

use chrono::{NaiveDate, NaiveTime};

use super::dates::{resolve_date, DateError};
use super::types::{ItemType, MetricKind, RawItem, RawMetric, RawWhen, Unit};

const LB_TO_KG: f64 = 0.453_592_37;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidationSettings {
    /// E-D7: size of one glass of water, user-configurable.
    pub glass_ml: f64,
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self { glass_ml: 250.0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidItem {
    Action {
        reminder: bool,
        title: String,
        date: Option<NaiveDate>,
        time: Option<NaiveTime>,
    },
    Metric {
        kind: MetricKind,
        date: NaiveDate,
        value: Option<f64>,
        unit: Option<Unit>,
        exercise: Option<String>,
        sets: Option<i64>,
        reps: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InvalidReason {
    #[error("invalid time: {0}")]
    InvalidTime(String),
    #[error("more than one of day_offset, date and weekday")]
    ConflictingDate,
    #[error(transparent)]
    Date(#[from] DateError),
    #[error("task or reminder without a title")]
    MissingTitle,
    #[error("task or reminder carrying a metric")]
    UnexpectedMetric,
    #[error("metric without a kind")]
    MissingMetricKind,
    #[error("metric without a value")]
    MissingValue,
    #[error("workout without sets, reps or weight")]
    EmptyWorkout,
    #[error("{kind:?} value {value} is outside its plausible range")]
    OutOfRange { kind: MetricKind, value: f64 },
}

/// Value in the kind's canonical unit: sleep h, water ml, weight and workout kg.
/// A workout value counts only as a positive weight in kg, lb or without a unit
/// (the model writes push-ups as `value 0, unit count` and "gym 1 hour" as `value 1, unit h`).
pub fn canonical_value(m: &RawMetric, s: &ValidationSettings) -> Option<f64> {
    let v = m.value?;
    if m.kind == Some(MetricKind::Workout) && (v <= 0.0 || !matches!(m.unit, None | Some(Unit::Kg) | Some(Unit::Lb))) {
        return None;
    }
    Some(match (m.kind, m.unit) {
        (Some(MetricKind::Water), Some(Unit::Glass)) => v * s.glass_ml,
        (Some(MetricKind::Water), Some(Unit::L)) => v * 1000.0,
        // No unit and a small number: the note meant litres ("2 litre su" -> 2).
        (Some(MetricKind::Water), None) if v < 10.0 => v * 1000.0,
        (Some(MetricKind::Weight | MetricKind::Workout), Some(Unit::Lb)) => v * LB_TO_KG,
        (Some(MetricKind::Sleep), Some(Unit::Min)) => v / 60.0,
        _ => v,
    })
}

fn canonical_unit(kind: MetricKind) -> Unit {
    match kind {
        MetricKind::Sleep => Unit::H,
        MetricKind::Water => Unit::Ml,
        MetricKind::Steps => Unit::Count,
        MetricKind::Weight | MetricKind::Workout => Unit::Kg,
    }
}

/// E-D8 plausible ranges, inclusive, in canonical units.
pub(crate) fn plausible_range(kind: MetricKind) -> (f64, f64) {
    match kind {
        MetricKind::Sleep => (0.0, 24.0),
        MetricKind::Water => (0.0, 10_000.0),
        MetricKind::Steps => (0.0, 100_000.0),
        MetricKind::Weight => (20.0, 400.0),
        MetricKind::Workout => (0.0, f64::INFINITY),
    }
}

pub fn validate(item: &RawItem, reference: NaiveDate, s: &ValidationSettings) -> Result<ValidItem, InvalidReason> {
    let empty = RawWhen::default();
    let when = item.when.as_ref().unwrap_or(&empty);

    let time = match &when.time {
        Some(t) => Some(NaiveTime::parse_from_str(t, "%H:%M").map_err(|_| InvalidReason::InvalidTime(t.clone()))?),
        None => None,
    };
    let anchors = [when.day_offset.is_some(), when.date.is_some(), when.weekday.is_some()];
    if anchors.iter().filter(|set| **set).count() > 1 {
        return Err(InvalidReason::ConflictingDate);
    }
    let date = resolve_date(item, reference)?;

    match item.kind {
        ItemType::Task | ItemType::Reminder => {
            let title = item.title.as_deref().map(str::trim).unwrap_or_default();
            if title.is_empty() {
                return Err(InvalidReason::MissingTitle);
            }
            if item.metric.is_some() {
                return Err(InvalidReason::UnexpectedMetric);
            }
            Ok(ValidItem::Action { reminder: item.kind == ItemType::Reminder, title: title.to_owned(), date, time })
        }
        ItemType::Metric => {
            let m = item.metric.as_ref().ok_or(InvalidReason::MissingMetricKind)?;
            let kind = m.kind.ok_or(InvalidReason::MissingMetricKind)?;
            let value = canonical_value(m, s);
            if kind == MetricKind::Workout {
                if m.sets.is_none() && m.reps.is_none() && value.is_none() {
                    return Err(InvalidReason::EmptyWorkout);
                }
            } else {
                let v = value.ok_or(InvalidReason::MissingValue)?;
                let (lo, hi) = plausible_range(kind);
                if !(lo..=hi).contains(&v) {
                    return Err(InvalidReason::OutOfRange { kind, value: v });
                }
            }
            Ok(ValidItem::Metric {
                kind,
                date: date.unwrap_or(reference),
                value,
                unit: value.map(|_| canonical_unit(kind)),
                exercise: m.exercise.clone(),
                sets: m.sets,
                reps: m.reps,
            })
        }
    }
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
    fn check(json: &str) -> Result<ValidItem, InvalidReason> {
        validate(&item(json), d("2026-10-06"), &ValidationSettings::default())
    }

    #[test]
    fn valid_task_is_trimmed_and_dated() {
        let got = check(r#"{"type":"reminder","title":"  Dişçi ","when":{"day_offset":1,"time":"09:00"}}"#).unwrap();
        assert_eq!(got, ValidItem::Action {
            reminder: true,
            title: "Dişçi".into(),
            date: Some(d("2026-10-07")),
            time: Some(NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
        });
    }

    #[test]
    fn task_rules() {
        assert_eq!(check(r#"{"type":"task","title":"   "}"#), Err(InvalidReason::MissingTitle));
        assert_eq!(check(r#"{"type":"task"}"#), Err(InvalidReason::MissingTitle));
        assert_eq!(check(r#"{"type":"task","title":"x","metric":{"kind":"sleep","value":7}}"#), Err(InvalidReason::UnexpectedMetric));
        assert_eq!(check(r#"{"type":"task","title":"x","when":{"time":"24:00"}}"#), Err(InvalidReason::InvalidTime("24:00".into())));
        assert_eq!(check(r#"{"type":"task","title":"x","when":{"day_offset":1,"weekday":"mon"}}"#), Err(InvalidReason::ConflictingDate));
        assert_eq!(
            check(r#"{"type":"task","title":"x","when":{"date":"2026-13-01"}}"#),
            Err(InvalidReason::Date(DateError::InvalidDate("2026-13-01".into())))
        );
    }

    #[test]
    fn units_are_canonicalised() {
        let s = ValidationSettings::default();
        let m = |j: &str| item(j).metric.unwrap();
        assert_eq!(canonical_value(&m(r#"{"type":"metric","metric":{"kind":"water","value":4,"unit":"glass"}}"#), &s), Some(1000.0));
        assert_eq!(canonical_value(&m(r#"{"type":"metric","metric":{"kind":"water","value":1.5,"unit":"l"}}"#), &s), Some(1500.0));
        assert_eq!(canonical_value(&m(r#"{"type":"metric","metric":{"kind":"water","value":2}}"#), &s), Some(2000.0));
        assert_eq!(canonical_value(&m(r#"{"type":"metric","metric":{"kind":"water","value":300}}"#), &s), Some(300.0));
        assert_eq!(canonical_value(&m(r#"{"type":"metric","metric":{"kind":"sleep","value":450,"unit":"min"}}"#), &s), Some(7.5));
        let lb = canonical_value(&m(r#"{"type":"metric","metric":{"kind":"weight","value":176,"unit":"lb"}}"#), &s).unwrap();
        assert!((lb - 79.832_257_12).abs() < 1e-6);
        let custom = ValidationSettings { glass_ml: 200.0 };
        assert_eq!(canonical_value(&m(r#"{"type":"metric","metric":{"kind":"water","value":4,"unit":"glass"}}"#), &custom), Some(800.0));
    }

    #[test]
    fn metric_ranges_follow_e_d8() {
        assert!(check(r#"{"type":"metric","metric":{"kind":"sleep","value":24,"unit":"h"}}"#).is_ok());
        assert!(matches!(check(r#"{"type":"metric","metric":{"kind":"sleep","value":25,"unit":"h"}}"#), Err(InvalidReason::OutOfRange { .. })));
        assert!(matches!(check(r#"{"type":"metric","metric":{"kind":"weight","value":19,"unit":"kg"}}"#), Err(InvalidReason::OutOfRange { .. })));
        assert!(matches!(check(r#"{"type":"metric","metric":{"kind":"steps","value":100001,"unit":"count"}}"#), Err(InvalidReason::OutOfRange { .. })));
        assert_eq!(check(r#"{"type":"metric","metric":{"kind":"steps","unit":"count"}}"#), Err(InvalidReason::MissingValue));
        assert_eq!(check(r#"{"type":"metric","metric":{"value":3}}"#), Err(InvalidReason::MissingMetricKind));
        assert_eq!(check(r#"{"type":"metric"}"#), Err(InvalidReason::MissingMetricKind));
    }

    #[test]
    fn workout_value_is_a_weight_only_in_kg_or_lb() {
        // F1 outputs: push-ups as `value 0, unit count`, "gym 1 hour" as `value 1, unit h`.
        let pushups = check(r#"{"type":"metric","metric":{"kind":"workout","exercise":"push-ups","sets":3,"reps":20,"value":0,"unit":"count"}}"#).unwrap();
        assert!(matches!(pushups, ValidItem::Metric { value: None, unit: None, sets: Some(3), reps: Some(20), .. }), "{pushups:?}");
        assert_eq!(check(r#"{"type":"metric","metric":{"kind":"workout","exercise":"gym","value":1,"unit":"h"}}"#), Err(InvalidReason::EmptyWorkout));
        let negative = check(r#"{"type":"metric","metric":{"kind":"workout","exercise":"squat","sets":3,"value":-5,"unit":"kg"}}"#).unwrap();
        assert!(matches!(negative, ValidItem::Metric { value: None, .. }), "{negative:?}");
        let no_unit = check(r#"{"type":"metric","metric":{"kind":"workout","exercise":"bench","sets":3,"reps":8,"value":60}}"#).unwrap();
        assert!(matches!(no_unit, ValidItem::Metric { value: Some(v), unit: Some(Unit::Kg), .. } if v == 60.0), "{no_unit:?}");
    }

    #[test]
    fn workout_needs_some_numbers_and_gets_the_reference_date() {
        assert_eq!(check(r#"{"type":"metric","metric":{"kind":"workout","exercise":"plank"}}"#), Err(InvalidReason::EmptyWorkout));
        let got = check(r#"{"type":"metric","metric":{"kind":"workout","exercise":"deadlift","sets":4,"reps":6,"value":110,"unit":"kg"}}"#).unwrap();
        assert_eq!(got, ValidItem::Metric {
            kind: MetricKind::Workout,
            date: d("2026-10-06"),
            value: Some(110.0),
            unit: Some(Unit::Kg),
            exercise: Some("deadlift".into()),
            sets: Some(4),
            reps: Some(6),
        });
    }
}
