//! The extraction prompt and schema measured in F1 (prompt v3), embedded verbatim.
//! Regenerate the assets with `research/f1-model-eval/export_rust_assets.py`.

use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};

const SCHEMA_JSON: &str = include_str!("../../assets/extraction_schema_v3.json");
const PROMPT_JSON: &str = include_str!("../../assets/extraction_prompt_v3.json");
const DAY_NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

#[derive(Deserialize)]
struct PromptAsset {
    system: String,
    few_shot: Vec<Shot>,
}

#[derive(Deserialize)]
struct Shot {
    user: String,
    assistant: String,
}

/// JSON schema the model output is constrained to (SRS C.3, v3 property order).
pub fn schema() -> Value {
    serde_json::from_str(SCHEMA_JSON).expect("embedded extraction schema is valid JSON")
}

/// FR-EXT-010: the model sees the reference date and its weekday next to the note.
pub fn user_message(reference: NaiveDate, text: &str) -> String {
    let day = DAY_NAMES[reference.weekday().num_days_from_monday() as usize];
    format!("Reference date: {} ({day})\nNote: {text}", reference.format("%Y-%m-%d"))
}

pub fn messages(reference: NaiveDate, text: &str) -> Value {
    let prompt: PromptAsset = serde_json::from_str(PROMPT_JSON).expect("embedded extraction prompt is valid JSON");
    let mut out = vec![json!({"role": "system", "content": prompt.system})];
    for shot in prompt.few_shot {
        out.push(json!({"role": "user", "content": shot.user}));
        out.push(json!({"role": "assistant", "content": shot.assistant}));
    }
    out.push(json!({"role": "user", "content": user_message(reference, text)}));
    Value::Array(out)
}

/// Body for llama-server's `/v1/chat/completions`, identical to the F1 harness.
pub fn request_body(reference: NaiveDate, text: &str) -> Value {
    json!({
        "messages": messages(reference, text),
        "temperature": 0,
        "max_tokens": 400,
        "cache_prompt": true,
        "response_format": {"type": "json_schema", "json_schema": {"name": "extraction", "schema": schema()}},
        "chat_template_kwargs": {"enable_thinking": false}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn messages_match_the_evaluated_python_prompt() {
        let fx: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/messages_sample.json")).unwrap();
        let got = messages(d(fx["ref"].as_str().unwrap()), fx["text"].as_str().unwrap());
        assert_eq!(got, fx["messages"]);
    }

    #[test]
    fn user_message_names_the_weekday() {
        assert_eq!(user_message(d("2026-10-11"), "x"), "Reference date: 2026-10-11 (Sunday)\nNote: x");
    }

    #[test]
    fn serialized_metric_properties_keep_the_v3_order() {
        // Grammar emits properties in declaration order; `value` must follow the workout fields (FR-EXT-029).
        let s = serde_json::to_string(&request_body(d("2026-10-06"), "x")).unwrap();
        let metric = &s[s.find("\"metric\":{").expect("metric schema present")..];
        let pos: Vec<usize> = ["\"kind\"", "\"exercise\"", "\"sets\"", "\"reps\"", "\"value\"", "\"unit\""]
            .iter()
            .map(|k| metric.find(k).unwrap_or_else(|| panic!("{k} missing")))
            .collect();
        assert!(pos.windows(2).all(|w| w[0] < w[1]), "order broken: {pos:?}");
    }

    #[test]
    fn request_body_uses_deterministic_grammar_constrained_settings() {
        let b = request_body(d("2026-10-06"), "x");
        assert_eq!(b["temperature"], 0);
        assert_eq!(b["max_tokens"], 400);
        assert_eq!(b["response_format"]["type"], "json_schema");
        assert_eq!(b["response_format"]["json_schema"]["schema"], schema());
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
    }
}
