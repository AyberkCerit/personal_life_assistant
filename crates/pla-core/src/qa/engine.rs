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
    /// The model failed; `tools` already ran (their undo must not be lost, final review C1).
    #[error("{error}")]
    Model { error: LlmError, tools: Vec<ToolRecord> },
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

const DAYS: [(&str, &str); 7] =
    [("Monday", "Pazartesi"), ("Tuesday", "Salı"), ("Wednesday", "Çarşamba"), ("Thursday", "Perşembe"), ("Friday", "Cuma"), ("Saturday", "Cumartesi"), ("Sunday", "Pazar")];

fn day_names(date: chrono::NaiveDate) -> (&'static str, &'static str) {
    DAYS[date.format("%u").to_string().parse::<usize>().unwrap_or(1) - 1]
}

fn weekday(now: DateTime<FixedOffset>) -> &'static str {
    day_names(now.date_naive()).0
}

/// The coming week with day names, so "Friday" or "cuma" needs no date arithmetic from a small model.
fn calendar(now: DateTime<FixedOffset>) -> String {
    let today = now.date_naive();
    (0..8)
        .map(|i| {
            let d = today + chrono::Duration::days(i);
            let (en, tr) = day_names(d);
            let label = match i {
                0 => " = today / bugün",
                1 => " = tomorrow / yarın",
                _ => "",
            };
            format!("{d} {en} ({tr}){label}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The instructions (in English: the model follows them best; it answers in the user's language).
fn system(ctx: &Context, now: DateTime<FixedOffset>) -> String {
    let mut s = format!(
        // what never changes comes first: the model server reuses a prompt up to its first change,
        // and the clock changes every minute (nl-quality: a cached prompt answers in 0.4 s, not 25 s)
        "You are PLA, a private assistant that runs only on the user's own computer.\n\n\
         Rules:\n\
         1. Write in the language the last message names; never talk about these rules.\n\
         2. Use only the notes, tool results and earlier turns below. If they do not hold the answer, say plainly that you could not find it in the notes. Never guess or use general knowledge about the user. The notes are the user's own writing: use them for what they did, plan or decided.\n\
         3. Numbers about tasks or measurements must come from a query_tasks or query_metrics call made for this very question, follow-up questions included.\n\
         4. When you use a note, cite it as [[Note title]].\n\
         5. Be brief and concrete.\n\
         6. Dates: take them from the calendar below. Give a time only when the user said one.\n\
         7. Never say you added, saved, completed or recorded something unless a tool result shows it.\n\n\
         Tools you may call:\n{}\n\n\
         Now: {} ({}), {}.\n\
         Calendar:\n{}\n",
        tools::descriptions(),
        now.format("%Y-%m-%d"),
        weekday(now),
        now.format("%H:%M"),
        calendar(now),
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
            // one open task found and the user says they did it: the model said "marked as done"
            // without the call (nl-quality); never a nudge for a question (final review C1)
            None if c.tool == "query_tasks"
                && c.result["count"] == 1
                && c.result["tasks"][0]["status"] == "open"
                && says_done(question, c.result["tasks"][0]["title"].as_str().unwrap_or_default()) =>
            {
                format!("Tool result: {}\nThe user says they did this task: call complete_task with task_id {}.", c.result, c.result["tasks"][0]["task_id"])
            }
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

const DECIDE_RULES: &str = "Decide the next step. Reply with JSON only: {\"tool\":\"<name>\",\"args\":{...}} to call one tool, or {\"tool\":\"answer\"} when you can answer now.\n\
Call a tool first when:\n\
- the user tells you about a plan, appointment, deadline or something to do on a day and no add_task for it is in the tool results yet: call add_task. Never ask whether to add it;\n\
- the user says they did one of their tasks (\"faturayı ödedim\", \"I called the bank\"): call query_tasks with a word of it, then complete_task with the task_id it gives;\n\
- the question asks for numbers, counts, totals or dates of tasks or measurements, also in a follow-up, and this question has no query result yet: call query_tasks or query_metrics;\n\
- the user reports a measurement of today or a past day, their body weight included (\"92 kiloyum\", \"I slept 7 hours\", \"walked 5000 steps\"): call log_metric; a wish or goal (\"75 kilo olmak istiyorum\") is not a measurement;\n\
- the user asks to keep something as a note (\"notlarıma ekle\", \"not al:\", \"kaydet\", \"make a note\", \"save this\"): call create_note. Choose the title yourself and never ask for it. When they say \"bunu\", \"onu\" or \"this\", the body is what they told you just before;\n\
- the user tells you something worth keeping (what they did, learned, decided, an idea) without asking for anything: call suggest_note once, then answer.\n\
When the user names the note (\"tarifler diye bir not\", \"a note called X\"), that name is its title.\n\
When you asked the user something and they say \"sen karar ver\" or \"you decide\", decide yourself and call the tool.\n\
What the user did, learned or wrote is in their notes: answer from the notes. query_tasks is for what is still to do.\n\
One message may need several tools: call them one after another, then answer.\n\
Greetings, thanks and small talk: answer, with no tool.\n\
Answer when the tool results already hold what is needed.\n\
Examples (dates from the calendar above):\n";

/// The decision instructions with the examples closest to this message (nl-quality plan § 3).
fn decide_text(question: &str, earlier: Option<&str>, today: chrono::NaiveDate) -> String {
    thread_local! { static BANK: Vec<super::examples::Example> = super::examples::bank(); }
    // the message again at the end: a small model weighs what it read last
    BANK.with(|bank| format!("{DECIDE_RULES}{}\n\nThe user's last message: \"{question}\"\nYour JSON:", super::examples::lines(bank, question, earlier, today)))
}

fn write_instruction(lang: &str, calls: &[ToolRecord]) -> String {
    // the real window: "the note was created" while only the offer and its Save button were there
    // the button's name as the panel shows it in that language
    let button = if lang == "en" { "Save" } else { "Kaydet" };
    let offered = if calls.iter().any(|c| c.tool == "suggest_note" && c.error.is_none()) {
        format!(" You only offered to keep it as a note: nothing is saved until I press the \"{button}\" button, so do not say it was saved or created.")
    } else {
        String::new()
    };
    format!("Now write your answer to my last message.{offered} Write it in {}.", super::lang::name(lang))
}

fn decide_body(ctx: &Context, question: &str, calls: &[ToolRecord], now: DateTime<FixedOffset>) -> Value {
    let earlier = ctx.turns.last().map(|t| t.question.as_str());
    json!({
        "messages": messages(ctx, question, calls, now, &decide_text(question, earlier, now.date_naive())),
        "temperature": 0,
        "max_tokens": 400,
        "cache_prompt": true,
        "response_format": { "type": "json_schema", "json_schema": { "name": "decision", "schema": tools::decision_schema() } },
        "chat_template_kwargs": { "enable_thinking": false }
    })
}

fn answer_body(ctx: &Context, question: &str, calls: &[ToolRecord], now: DateTime<FixedOffset>, lang: &str) -> Value {
    json!({
        "messages": messages(ctx, question, calls, now, &write_instruction(lang, calls)),
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
    let cost = context::tokens(&text) + 30; // with the call and the "Tool result:" wrapper
    if cost > *left {
        let keep: String = text.chars().take(*left * 2).collect();
        rec.result = Value::String(format!("{keep}… (cut: too long)"));
        *left = 0;
    } else {
        *left -= cost;
    }
    rec
}

/// A small model copies the time of an earlier task into a new one ("cuma annemi ara" became a
/// 10:00 reminder in the real window). A time needs a number in the question; without one it goes.
fn without_made_up_time(tool: &str, mut args: Value, question: &str) -> Value {
    if tool == "add_task" && !question.chars().any(|c| c.is_ascii_digit()) {
        if let Some(o) = args.as_object_mut() {
            o.remove("time");
            o.remove("remind");
        }
    }
    afternoon(tool, args, question)
}

/// Words of `text`, folded as the index folds them, without punctuation.
fn words(text: &str) -> Vec<String> {
    crate::index::key(text).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

/// "saat 3'te" is 15:00, as people say it (the eval saw 03:00 for a barber's appointment). Only a
/// bare hour moves: "7:30", "6'da kalk", "sabah 7", "5:30am" and "gece 3" stay as they are; "akşam 7"
/// and "at 7 pm" are the evening either way (final review I1).
fn afternoon(tool: &str, mut args: Value, question: &str) -> Value {
    let Some(time) = args["time"].as_str().filter(|_| tool == "add_task").map(str::to_owned) else { return args };
    let Some((h, m)) = time.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.to_owned()))) else { return args };
    if !(1..=7).contains(&h) {
        return args;
    }
    let lower = crate::index::key(question);
    let w = words(question);
    // minutes written by the user ("7:30", "6.45") mean the time as written
    let with_minutes = lower.as_bytes().windows(3).any(|b| b[0].is_ascii_digit() && (b[1] == b':' || b[1] == b'.') && b[2].is_ascii_digit());
    let says = |list: &[&str]| w.iter().any(|x| list.iter().any(|p| x == p || x.starts_with(p) && p.len() >= 5));
    let am = lower.contains("a.m") || w.iter().any(|x| x == "am" || (x.ends_with("am") && x.trim_end_matches("am").chars().all(|c| c.is_ascii_digit() || c == ':') && x.len() > 2));
    let pm = lower.contains("p.m") || w.iter().any(|x| x == "pm" || (x.ends_with("pm") && x.trim_end_matches("pm").chars().all(|c| c.is_ascii_digit() || c == ':') && x.len() > 2));
    let evening = pm || says(&["akşam", "aksam", "evening", "night", "tonight", "öğleden", "ogleden", "afternoon"]);
    let early = am || says(&["sabah", "morning", "erken", "gece", "kalk", "uyan", "alarm", "wake"]);
    if evening || (!early && !with_minutes) {
        args["time"] = Value::String(format!("{:02}:{m}", h + 12));
    }
    args
}

/// Words that point back at the earlier message ("notlarıma ekle bunu", "save this"). Not "that" or
/// "it": "make a note that …" brings its own content (final review C3).
const POINTERS: [&str; 8] = ["bunu", "onu", "şunu", "bunları", "onları", "buna", "ona", "this"];

fn points_back(text: &str) -> bool {
    words(text).iter().any(|w| POINTERS.contains(&w.as_str()))
}

/// The words of a note request itself; what is left is the user's own content.
const COMMAND_WORDS: [&str; 30] = [
    "notlarıma", "notlarima", "notlara", "notuma", "notlarım", "not", "al", "et", "ekle", "kaydet", "yaz", "oluştur", "olustur", "yeni", "bir", "ve",
    "bunu", "onu", "şunu", "buna", "ona", "save", "note", "notes", "make", "this", "to", "my", "a", "of",
];

/// Whether `text` asks for a note without content of its own ("notlarıma ekle bunu", "save this").
fn command_only(text: &str) -> bool {
    !text.contains(':') && words(text).iter().all(|w| COMMAND_WORDS.contains(&w.as_str()))
}

/// What "bunu" means: the latest earlier message that is not itself a request about notes.
fn referred(turns: &[Turn]) -> Option<&str> {
    turns.iter().rev().map(|t| t.question.as_str()).find(|q| !points_back(q) && !command_only(q) && q.split_whitespace().count() > 3)
}

/// A small model writes the command ("notlarıma ekle bunu") or nothing as the note's body: then, and
/// only then, the earlier message is the body (the owner's screenshot asked for a title three times).
/// A request with content of its own keeps it ("not al: süt ekmek", final review C3).
fn with_referred_body(tool: &str, mut args: Value, question: &str, turns: &[Turn]) -> Value {
    if tool != "create_note" {
        return args;
    }
    if let Some(name) = named_title(question) {
        args["title"] = Value::String(name);
    }
    let body = args["body"].as_str().unwrap_or_default().trim().to_owned();
    let empty = body.is_empty() || command_only(&body) || crate::index::key(&body) == crate::index::key(question);
    let refers = points_back(question) || delegates(question);
    if empty && (refers || command_only(question)) {
        if let Some(earlier) = referred(turns) {
            args["body"] = Value::String(earlier.to_owned());
        }
    }
    args
}

/// "tarifler diye bir not oluştur", "a note called Packing list: …": the title the user gave. "X
/// diye not al" means "note that X" and names nothing (final review M3).
fn named_title(question: &str) -> Option<String> {
    let original: Vec<&str> = question.split_whitespace().collect();
    let keys: Vec<String> = original.iter().map(|w| crate::index::key(w).trim_matches(|c: char| !c.is_alphanumeric()).to_owned()).collect();
    if let Some(i) = keys.iter().position(|w| w == "diye") {
        let rest: Vec<&str> = keys[i + 1..].iter().map(String::as_str).filter(|w| *w != "bir").collect();
        let creates = rest.first().is_some_and(|w| w.starts_with("not")) && rest.get(1).is_some_and(|w| ["oluştur", "olustur", "aç", "ac", "aç,", "yarat"].iter().any(|v| w.starts_with(v)));
        if creates && i > 0 {
            let name = original[i.saturating_sub(3)..i].join(" ");
            let name = name.rsplit([',', ':', '.']).next().unwrap_or(&name).trim().to_owned();
            return (!name.is_empty()).then_some(name);
        }
    }
    // ASCII lower case keeps every byte where it was, so positions carry over to the question
    let ascii = question.to_ascii_lowercase();
    let (at, len) = ascii.find("note called ").map(|i| (i, 12)).or_else(|| ascii.find("note named ").map(|i| (i, 11)))?;
    let name = question[at + len..].split([':', ',', '.']).next().unwrap_or_default().trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// A suggestion is for something the user told, not asked or greeted (the owner's decision a).
fn worth_suggesting(question: &str, done: &[ToolRecord]) -> bool {
    let words = question.split_whitespace().count();
    !question.trim_end().ends_with('?') && words >= 4 && !done.iter().any(|r| r.ok && r.tool != "search_notes")
}

/// Where the message "bunu" points at was kept already: a note offered and saved, or written, and not
/// taken back since. The real window wrote it twice after Save; an undone one may be written again
/// (final review I3).
fn already_kept(question: &str, turns: &[Turn]) -> Option<String> {
    if !(delegates(question) || points_back(question) && command_only(question)) {
        return None;
    }
    let earlier = referred(turns)?;
    let turn = turns.iter().rev().find(|t| t.question == earlier)?;
    turn.tools.iter().filter(|r| r.result["undone"] != true).find_map(|r| match r.tool.as_str() {
        "suggest_note" => r.result["saved"].as_str().map(str::to_owned),
        "create_note" if r.ok => r.result["note"].as_str().map(str::to_owned),
        _ => None,
    })
}

/// The task a query of this question found, by id: the only tasks the assistant may complete. The
/// eval saw the model add a task and then complete it in the same breath.
fn found_task<'a>(done: &'a [ToolRecord], id: &str) -> Option<&'a Value> {
    done.iter()
        .filter(|r| r.ok && r.tool == "query_tasks")
        .flat_map(|r| r.result["tasks"].as_array().into_iter().flatten())
        .find(|t| t["task_id"] == id)
}

/// Question words: a message with one asks, it does not report (final review C1).
const ASKING: [&str; 16] = ["ne", "neler", "kaç", "kac", "hangi", "nasıl", "nasil", "mi", "mı", "mu", "mü", "what", "which", "how", "when", "did"];

/// Common English past forms whose task is written in the present ("paid" → "Pay the rent").
const IRREGULAR: [(&str, &str); 8] = [("paid", "pay"), ("bought", "buy"), ("sent", "send"), ("went", "go"), ("did", "do"), ("made", "make"), ("took", "take"), ("got", "get")];

/// Whether the user says they did `title`: an explicit "tamamla / bitti / done", or a verb in the past
/// whose stem is the task's verb ("ödedim" ~ "Fatura öde", "aradım" ~ "Annemi ara", "called" ~ "Call
/// the bank"). "unuttum" or "aldım" next to an unrelated task does not count (final review C1, I2).
fn says_done(question: &str, title: &str) -> bool {
    !asks(question) && (says_finished(question) || did_its_verb(question, title))
}

/// A question, not a report: a question mark or a question word.
fn asks(question: &str) -> bool {
    question.trim_end().ends_with('?') || words(question).iter().any(|w| ASKING.contains(&w.as_str()))
}

/// "tamamla", "bitti", "done": finished, without saying which task.
fn says_finished(question: &str) -> bool {
    words(question).iter().any(|w| ["tamamla", "tamamlandı", "tamamlandi", "bitti", "bitirdim", "hallettim", "done", "finished", "completed"].contains(&w.as_str()))
}

/// A past verb in the message on the task's own verb: "ödedim" ~ "Fatura öde".
fn did_its_verb(question: &str, title: &str) -> bool {
    let q = words(question);
    let title_words = words(title);
    let verb = |base: &str| title_words.iter().any(|t| t == base || (t.chars().count() >= 3 && base.chars().count() >= 3 && (base.starts_with(t.as_str()) || t.starts_with(base))));
    q.iter().any(|w| {
        if let Some((_, base)) = IRREGULAR.iter().find(|(past, _)| past == w) {
            return verb(base);
        }
        ["dım", "dim", "dum", "düm", "tım", "tim", "tum", "tüm", "ed"].iter().find_map(|e| w.strip_suffix(e)).is_some_and(|stem| stem.chars().count() >= 2 && verb(stem))
    })
}

/// "sen karar ver", "you decide": the user leaves the choice to PLA. "karar verdim" is not it.
fn delegates(question: &str) -> bool {
    let w = words(question);
    let pair = |a: &str, b: &str| w.windows(2).any(|p| p[0] == a && p[1] == b);
    pair("karar", "ver") || pair("sen", "seç") || pair("sen", "sec") || pair("sen", "bil") || pair("fark", "etmez") || w.iter().any(|x| x == "farketmez")
        || pair("you", "decide") || pair("up", "to") && w.iter().any(|x| x == "you") || pair("your", "call")
}

/// The earlier message asked for a note in so many words ("notlarıma ekle", "not al", "save this").
fn asked_for_note(text: &str) -> bool {
    let w = words(text);
    w.iter().any(|x| ["notlarıma", "notlarima", "notlara", "notuma", "kaydet", "note", "save"].contains(&x.as_str()))
        || w.windows(2).any(|p| p[0] == "not" && ["al", "et", "ekle", "oluştur", "olustur", "aç", "ac", "yaz"].contains(&p[1].as_str()))
}

/// What the code does when the model only answers but the user's words leave no doubt (the eval's
/// remaining misses, narrowly): a task the model found alone and the user says they did gets completed
/// (with its Undo); "sen karar ver" right after asking for a note writes that note. Never after
/// another write in this question (final review C2).
fn fallback(question: &str, turns: &[Turn], done: &[ToolRecord]) -> Option<(String, Value)> {
    if done.iter().any(|r| r.ok && WRITES.contains(&r.tool.as_str())) {
        return None;
    }
    if let Some(found) = done.iter().rev().find(|r| r.ok && r.tool == "query_tasks") {
        let tasks = found.result["tasks"].as_array()?;
        // the one open task whose verb the user put in the past, even when the search found others
        // (real window); a bare "tamamla" only when one task is open
        if asks(question) {
            return None;
        }
        let open: Vec<&Value> = tasks.iter().filter(|t| t["status"] == "open").collect();
        let by_verb: Vec<&&Value> = open.iter().filter(|t| did_its_verb(question, t["title"].as_str().unwrap_or_default())).collect();
        let pick = match (by_verb.as_slice(), open.as_slice()) {
            ([one], _) => Some(**one),
            ([], [only]) if says_finished(question) => Some(*only),
            _ => None,
        };
        if let Some(task) = pick {
            return Some(("complete_task".into(), json!({ "task_id": task["task_id"] })));
        }
        return None;
    }
    if delegates(question) && turns.last().is_some_and(|t| asked_for_note(&t.question)) {
        return Some(("create_note".into(), json!({ "title": "", "body": referred(turns)? })));
    }
    None
}

/// The assistant's writes (what the fallback must not add to).
const WRITES: [&str; 4] = ["add_task", "complete_task", "log_metric", "create_note"];

/// Whether this call was made already for this question: the same call, or a task with the same
/// title and date (a write must not happen twice because a small model asked twice).
fn repeats(done: &[ToolRecord], tool: &str, args: &Value) -> bool {
    let same_task = |r: &ToolRecord| {
        tool == "add_task"
            && r.tool == "add_task"
            && r.args["date"] == args["date"]
            && crate::index::key(r.args["title"].as_str().unwrap_or_default()) == crate::index::key(args["title"].as_str().unwrap_or_default())
    };
    done.iter().any(|r| r.ok && ((r.tool == tool && r.args == *args) || same_task(r)))
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
    memory: context::Memory<'_>,
    ui_lang: &str,
    cancel: &AtomicBool,
    on: &mut dyn FnMut(Progress),
) -> Result<Answer, QaError> {
    let ctx = context::build(env.cache, question, turns, env.now, memory, &env.vault.config.folders.reports)?;
    let lang = super::lang::answer_language(question, turns.last().map(|t| t.question.as_str()), ui_lang);
    let mut calls: Vec<ToolRecord> = Vec::new(); // as the model sees them
    let mut shown: Vec<ToolRecord> = Vec::new(); // as the user sees them
    let mut tool_left = ctx.tool_budget;
    let mut refused_once = false;
    let stopped = |shown: &[ToolRecord], partial: String| QaError::Stopped { partial, tools: shown.to_vec() };
    while calls.iter().filter(|c| c.error.is_none()).count() < MAX_CALLS {
        on(Progress::Working(None));
        let raw = match model.complete(&decide_body(&ctx, question, &calls, env.now), cancel) {
            Err(LlmError::Cancelled) => return Err(stopped(&shown, String::new())),
            Err(error) => return Err(QaError::Model { error, tools: shown }),
            Ok(raw) => raw,
        };
        let decided = match tools::parse_decision(&raw) {
            // a note offered for a greeting or a question counts as answering (the owner's decision a)
            Ok(Decision::Call { tool, .. }) if tool == "suggest_note" && !worth_suggesting(question, &shown) => None,
            // asked again what is done already (one task added three times, a search made twice):
            // as good as answering, so the fallback still gets its turn (eval: "elektrikçiyi aradım")
            Ok(Decision::Call { tool, args }) if repeats(&shown, &tool, &args) => None,
            Ok(Decision::Call { tool, args }) => Some((tool, args)),
            Ok(Decision::Answer) | Err(_) => None, // the grammar makes Err rare
        };
        let Some((tool, args)) = decided.or_else(|| fallback(question, turns, &shown)) else { break };
        let args = with_referred_body(&tool, without_made_up_time(&tool, args, question), question, turns);
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(stopped(&shown, String::new())); // no write after Stop
        }
        if repeats(&shown, &tool, &args) {
            break; // what the fallback chose was done already
        }
        on(Progress::Working(Some(tool.clone())));
        let kept = (tool == "create_note").then(|| already_kept(question, turns)).flatten();
        let kept_path = kept.clone();
        let target = found_task(&shown, args["task_id"].as_str().unwrap_or_default());
        let done_by_user = target.is_some_and(|t| says_done(question, t["title"].as_str().unwrap_or_default()));
        let ran = if tool == "complete_task" && !done_by_user {
            Err("complete a task only when the user says they did it; take its task_id from query_tasks".to_owned())
        } else if let Some(path) = kept.clone() {
            Err(format!("that is saved already as the note {path}; tell the user, write nothing"))
        } else {
            tools::run(env, &tool, &args)
        };
        match ran {
            Ok(rec) => {
                calls.push(trimmed(&rec, &mut tool_left));
                on(Progress::Tool(Box::new(rec.clone())));
                shown.push(rec);
            }
            // saved already: the model hears where, the user sees no failed tool (real window)
            Err(error) if kept_path.is_some() => {
                calls.push(ToolRecord { tool, args, ok: false, result: Value::Null, error: Some(error), undo: None });
                break;
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
    let streamed = model.stream(&answer_body(&ctx, question, &calls, env.now, lang), cancel, &mut |t: &str| {
        text.push_str(t);
        on(Progress::Token(t.to_owned()));
    });
    match streamed {
        Err(LlmError::Cancelled) => return Err(stopped(&shown, text)),
        Err(error) => return Err(QaError::Model { error, tools: shown }),
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
    // Only an answer from the notes falls back on them: not one about what a tool wrote or counted.
    let from_notes = shown.iter().all(|c| c.tool == "search_notes");
    if sources.is_empty() && from_notes {
        let from_search = shown.iter().filter(|c| c.tool == "search_notes").flat_map(|c| c.result["notes"].as_array().cloned().unwrap_or_default());
        for path in from_search.filter_map(|n| n["note"].as_str().map(str::to_owned)).chain(ctx.sources.iter().cloned()) {
            if !sources.contains(&path) && sources.len() < 3 {
                sources.push(path);
            }
        }
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
        let a = answer(&mut model, &tool_env, "Yarın saat 10'da doktor randevum var, hatırlat", &[], None, "tr", &AtomicBool::new(false), &mut |p| events.push(p)).unwrap();
        assert_eq!(a.tools.len(), 1);
        assert_eq!(a.tools[0].undo.as_ref().unwrap().kind, "task");
        assert_eq!(a.text, "Yarın 10:00 için hatırlatıcı ekledim.");
        assert!(events.iter().any(|p| matches!(p, Progress::Tool(r) if r.tool == "add_task")));
        assert!(events.iter().filter(|p| matches!(p, Progress::Token(_))).count() > 1, "streamed");
        // the decision is bound by the grammar; the date to resolve "yarın" is in the instructions
        let decide = &model.bodies[0];
        assert!(decide["response_format"]["json_schema"]["schema"]["anyOf"].is_array());
        let system = decide["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("2026-10-06 (Tuesday)"));
        assert!(system.contains("2026-10-07 Wednesday (Çarşamba) = tomorrow / yarın"));
        assert!(system.contains("2026-10-09 Friday (Cuma)"), "\"cuma\" needs no arithmetic");
        assert!(a.sources.is_empty(), "an answer about what a tool did names no notes");
    }

    #[test]
    fn a_time_the_user_did_not_say_is_dropped() {
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let call = "{\"tool\":\"add_task\",\"args\":{\"title\":\"Annemi ara\",\"date\":\"2026-10-09\",\"time\":\"10:00\",\"remind\":true}}";
        let mut model = scripted(&[call], "Tamam.");
        let a = answer(&mut model, &tool_env, "Cuma günü annemi aramam lazım", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!((a.tools[0].result["time"].clone(), a.tools[0].result["remind"].clone()), (Value::Null, Value::Bool(false)));
        let mut model = scripted(&[call], "Tamam.");
        let a = answer(&mut model, &tool_env, "Cuma 10'da annemi ara", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.tools[0].result["time"], "10:00", "said, so kept");
    }

    /// Decides once, then fails while answering (llama-server died).
    struct Breaks;
    impl ChatModel for Breaks {
        fn complete(&mut self, _: &Value, _: &AtomicBool) -> Result<String, LlmError> {
            Ok("{\"tool\":\"add_task\",\"args\":{\"title\":\"Doktor\",\"date\":\"2026-10-07\"}}".into())
        }
        fn stream(&mut self, _: &Value, _: &AtomicBool, _: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
            Err(LlmError::Http("the answer stream broke off".into()))
        }
    }

    #[test]
    fn a_failed_answer_keeps_what_its_tools_did() {
        // final review C1: the task stays, so its undo must too
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let err = answer(&mut Breaks, &tool_env, "Yarın doktor", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        let QaError::Model { tools, .. } = err else { panic!("expected a model error") };
        assert_eq!(tools.len(), 1);
        assert!(tools[0].undo.is_some());
    }

    #[test]
    fn nothing_is_written_after_stop() {
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let cancel = AtomicBool::new(false);
        let mut model = scripted(&["{\"tool\":\"add_task\",\"args\":{\"title\":\"x\",\"date\":\"2026-10-07\"}}"], "");
        let err = answer(&mut model, &tool_env, "x ekle", &[], None, "tr", &cancel, &mut |p| {
            if matches!(p, Progress::Working(None)) {
                cancel.store(true, Ordering::SeqCst); // Stop lands while the model decides
            }
        });
        assert!(matches!(err, Err(QaError::Stopped { .. })));
        let n: i64 = e.pla.query_row("SELECT count(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_task_asked_for_twice_is_added_once() {
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let call = "{\"tool\":\"add_task\",\"args\":{\"title\":\"Market alışverişi\",\"date\":\"2026-10-10\"}}";
        let again = "{\"tool\":\"add_task\",\"args\":{\"title\":\"market ALIŞVERİŞİ\",\"date\":\"2026-10-10\"}}";
        let mut model = scripted(&[call, again, call], "Ekledim.");
        let a = answer(&mut model, &tool_env, "Cumartesi markete gitmem gerekiyor", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.tools.len(), 1);
        let n: i64 = e.pla.query_row("SELECT count(*) FROM task", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_bad_call_is_returned_once_then_shown_to_the_user() {
        // FR-QA-016
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let bad = "{\"tool\":\"add_task\",\"args\":{\"title\":\"x\",\"date\":\"yarın\"}}";
        let mut model = scripted(&[bad, bad], "Ekleyemedim.");
        let a = answer(&mut model, &tool_env, "x ekle", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.tools.len(), 1, "only the second refusal reaches the user");
        assert!(a.tools[0].error.as_deref().unwrap().contains("not a date"));
        let second = model.bodies[1]["messages"].as_array().unwrap();
        assert!(second.last().unwrap()["content"].as_str().unwrap().contains("Tool error"), "the model heard the first one");
    }

    #[test]
    fn at_most_three_tool_calls() {
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let q = |list: &str| format!("{{\"tool\":\"query_tasks\",\"args\":{{\"list\":\"{list}\"}}}}");
        let calls = [q("today"), q("upcoming"), q("completed"), q("all"), q("today")];
        let mut model = scripted(&calls.iter().map(String::as_str).collect::<Vec<_>>(), "Tamam.");
        let a = answer(&mut model, &tool_env, "görevlerim", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
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
        let err = answer(&mut model, &tool_env, "say", &[], None, "tr", &cancel, &mut |p| {
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
        let a = answer(&mut model, &tool_env, "PLA kararları neler", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.sources, ["notes/PLA.md"]);
        let mut model = scripted(&[], "Tauri seçildi.");
        let a = answer(&mut model, &tool_env, "PLA kararları neler", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.sources, ["notes/PLA.md"], "uncited: the notes its context came from");
        assert_eq!(cited("[[A|x]] ve [[B#h]] ve [[A]]"), ["A", "B"]);
    }

    #[test]
    fn the_answer_language_is_named() {
        // FR-QA-012; "selam" was answered in English in the real window
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let mut model = scripted(&[], "Selam!");
        answer(&mut model, &tool_env, "selam", &[], None, "en", &AtomicBool::new(false), &mut |_| {}).unwrap();
        let last = model.bodies[1]["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_owned();
        assert!(last.ends_with("Write it in Turkish."), "{last}");
        let offer = ToolRecord { tool: "suggest_note".into(), args: json!({}), ok: true, result: json!({}), error: None, undo: None };
        assert!(write_instruction("tr", &[offer.clone()]).contains("press the \"Kaydet\" button"));
        assert!(write_instruction("en", &[offer]).contains("press the \"Save\" button"));
        assert!(!write_instruction("tr", &[]).contains("Save"));
    }

    fn turn(q: &str) -> Turn {
        Turn { turn_id: q.into(), question: q.into(), answer: String::new(), tools: Vec::new(), created_at: String::new(), status: "done".into(), new_topic: false }
    }

    #[test]
    fn this_means_what_the_user_said_before() {
        // the owner's screenshot: "notlarıma ekle bunu" after a sentence about the lesson
        let said = "bu gün derste işletim sistemleri öğrendik chapter 1 olarak";
        let turns = [turn(said), turn("notlarıma ekle bunu")];
        let args = with_referred_body("create_note", json!({ "title": "Ders", "body": "notlarıma ekle" }), "sen karar ver", &turns);
        assert_eq!(args["body"], said, "past the request itself, to what it pointed at");
        let kept = with_referred_body("create_note", json!({ "title": "Ders", "body": "İşletim sistemleri chapter 1" }), "bunu not al", &turns[..1]);
        assert_eq!(kept["body"], "İşletim sistemleri chapter 1", "a body about it stays");
        let own = with_referred_body("create_note", json!({ "title": "Market", "body": "süt, ekmek" }), "not al: market listesi süt, ekmek, yumurta ve peynir", &turns[..1]);
        assert_eq!(own["body"], "süt, ekmek", "a long message with its own content is not about the earlier one");
    }

    #[test]
    fn an_hour_people_say_without_the_morning_is_the_afternoon() {
        let t = |time: &str, q: &str| afternoon("add_task", json!({ "time": time }), q)["time"].as_str().unwrap().to_owned();
        assert_eq!(t("03:00", "saat 3'te olsun"), "15:00", "seen in the eval");
        assert_eq!(t("07:00", "akşam 7'de halı saha"), "19:00");
        assert_eq!(t("03:00", "tamam 3'te"), "15:00", "\"tamam\" is not a.m.");
        assert_eq!(t("07:00", "sabah 7'de koşu"), "07:00");
        assert_eq!(t("05:00", "gece 5'te uçak"), "05:00");
        assert_eq!(t("06:00", "at 6am"), "06:00");
        assert_eq!(t("03:00", "03:00'te alarm"), "03:00", "written as early as it is");
        assert_eq!(t("10:00", "10'da toplantı"), "10:00");
        // final review I1: what the user wrote or meant early stays
        assert_eq!(t("07:30", "7:30'da kahvaltı"), "07:30");
        assert_eq!(t("05:30", "5:30am flight"), "05:30");
        assert_eq!(t("06:00", "yarın 6'da kalk"), "06:00");
        assert_eq!(t("07:00", "Friday night at 7"), "19:00");
        assert_eq!(t("07:30", "akşam 7:30'da sinema"), "19:30");
    }

    #[test]
    fn a_task_is_done_only_when_the_user_says_so() {
        // final review C1, I2: "unuttum" or an unrelated "aldım" next to one found task
        assert!(says_done("faturayı ödedim", "Fatura öde"));
        assert!(says_done("annemi aradım, o görevi tamamla", "Annemi ara"));
        assert!(says_done("I paid the rent, mark it done", "Pay the rent"));
        assert!(says_done("I called the bank", "Call the bank"));
        assert!(!says_done("yarın ne işim var, unuttum", "Annemi ara"));
        assert!(!says_done("annemin doğum günü hediyesini aldım", "Annemi ara"));
        assert!(!says_done("faturayı ödedim mi", "Fatura öde"), "a question");
        assert!(!says_done("geçen hafta ne yaptım", "Rapor yap"));
    }

    #[test]
    fn the_task_the_user_did_is_completed_among_others() {
        // the real window: "Annemi aramak" and a second open task; nothing was completed
        let e = env();
        for title in ["Annemi aramak", "Güç"] {
            crate::tasks::add_task_as(&e.pla, &crate::tasks::TaskInput { title: title.into(), details: None, date: Some("2026-10-07".into()), time: None, remind: Some(false) }, "manual", now()).unwrap();
        }
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let mut model = scripted(&["{\"tool\":\"query_tasks\",\"args\":{\"list\":\"all\"}}"], "Tamam.");
        let a = answer(&mut model, &tool_env, "annemi aradım, o görevi tamamla", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        let done = a.tools.iter().find(|t| t.tool == "complete_task" && t.ok).expect("completed");
        assert_eq!(done.result["title"], "Annemi aramak");
    }

    #[test]
    fn a_search_asked_twice_still_completes_the_task() {
        // eval final3: the model repeated query_tasks instead of answering, and nothing was completed
        let e = env();
        crate::tasks::add_task_as(&e.pla, &crate::tasks::TaskInput { title: "Elektrikçiyi ara".into(), details: None, date: Some("2026-10-06".into()), time: None, remind: Some(false) }, "manual", now()).unwrap();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let query = "{\"tool\":\"query_tasks\",\"args\":{\"list\":\"all\",\"text\":\"elektrikçiyi aradım\"}}";
        let mut model = scripted(&[query, query], "Tamamlandı.");
        let a = answer(&mut model, &tool_env, "elektrikçiyi aradım", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(a.tools.iter().any(|t| t.tool == "complete_task" && t.ok), "{:?}", a.tools);
    }

    #[test]
    fn a_question_with_one_found_task_completes_nothing() {
        let e = env();
        crate::tasks::add_task_as(&e.pla, &crate::tasks::TaskInput { title: "Annemi ara".into(), details: None, date: Some("2026-10-07".into()), time: None, remind: Some(false) }, "manual", now()).unwrap();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let query = "{\"tool\":\"query_tasks\",\"args\":{\"list\":\"upcoming\"}}";
        let mut model = scripted(&[query], "Yarın annemi arayacaksın.");
        let a = answer(&mut model, &tool_env, "yarın ne işim var, unuttum", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(!a.tools.iter().any(|t| t.tool == "complete_task"), "{:?}", a.tools);
        let decide = model.bodies[1]["messages"].as_array().unwrap();
        assert!(!decide.iter().any(|m| m["content"].as_str().unwrap_or_default().contains("call complete_task with task_id")), "no nudge either");
    }

    #[test]
    fn you_decide_after_a_task_writes_no_note() {
        // final review C2: "toplantı ekle" → a question → "sen karar ver" → add_task, and no note
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let add = "{\"tool\":\"add_task\",\"args\":{\"title\":\"Toplantı\",\"date\":\"2026-10-07\",\"time\":\"14:00\"}}";
        let mut model = scripted(&[add], "Ekledim.");
        let a = answer(&mut model, &tool_env, "sen karar ver", &[turn("yarın öğleden sonra toplantı ekle")], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(a.tools.iter().map(|t| t.tool.as_str()).collect::<Vec<_>>(), ["add_task"]);
        assert!(!delegates("karar verdim, yarın başlıyorum"));
    }

    #[test]
    fn a_note_request_with_its_own_content_keeps_it() {
        // final review C3
        let turns = [turn("how many steps did I walk this week?")];
        let own = with_referred_body("create_note", json!({ "title": "Dentist", "body": "" }), "make a note that the dentist moved to Monday", &turns);
        assert_eq!(own["body"], "", "\"that\" does not point back; the engine keeps what the model wrote");
        let milk = with_referred_body("create_note", json!({ "title": "Market", "body": "süt ekmek" }), "not al: süt ekmek", &[turn("bugün derste işletim sistemleri öğrendik")]);
        assert_eq!(milk["body"], "süt ekmek");
    }

    #[test]
    fn an_undone_note_may_be_written_again() {
        // final review I3
        let mut told = turn("bu gün derste işletim sistemleri öğrendik chapter 1 olarak");
        told.tools = vec![ToolRecord {
            tool: "suggest_note".into(),
            args: json!({}),
            ok: true,
            result: json!({ "title": "Ders", "body": "x", "saved": "notes/Ders.md", "undone": true }),
            error: None,
            undo: None,
        }];
        assert_eq!(already_kept("notlarıma ekle bunu", &[told.clone()]), None);
        told.tools[0].result.as_object_mut().unwrap().remove("undone");
        assert_eq!(already_kept("notlarıma ekle bunu", &[told.clone()]).as_deref(), Some("notes/Ders.md"));
        assert_eq!(already_kept("not al: wifi 1234", &[told]), None, "its own content");
    }

    #[test]
    fn what_was_saved_already_is_not_written_twice() {
        // the real window: Save on the offer, then "notlarıma ekle bunu" made "… Chapter 1 2.md"
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let mut told = turn("bu gün derste işletim sistemleri öğrendik chapter 1 olarak");
        told.tools = vec![ToolRecord {
            tool: "suggest_note".into(),
            args: json!({}),
            ok: true,
            result: json!({ "suggested": true, "title": "Ders", "body": "x", "saved": "notes/Ders.md" }),
            error: None,
            undo: None,
        }];
        let call = "{\"tool\":\"create_note\",\"args\":{\"title\":\"Ders\",\"body\":\"İşletim sistemleri chapter 1\"}}";
        let mut model = scripted(&[call, call], "Zaten kayıtlı.");
        let a = answer(&mut model, &tool_env, "notlarıma ekle bunu", &[told], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(!a.tools.iter().any(|t| t.tool == "create_note" && t.ok), "{:?}", a.tools);
        assert!(!e.vault.root.join("notes").exists() || std::fs::read_dir(e.vault.root.join("notes")).unwrap().next().is_none());
        let heard = model.bodies[1]["messages"].as_array().unwrap().iter().any(|m| m["content"].as_str().unwrap_or_default().contains("notes/Ders.md"));
        assert!(heard, "the model is told where it is");
    }

    #[test]
    fn a_note_named_by_the_user_keeps_its_name() {
        assert_eq!(named_title("tarifler diye bir not oluştur, mercimek çorbası: 1 su bardağı").as_deref(), Some("tarifler"));
        assert_eq!(named_title("İşletim Sistemleri diye not aç").as_deref(), Some("İşletim Sistemleri"));
        assert_eq!(named_title("make a note called Packing list: charger, socks").as_deref(), Some("Packing list"));
        assert_eq!(named_title("not al: süt, ekmek"), None);
    }

    #[test]
    fn only_a_task_found_for_this_question_is_completed() {
        // the eval: the model added "Kargoyu al", then completed it
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let mut model = scripted(&["{\"tool\":\"add_task\",\"args\":{\"title\":\"Kargoyu al\",\"date\":\"2026-10-09\"}}", "{\"tool\":\"complete_task\",\"args\":{\"task_id\":\"x\"}}"], "Tamam.");
        answer(&mut model, &tool_env, "cuma kargoyu al", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        let open: i64 = e.pla.query_row("SELECT count(*) FROM task WHERE status = 'open'", [], |r| r.get(0)).unwrap();
        assert_eq!(open, 1);
    }

    #[test]
    fn a_task_the_user_did_is_completed_when_the_model_only_searched() {
        // the eval: "faturayı ödedim" found "Fatura öde", then the model just answered
        let e = env();
        crate::tasks::add_task_as(&e.pla, &crate::tasks::TaskInput { title: "Fatura öde".into(), details: None, date: Some("2026-10-06".into()), time: None, remind: Some(false) }, "manual", now()).unwrap();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let mut model = scripted(&["{\"tool\":\"query_tasks\",\"args\":{\"list\":\"all\",\"text\":\"faturayı ödedim\"}}"], "Tamam.");
        let a = answer(&mut model, &tool_env, "faturayı ödedim", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(a.tools.iter().any(|t| t.tool == "complete_task" && t.ok && t.undo.is_some()), "{:?}", a.tools);
        let mut model = scripted(&["{\"tool\":\"query_tasks\",\"args\":{\"list\":\"all\",\"text\":\"fatura\"}}"], "Evet.");
        let asked = answer(&mut model, &tool_env, "faturayı ödedim mi?", &[], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(!asked.tools.iter().any(|t| t.tool == "complete_task"), "a question completes nothing");
    }

    #[test]
    fn you_decide_writes_the_note_that_was_asked_for() {
        // the owner's screenshot: "sen karar ver" after "notlarıma ekle bunu"
        let e = env();
        let tool_env = ToolEnv { vault: &e.vault, pla: &e.pla, cache: &e.cache, now: now(), validation: ValidationSettings::default() };
        let said = "bu gün derste işletim sistemleri öğrendik chapter 1 olarak";
        let turns = [turn(said), turn("notlarıma ekle bunu")];
        // the real model offered a note instead, which a short reply is not worth (eval v4)
        let mut model = scripted(&["{\"tool\":\"suggest_note\",\"args\":{\"title\":\"x\",\"body\":\"y\"}}"], "Kaydettim.");
        let a = answer(&mut model, &tool_env, "sen karar ver", &turns, None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        let note = a.tools.iter().find(|t| t.tool == "create_note").expect("a note");
        assert!(std::fs::read_to_string(e.vault.root.join(note.result["note"].as_str().unwrap())).unwrap().contains("işletim sistemleri"));
        let mut model = scripted(&[], "Tamam.");
        let chat = answer(&mut model, &tool_env, "sen karar ver", &[turn("akşam ne yesek bilemedim")], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(chat.tools.is_empty(), "no note was asked for");
    }

    #[test]
    fn a_note_is_suggested_for_what_was_told_only() {
        assert!(worth_suggesting("bu gün derste işletim sistemleri öğrendik", &[]));
        assert!(!worth_suggesting("selam naber", &[]), "a greeting");
        assert!(!worth_suggesting("bugün ne yaptım acaba?", &[]), "a question");
        let task = ToolRecord { tool: "add_task".into(), args: json!({}), ok: true, result: Value::Null, error: None, undo: None };
        assert!(!worth_suggesting("yarın saat 14:00'te dişçi randevum var", &[task]), "already kept as a task");
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
        answer(&mut model, &tool_env, "Peki ya cuma?", &[earlier], None, "tr", &AtomicBool::new(false), &mut |_| {}).unwrap();
        let msgs = model.bodies[1]["messages"].as_array().unwrap();
        assert_eq!(msgs[1]["content"], "Yarın ne işim var?");
        assert_eq!(msgs[2]["content"], "Dişçi.");
        assert!(msgs.last().unwrap()["content"].as_str().unwrap().starts_with("Peki ya cuma?"));
    }
}
