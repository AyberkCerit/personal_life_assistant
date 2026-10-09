//! The assistant's tools (FR-QA-004…009, 016): the only things the model may do. None deletes,
//! each write touches one item, and every argument is checked here before anything runs.

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime};
use rusqlite::Connection;
use serde_json::{json, Value};

use super::{ToolRecord, Undo};
use crate::extraction::{MetricKind, Unit, ValidationSettings};
use crate::metrics::{self, MetricInput};
use crate::tasks::{self, TaskInput, TaskList};
use crate::vault::Vault;

pub const TOOLS: [&str; 8] = ["search_notes", "query_tasks", "query_metrics", "add_task", "complete_task", "log_metric", "create_note", "suggest_note"];
const KINDS: [&str; 5] = ["sleep", "water", "steps", "weight", "workout"];
const UNITS: [&str; 8] = ["h", "min", "ml", "l", "glass", "kg", "lb", "count"];

/// What the model reads about each tool (in the system prompt).
pub fn descriptions() -> &'static str {
    "- search_notes {query}: find the user's notes by words.\n\
     - query_tasks {list: today|upcoming|completed|all, text?}: tasks with their task_id, date, time and status.\n\
     - query_metrics {kind: sleep|water|steps|weight|workout, days: 1-365}: daily values, total, average, min, max.\n\
     - add_task {title, date?: YYYY-MM-DD, time?: HH:MM, remind?: true|false}: add one task; with a time it is a reminder unless remind is false.\n\
     - complete_task {task_id}: mark one open task done (take the task_id from query_tasks).\n\
     - log_metric {kind, date: YYYY-MM-DD, value, unit?: h|min|ml|l|glass|kg|lb|count, exercise?}: record one measurement.\n\
     - create_note {title, body (at most 600 characters)}: write one new note to the user's notes. Choose a short title yourself; never ask for one.
     - suggest_note {title, body}: offer to keep what the user just told you as a note; nothing is written unless they press Save."
}

fn obj(tool: &str, props: Value, required: &[&str]) -> Value {
    let mut req = vec!["tool".to_owned()];
    let mut schema = json!({ "type": "object", "properties": { "tool": { "const": tool } }, "additionalProperties": false });
    if !props.as_object().is_some_and(|p| p.is_empty()) {
        schema["properties"]["args"] = json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false });
        req.push("args".into());
    }
    schema["required"] = json!(req);
    schema
}

/// FR-QA-005: the grammar for the decision step: either `{"tool":"answer"}` or one tool call.
pub fn decision_schema() -> Value {
    let date = json!({ "type": "string", "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}$" });
    let time = json!({ "type": "string", "pattern": "^[0-9]{2}:[0-9]{2}$" });
    json!({ "anyOf": [
        obj("answer", json!({}), &[]),
        obj("search_notes", json!({ "query": { "type": "string", "minLength": 1, "maxLength": 100 } }), &["query"]),
        obj("query_tasks", json!({ "list": { "enum": ["today", "upcoming", "completed", "all"] }, "text": { "type": "string", "maxLength": 100 } }), &["list"]),
        obj("query_metrics", json!({ "kind": { "enum": KINDS }, "days": { "type": "integer", "minimum": 1, "maximum": 365 } }), &["kind", "days"]),
        obj("add_task", json!({ "title": { "type": "string", "minLength": 1, "maxLength": 200 }, "date": date, "time": time, "remind": { "type": "boolean" } }), &["title"]),
        obj("complete_task", json!({ "task_id": { "type": "string", "minLength": 1, "maxLength": 64 } }), &["task_id"]),
        obj("log_metric", json!({ "kind": { "enum": KINDS }, "date": date, "value": { "type": "number" }, "unit": { "enum": UNITS }, "exercise": { "type": "string", "maxLength": 100 } }), &["kind", "date", "value"]),
        obj("create_note", json!({ "title": { "type": "string", "minLength": 1, "maxLength": 120 }, "body": { "type": "string", "maxLength": 600 } }), &["title", "body"]),
        obj("suggest_note", json!({ "title": { "type": "string", "minLength": 1, "maxLength": 120 }, "body": { "type": "string", "maxLength": 600 } }), &["title", "body"]),
    ]})
}

/// The model's decision, read and checked.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Answer,
    Call { tool: String, args: Value },
}

/// Reads `{"tool": …, "args": …}`. An unknown tool or a non-object is an error for the model.
pub fn parse_decision(raw: &str) -> Result<Decision, String> {
    let v: Value = serde_json::from_str(raw.trim()).map_err(|e| format!("not JSON: {e}"))?;
    let tool = v["tool"].as_str().ok_or("missing \"tool\"")?;
    if tool == "answer" {
        return Ok(Decision::Answer);
    }
    if !TOOLS.contains(&tool) {
        return Err(format!("unknown tool {tool:?}"));
    }
    let args = if v["args"].is_null() { json!({}) } else { v["args"].clone() };
    if !args.is_object() {
        return Err("\"args\" must be an object".into());
    }
    Ok(Decision::Call { tool: tool.to_owned(), args })
}

/// What a tool can reach: the vault, pla.db (as the worker, its only writer), cache.db for search.
pub struct ToolEnv<'a> {
    pub vault: &'a Vault,
    pub pla: &'a Connection,
    pub cache: &'a Connection,
    pub now: DateTime<FixedOffset>,
    pub validation: ValidationSettings,
}

fn text(args: &Value, key: &str, max: usize) -> Result<Option<String>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) if s.chars().count() <= max => Ok(Some(s.trim().to_owned())),
        Value::String(_) => Err(format!("{key} is longer than {max} characters")),
        _ => Err(format!("{key} must be text")),
    }
}

fn required(args: &Value, key: &str, max: usize) -> Result<String, String> {
    text(args, key, max)?.ok_or_else(|| format!("{key} is required"))
}

fn date(args: &Value, key: &str) -> Result<Option<String>, String> {
    let Some(d) = text(args, key, 10)? else { return Ok(None) };
    NaiveDate::parse_from_str(&d, "%Y-%m-%d").map_err(|_| format!("{key} {d:?} is not a date (YYYY-MM-DD)"))?;
    Ok(Some(d))
}

/// Writes a new note into the user's notes (the owner's decision: what they ask to keep goes there,
/// titled by the model). FR-QA-009: marked as written by PLA, so it is never mined for tasks.
pub fn write_note(vault: &Vault, title: &str, body: &str) -> Result<String, String> {
    let rel = crate::files::create_note(vault, &vault.config.folders.notes, title).map_err(|e| e.to_string())?;
    let path = crate::files::resolve(vault, &rel).map_err(|e| e.to_string())?;
    let content = format!("---\npla_generated: true\n---\n# {title}\n\n{body}\n");
    if let Err(e) = crate::fs_atomic::write_atomic(&path, content.as_bytes()) {
        let _ = std::fs::remove_file(&path); // not an empty note left behind
        return Err(e.to_string());
    }
    Ok(rel)
}

/// Whether a task title fits the words the model searched with. Turkish endings vary ("faturayı
/// ödedim" for the task "Fatura öde"), so a word matches by its first four letters.
fn title_matches(title: &str, text: &str) -> bool {
    let words = |s: &str| crate::index::key(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let title_words = words(title);
    if crate::index::key(title).contains(&crate::index::key(text)) {
        return true;
    }
    // most of the words must fit: "annemin hediyesi" is not "Annemi ara" (final review I2)
    let asked: Vec<String> = words(text).into_iter().filter(|w| w.chars().count() >= 3).collect();
    let fits = asked
        .iter()
        .filter(|w| {
            let stem: String = w.chars().take(4).collect();
            title_words.iter().any(|t| (w.chars().count() >= 4 && t.starts_with(&stem)) || (t.chars().count() >= 3 && w.starts_with(t.as_str())))
        })
        .count();
    !asked.is_empty() && fits * 2 > asked.len()
}

/// A file name from the model's title: characters Windows refuses become spaces; an empty one comes
/// from the first words of the body, or the date.
pub fn note_title(title: &str, body: &str, today: NaiveDate) -> String {
    let clean = |s: &str| {
        let s: String = s.chars().map(|c| if c.is_control() || "<>:\"/\\|?*#^[]".contains(c) { ' ' } else { c }).collect();
        let words: Vec<&str> = s.split_whitespace().collect();
        words.join(" ").trim_matches(|c: char| c == '.' || c == ' ').chars().take(80).collect::<String>().trim().to_owned()
    };
    let t = clean(title);
    if !t.is_empty() && crate::fileops::check_name(&t).is_ok() {
        return t;
    }
    let from_body = clean(&body.split_whitespace().take(6).collect::<Vec<_>>().join(" "));
    if !from_body.is_empty() && crate::fileops::check_name(&from_body).is_ok() {
        return from_body;
    }
    format!("Not {today}")
}

fn kind(args: &Value) -> Result<MetricKind, String> {
    serde_json::from_value(args["kind"].clone()).map_err(|_| format!("kind must be one of {}", KINDS.join(", ")))
}

fn unknown_keys(args: &Value, allowed: &[&str]) -> Result<(), String> {
    match args.as_object().and_then(|o| o.keys().find(|k| !allowed.contains(&k.as_str()))) {
        Some(k) => Err(format!("unknown argument {k:?}")),
        None => Ok(()),
    }
}

/// FR-QA-004/005/016: checks `args`, then runs the tool. `Err` is a refusal the model hears once.
pub fn run(env: &ToolEnv<'_>, tool: &str, args: &Value) -> Result<ToolRecord, String> {
    let done = |result: Value, undo: Option<Undo>| ToolRecord { tool: tool.to_owned(), args: args.clone(), ok: true, result, error: None, undo };
    let today = env.now.date_naive();
    match tool {
        "search_notes" => {
            unknown_keys(args, &["query"])?;
            let query = required(args, "query", 100)?;
            let mut hits: Vec<Value> = crate::index::search(env.cache, &query, None, None, 5)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|h| json!({ "note": h.note_path, "title": h.title, "line": h.line_text.unwrap_or(h.snippet) }))
                .collect();
            if hits.is_empty() {
                hits = crate::index::retrieve(env.cache, &query, 5)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|n| json!({ "note": n.note_path, "title": n.title, "line": n.body.chars().take(160).collect::<String>() }))
                    .collect();
            }
            Ok(done(json!({ "notes": hits }), None))
        }
        "query_tasks" => {
            unknown_keys(args, &["list", "text"])?;
            let list = args["list"].as_str().ok_or("list is required")?;
            let lists: &[TaskList] = match list {
                "today" => &[TaskList::Today],
                "upcoming" => &[TaskList::Upcoming],
                "completed" => &[TaskList::Completed],
                "all" => &[TaskList::Today, TaskList::Upcoming],
                other => return Err(format!("list {other:?} must be today, upcoming, completed or all")),
            };
            let filter = text(args, "text", 100)?;
            let mut found = Vec::new();
            for l in lists {
                for t in tasks::list_tasks(env.pla, *l, today).map_err(|e| e.to_string())? {
                    if filter.as_ref().is_none_or(|f| title_matches(&t.title, f)) {
                        found.push(json!({ "task_id": t.task_id, "title": t.title, "date": t.date, "time": t.time, "status": t.status, "reminder": t.notify_at.is_some() }));
                    }
                }
            }
            found.truncate(30);
            Ok(done(json!({ "tasks": found, "count": found.len() }), None))
        }
        "query_metrics" => {
            unknown_keys(args, &["kind", "days"])?;
            let kind = kind(args)?;
            let days = args["days"].as_i64().filter(|d| (1..=365).contains(d)).ok_or("days must be a whole number from 1 to 365")?;
            let s = metrics::summary(env.pla, kind, days, today).map_err(|e| e.to_string())?;
            let total: f64 = s.days.iter().map(|d| d.value).sum();
            let unit = match kind {
                MetricKind::Sleep => "h",
                MetricKind::Water => "ml",
                MetricKind::Steps => "count",
                MetricKind::Weight | MetricKind::Workout => "kg",
            };
            let values: Vec<Value> = s.days.iter().map(|d| json!({ "date": d.date, "value": d.value })).collect();
            Ok(done(
                json!({ "kind": args["kind"], "unit": unit, "from": (today - Duration::days(days - 1)).to_string(), "to": today.to_string(),
                        "days_with_values": values.len(), "total": total, "average": s.average, "min": s.min, "max": s.max, "values": values }),
                None,
            ))
        }
        "add_task" => {
            unknown_keys(args, &["title", "date", "time", "remind"])?;
            let title = required(args, "title", 200)?;
            let date = date(args, "date")?;
            let time = text(args, "time", 5)?;
            if let Some(t) = &time {
                NaiveTime::parse_from_str(t, "%H:%M").map_err(|_| format!("time {t:?} is not HH:MM"))?;
                if date.is_none() {
                    return Err("a time needs a date".into());
                }
            }
            // a task at a set time is a reminder unless the model says otherwise ("randevum var")
            let remind = match &args["remind"] {
                Value::Null => time.is_some(),
                Value::Bool(b) => *b,
                _ => return Err("remind must be true or false".into()),
            };
            if remind && time.is_none() {
                return Err("a reminder needs a date and a time".into());
            }
            let input = TaskInput { title: title.clone(), details: None, date: date.clone(), time: time.clone(), remind: Some(remind) };
            let id = tasks::add_task_as(env.pla, &input, "assistant", env.now).map_err(|e| e.to_string())?;
            Ok(done(json!({ "task_id": id, "title": title, "date": date, "time": time, "remind": remind }), Some(Undo { kind: "task".into(), id })))
        }
        "complete_task" => {
            unknown_keys(args, &["task_id"])?;
            let id = required(args, "task_id", 64)?;
            let found: Option<(String, String)> = env
                .pla
                .query_row("SELECT title, status FROM task WHERE task_id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?)))
                .ok();
            let Some((title, status)) = found else { return Err(format!("there is no task {id:?}; take the task_id from query_tasks")) };
            if status != "open" {
                return Err(format!("task {title:?} is not open"));
            }
            tasks::set_done(env.pla, &id, true, env.now).map_err(|e| e.to_string())?;
            Ok(done(json!({ "task_id": id, "title": title }), Some(Undo { kind: "task_done".into(), id })))
        }
        "log_metric" => {
            unknown_keys(args, &["kind", "date", "value", "unit", "exercise"])?;
            let kind = kind(args)?;
            let date = date(args, "date")?.ok_or("date is required")?;
            let value = args["value"].as_f64().ok_or("value must be a number")?;
            let unit: Option<Unit> = match &args["unit"] {
                Value::Null => None,
                u => Some(serde_json::from_value(u.clone()).map_err(|_| format!("unit must be one of {}", UNITS.join(", ")))?),
            };
            let exercise = text(args, "exercise", 100)?;
            let input = MetricInput { kind, date: date.clone(), value: Some(value), unit, exercise, sets: None, reps: None, confirmed: false };
            let id = metrics::log_metric_as(env.pla, &input, &env.validation, "assistant", env.now).map_err(|e| e.to_string())?;
            Ok(done(json!({ "metric_id": id, "kind": args["kind"], "date": date, "value": value, "unit": args["unit"] }), Some(Undo { kind: "metric".into(), id })))
        }
        "create_note" => {
            unknown_keys(args, &["title", "body"])?;
            let body = text(args, "body", 600)?.unwrap_or_default();
            let title = note_title(&text(args, "title", 120)?.unwrap_or_default(), &body, today);
            let rel = write_note(env.vault, &title, &body)?;
            Ok(done(json!({ "note": rel, "title": title }), Some(Undo { kind: "note".into(), id: rel })))
        }
        "suggest_note" => {
            // nothing is written: the panel offers a Save button (the owner's decision a)
            unknown_keys(args, &["title", "body"])?;
            let body = required(args, "body", 600)?;
            let title = note_title(&text(args, "title", 120)?.unwrap_or_default(), &body, today);
            Ok(done(json!({ "suggested": true, "title": title, "body": body }), None))
        }
        other => Err(format!("unknown tool {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_parts() -> (tempfile::TempDir, Vault, Connection, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = crate::vault::open_vault(&tmp.path().join("kasa")).unwrap_or_else(|_| {
            std::fs::create_dir_all(tmp.path().join("kasa")).unwrap();
            crate::vault::open_vault(&tmp.path().join("kasa")).unwrap()
        });
        let dbs = crate::db::open_databases(&tmp.path().join("data")).unwrap();
        (tmp, vault, dbs.pla, dbs.cache)
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-10-06T09:00:00+03:00").unwrap()
    }

    #[test]
    fn decisions_are_read_strictly() {
        assert_eq!(parse_decision("{\"tool\":\"answer\"}"), Ok(Decision::Answer));
        assert_eq!(
            parse_decision("{\"tool\":\"add_task\",\"args\":{\"title\":\"Doktor\"}}"),
            Ok(Decision::Call { tool: "add_task".into(), args: json!({ "title": "Doktor" }) })
        );
        assert!(parse_decision("{\"tool\":\"delete_task\",\"args\":{}}").is_err(), "no deleting tool (FR-QA-006)");
        assert!(parse_decision("evet").is_err());
        let schema = decision_schema();
        assert_eq!(schema["anyOf"].as_array().unwrap().len(), 9);
    }

    #[test]
    fn a_reminder_is_added_as_the_assistants_and_can_be_undone() {
        // FR-QA-008, the owner's example: "Yarın saat 10'da doktor randevum var"
        let (_t, vault, pla, cache) = env_parts();
        let env = ToolEnv { vault: &vault, pla: &pla, cache: &cache, now: now(), validation: ValidationSettings::default() };
        let rec = run(&env, "add_task", &json!({ "title": "Doktor randevusu", "date": "2026-10-07", "time": "10:00", "remind": true })).unwrap();
        let id = rec.result["task_id"].as_str().unwrap().to_owned();
        let (origin, notify): (String, Option<String>) =
            pla.query_row("SELECT origin, notify_at FROM task WHERE task_id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((origin.as_str(), notify.is_some()), ("assistant", true));
        super::super::undo(&pla, rec.undo.as_ref().unwrap(), now()).unwrap();
        let left: i64 = pla.query_row("SELECT count(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
        // a task at a set time is a reminder when the model leaves `remind` out (seen in the real window)
        let rec = run(&env, "add_task", &json!({ "title": "Doktor", "date": "2026-10-07", "time": "10:00" })).unwrap();
        assert_eq!(rec.result["remind"], true);
        let rec = run(&env, "add_task", &json!({ "title": "Toplantı", "date": "2026-10-07", "time": "11:00", "remind": false })).unwrap();
        assert_eq!(rec.result["remind"], false);
    }

    #[test]
    fn bad_arguments_are_refused_before_anything_is_written() {
        // FR-QA-005/016
        let (_t, vault, pla, cache) = env_parts();
        let env = ToolEnv { vault: &vault, pla: &pla, cache: &cache, now: now(), validation: ValidationSettings::default() };
        assert!(run(&env, "add_task", &json!({ "title": "x", "date": "yarın" })).unwrap_err().contains("not a date"));
        assert!(run(&env, "add_task", &json!({ "title": "x", "time": "10:00" })).unwrap_err().contains("needs a date"));
        assert!(run(&env, "add_task", &json!({ "title": "x", "remind": true })).unwrap_err().contains("reminder"));
        assert!(run(&env, "add_task", &json!({ "title": "x", "owner": "y" })).unwrap_err().contains("unknown argument"));
        assert!(run(&env, "complete_task", &json!({ "task_id": "yok" })).unwrap_err().contains("query_tasks"));
        assert!(run(&env, "log_metric", &json!({ "kind": "sleep", "date": "2026-10-06", "value": 70 })).is_err(), "out of range needs the user");
        let count: i64 = pla.query_row("SELECT count(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn numbers_come_from_the_records() {
        // FR-QA-007
        let (_t, vault, pla, cache) = env_parts();
        let env = ToolEnv { vault: &vault, pla: &pla, cache: &cache, now: now(), validation: ValidationSettings::default() };
        run(&env, "log_metric", &json!({ "kind": "sleep", "date": "2026-10-05", "value": 7 })).unwrap();
        let rec = run(&env, "log_metric", &json!({ "kind": "sleep", "date": "2026-10-04", "value": 6.5 })).unwrap();
        let q = run(&env, "query_metrics", &json!({ "kind": "sleep", "days": 7 })).unwrap();
        assert_eq!(q.result["total"], json!(13.5));
        assert_eq!(q.result["days_with_values"], json!(2));
        super::super::undo(&pla, rec.undo.as_ref().unwrap(), now()).unwrap();
        let q = run(&env, "query_metrics", &json!({ "kind": "sleep", "days": 7 })).unwrap();
        assert_eq!(q.result["total"], json!(7.0));
    }

    #[test]
    fn tasks_are_found_and_completed_one_at_a_time() {
        let (_t, vault, pla, cache) = env_parts();
        let env = ToolEnv { vault: &vault, pla: &pla, cache: &cache, now: now(), validation: ValidationSettings::default() };
        run(&env, "add_task", &json!({ "title": "Fatura öde", "date": "2026-10-06" })).unwrap();
        run(&env, "add_task", &json!({ "title": "Annemi ara", "date": "2026-10-08" })).unwrap();
        let q = run(&env, "query_tasks", &json!({ "list": "all", "text": "FATURA" })).unwrap();
        assert_eq!(q.result["count"], json!(1));
        // nl-quality: the model searched with the user's own words and found nothing
        let said = run(&env, "query_tasks", &json!({ "list": "all", "text": "faturayı ödedim" })).unwrap();
        assert_eq!(said.result["count"], json!(1));
        let said = run(&env, "query_tasks", &json!({ "list": "all", "text": "annemi aradım" })).unwrap();
        assert_eq!(said.result["tasks"][0]["title"], "Annemi ara");
        let said = run(&env, "query_tasks", &json!({ "list": "all", "text": "su iç" })).unwrap();
        assert_eq!(said.result["count"], json!(0), "short words alone match nothing");
        let said = run(&env, "query_tasks", &json!({ "list": "all", "text": "annemin hediyesi" })).unwrap();
        assert_eq!(said.result["count"], json!(0), "one word of two is not the task");
        let id = q.result["tasks"][0]["task_id"].as_str().unwrap().to_owned();
        let done = run(&env, "complete_task", &json!({ "task_id": id })).unwrap();
        assert!(run(&env, "complete_task", &json!({ "task_id": id })).unwrap_err().contains("not open"));
        super::super::undo(&pla, done.undo.as_ref().unwrap(), now()).unwrap();
        let status: String = pla.query_row("SELECT status FROM task WHERE task_id = ?1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(status, "open");
    }

    #[test]
    fn notes_written_by_the_assistant_are_marked() {
        // FR-QA-009
        let (_t, vault, pla, cache) = env_parts();
        let env = ToolEnv { vault: &vault, pla: &pla, cache: &cache, now: now(), validation: ValidationSettings::default() };
        let rec = run(&env, "create_note", &json!({ "title": "Toplantı özeti", "body": "Karar: cuma teslim." })).unwrap();
        let rel = rec.result["note"].as_str().unwrap();
        assert!(rel.starts_with("notes/"), "the owner's decision: into the notes");
        let text = std::fs::read_to_string(vault.root.join(rel)).unwrap();
        assert!(text.starts_with("---\npla_generated: true\n---\n# Toplantı özeti"));
        assert_eq!(rec.undo.unwrap().kind, "note");
    }

    #[test]
    fn a_note_gets_a_usable_title_without_asking() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        assert_eq!(note_title("İşletim Sistemleri: Chapter 1", "", today), "İşletim Sistemleri Chapter 1");
        assert_eq!(note_title("  ", "bugün derste işletim sistemleri chapter 1 öğrendik ve çok sevdik", today), "bugün derste işletim sistemleri chapter 1");
        assert_eq!(note_title("CON", "", today), "Not 2026-10-09", "a reserved name");
        assert_eq!(note_title("a/b?.", "", today), "a b");
    }

    #[test]
    fn a_suggestion_writes_nothing() {
        let (_t, vault, pla, cache) = env_parts();
        let env = ToolEnv { vault: &vault, pla: &pla, cache: &cache, now: now(), validation: ValidationSettings::default() };
        let rec = run(&env, "suggest_note", &json!({ "title": "Ders", "body": "Chapter 1" })).unwrap();
        assert_eq!((rec.result["suggested"].clone(), rec.undo.is_none()), (json!(true), true));
        assert!(!vault.root.join("notes").exists() || std::fs::read_dir(vault.root.join("notes")).unwrap().next().is_none());
    }
}
