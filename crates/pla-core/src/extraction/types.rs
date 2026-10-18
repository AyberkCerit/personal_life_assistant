//! Raw model output, exactly as constrained by the v4 extraction schema (SRS C.3).

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
    /// First on purpose: the grammar emits properties in this order (schema v4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<Vec<Weekday>>,
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

impl MetricKind {
    /// In the schema's order.
    pub const ALL: [Self; 5] = [Self::Sleep, Self::Water, Self::Steps, Self::Weight, Self::Workout];

    /// The name the schema, the database (`metric_record.type`) and the tools use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sleep => "sleep",
            Self::Water => "water",
            Self::Steps => "steps",
            Self::Weight => "weight",
            Self::Workout => "workout",
        }
    }

    /// E-D7: the unit a value of this kind is stored in.
    pub fn canonical_unit(self) -> Unit {
        match self {
            Self::Sleep => Unit::H,
            Self::Water => Unit::Ml,
            Self::Steps => Unit::Count,
            Self::Weight | Self::Workout => Unit::Kg,
        }
    }
}

/// A kind or unit name that is not one of PLA's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown name {0:?}")]
pub struct UnknownName(pub String);

impl std::str::FromStr for MetricKind {
    type Err = UnknownName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|k| k.as_str() == s).ok_or_else(|| UnknownName(s.to_owned()))
    }
}

impl Unit {
    pub const ALL: [Self; 8] = [Self::H, Self::Min, Self::Ml, Self::L, Self::Glass, Self::Count, Self::Kg, Self::Lb];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::H => "h",
            Self::Min => "min",
            Self::Ml => "ml",
            Self::L => "l",
            Self::Glass => "glass",
            Self::Count => "count",
            Self::Kg => "kg",
            Self::Lb => "lb",
        }
    }
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
    fn parses_a_repeat() {
        let ex = parse_extraction(r#"{"items":[{"type":"task","title":"Spor","when":{"repeat":["sun","mon"],"time":"20:00"}}]}"#).unwrap();
        assert_eq!(ex.items[0].when.as_ref().unwrap().repeat, Some(vec![Weekday::Sun, Weekday::Mon]));
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
    fn kinds_and_units_have_one_name_each() {
        // architecture review 5.4: the names the database, the schema and the tools use are serde's
        let serde_name = |v: serde_json::Value| v.as_str().unwrap().to_owned();
        for kind in MetricKind::ALL {
            assert_eq!(kind.as_str(), serde_name(serde_json::to_value(kind).unwrap()));
            assert_eq!(kind.as_str().parse::<MetricKind>().unwrap(), kind);
        }
        for unit in Unit::ALL {
            assert_eq!(unit.as_str(), serde_name(serde_json::to_value(unit).unwrap()));
        }
        assert!("steps ".parse::<MetricKind>().is_err());
        assert_eq!(MetricKind::ALL.map(MetricKind::as_str), ["sleep", "water", "steps", "weight", "workout"]);
        assert_eq!(Unit::ALL.len(), 8);
    }

    #[test]
    fn each_kind_is_stored_in_its_canonical_unit() {
        // E-D7: sleep h, water ml, steps count, weight and workout kg
        let units = MetricKind::ALL.map(|k| k.canonical_unit().as_str());
        assert_eq!(units, ["h", "ml", "count", "kg", "kg"]);
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
