//! Replays every item Gemma 4 E2B Q3_K_M produced in F1 (dev + held-out set) through the Rust rules
//! and requires the same date, validity and canonical value as the Python reference implementation.
//! Regenerate the fixture with research/f1-model-eval/export_rust_assets.py.

use chrono::NaiveDate;
use pla_core::extraction::{resolve_date, validate, RawItem, ValidItem, ValidationSettings};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    id: String,
    #[serde(rename = "ref")]
    reference: String,
    item: serde_json::Value,
    date: Option<String>,
    valid: bool,
    value: Option<f64>,
}

#[test]
fn rust_rules_match_python_on_f1_predictions() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/parity_v3.json")).unwrap();
    assert!(cases.len() > 300, "fixture looks truncated: {} cases", cases.len());
    let settings = ValidationSettings::default();
    let mut failures = Vec::new();

    for c in &cases {
        let reference = NaiveDate::parse_from_str(&c.reference, "%Y-%m-%d").unwrap();
        let item: RawItem = match serde_json::from_value(c.item.clone()) {
            Ok(item) => item,
            Err(e) => {
                failures.push(format!("{}: does not parse: {e}", c.id));
                continue;
            }
        };
        let date = match resolve_date(&item, reference) {
            Ok(Some(d)) => Some(d.to_string()),
            Ok(None) => None,
            Err(_) => Some("invalid".to_owned()),
        };
        if date != c.date {
            failures.push(format!("{}: date {date:?}, python {:?}", c.id, c.date));
        }
        let got = validate(&item, reference, &settings);
        if got.is_ok() != c.valid {
            failures.push(format!("{}: {got:?}, python valid={}", c.id, c.valid));
        }
        if let Ok(ValidItem::Metric { value, .. }) = &got {
            let same = match (value, c.value) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-9,
                (None, None) => true,
                _ => false,
            };
            if !same {
                failures.push(format!("{}: value {value:?}, python {:?}", c.id, c.value));
            }
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}
