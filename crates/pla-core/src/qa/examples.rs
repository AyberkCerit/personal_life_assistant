//! Dynamic examples (nl-quality plan § 3): instead of five fixed examples, the decision step sees
//! the bank's examples closest to the user's message. A small model copies what it is shown; the
//! right example ("79 kiloyum → log_metric weight") does more than any rule.

use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;
use serde_json::Value;

const BANK: &str = include_str!("../../assets/qa_examples.jsonl");

#[derive(Debug, Clone, Deserialize)]
pub struct Example {
    pub q: String,
    /// The user's message before this one, when the example needs it ("bunu not al").
    #[serde(default)]
    pub prev: Option<String>,
    /// The decision, with dates as `{today}`, `{tomorrow}`, `{yesterday}`, `{mon}`…`{sun}`, `{day25}`.
    pub d: Value,
}

pub fn bank() -> Vec<Example> {
    BANK.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).expect("assets/qa_examples.jsonl")).collect()
}

/// Word stems: lower case, Turkish letters folded, the first five letters (Turkish suffixes vary).
fn stems(text: &str) -> Vec<String> {
    let mut out: Vec<String> = crate::index::key(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().take(5).collect())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let common = a.iter().filter(|w| b.binary_search(w).is_ok()).count() as f64;
    common / ((a.len() * b.len()) as f64).sqrt()
}

/// The `n` examples closest to `question` (and the message before it), at most two per tool and
/// always one plain answer among them, so the model also sees when not to call anything.
pub fn select<'a>(bank: &'a [Example], question: &str, earlier: Option<&str>, n: usize) -> Vec<&'a Example> {
    let q = stems(question);
    let before = earlier.map(stems).unwrap_or_default();
    let mut scored: Vec<(f64, usize)> = bank
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut s = similarity(&q, &stems(&e.q));
            if let Some(p) = &e.prev {
                s += 0.3 * similarity(&before, &stems(p)); // a follow-up looks like a follow-up
            }
            (s, i)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let tool = |e: &Example| e.d["tool"].as_str().unwrap_or_default().to_owned();
    let mut picked: Vec<&Example> = Vec::new();
    for (_, i) in &scored {
        let e = &bank[*i];
        if picked.len() == n {
            break;
        }
        if picked.iter().filter(|p| tool(p) == tool(e)).count() < 2 {
            picked.push(e);
        }
    }
    if !picked.iter().any(|e| tool(e) == "answer") {
        if let Some((_, i)) = scored.iter().find(|(_, i)| tool(&bank[*i]) == "answer") {
            picked.pop();
            picked.push(&bank[*i]);
        }
    }
    picked
}

/// The next `weekday` after `today` (1–7 days ahead), as the calendar in the prompt reads it.
fn upcoming(today: NaiveDate, weekday: u32) -> NaiveDate {
    let ahead = (weekday + 7 - today.weekday().num_days_from_monday()) % 7;
    today + Duration::days(if ahead == 0 { 7 } else { ahead } as i64)
}

/// The example's decision as JSON text, its date tokens made real for `today`.
pub fn render(e: &Example, today: NaiveDate) -> String {
    let mut text = e.d.to_string();
    let day25 = if today.day() <= 25 { today.with_day(25) } else { (today + Duration::days(31)).with_day(25) };
    let mut tokens: Vec<(String, NaiveDate)> = vec![
        ("{today}".into(), today),
        ("{tomorrow}".into(), today + Duration::days(1)),
        ("{yesterday}".into(), today - Duration::days(1)),
        ("{day25}".into(), day25.unwrap_or(today)),
    ];
    for (i, d) in ["mon", "tue", "wed", "thu", "fri", "sat", "sun"].iter().enumerate() {
        tokens.push((format!("{{{d}}}"), upcoming(today, i as u32)));
    }
    for (token, date) in tokens {
        text = text.replace(&token, &date.to_string());
    }
    text
}

/// The lines the decision prompt shows.
pub fn lines(bank: &[Example], question: &str, earlier: Option<&str>, today: NaiveDate) -> String {
    select(bank, question, earlier, 6)
        .iter()
        .map(|e| match &e.prev {
            Some(p) => format!("(after \"{p}\") \"{}\" -> {}", e.q, render(e, today)),
            None => format!("\"{}\" -> {}", e.q, render(e, today)),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn every_example_is_a_decision_the_grammar_allows() {
        let schema_tools: Vec<String> = crate::qa::tools::decision_schema()["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["properties"]["tool"]["const"].as_str().unwrap().to_owned())
            .collect();
        for e in bank() {
            let rendered = render(&e, d("2026-10-09"));
            assert!(!rendered.contains('{') || !rendered.contains("{to"), "{rendered}");
            let tool = e.d["tool"].as_str().unwrap();
            assert!(schema_tools.iter().any(|t| t == tool), "{tool} in {:?}", e.q);
            assert!(crate::qa::tools::parse_decision(&rendered).is_ok(), "{rendered}");
        }
    }

    #[test]
    fn the_closest_examples_are_shown() {
        // the owner's screenshot: "130 kiloyum" was answered without logging the weight
        let bank = bank();
        let picked = select(&bank, "130 kiloyum", None, 6);
        assert_eq!(picked[0].d["args"]["kind"], "weight", "{:?}", picked.iter().map(|e| &e.q).collect::<Vec<_>>());
        assert!(picked.iter().any(|e| e.d["tool"] == "answer"), "one plain answer always");
        let picked = select(&bank, "notlarıma ekle bunu", Some("bugün derste işletim sistemleri öğrendik"), 6);
        assert_eq!(picked[0].d["tool"], "create_note");
    }

    #[test]
    fn dates_are_made_real() {
        let bank = bank();
        let e = bank.iter().find(|e| e.q.starts_with("Cumartesi markete")).unwrap();
        assert!(render(e, d("2026-10-09")).contains("2026-10-10"), "Friday → the next Saturday");
        let e = bank.iter().find(|e| e.q.starts_with("I need to pay the rent on Friday")).unwrap();
        assert!(render(e, d("2026-10-09")).contains("2026-10-16"), "on a Friday, \"Friday\" is next week's");
    }
}
