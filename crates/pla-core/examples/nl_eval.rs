//! The natural-language quality evaluation (nl-quality plan § 1): the app's own code — the
//! assistant (`qa::engine::answer`) and extraction (model → guard → validation) — run against the
//! real model on the cases in `tests/nl/`. Not part of `cargo test`: it needs the model.
//!   $env:PLA_LLAMA_SERVER = "C:\dev\PLA\apps\desktop\src-tauri\llama\llama-server.exe"
//!   $env:PLA_MODEL = "C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
//!   cargo run --release -p pla-core --example nl_eval -- <label> [id-prefix|assistant|extraction|memory] [--no-memory]
//! `--no-memory` leaves the cases' `memory` unwritten and unloaded (the without-memory run).
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

#[path = "memory_fixture/mod.rs"]
mod memory_fixture;

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
        // "+3" and "-3" are days from today; a date stays as it is
        _ if s.starts_with(['+', '-']) => s.parse::<i64>().ok(),
        _ => None,
    };
    match offset {
        Some(d) => (today + chrono::Duration::days(d)).to_string(),
        None => s.to_owned(),
    }
}

/// How close two messages are: shared word stems (five letters, digits as one sign), Jaccard.
fn closeness(a: &str, b: &str) -> f64 {
    let stems = |s: &str| -> std::collections::BTreeSet<String> {
        pla_core::index::key(s)
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| if w.chars().all(|c| c.is_ascii_digit()) { "#".to_owned() } else { w.chars().take(5).collect() })
            .collect()
    };
    let (x, y) = (stems(a), stems(b));
    let union = x.union(&y).count();
    if union == 0 {
        0.0
    } else {
        x.intersection(&y).count() as f64 / union as f64
    }
}

/// A held-out case shares less than this with every example in the bank.
const HELD_OUT_MAX: f64 = 0.5;

/// The answer's language, judged apart from the app's own detection.
fn answer_language(text: &str) -> Option<&'static str> {
    let lower = pla_core::index::key(text); // "İyi" → "iyi" (Rust's own lower case adds a dot)
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let tr_letters = lower.chars().filter(|c| "çğıöşü".contains(*c)).count();
    const TR: [&str; 20] = [
        "ve",
        "bir",
        "bu",
        "için",
        "ile",
        "de",
        "da",
        "ne",
        "var",
        "yok",
        "olarak",
        "sana",
        "size",
        "senin",
        "notlarında",
        "notlar",
        "bugün",
        "yarın",
        "evet",
        "tamam",
    ];
    const EN: [&str; 20] =
        ["the", "and", "you", "your", "is", "are", "to", "of", "in", "i", "it", "that", "for", "have", "with", "notes", "today", "this", "can", "not"];
    let tr = words.iter().filter(|w| TR.contains(w)).count() + tr_letters.min(5);
    let en = words.iter().filter(|w| EN.contains(w)).count();
    match (tr, en) {
        (0, 0) => None, // "Hi." tells nothing either way
        (tr, en) if tr >= en => Some("tr"),
        _ => Some("en"),
    }
}

/// The words that speak to the user as "sen": the pronoun, the verb ending -sın/-sin/-sun/-sün,
/// the conditional -rsan/-rsen, and "your" on a plural (-ların, -lerinde). Rough on purpose: the
/// failures are read by hand, and a few common words with the same ending are let through.
fn informal_words(text: &str) -> Vec<String> {
    const PRONOUNS: [&str; 7] = ["sen", "sana", "seni", "senin", "sende", "senden", "seninle"];
    const ENDINGS: [&str; 13] = ["sin", "sun", "sün", "rsan", "rsen", "larin", "lerin", "larinda", "lerinde", "larindan", "lerinden", "larina", "lerine"];
    const NOT_SEN: [&str; 6] = ["kesin", "olsun", "insan", "hasan", "gelsin", "versin"];
    // `key` folds ı to i: "mısın" is "misin", "notlarında" is "notlarinda"
    pla_core::index::key(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| PRONOUNS.contains(w) || (!NOT_SEN.contains(w) && ENDINGS.iter().any(|e| w.len() > e.len() && w.ends_with(e))))
        .map(str::to_owned)
        .collect()
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

fn assistant_case(model: &mut ModelHost, case: &Value, with_memory: bool) -> Result<(Vec<String>, Value), Box<dyn std::error::Error>> {
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
    if with_memory && case["memory"].is_object() {
        memory_fixture::write_memory(&vault.root.join(&vault.config.folders.memory), &case["memory"])?;
    }
    let recall = if with_memory { pla_core::recall::load(&vault) } else { pla_core::recall::Recall::default() };
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
    // name, address and formality as the settings store them; none = the default
    let persona: pla_core::settings::Persona = case.get("persona").map(|p| serde_json::from_value(p.clone())).transpose()?.unwrap_or_default();
    let answer = engine::answer(model, &env, &engine::Question { text: q, turns: &turns, memory: None, recall: Some(&recall), ui_lang: lang, persona: &persona }, &AtomicBool::new(false), &mut |_| {})?;

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
        wanted.sort();
        // a tool named twice must run twice ("add_task", "add_task"); once means at least once
        if wanted.windows(2).all(|p| p[0] != p[1]) {
            got.dedup();
        }
        if got != wanted {
            fails.push(format!("writes {got:?}, wanted {wanted:?}"));
        }
    }
    // every task wanted, each matched by an add_task of its own
    for w in want["adds"].as_array().cloned().unwrap_or_default() {
        let found = answer.tools.iter().any(|t| t.ok && t.tool == "add_task" && w.as_object().unwrap().iter().all(|(k, v)| arg_matches(k, v, &t.args[k])));
        if !found {
            fails.push(format!("no add_task {w}"));
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
    if want["formal"].as_bool() == Some(true) {
        let informal = informal_words(&answer.text);
        if !informal.is_empty() {
            fails.push(format!("speaks as sen: {informal:?}"));
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
    if let Some(any) = want["contains_any"].as_array() {
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
    // as the pipeline does: block by block (a list item is a block), each guarded with its section
    let settings = ValidationSettings::default();
    let mut raw = Vec::new();
    let mut items: Vec<ValidItem> = Vec::new();
    for block in pla_core::notes::split_blocks(text) {
        let answer = model.extract_raw(reference, &block.text)?;
        let section = pla_core::extraction::section_of(text, &block.text);
        let extraction = pla_core::extraction::guard(&block.text, section, parse_extraction(&answer)?);
        items.extend(extraction.items.iter().filter_map(|i| validate(i, reference, &settings).ok()));
        raw.push(answer);
    }
    let shown: Vec<Value> = items
        .iter()
        .map(|i| match i {
            ValidItem::Action { title, date, time, reminder } => json!({ "type": if *reminder { "reminder" } else { "task" }, "title": title, "date": date.map(|d| d.to_string()), "time": time.map(|t| t.format("%H:%M").to_string()) }),
            ValidItem::Routine { title, days, time } => {
                let codes: Vec<Value> = days.iter().map(|d| serde_json::to_value(d).unwrap()).collect();
                json!({ "type": "routine", "kind": "routine", "title": title, "days": codes, "time": time.map(|t| t.format("%H:%M").to_string()) })
            }
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

/// `text` folded without Turkish letters, the way the memory cases spell `has` and `not`.
fn loose(text: &str) -> String {
    pla_core::index::key(text).chars().map(|c| match c {
        'ç' => 'c',
        'ğ' => 'g',
        'ş' => 's',
        'ö' => 'o',
        'ü' => 'u',
        c => c,
    }).collect()
}

/// Whether `got` is one of the `|`-separated `alternatives`.
fn one_of(alternatives: &str, got: &str) -> bool {
    alternatives.split('|').any(|a| loose(a.trim()) == loose(got.trim()))
}

fn role_name(role: pla_core::recall::Role) -> &'static str {
    use pla_core::recall::Role;
    match role {
        Role::About => "about",
        Role::Rules => "rules",
        Role::State => "state",
        Role::People => "people",
        Role::Archive => "archive",
    }
}

/// The topic as `recall::apply` stores it (a person's topic starts with their name).
fn stored_topic(p: &pla_core::recall::Proposal) -> String {
    use pla_core::recall::topic_key;
    let key = topic_key(&p.topic);
    match &p.person {
        Some(name) if p.role == pla_core::recall::Role::People => {
            let pk = topic_key(name);
            if key == pk || key.starts_with(&format!("{pk}-")) { key } else { format!("{pk}-{key}") }
        }
        _ => key,
    }
}

fn wanted_add(w: &Value, p: &pla_core::recall::Proposal) -> bool {
    let text = loose(&p.text);
    let groups = |k: &str| w[k].as_array().cloned().unwrap_or_default().into_iter().filter_map(|v| v.as_str().map(str::to_owned)).collect::<Vec<_>>();
    w["file"].as_str().is_none_or(|f| one_of(f, role_name(p.role)))
        && w["person"].as_str().is_none_or(|n| p.person.as_deref().is_some_and(|got| one_of(n, got)))
        && w["topic"].as_str().is_none_or(|t| pla_core::recall::topic_key(t) == stored_topic(p))
        && groups("has").iter().all(|g| g.split('|').any(|a| text.contains(&loose(a))))
        && !groups("not").iter().any(|g| g.split('|').any(|a| text.contains(&loose(a))))
}

/// Chat → memory (plan Task 8): the app's request, the model, `parse` and `guard`, then the
/// changes compared with the wanted ones. `got` carries `wanted`, `found` and `wrong` counts.
fn memory_case(model: &mut ModelHost, case: &Value) -> Result<(Vec<String>, Value), Box<dyn std::error::Error>> {
    use pla_core::qa::ChatModel;
    use pla_core::recall::{extract, Change};
    let msg = case["msg"].as_str().unwrap();
    let prev = case["prev"].as_str();
    let topics: Vec<String> = case["topics"].as_array().cloned().unwrap_or_default().iter().filter_map(|t| t.as_str().map(str::to_owned)).collect();
    let rejected: Vec<(String, String)> = case["rejected"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| (r[0].as_str().unwrap_or_default().to_owned(), r[1].as_str().unwrap_or_default().to_owned()))
        .collect();
    let lang = if case["ui"] == "en" { pla_core::lang::Lang::En } else { pla_core::lang::Lang::Tr };
    let raw = model.complete(&extract::request(msg, prev, &topics, &rejected, lang), &AtomicBool::new(false))?;
    let changes = extract::guard(extract::parse(&raw)?, msg, prev, &topics, &rejected);

    let want = &case["expect"];
    let adds = want["adds"].as_array().cloned().unwrap_or_default();
    let forgets: Vec<String> = want["forget"].as_array().cloned().unwrap_or_default().iter().filter_map(|t| t.as_str().map(pla_core::recall::topic_key)).collect();
    let mut taken = vec![false; adds.len()];
    let mut forgot = vec![false; forgets.len()];
    let mut fails = Vec::new();
    let mut shown = Vec::new();
    for c in &changes {
        match c {
            Change::Add(p) => {
                shown.push(json!({ "add": p.text, "file": role_name(p.role), "person": p.person, "topic": stored_topic(p) }));
                match (0..adds.len()).find(|&i| !taken[i] && wanted_add(&adds[i], p)) {
                    Some(i) => taken[i] = true,
                    None => fails.push(format!("wrong write: {} ({}{})", p.text, role_name(p.role), p.person.as_deref().map(|n| format!(", {n}")).unwrap_or_default())),
                }
            }
            Change::Forget { topic, reason } => {
                shown.push(json!({ "forget": topic, "reason": reason }));
                match (0..forgets.len()).find(|&i| !forgot[i] && forgets[i] == pla_core::recall::topic_key(topic)) {
                    Some(i) => forgot[i] = true,
                    None => fails.push(format!("wrong write: forget {topic}")),
                }
            }
        }
    }
    let wrong = fails.len();
    for (w, _) in adds.iter().zip(&taken).filter(|(_, t)| !**t) {
        fails.push(format!("missing {w}"));
    }
    for (t, _) in forgets.iter().zip(&forgot).filter(|(_, f)| !**f) {
        fails.push(format!("did not forget {t}"));
    }
    let found = taken.iter().chain(&forgot).filter(|t| **t).count();
    Ok((fails, json!({ "raw": raw, "changes": shown, "wanted": adds.len() + forgets.len(), "found": found, "wrong": wrong })))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let with_memory = !std::env::args().any(|a| a == "--no-memory");
    let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--no-memory").collect();
    let label = args.first().cloned().unwrap_or_else(|| "run".into());
    let only = args.get(1).cloned().unwrap_or_default();
    let mut cfg = ServerConfig::new(PathBuf::from(std::env::var("PLA_LLAMA_SERVER")?), PathBuf::from(std::env::var("PLA_MODEL")?));
    cfg.startup_timeout = Duration::from_secs(120);
    let mut model = ModelHost::new(cfg, Duration::from_secs(600));

    // the bank must not hold the cases themselves: the model would be shown the answer; held-out
    // cases must not even be close to an example (final review I5)
    let bank_q: Vec<String> = pla_core::qa::examples::bank().into_iter().map(|e| e.q).collect();
    let cases = std::fs::read_to_string(format!("{CASES}/assistant.jsonl"))?;
    for line in cases.lines().filter(|l| l.starts_with('{')) {
        let case: Value = serde_json::from_str(line)?;
        let q = case["q"].as_str().unwrap_or_default();
        let closest = bank_q.iter().map(|b| (closeness(q, b), b)).max_by(|a, b| a.0.total_cmp(&b.0));
        if bank_q.iter().any(|b| b.to_lowercase() == q.to_lowercase()) {
            return Err(format!("case {} is in assets/qa_examples.jsonl", case["id"]).into());
        }
        if let (Some((score, near)), true) = (closest, case["cat"] == "heldout") {
            if score >= HELD_OUT_MAX {
                return Err(format!("held-out case {} is {score:.2} close to the example {near:?}", case["id"]).into());
            }
        }
    }

    // the same rule for the chat → memory cases: a held-out message is not close to a dev case or
    // to an example in the prompt, and no case is an example
    let memory_cases: Vec<Value> =
        std::fs::read_to_string(format!("{CASES}/memory.jsonl"))?.lines().filter(|l| l.starts_with('{')).map(serde_json::from_str).collect::<Result<_, _>>()?;
    let examples = pla_core::recall::extract::example_messages();
    let near: Vec<&str> = memory_cases.iter().filter(|c| c["cat"] == "dev").filter_map(|c| c["msg"].as_str()).chain(examples.iter().copied()).collect();
    for case in &memory_cases {
        let msg = case["msg"].as_str().unwrap_or_default();
        if examples.iter().any(|e| e.to_lowercase() == msg.to_lowercase()) {
            return Err(format!("case {} is an example in recall::extract", case["id"]).into());
        }
        if case["cat"] != "heldout" {
            continue;
        }
        if let Some((score, close)) = near.iter().map(|d| (closeness(msg, d), d)).max_by(|a, b| a.0.total_cmp(&b.0)).filter(|(s, _)| *s >= HELD_OUT_MAX) {
            return Err(format!("held-out case {} is {score:.2} close to {close:?}", case["id"]).into());
        }
    }

    let mut results = Vec::new();
    let mut score: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    // memory: (found, wanted, wrong writes) per category
    let mut writes: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    let started = Instant::now();
    for (file, kind) in [("assistant.jsonl", "assistant"), ("extraction.jsonl", "extraction"), ("memory.jsonl", "memory")] {
        for line in std::fs::read_to_string(format!("{CASES}/{file}"))?.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("//")) {
            let case: Value = serde_json::from_str(line).map_err(|e| format!("{file}: {e}: {line}"))?;
            let id = case["id"].as_str().unwrap();
            // `assistant`, `extraction` or `memory` runs one file; anything else is an id prefix
            let wanted = if ["assistant", "extraction", "memory"].contains(&only.as_str()) { only == kind } else { id.starts_with(&only) };
            if !wanted {
                continue;
            }
            let t = Instant::now();
            let outcome = match kind {
                "assistant" => assistant_case(&mut model, &case, with_memory),
                "extraction" => extraction_case(&mut model, &case),
                _ => memory_case(&mut model, &case),
            };
            let (fails, got) = outcome.unwrap_or_else(|e| (vec![format!("error: {e}")], Value::Null));
            let cat = format!("{kind}/{}", case["cat"].as_str().unwrap_or("-"));
            if kind == "memory" {
                let w = writes.entry(cat.clone()).or_default();
                w.0 += got["found"].as_u64().unwrap_or(0);
                w.1 += got["wanted"].as_u64().unwrap_or(0);
                w.2 += got["wrong"].as_u64().unwrap_or(0);
            }
            let e = score.entry(cat).or_default();
            e.1 += 1;
            if fails.is_empty() {
                e.0 += 1;
            }
            println!(
                "{} {id} ({:.1}s){}",
                if fails.is_empty() { "PASS" } else { "FAIL" },
                t.elapsed().as_secs_f64(),
                if fails.is_empty() { String::new() } else { format!(": {}", fails.join("; ")) }
            );
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
    if !writes.is_empty() {
        println!("\n| memory | right writes | wrong writes |\n| --- | --- | --- |");
        for (cat, (found, wanted, wrong)) in &writes {
            println!("| {cat} | {found}/{wanted} ({:.0}%) | {wrong} |", 100.0 * *found as f64 / (*wanted).max(1) as f64);
        }
    }
    println!("\n{out} in {:.0}s", started.elapsed().as_secs_f64());
    Ok(())
}
