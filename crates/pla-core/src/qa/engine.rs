//! One question, start to end (FR-QA-003…016): build the context, let the model call up to three
//! tools through a grammar, then stream its answer in the question's language with its sources.

use std::sync::atomic::AtomicBool;

use chrono::{DateTime, FixedOffset};
use serde_json::{json, Value};

use super::context::{self, Context, ANSWER_TOKENS};
use super::tools::{self, Decision, ToolEnv};
use super::{ChatModel, ToolRecord, Turn};
use crate::llm::LlmError;

/// Tool calls one question may make.
pub const MAX_CALLS: usize = 3;

/// What the panel hears while a question is answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// The model is choosing (`None`) or running a tool.
    Working(Option<String>),
    Tool(Box<ToolRecord>),
    Token(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub text: String,
    pub tools: Vec<ToolRecord>,
    /// Notes the answer rests on (FR-QA-010).
    pub sources: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum QaError {
    #[error("stopped")]
    Stopped { partial: String, tools: Vec<ToolRecord> },
    #[error(transparent)]
    Model(LlmError),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

fn weekday(now: DateTime<FixedOffset>) -> &'static str {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][now.date_naive().format("%u").to_string().parse::<usize>().unwrap_or(1) - 1]
}

/// The instructions (in English: the model follows them best; it answers in the user's language).
fn system(ctx: &Context, now: DateTime<FixedOffset>) -> String {
    let mut s = format!(
        "You are PLA, a private assistant that runs only on the user's own computer.\n\
         Now: {} ({}), {}.\n\n\
         Rules:\n\
         1. Answer in the same language as the user's question (FR-QA-012).\n\
         2. Use only the notes, tool results and earlier turns below. If they do not hold the answer, say plainly that you could not find it in the notes. Never guess or use general knowledge about the user.\n\
         3. Numbers about tasks or measurements must come from query_tasks or query_metrics results.\n\
         4. When you use a note, cite it as [[Note title]].\n\
         5. Be brief and concrete.\n\n\
         Tools you may call:\n{}\n",
        now.format("%Y-%m-%d"),
        weekday(now),
        now.format("%H:%M"),
        tools::descriptions()
    );
    if !ctx.recent.is_empty() {
        s.push_str("\n## Notes from the last 48 hours\n");
        s.push_str(&ctx.recent);
    }
    if !ctx.found.is_empty() {
        s.push_str("\n## Notes found for this question\n");
        s.push_str(&ctx.found);
    }
    if ctx.recent.is_empty() && ctx.found.is_empty() {
        s.push_str("\n(No notes matched this question.)\n");
    }
    s
}

fn messages(ctx: &Context, question: &str, calls: &[ToolRecord], now: DateTime<FixedOffset>, last: &str) -> Value {
    let mut m = vec![json!({ "role": "system", "content": system(ctx, now) })];
    for t in &ctx.turns {
        m.push(json!({ "role": "user", "content": t.question }));
        m.push(json!({ "role": "assistant", "content": t.answer }));
    }
    m.push(json!({ "role": "user", "content": question }));
    for c in calls {
        m.push(json!({ "role": "assistant", "content": json!({ "tool": c.tool, "args": c.args }).to_string() }));
        let result = match &c.error {
            Some(e) => format!("Tool error: {e}. Fix the arguments or answer without the tool."),
            None => format!("Tool result: {}", c.result),
        };
        m.push(json!({ "role": "user", "content": result }));
    }
    if !last.is_empty() {
        m.push(json!({ "role": "user", "content": last }));
    }
    // Two user turns in a row would break the chat template: join them.
    let mut joined: Vec<Value> = Vec::new();
    for msg in m {
        match joined.last_mut() {
            Some(prev) if prev["role"] == msg["role"] && msg["role"] == "user" => {
                let text = format!("{}\n\n{}", prev["content"].as_str().unwrap_or_default(), msg["content"].as_str().unwrap_or_default());
                prev["content"] = Value::String(text);
            }
            _ => joined.push(msg),
        }
    }
    Value::Array(joined)
}

const DECIDE: &str = "Decide the next step. Reply with JSON only: {\"tool\":\"answer\"} when you can answer now (or when no tool fits), or {\"tool\":\"<name>\",\"args\":{...}} to call one tool.";
const WRITE: &str = "Now write your answer to the question for the user.";

fn decide_body(ctx: &Context, question: &str, calls: &[ToolRecord], now: DateTime<FixedOffset>) -> Value {
    json!({
        "messages": messages(ctx, question, calls, now, DECIDE),
        "temperature": 0,
        "max_tokens": 300,
        "cache_prompt": true,
        "response_format": { "type": "json_schema", "json_schema": { "name": "decision", "schema": tools::decision_schema() } },
        "chat_template_kwargs": { "enable_thinking": false }
    })
}

fn answer_body(ctx: &Context, question: &str, calls: &[ToolRecord], now: DateTime<FixedOffset>) -> Value {
    json!({
        "messages": messages(ctx, question, calls, now, if calls.is_empty() { "" } else { WRITE }),
        "temperature": 0.3,
        "max_tokens": ANSWER_TOKENS,
        "cache_prompt": true,
        "chat_template_kwargs": { "enable_thinking": false }
    })
}

/// A tool result for the model, cut to what is left of the tool share.
fn trimmed(rec: &ToolRecord, left: &mut usize) -> ToolRecord {
    let mut rec = rec.clone();
    let text = rec.result.to_string();
    let cost = context::tokens(&text);
    if cost > *left {
        let keep: String = text.chars().take(*left * 7 / 2).collect();
        rec.result = Value::String(format!("{keep}… (cut: too long)"));
        *left = 0;
    } else {
        *left -= cost;
    }
    rec
}

/// `[[…]]` targets in `text`, in order, once each.
pub fn cited(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        let Some(close) = rest[open + 2..].find("]]") else { break };
        let target = rest[open + 2..open + 2 + close].split(['|', '#']).next().unwrap_or_default().trim().to_owned();
        if !target.is_empty() && !out.contains(&target) {
            out.push(target);
        }
        rest = &rest[open + 2 + close + 2..];
    }
    out
}

/// FR-QA-003…016 for one question. `turns` are the follow-ups (oldest first).
pub fn answer(
    model: &mut dyn ChatModel,
    env: &ToolEnv<'_>,
    question: &str,
    turns: &[Turn],
    cancel: &AtomicBool,
    on: &mut dyn FnMut(Progress),
) -> Result<Answer, QaError> {
    let ctx = context::build(env.cache, question, turns, env.now)?;
    let mut calls: Vec<ToolRecord> = Vec::new(); // as the model sees them
    let mut shown: Vec<ToolRecord> = Vec::new(); // as the user sees them
    let mut tool_left = ctx.tool_budget;
    let mut refused_once = false;
    let stopped = |shown: &[ToolRecord], partial: String| QaError::Stopped { partial, tools: shown.to_vec() };
    while calls.iter().filter(|c| c.error.is_none()).count() < MAX_CALLS {
        on(Progress::Working(None));
        let raw = match model.complete(&decide_body(&ctx, question, &calls, env.now), cancel) {
            Err(LlmError::Cancelled) => return Err(stopped(&shown, String::new())),
            other => other.map_err(QaError::Model)?,
        };
        let (tool, args) = match tools::parse_decision(&raw) {
            Ok(Decision::Answer) => break,
            Ok(Decision::Call { tool, args }) => (tool, args),
            Err(_) => break, // the grammar makes this rare; answer without tools
        };
        on(Progress::Working(Some(tool.clone())));
        match tools::run(env, &tool, &args) {
            Ok(rec) => {
                calls.push(trimmed(&rec, &mut tool_left));
                on(Progress::Tool(Box::new(rec.clone())));
                shown.push(rec);
            }
            Err(error) => {
                // FR-QA-016: the model hears the error once; a second refusal goes to the user.
                let rec = ToolRecord { tool, args, ok: false, result: Value::Null, error: Some(error), undo: None };
                calls.push(rec.clone());
                if refused_once {
                    on(Progress::Tool(Box::new(rec.clone())));
                    shown.push(rec);
                    break;
                }
                refused_once = true;
            }
        }
        if tool_left == 0 {
            break;
        }
    }
    let mut text = String::new();
    let streamed = model.stream(&answer_body(&ctx, question, &calls, env.now), cancel, &mut |t: &str| {
        text.push_str(t);
        on(Progress::Token(t.to_owned()));
    });
    match streamed {
        Err(LlmError::Cancelled) => return Err(stopped(&shown, text)),
        Err(e) => return Err(QaError::Model(e)),
        Ok(whole) if text.is_empty() => text = whole,
        Ok(_) => {}
    }
    // FR-QA-010: the notes it cites; when it cites none, the notes its context came from.
    let mut sources: Vec<String> = Vec::new();
    for target in cited(&text) {
        if let Ok(Some(path)) = crate::index::resolve_link(env.cache, &target) {
            if !sources.contains(&path) {
                sources.push(path);
            }
        }
    }
    if sources.is_empty() {
        let from_search = shown.iter().filter(|c| c.tool == "search_notes").flat_map(|c| c.result["notes"].as_array().cloned().unwrap_or_default());
        sources = from_search.filter_map(|n| n["note"].as_str().map(str::to_owned)).chain(ctx.sources.iter().cloned()).take(3).collect();
        sources.dedup();
    }
    Ok(Answer { text: text.trim().to_owned(), tools: shown, sources })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction::ValidationSettings;
    use std::collections::VecDeque;
    use std::sync::atomic::Ordering;

    /// Answers from a script: decisions first, then the streamed answer.
    struct Scripted {
        decisions: VecDeque<String>,
        answer: String,
        bodies: Vec<Value>,
    }

    impl ChatModel for Scripted {
        fn complete(&mut self, body: &Value, cancel: &AtomicBool) -> Result<String, LlmError> {
            if cancel.load(Ordering::SeqCst) {
                return Err(LlmError::Cancelled);
            }
            self.bodies.push(body.clone());
            Ok(self.decisions.pop_front().unwrap_or_else(|| "{\"tool\":\"answer\"}".into()))
        }
        fn stream(&mut self, body: &Value, cancel: &AtomicBool, on_token: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
            self.bodies.push(body.clone());
            for word in self.answer.split_inclusive(' ') {
                if cancel.load(Ordering::SeqCst) {
                    return Err(LlmError::Cancelled);
                }
                on_token(word);
            }
            Ok(self.answer.clone())
        }
    }

    fn scripted(decisions: &[&str], answer: &str) -> Scripted {
        Scripted { decisions: decisions.iter().map(|s| s.to_string()).collect(), answer: answer.into(), bodies: Vec::new() }
    }

    struct Env {
        _tmp: tempfile::TempDir,
        vault: crate::vault::Vault,
        pla: rusqlite::Connection,
        cache: rusqlite::Connection,
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("kasa")).unwrap();
        let vault = crate::vault::open_vault(&tmp.path().join("kasa")).unwrap();
        let dbs = crate::db::open_databases(&tmp.path().join("data")).unwrap();
        Env { _tmp: tmp, vault, pla: dbs.pla, cache: dbs.cache }
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-10-06T09:00:00+03:00").unwrap()
    }

    #[test]
    fn a_reminder_from_a_sentence() {
        // The owner's example: the model adds the reminder, then says so in the question's language.
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let mut model = scripted(
            &["{\"tool\":\"add_task\",\"args\":{\"title\":\"Doktor randevusu\",\"date\":\"2026-10-07\",\"time\":\"10:00\",\"remind\":true}}"],
            "Yarın 10:00 için hatırlatıcı ekledim.",
        );
        let mut events = Vec::new();
        let a = answer(&mut model, &tool_env, "Yarın saat 10'da doktor randevum var, hatırlat", &[], &AtomicBool::new(false), &mut |p| events.push(p)).unwrap();
        assert_eq!(a.tools.len(), 1);
        assert_eq!(a.tools[0].undo.as_ref().unwrap().kind, "task");
        assert_eq!(a.text, "Yarın 10:00 için hatırlatıcı ekledim.");
        assert!(events.iter().any(|p| matches!(p, Progress::Tool(r) if r.tool == "add_task")));
        assert!(events.iter().filter(|p| matches!(p, Progress::Token(_))).count() > 1, "streamed");
        // the decision is bound by the grammar; the date to resolve "yarın" is in the instructions
        let decide = &model.bodies[0];
        assert!(decide["response_format"]["json_schema"]["schema"]["anyOf"].is_array());
        assert!(decide["messages"][0]["content"].as_str().unwrap().contains("2026-10-06 (Tuesday)"));
    }

    #[test]
    fn a_bad_call_is_returned_once_then_shown_to_the_user() {
        // FR-QA-016
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let bad = "{\"tool\":\"add_task\",\"args\":{\"title\":\"x\",\"date\":\"yarın\"}}";
        let mut model = scripted(&[bad, bad], "Ekleyemedim.");
        let a = answer(&mut model, &tool_env, "x ekle", &[], &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.tools.len(), 1, "only the second refusal reaches the user");
        assert!(a.tools[0].error.as_deref().unwrap().contains("not a date"));
        let second = model.bodies[1]["messages"].as_array().unwrap();
        assert!(second.last().unwrap()["content"].as_str().unwrap().contains("Tool error"), "the model heard the first one");
    }

    #[test]
    fn at_most_three_tool_calls() {
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let q = "{\"tool\":\"query_tasks\",\"args\":{\"list\":\"all\"}}";
        let mut model = scripted(&[q, q, q, q, q], "Tamam.");
        let a = answer(&mut model, &tool_env, "görevlerim", &[], &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.tools.len(), MAX_CALLS);
    }

    #[test]
    fn stop_keeps_what_was_said_and_done() {
        // FR-QA-013
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let cancel = AtomicBool::new(false);
        let mut model = scripted(&[], "bir iki üç dört beş");
        let mut seen = 0;
        let err = answer(&mut model, &tool_env, "say", &[], &cancel, &mut |p| {
            if matches!(p, Progress::Token(_)) {
                seen += 1;
                if seen == 2 {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        })
        .unwrap_err();
        let QaError::Stopped { partial, .. } = err else { panic!("expected Stopped") };
        assert_eq!(partial, "bir iki ");
    }

    #[test]
    fn answers_name_their_sources() {
        // FR-QA-010
        let e = env();
        crate::index::index_note(&e.cache, "notes/PLA.md", "# PLA\n\nKarar: Tauri kullanılacak.", 0, 1).unwrap();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let mut model = scripted(&[], "Tauri seçildi ([[PLA]]).");
        let a = answer(&mut model, &tool_env, "PLA kararları neler", &[], &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.sources, ["notes/PLA.md"]);
        let mut model = scripted(&[], "Tauri seçildi.");
        let a = answer(&mut model, &tool_env, "PLA kararları neler", &[], &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.sources, ["notes/PLA.md"], "uncited: the notes its context came from");
        assert_eq!(cited("[[A|x]] ve [[B#h]] ve [[A]]"), ["A", "B"]);
    }

    #[test]
    fn follow_ups_go_back_to_the_model() {
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let earlier = Turn {
            turn_id: "1".into(),
            question: "Yarın ne işim var?".into(),
            answer: "Dişçi.".into(),
            tools: Vec::new(),
            created_at: String::new(),
            status: "done".into(),
            new_topic: false,
        };
        let mut model = scripted(&[], "Cuma boş.");
        answer(&mut model, &tool_env, "Peki ya cuma?", &[earlier], &AtomicBool::new(false), &mut |_| {}).unwrap();
        let msgs = model.bodies[1]["messages"].as_array().unwrap();
        assert_eq!(msgs[1]["content"], "Yarın ne işim var?");
        assert_eq!(msgs[2]["content"], "Dişçi.");
        assert_eq!(msgs.last().unwrap()["content"], "Peki ya cuma?");
    }
}
