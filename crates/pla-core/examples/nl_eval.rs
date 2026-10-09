//! The natural-language quality evaluation (nl-quality plan § 1): the app's own code — the
//! assistant (`qa::engine::answer`) and extraction (model → guard → validation) — run against the
//! real model on the cases in `tests/nl/`. Not part of `cargo test`: it needs the model.
//!   $env:PLA_LLAMA_SERVER = "C:\dev\PLA\apps\desktop\src-tauri\llama\llama-server.exe"
//!   $env:PLA_MODEL = "C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
//!   cargo run --release -p pla-core --example nl_eval -- <label> [id-prefix]
//! Writes research/nl-eval/results/<label>.jsonl and prints the score by category.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, NaiveDate};
use pla_core::extraction::{parse_extraction, validate, ValidItem, ValidationSettings};
use pla_core::llm::{ModelHost, ServerConfig};
use pla_core::pipeline::Extractor;
use pla_core::qa::engine;
use pla_core::qa::tools::ToolEnv;
use pla_core::qa::Turn;
use serde_json::{json, Value};

const NOW: &str = "2026-10-09T21:00:00+03:00";
const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/nl");
const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../research/nl-eval/results");

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(NOW).unwrap()
}

/// "today", "tomorrow", "yesterday", "+N"/"-N" days, or a date as it is.
fn date_token(s: &str) -> String {
    let today = now().date_naive();
    let offset = match s {
        "today" => Some(0),
        "tomorrow" => Some(1),
        "yesterday" => Some(-1),
        _ => s.strip_prefix('+').or(s.strip_prefix('-').map(|_| s)).and_then(|n| n.parse::<i64>().ok()),
    };
    match offset {
        Some(d) => (today + chrono::Duration::days(d)).to_string(),
        None => s.to_owned(),
    }
}

/// The answer's language, judged apart from the app's own detection.
fn answer_language(text: &str) -> Option<&'static str> {
    let lower = pla_core::index::key(text); // "İyi" → "iyi" (Rust's own lower case adds a dot)
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let tr_letters = lower.chars().filter(|c| "çğıöşü".contains(*c)).count();
    const TR: [&str; 20] = ["ve", "bir", "bu", "için", "ile", "de", "da", "ne", "var", "yok", "olarak", "sana", "size", "senin", "notlarında", "notlar", "bugün", "yarın", "evet", "tamam"];
    const EN: [&str; 20] = ["the", "and", "you", "your", "is", "are", "to", "of", "in", "i", "it", "that", "for", "have", "with", "notes", "today", "this", "can", "not"];
    let tr = words.iter().filter(|w| TR.contains(w)).count() + tr_letters.min(5);
    let en = words.iter().filter(|w| EN.contains(w)).count();
    match (tr, en) {
        (0, 0) => None, // "Hi." tells nothing either way
        (tr, en) if tr >= en => Some("tr"),
        _ => Some("en"),
    }
}

/// Whether `got` satisfies `want`: numbers equal, text contains (case-insensitively), dates by token.
fn arg_matches(key: &str, want: &Value, got: &Value) -> bool {
    match want {
        Value::Number(n) => got.as_f64().is_some_and(|g| (g - n.as_f64().unwrap()).abs() < 1e-6),
        Value::String(s) if key == "date" => got.as_str() == Some(date_token(s).as_str()),
        Value::String(s) => got.as_str().is_some_and(|g| pla_core::index::key(g).contains(&pla_core::index::key(s))),
        other => got == other,
    }
}

const WRITES: [&str; 4] = ["add_task", "complete_task", "log_metric", "create_note"];

fn assistant_case(model: &mut ModelHost, case: &Value) -> Result<(Vec<String>, Value), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    std::fs::create_dir_all(tmp.path().join("kasa"))?;
    let vault = pla_core::vault::open_vault(&tmp.path().join("kasa"))?;
    let dbs = pla_core::db::open_databases(&tmp.path().join("data"))?;
    let ms = now().timestamp_millis();
    for n in case["notes"].as_array().cloned().unwrap_or_default() {
        let (path, text) = (n["path"].as_str().unwrap(), n["text"].as_str().unwrap());
        let file = vault.root.join(path);
        std::fs::create_dir_all(file.parent().unwrap())?;
        std::fs::write(&file, text)?;
        let age = n["age_h"].as_i64().unwrap_or(2);
        pla_core::index::index_note(&dbs.cache, path, text, ms - age * 3_600_000, text.len() as i64)?;
    }
    for t in case["tasks"].as_array().cloned().unwrap_or_default() {
        let input = pla_core::tasks::TaskInput {
            title: t["title"].as_str().unwrap().into(),
            details: None,
            date: t["date"].as_str().map(date_token),
            time: t["time"].as_str().map(str::to_owned),
            remind: Some(false),
        };
        pla_core::tasks::add_task_as(&dbs.pla, &input, "manual", now())?;
    }
    let turns: Vec<Turn> = case["turns"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, t)| Turn {
            turn_id: format!("t{i}"),
            question: t[0].as_str().unwrap().into(),
            answer: t[1].as_str().unwrap().into(),
            tools: Vec::new(),
            created_at: String::new(),
            status: "done".into(),
            new_topic: false,
        })
        .collect();
    let env = ToolEnv { vault: &vault, pla: &dbs.pla, cache: &dbs.cache, now: now(), validation: ValidationSettings::default() };
    let lang = case["ui"].as_str().unwrap_or("tr");
    let q = case["q"].as_str().unwrap();
    let answer = engine::answer(model, &env, q, &turns, None, lang, &AtomicBool::new(false), &mut |_| {})?;

    let want = &case["expect"];
    let mut fails = Vec::new();
    let called: Vec<&str> = answer.tools.iter().filter(|t| t.ok).map(|t| t.tool.as_str()).collect();
    if let Some(l) = want["lang"].as_str() {
        if let Some(got) = answer_language(&answer.text).filter(|got| *got != l) {
            fails.push(format!("answer in {got}, wanted {l}"));
        }
    }
    if let Some(writes) = want["writes"].as_array() {
        let mut got: Vec<&str> = called.iter().copied().filter(|t| WRITES.contains(t)).collect();
        let mut wanted: Vec<&str> = writes.iter().filter_map(Value::as_str).collect();
        got.sort();
        got.dedup();
        wanted.sort();
        if got != wanted {
            fails.push(format!("writes {got:?}, wanted {wanted:?}"));
        }
    }
    for r in want["reads"].as_array().cloned().unwrap_or_default() {
        if !called.contains(&r.as_str().unwrap()) {
            fails.push(format!("did not call {r}"));
        }
    }
    if let Some(args) = want["args"].as_object() {
        for (tool, keys) in args {
            let Some(rec) = answer.tools.iter().find(|t| t.ok && t.tool == *tool) else { continue };
            for (k, v) in keys.as_object().unwrap() {
                if !arg_matches(k, v, &rec.args[k]) {
                    fails.push(format!("{tool}.{k} = {}, wanted {v}", rec.args[k]));
                }
            }
        }
    }
    if let Some(s) = want["suggest"].as_bool() {
        let got = answer.tools.iter().any(|t| t.tool == "suggest_note");
        if got != s {
            fails.push(format!("suggested a note: {got}, wanted {s}"));
        }
    }
    if want["no_question"].as_bool() == Some(true) && answer.text.contains('?') {
        fails.push("asked back".into());
    }
    if let Some(any) = want["contains"].as_array() {
        let lower = answer.text.to_lowercase();
        if !any.iter().filter_map(Value::as_str).any(|s| lower.contains(&s.to_lowercase())) {
            fails.push(format!("answer has none of {any:?}"));
        }
    }
    let tools: Vec<Value> = answer.tools.iter().map(|t| json!({ "tool": t.tool, "args": t.args, "ok": t.ok, "error": t.error })).collect();
    Ok((fails, json!({ "answer": answer.text, "tools": tools })))
}

fn extraction_case(model: &mut ModelHost, case: &Value) -> Result<(Vec<String>, Value), Box<dyn std::error::Error>> {
    let reference = case["ref"].as_str().map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d")).transpose()?.unwrap_or(now().date_naive());
    let text = case["text"].as_str().unwrap();
    let raw = model.extract_raw(reference, text)?;
    let extraction = pla_core::extraction::guard(text, parse_extraction(&raw)?);
    let settings = ValidationSettings::default();
    let items: Vec<ValidItem> = extraction.items.iter().filter_map(|i| validate(i, reference, &settings).ok()).collect();
    let shown: Vec<Value> = items
        .iter()
        .map(|i| match i {
            ValidItem::Action { title, date, time, reminder } => json!({ "type": if *reminder { "reminder" } else { "task" }, "title": title, "date": date.map(|d| d.to_string()), "time": time.map(|t| t.format("%H:%M").to_string()) }),
            ValidItem::Metric { kind, date, value, exercise, sets, reps, .. } => json!({ "type": "metric", "kind": kind, "date": date.to_string(), "value": value, "exercise": exercise, "sets": sets, "reps": reps }),
        })
        .collect();
    let want = &case["expect"];
    let mut fails = Vec::new();
    if want["empty"].as_bool() == Some(true) && !shown.is_empty() {
        fails.push(format!("{} items, wanted none", shown.len()));
    }
    for kind in want["none_of"].as_array().cloned().unwrap_or_default() {
        if shown.iter().any(|i| i["kind"] == kind) {
            fails.push(format!("has a {kind} item"));
        }
    }
    for w in want["items"].as_array().cloned().unwrap_or_default() {
        let found = shown.iter().any(|got| {
            w.as_object().unwrap().iter().all(|(k, v)| match (k.as_str(), v) {
                ("value", Value::Null) => got["value"].is_null(),
                (k, v) => arg_matches(k, v, &got[k]),
            })
        });
        if !found {
            fails.push(format!("missing {w}"));
        }
    }
    Ok((fails, json!({ "raw": raw, "items": shown })))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let label = std::env::args().nth(1).unwrap_or_else(|| "run".into());
    let only = std::env::args().nth(2).unwrap_or_default();
    let mut cfg = ServerConfig::new(PathBuf::from(std::env::var("PLA_LLAMA_SERVER")?), PathBuf::from(std::env::var("PLA_MODEL")?));
    cfg.startup_timeout = Duration::from_secs(120);
    let mut model = ModelHost::new(cfg, Duration::from_secs(600));

    // the bank must not hold the cases themselves: the model would be shown the answer
    let bank: Vec<String> = pla_core::qa::examples::bank().into_iter().map(|e| e.q.to_lowercase()).collect();
    let cases = std::fs::read_to_string(format!("{CASES}/assistant.jsonl"))?;
    for line in cases.lines().filter(|l| l.starts_with('{')) {
        let case: Value = serde_json::from_str(line)?;
        let q = case["q"].as_str().unwrap_or_default().to_lowercase();
        if bank.contains(&q) {
            return Err(format!("case {} is in assets/qa_examples.jsonl", case["id"]).into());
        }
    }

    let mut results = Vec::new();
    let mut score: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let started = Instant::now();
    for (file, kind) in [("assistant.jsonl", "assistant"), ("extraction.jsonl", "extraction")] {
        for line in std::fs::read_to_string(format!("{CASES}/{file}"))?.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("//")) {
            let case: Value = serde_json::from_str(line).map_err(|e| format!("{file}: {e}: {line}"))?;
            let id = case["id"].as_str().unwrap();
            if !id.starts_with(&only) {
                continue;
            }
            let t = Instant::now();
            let outcome = if kind == "assistant" { assistant_case(&mut model, &case) } else { extraction_case(&mut model, &case) };
            let (fails, got) = outcome.unwrap_or_else(|e| (vec![format!("error: {e}")], Value::Null));
            let cat = format!("{kind}/{}", case["cat"].as_str().unwrap_or("-"));
            let e = score.entry(cat).or_default();
            e.1 += 1;
            if fails.is_empty() {
                e.0 += 1;
            }
            println!("{} {id} ({:.1}s){}", if fails.is_empty() { "PASS" } else { "FAIL" }, t.elapsed().as_secs_f64(), if fails.is_empty() { String::new() } else { format!(": {}", fails.join("; ")) });
            results.push(json!({ "id": id, "pass": fails.is_empty(), "fails": fails, "got": got }));
        }
    }
    std::fs::create_dir_all(OUT)?;
    let out = format!("{OUT}/{label}.jsonl");
    std::fs::write(&out, results.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n")?;
    let (pass, all) = score.values().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    println!("\n| category | pass |\n| --- | --- |");
    for (cat, (p, n)) in &score {
        println!("| {cat} | {p}/{n} |");
    }
    println!("| **total** | **{pass}/{all}** ({:.0}%) |", 100.0 * pass as f64 / all.max(1) as f64);
    println!("\n{out} in {:.0}s", started.elapsed().as_secs_f64());
    Ok(())
}
