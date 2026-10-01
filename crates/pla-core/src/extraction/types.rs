//! Raw model output, exactly as constrained by the v3 extraction schema (SRS C.3).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawExtraction {
    pub items: Vec<RawItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Task,
    Reminder,
    Metric,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawItem {
    #[serde(rename = "type")]
    pub kind: ItemType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<RawWhen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<RawMetric>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawWhen {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_offset: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekday: Option<Weekday>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub which: Option<Which>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn num_days_from_monday(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Which {
    This,
    Next,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RawMetric {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MetricKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exercise: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sets: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reps: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MetricKind {
    Sleep,
    Water,
    Steps,
    Weight,
    Workout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    H,
    Min,
    Ml,
    L,
    Glass,
    Count,
    Kg,
    Lb,
}

/// Parses the model's JSON answer. Grammar-constrained output is always
/// well-formed, but an answer cut off at `max_tokens` is not, so this can fail.
pub fn parse_extraction(raw: &str) -> Result<RawExtraction, serde_json::Error> {
    serde_json::from_str(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_task_and_a_workout() {
        let raw = r#"{"items": [
            {"type": "task", "title": "Dişçi", "when": {"day_offset": 1, "time": "09:00"}},
            {"type": "metric", "metric": {"kind": "workout", "exercise": "deadlift", "sets": 4, "reps": 6, "value": 110, "unit": "kg"}}
        ]}"#;
        let ex = parse_extraction(raw).unwrap();
        assert_eq!(ex.items.len(), 2);
        assert_eq!(ex.items[0].kind, ItemType::Task);
        assert_eq!(ex.items[0].when.as_ref().unwrap().day_offset, Some(1));
        let m = ex.items[1].metric.as_ref().unwrap();
        assert_eq!(m.kind, Some(MetricKind::Workout));
        assert_eq!(m.value, Some(110.0));
        assert_eq!(m.unit, Some(Unit::Kg));
    }

    #[test]
    fn parses_weekday_codes() {
        let ex = parse_extraction(r#"{"items": [{"type": "reminder", "title": "x", "when": {"weekday": "sun", "which": "next"}}]}"#).unwrap();
        let w = ex.items[0].when.as_ref().unwrap();
        assert_eq!(w.weekday, Some(Weekday::Sun));
        assert_eq!(Weekday::Sun.num_days_from_monday(), 6);
        assert_eq!(w.which, Some(Which::Next));
    }

    #[test]
    fn empty_items_is_valid() {
        assert!(parse_extraction(r#"{"items": []}"#).unwrap().items.is_empty());
    }

    #[test]
    fn rejects_unknown_fields_and_truncated_json() {
        assert!(parse_extraction(r#"{"items": [{"type": "task", "colour": "red"}]}"#).is_err());
        assert!(parse_extraction(r#"{"items": [{"type": "task", "title": "Diş"#).is_err());
    }
}
