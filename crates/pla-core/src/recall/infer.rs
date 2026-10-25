//! What the notes say about the user's current work (spec 2026-10-18, "Notlardan"): in the
//! maintenance window each note changed since its last read is read once, and its lines in the
//! State file are replaced by what it says now. Only the note's own words are a source, and
//! nothing outside the State file is written.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, FixedOffset};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fs_atomic::write_atomic;
use crate::index;
use crate::lang::Lang;
use crate::llm::LlmError;
use crate::notes::is_excluded;
use crate::qa::lang::{answer_language, is_day_event, is_general, is_hearsay, is_list, is_person_news, is_unsure, name};
use crate::qa::ChatModel;
use crate::util::{content_hash, tokens};
use crate::vault::{Folders, Vault};

use super::apply::{apply, rejected, Change, Proposal, RecallError, SOURCE_CHANGED, SOURCE_DELETED};
use super::extract::{closeness, same_language, REJECTED_SIMILARITY};
use super::format::{parse_doc, render_doc, topic_key, visible_date, Block, Entry, Kind, Role};

/// Notes read in one maintenance window, as many as the day summaries take.
pub const NOTES_PER_WINDOW: usize = 10;
/// A note nobody touched for this long says little about the current state: it is not read.
const RECENT_DAYS: i64 = 30;
/// One note gives at most this many lines, so it cannot fill the State file alone.
const LINES_PER_NOTE: usize = 3;
/// A line longer than this is a paragraph, not a state.
const LINE_CHARS: usize = 160;
/// The note's share of the 8K window, after the instructions and the examples.
const NOTE_TOKENS: usize = 2500;

const SYSTEM: &str = "You keep the user's current state: where their projects, studies and open work stand. You are given one of the user's own notes. List what the note states as the current state of the user's own work: what is chosen, decided, done, under way, waiting or the next step. Write nothing the note only guesses, asks, wishes or considers, no general knowledge, reference material, quotes or how-to steps, nothing about other people's private lives, and nothing that is only a passing event of one day. At most three lines, the most important first. For each line: evidence is the note's exact words; topic is a short kebab-case key for the project or decision, the topic of the note's earlier line or a known topic when the line updates it; text is one short sentence in the language the turn names, without a subject (\"Tezin ikinci bölümünü yazıyor.\", not \"Kullanıcı tezin ikinci bölümünü yazıyor.\"). The note's earlier lines are what it said before: list them again as they are when they still hold, changed when the note changed them, and leave them out when the note no longer says them. Never write a rejected line again. Answer with an empty list when the note says nothing about the state of the user's work.";

/// `(note title, note text, earlier lines as (topic, text), answer)`.
type Example = (&'static str, &'static str, &'static [(&'static str, &'static str)], &'static str);

/// Worked examples shown before the note. They share no case with `tests/nl/memory.jsonl`
/// (nl_eval checks the held-out cases against them).
const EXAMPLES: &[Example] = &[
    (
        "Mutfak tadilatı",
        "Dolap ölçüleri alındı. Ustayla anlaştık, işe 3 Kasım'da başlıyor. Tezgâh için granit mi kuvars mı, henüz bilmiyorum.",
        &[],
        r#"{"memories":[{"evidence":"Ustayla anlaştık, işe 3 Kasım'da başlıyor","topic":"mutfak-tadilati","text":"Mutfak tadilatı için ustayla anlaştı, iş 3 Kasım'da başlıyor."}]}"#,
    ),
    (
        "Ehliyet",
        "Yazılı sınavı geçtim. Direksiyon sınavı 12 Aralık'ta.",
        &[("ehliyet", "Yazılı sınava çalışıyor.")],
        r#"{"memories":[{"evidence":"Yazılı sınavı geçtim","topic":"ehliyet","text":"Ehliyetin yazılı sınavını geçti, direksiyon sınavı 12 Aralık'ta."}]}"#,
    ),
    (
        "Ekşi maya",
        "Un, su ve tuzu karıştır. 12 saat beklet. 230 derecede 40 dakika pişir.",
        &[],
        r#"{"memories":[]}"#,
    ),
    (
        "3 Eylül",
        "Sabah koşuya çıktım, hava serindi. Akşam kardeşimle sinemaya gittik.",
        &[],
        r#"{"memories":[]}"#,
    ),
    (
        "Excel ipuçları",
        "DÜŞEYARA ile başka tablodan değer çekilir. Ctrl+Shift+L filtreyi açar.",
        &[],
        r#"{"memories":[]}"#,
    ),
    (
        "Bike tyres",
        "Inflate road tyres to 6 bar. Check the pressure every week.",
        &[],
        r#"{"memories":[]}"#,
    ),
    (
        "Garden shed",
        "Bought the timber. The roof is next, maybe in spring if the weather allows.",
        &[],
        r#"{"memories":[{"evidence":"Bought the timber","topic":"garden-shed","text":"Bought the timber for the garden shed; the roof is next."}]}"#,
    ),
];

/// The example notes, so an evaluation can keep its held-out cases away from them.
pub fn example_notes() -> Vec<&'static str> {
    EXAMPLES.iter().map(|e| e.1).collect()
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "memories": {"type": "array", "items": {"type": "object", "properties": {
                "evidence": {"type": "string"},
                "topic": {"type": "string"},
                "text": {"type": "string"}
            }, "required": ["evidence", "topic", "text"]}}
        },
        "required": ["memories"]
    })
}

#[derive(Debug, thiserror::Error)]
pub enum InferError {
    #[error(transparent)]
    Model(#[from] LlmError),
    #[error(transparent)]
    Recall(#[from] RecallError),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// The note a line was inferred from (`kaynak` without its `#hash`).
fn source_path(e: &Entry) -> Option<&str> {
    e.source.as_deref().map(|s| s.rsplit_once('#').map_or(s, |(path, _)| path))
}

/// `note_path`'s inferences among `current`.
fn own<'a>(current: &'a [Entry], note_path: &str) -> Vec<&'a Entry> {
    current.iter().filter(|e| e.kind == Kind::Inferred && source_path(e) == Some(note_path)).collect()
}

/// The notes to read: those whose content changed since their last read (and that were changed
/// lately), newest first, after the ones gone from the vault, at most `limit`. Never the memory
/// folder or an excluded note.
pub fn changed_notes(cache: &Connection, db: &Connection, folders: &Folders, now: DateTime<FixedOffset>, limit: usize) -> Vec<String> {
    let read: HashMap<String, String> = match db.prepare("SELECT note_path, content_hash FROM memory_source") {
        Ok(mut st) => st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default(),
        Err(_) => return Vec::new(),
    };
    let indexed: Vec<(String, String, i64)> = match cache.prepare("SELECT note_path, content_hash, mtime FROM note_index ORDER BY mtime DESC, note_path") {
        Ok(mut st) => st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map(|rows| rows.flatten().collect()).unwrap_or_default(),
        Err(_) => return Vec::new(),
    };
    let indexed: Vec<_> = indexed.into_iter().filter(|(path, _, _)| !is_excluded(path, folders)).collect();
    let present: HashSet<&str> = indexed.iter().map(|(path, _, _)| path.as_str()).collect();
    let mut out: Vec<String> = read.keys().filter(|p| !present.contains(p.as_str())).cloned().collect();
    out.sort();
    let since = now.timestamp_millis() - RECENT_DAYS * 86_400_000;
    out.extend(indexed.iter().filter(|(path, hash, mtime)| *mtime >= since && read.get(path) != Some(hash)).map(|(path, _, _)| path.clone()));
    out.truncate(limit);
    out
}

/// Remembers that `note_path` was read with `hash`; `None` forgets a note that is gone.
pub fn seen(db: &Connection, note_path: &str, hash: Option<&str>, now: DateTime<FixedOffset>) -> rusqlite::Result<()> {
    match hash {
        Some(h) => db.execute(
            "INSERT INTO memory_source (note_path, content_hash, read_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(note_path) DO UPDATE SET content_hash = excluded.content_hash, read_at = excluded.read_at",
            params![note_path, h, now.to_rfc3339()],
        )?,
        None => db.execute("DELETE FROM memory_source WHERE note_path = ?1", [note_path])?,
    };
    Ok(())
}

/// The user turn: what is known, then the note and the language its lines are written in.
fn user_turn(title: &str, body: &str, earlier: &[(&str, &str)], topics: &[&str], rejected: &[(String, String)], language: &str) -> String {
    let mut user = String::new();
    user.push_str(&format!("Known topics: {}\n", if topics.is_empty() { "(none)".to_string() } else { topics.join(", ") }));
    if !rejected.is_empty() {
        user.push_str("Rejected, never write again:\n");
        for (topic, text) in rejected {
            user.push_str(&format!("- {topic}: {text}\n"));
        }
    }
    if earlier.is_empty() {
        user.push_str("The note's earlier lines: (none)\n");
    } else {
        user.push_str("The note's earlier lines:\n");
        for (topic, text) in earlier {
            user.push_str(&format!("- {topic}: {text}\n"));
        }
    }
    user.push_str(&format!("Note [[{title}]]:\n{}\n\nWrite text in {language}.", body.trim()));
    user
}

/// Body for llama-server's `/v1/chat/completions`: the note, its earlier lines in `current` (the
/// State file's entries), the other topics in use and the lines the user turned down. Lines are
/// written in the note's language (`lang` decides a note that does not tell).
pub fn request(note_path: &str, text: &str, current: &[Entry], rejected: &[(String, String)], lang: Lang) -> Value {
    let note = index::parse_note(note_path, text);
    let mut body = note.body;
    while tokens(&body) > NOTE_TOKENS && !body.is_empty() {
        body = body.chars().take(body.chars().count() * 9 / 10).collect();
    }
    let ui = if lang == Lang::En { "en" } else { "tr" };
    let language = name(answer_language(&body, None, ui));
    let mut messages = vec![json!({"role": "system", "content": SYSTEM})];
    for (title, text, earlier, answer) in EXAMPLES {
        let shown = name(answer_language(text, None, "tr"));
        messages.push(json!({"role": "user", "content": user_turn(title, text, earlier, &[], &[], shown)}));
        messages.push(json!({"role": "assistant", "content": answer}));
    }
    let earlier: Vec<(&str, &str)> = own(current, note_path).iter().map(|e| (e.topic.as_str(), e.text.as_str())).collect();
    let topics: Vec<&str> = current.iter().filter(|e| e.kind != Kind::Manual && !earlier.iter().any(|(t, _)| *t == e.topic)).map(|e| e.topic.as_str()).collect();
    messages.push(json!({"role": "user", "content": user_turn(&note.title, &body, &earlier, &topics, rejected, language)}));
    json!({
        "messages": messages,
        "temperature": 0,
        "max_tokens": 400,
        "cache_prompt": true,
        "response_format": {"type": "json_schema", "json_schema": {"name": "state", "schema": schema()}},
        "chat_template_kwargs": {"enable_thinking": false}
    })
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    memories: Vec<Line>,
}

#[derive(Deserialize)]
struct Line {
    #[serde(default)]
    evidence: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    text: String,
}

/// The sentences of `text` (folded with `index::key`, end marks kept) holding `quote` word for
/// word (a heading it starts with left out); `None` when the quote is not the note's, is a single
/// word or only a heading, or runs over more than two sentences (a whole recipe is not one state).
fn sentence_of(text: &str, quote: &str) -> Option<String> {
    let quote = index::key(quote.trim());
    let quote = quote.lines().skip_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    let quote = quote.trim();
    let inner = quote.trim_end_matches(['.', '!', '?']).matches(['.', '!', '?', '\n']).count();
    if index::loose_words(quote).len() < 2 || inner > 1 {
        return None;
    }
    let at = text.find(quote)?;
    let after = at + quote.len();
    let marks: &[char] = &['.', '!', '?', '\n'];
    let start = text[..at].rfind(marks).map_or(0, |i| i + 1);
    let end = text[after..].find(marks).map_or(text.len(), |i| after + i + 1);
    Some(text[start..end].to_string())
}

/// Word stems (five letters) of the words of `text` with five letters or more ("için", "ile"
/// and the like say nothing of their own).
fn stems(text: &str) -> HashSet<String> {
    index::loose_words(text).into_iter().filter(|w| w.chars().count() >= 5).map(|w| w.chars().take(5).collect()).collect()
}

/// The changes the code lets through, whatever the model said: each line's quote is in the note
/// word for word and its sentence states it (`qa::lang::is_unsure`, `is_general`), it is short,
/// it carries nothing of an earlier line the note no longer says, nothing close to a rejected
/// line comes back, and at most `LINES_PER_NOTE` lines are kept. Every line is an inference of
/// the State file with `source = "{note_path}#{note_hash}"`. The note's earlier lines it no longer
/// gives are dropped (`SOURCE_CHANGED`); one it gives again unchanged stays as it is, and a lone
/// earlier line the model gave a new topic is updated under its own. An answer that is not JSON
/// changes nothing.
pub fn changes(raw: &str, note_path: &str, note_hash: &str, text: &str, current: &[Entry], rejected: &[(String, String)]) -> Vec<Change> {
    let Ok(answer) = serde_json::from_str::<Answer>(raw.trim()) else { return Vec::new() };
    let earlier = own(current, note_path);
    let earlier_topics: Vec<String> = earlier.iter().map(|e| topic_key(&e.topic)).collect();
    let folded = index::key(text);
    let in_note = stems(text);
    // a daily note is named by its day (`daily/2026-10-23.md`), whatever the folder is called
    let stem = note_path.rsplit('/').next().unwrap_or(note_path).trim_end_matches(".md");
    let daily = chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d").is_ok();
    // a sentence that opens with the note's own subject ("Website is live" in "Website") is no one's news
    let title = index::loose_words(&index::parse_note(note_path, text).title);
    // what the earlier lines said that the note says no more ("tamirde" once the note is a how-to)
    let stale: HashSet<String> = earlier.iter().flat_map(|e| stems(&e.text)).filter(|s| !in_note.contains(s)).collect();
    let mut kept: Vec<(String, String)> = Vec::new();
    for line in answer.memories {
        if kept.len() >= LINES_PER_NOTE {
            break;
        }
        // "bütçe" for the earlier "butce": the earlier line's topic, so it is updated, not dropped
        let topic = topic_key(&line.topic);
        let topic = earlier_topics.iter().find(|t| index::loose(t) == index::loose(&topic)).cloned().unwrap_or(topic);
        // a line in another language than the note: the note's own words are the line
        let line_text = if same_language(line.text.trim(), text) { line.text.trim() } else { line.evidence.trim() };
        if line_text.is_empty() || topic.is_empty() || kept.iter().any(|(t, _)| *t == topic) || line_text.chars().count() > LINE_CHARS {
            continue;
        }
        let Some(sentence) = sentence_of(&folded, &line.evidence) else { continue };
        let turned_down = |t: &String| closeness(line_text, t) >= REJECTED_SIMILARITY || closeness(&line.evidence, t) >= REJECTED_SIMILARITY;
        // how things are, a bare list, someone's news, and in a daily note what one part of the day held
        let news = |s: &str| is_person_news(s) && !index::loose_words(s).first().is_some_and(|w| title.contains(w));
        // the folded sentence has lost its capitals: a name is looked for in the quote as written
        let not_state = sentence.split(['.', '!', '\n']).any(|s| is_general(s) || is_list(s) || (daily && is_day_event(s)))
            || line.evidence.split(['.', '!', '\n']).any(news)
            || is_hearsay(&sentence);
        if is_unsure(&sentence) || not_state ||stems(line_text).iter().any(|s| stale.contains(s)) || rejected.iter().any(|(_, t)| turned_down(t)) {
            continue;
        }
        kept.push((topic, line_text.to_string()));
    }
    // One earlier line left and one line on a new topic: the same subject, renamed by the model.
    let left: Vec<&String> = earlier_topics.iter().filter(|t| !kept.iter().any(|(k, _)| k == *t)).collect();
    let new: Vec<usize> = (0..kept.len()).filter(|&i| !earlier_topics.contains(&kept[i].0)).collect();
    if let ([old], [i]) = (left.as_slice(), new.as_slice()) {
        kept[*i].0 = (*old).clone();
    }
    let mut out = Vec::new();
    for (topic, line_text) in &kept {
        if earlier.iter().any(|e| topic_key(&e.topic) == *topic && e.text == *line_text) {
            continue;
        }
        out.push(Change::Add(Proposal {
            text: line_text.clone(),
            topic: topic.clone(),
            role: Role::State,
            person: None,
            kind: Kind::Inferred,
            source: Some(format!("{note_path}#{note_hash}")),
        }));
    }
    for e in earlier {
        if !kept.iter().any(|(t, _)| *t == topic_key(&e.topic)) {
            out.push(Change::Forget { topic: e.topic.clone(), reason: SOURCE_CHANGED.to_string() });
        }
    }
    out
}

/// Reads one note and writes what it says into the State file; returns how many lines were
/// written. A note gone from the vault drops its lines without asking the model, and a note PLA
/// generated gives none. The note counts as read afterwards (`seen`).
pub fn infer_note(
    vault: &Vault,
    db: &Connection,
    model: &mut dyn ChatModel,
    note_path: &str,
    now: DateTime<FixedOffset>,
    lang: Lang,
    cancel: &AtomicBool,
) -> Result<usize, InferError> {
    let current = super::load(vault).state;
    let (changes, hash) = match std::fs::read_to_string(vault.root.join(note_path)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let gone = own(&current, note_path).iter().map(|e| Change::Forget { topic: e.topic.clone(), reason: SOURCE_DELETED.to_string() }).collect();
            (gone, None)
        }
        Err(e) => return Err(RecallError::Io(e).into()),
        Ok(text) => {
            let hash = content_hash(text.as_bytes());
            let changes = if crate::notes::is_generated(&text) {
                changes(r#"{"memories":[]}"#, note_path, &hash, &text, &current, &[])
            } else {
                let rejected = rejected(db);
                let raw = model.complete(&request(note_path, &text, &current, &rejected, lang), cancel)?;
                let short: String = hash.chars().take(8).collect();
                changes(&raw, note_path, &short, &text, &current, &rejected)
            };
            (changes, Some(hash))
        }
    };
    let written = if changes.is_empty() { 0 } else { apply(vault, db, &changes, now, lang, &|_| {})?.len() };
    seen(db, note_path, hash.as_deref(), now)?;
    Ok(written)
}

/// The State file's first line under its heading: "Son güncelleme: 3 kayıt, 18 Eki" (the lines
/// the last maintenance wrote, and when). Not a record: the memory block never shows it.
pub fn stamp(vault: &Vault, count: usize, now: DateTime<FixedOffset>, lang: Lang) -> Result<(), RecallError> {
    let dir = vault.root.join(&vault.config.folders.memory);
    let other = if lang == Lang::Tr { Lang::En } else { Lang::Tr };
    let Some(file) = [lang, other].into_iter().map(|l| Role::State.file_name(l)).find(|n| dir.join(n).is_file()) else { return Ok(()) };
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path).map_err(|error| RecallError::Unreadable { file: file.to_string(), error })?;
    let mut doc = parse_doc(Role::State, &text);
    let day = visible_date(now.date_naive(), lang);
    let line = match lang {
        Lang::Tr => format!("Son güncelleme: {count} kayıt, {day}"),
        Lang::En => format!("Last updated: {count} record{}, {day}", if count == 1 { "" } else { "s" }),
    };
    let is_stamp = |b: &Block| matches!(b, Block::Raw(l) if l.starts_with("Son güncelleme:") || l.starts_with("Last updated:"));
    match doc.blocks.iter().position(is_stamp) {
        Some(i) => doc.blocks[i] = Block::Raw(line),
        None => match doc.blocks.iter().position(|b| matches!(b, Block::Raw(l) if l.starts_with("# "))) {
            Some(h) => {
                doc.blocks.splice(h + 1..h + 1, [Block::Raw(String::new()), Block::Raw(line)]);
            }
            None => {
                doc.blocks.splice(0..0, [Block::Raw(line), Block::Raw(String::new())]);
            }
        },
    }
    write_atomic(&path, render_doc(&doc, lang).as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::apply::{apply, rejected, Proposal, SOURCE_DELETED};
    use crate::recall::format::{parse, Kind, Role};
    use crate::util::content_hash;
    use crate::vault::open_vault;

    struct Fixture {
        _tmp: tempfile::TempDir,
        _data: tempfile::TempDir,
        vault: Vault,
        db: Connection,
        cache: Connection,
    }

    /// 2026-10-`day`, 10:00 Istanbul.
    fn at(day: u32) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(&format!("2026-10-{day:02}T10:00:00+03:00")).unwrap()
    }

    const NOW: u32 = 24;

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let data = tempfile::tempdir().unwrap();
            let vault = open_vault(tmp.path()).unwrap();
            let dbs = crate::db::open_databases(data.path()).unwrap();
            Fixture { _tmp: tmp, _data: data, vault, db: dbs.pla, cache: dbs.cache }
        }

        /// Writes and indexes a note changed `days_ago` days before `NOW`.
        fn note_at(&self, rel: &str, text: &str, days_ago: i64) {
            let path = self.vault.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            let mtime = at(NOW).timestamp_millis() - days_ago * 86_400_000;
            crate::index::index_note(&self.cache, rel, text, mtime, text.len() as i64).unwrap();
        }

        fn note(&self, rel: &str, text: &str) {
            self.note_at(rel, text, 1);
        }

        fn changed(&self, limit: usize) -> Vec<String> {
            changed_notes(&self.cache, &self.db, &self.vault.config.folders, at(NOW), limit)
        }

        fn mark(&self, rel: &str) {
            let text = std::fs::read_to_string(self.vault.root.join(rel)).unwrap();
            seen(&self.db, rel, Some(&content_hash(text.as_bytes())), at(NOW)).unwrap();
        }

        fn infer(&self, rel: &str, answer: &str, day: u32) -> usize {
            infer_note(&self.vault, &self.db, &mut Says(answer.into()), rel, at(day), Lang::Tr, &AtomicBool::new(false)).unwrap()
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.vault.root.join(&self.vault.config.folders.memory).join(name)).unwrap_or_default()
        }

        fn state(&self) -> Vec<Entry> {
            parse(Role::State, &self.read("Durum.md"))
        }
    }

    /// A model that always gives the same answer.
    struct Says(String);

    impl ChatModel for Says {
        fn complete(&mut self, _: &Value, _: &AtomicBool) -> Result<String, LlmError> {
            Ok(self.0.clone())
        }
        fn stream(&mut self, _: &Value, _: &AtomicBool, _: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
            Ok(self.0.clone())
        }
    }

    fn answer(lines: &[(&str, &str, &str)]) -> String {
        let memories: Vec<Value> = lines.iter().map(|(evidence, topic, text)| serde_json::json!({"evidence": evidence, "topic": topic, "text": text})).collect();
        serde_json::json!({ "memories": memories }).to_string()
    }

    fn inferred(topic: &str, text: &str, source: &str) -> Entry {
        Entry { id: topic.into(), text: text.into(), kind: Kind::Inferred, topic: topic.into(), source: Some(source.into()), date: at(20).date_naive(), person: None, notes: Vec::new() }
    }

    fn added(changes: &[Change]) -> Vec<&Proposal> {
        changes.iter().filter_map(|c| if let Change::Add(p) = c { Some(p) } else { None }).collect()
    }

    #[test]
    fn only_changed_notes_are_read() {
        let f = Fixture::new();
        f.note_at("notes/a.md", "# A\n\nX8 gövdesini seçtim.", 2);
        f.note("notes/b.md", "# B\n\nTezin ikinci bölümünü yazıyorum.");
        f.note_at("notes/eski.md", "# Eski\n\nGeçen yılın planı.", 90);
        assert_eq!(f.changed(10), ["notes/b.md", "notes/a.md"], "newest first; a note untouched for months is not current state");
        assert_eq!(f.changed(1), ["notes/b.md"]);
        f.mark("notes/a.md");
        f.mark("notes/b.md");
        assert!(f.changed(10).is_empty(), "nothing changed since the last read");
        f.note("notes/a.md", "# A\n\nTalon gövdesine geçtim.");
        assert_eq!(f.changed(10), ["notes/a.md"]);
    }

    #[test]
    fn memory_and_excluded_notes_are_never_read() {
        let f = Fixture::new();
        for rel in ["memory/Durum.md", "memory/Arşiv.md", "reports/2026-W42.md", "templates/gün.md", ".obsidian/x.md", "notes/a (conflict).md"] {
            f.note(rel, "# X\n\nTezin ikinci bölümünü yazıyorum.");
        }
        f.note("notes/ok.md", "# Ok\n\nTezin ikinci bölümünü yazıyorum.");
        assert_eq!(f.changed(10), ["notes/ok.md"]);
    }

    #[test]
    fn a_changed_note_replaces_its_own_inferences() {
        let f = Fixture::new();
        let first = "# Tez\n\nTezin ikinci bölümünü yazıyorum. İHA için X8 gövdesi önde.";
        f.note("notes/tez.md", first);
        f.note("notes/bütçe.md", "# Bütçe\n\nProje bütçesi onaylandı.");
        f.infer("notes/tez.md", &answer(&[("Tezin ikinci bölümünü yazıyorum", "tez", "Tezin ikinci bölümünü yazıyor."), ("X8 gövdesi önde", "iha-govde", "İHA gövdesinde X8 önde.")]), 20);
        f.infer("notes/bütçe.md", &answer(&[("Proje bütçesi onaylandı", "butce", "Proje bütçesi onaylandı.")]), 20);
        assert_eq!(f.state().len(), 3);

        // The note now says something else about the thesis and nothing about the airframe.
        let now = "# Tez\n\nTezin ikinci bölümü bitti, üçüncü bölüme geçtim.";
        f.note("notes/tez.md", now);
        let hash: String = content_hash(now.as_bytes()).chars().take(8).collect();
        let current = f.state();
        let raw = answer(&[("üçüncü bölüme geçtim", "tez", "Tezin üçüncü bölümünde.")]);
        let got = changes(&raw, "notes/tez.md", &hash, now, &current, &[]);
        let p = added(&got);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].source.as_deref(), Some(format!("notes/tez.md#{hash}").as_str()));
        assert!(got.contains(&Change::Forget { topic: "iha-govde".into(), reason: crate::recall::apply::SOURCE_CHANGED.into() }), "{got:?}");
        assert!(!got.iter().any(|c| matches!(c, Change::Forget { topic, .. } if topic == "butce")), "another note's line stays");
        // A lone earlier line the model renamed is updated under its own topic, and a line that
        // carries what the note no longer says is not written.
        let kurs = [inferred("dil-kursu", "Almanca A1 kursuna gidiyor.", "notes/Almanca.md#0000")];
        let note = "# Almanca\n\nA1 sertifikasını aldım, A2 kursuna kaydoldum.";
        let got = changes(&answer(&[("A2 kursuna kaydoldum", "almanca", "A2 kursuna kaydoldu.")]), "notes/Almanca.md", "1", note, &kurs, &[]);
        assert_eq!(added(&got).iter().map(|p| p.topic.as_str()).collect::<Vec<_>>(), ["dil-kursu"], "{got:?}");
        assert_eq!(got.len(), 1, "nothing dropped: {got:?}");
        let bike = [inferred("bisiklet", "Bisiklet tamirde.", "notes/Bisiklet.md#0000")];
        let note = "# Bisiklet\n\nZincir yağı: kuru hava için wax, yağmur için ıslak yağ.";
        let got = changes(&answer(&[("kuru hava için wax", "bisiklet", "Bisiklet tamirde, zincir için wax seçildi.")]), "notes/Bisiklet.md", "1", note, &bike, &[]);
        assert_eq!(got, [Change::Forget { topic: "bisiklet".into(), reason: crate::recall::apply::SOURCE_CHANGED.into() }]);
        // The same line again is no change at all (the archive does not fill up with copies).
        let same = changes(&answer(&[("Proje bütçesi onaylandı", "butce", "Proje bütçesi onaylandı.")]), "notes/bütçe.md", "ffffffff", "Proje bütçesi onaylandı.", &current, &[]);
        assert!(same.is_empty(), "{same:?}");

        f.infer("notes/tez.md", &raw, 21);
        let texts: Vec<String> = f.state().into_iter().map(|e| e.text).collect();
        assert_eq!(texts, ["Tezin üçüncü bölümünde.", "Proje bütçesi onaylandı."]);
        let archive = f.read("Arşiv.md");
        assert!(archive.contains("Tezin ikinci bölümünü yazıyor.") && archive.contains("X8 önde"), "{archive}");
        assert!(rejected(&f.db).is_empty(), "a changed note rejects nothing");
    }

    #[test]
    fn a_deleted_note_drops_its_inferences() {
        let f = Fixture::new();
        f.note("notes/tez.md", "# Tez\n\nTezin ikinci bölümünü yazıyorum.");
        f.infer("notes/tez.md", &answer(&[("Tezin ikinci bölümünü yazıyorum", "tez", "Tezin ikinci bölümünü yazıyor.")]), 20);
        assert!(f.changed(10).is_empty());
        std::fs::remove_file(f.vault.root.join("notes/tez.md")).unwrap();
        crate::index::remove_note(&f.cache, "notes/tez.md").unwrap();
        assert_eq!(f.changed(10), ["notes/tez.md"], "a note gone from the vault is read once more, to drop its lines");
        assert_eq!(f.infer("notes/tez.md", "not called", 21), 0);
        assert!(f.state().is_empty());
        assert!(rejected(&f.db).is_empty(), "the user rejected nothing");
        assert!(f.changed(10).is_empty());

        // A line the user said on the same topic is theirs: a gone note never takes it with it.
        let said = Proposal { text: "Tezi bitirdim.".into(), topic: "tez".into(), role: Role::State, person: None, kind: Kind::Said, source: None };
        apply(&f.vault, &f.db, &[Change::Add(said)], at(22), Lang::Tr, &|_| {}).unwrap();
        apply(&f.vault, &f.db, &[Change::Forget { topic: "tez".into(), reason: SOURCE_DELETED.into() }], at(23), Lang::Tr, &|_| {}).unwrap();
        assert_eq!(f.state()[0].text, "Tezi bitirdim.");
    }

    #[test]
    fn an_inference_never_lands_outside_current_state() {
        let text = "# İHA\n\nX8 gövdesini seçtim. Belki Talon'a geçerim. Kanat açıklığı ne olmalı? Motor siparişi verildi.";
        let rejected = [("motor".to_string(), "Motor siparişi verildi.".to_string())];
        let raw = serde_json::json!({"memories": [
            {"evidence": "X8 gövdesini seçtim", "topic": "iha-govde", "text": "İHA için X8 gövdesini seçti.", "file": "about", "person": "Deniz"},
            {"evidence": "Talon gövdesini seçtim", "topic": "talon", "text": "Talon'u seçti."},
            {"evidence": "Belki Talon'a geçerim", "topic": "talon", "text": "Talon'a geçecek."},
            {"evidence": "Kanat açıklığı ne olmalı", "topic": "kanat", "text": "Kanat açıklığını düşünüyor."},
            {"evidence": "Motor siparişi verildi", "topic": "motor", "text": "Motor siparişi verildi."},
            {"evidence": "", "topic": "bos", "text": "Kanıtsız."},
            {"evidence": "X8 gövdesini seçtim", "topic": "", "text": "Konusuz."}
        ]})
        .to_string();
        let got = changes(&raw, "iha/İHA.md", "1234abcd", text, &[], &rejected);
        let p = added(&got);
        assert_eq!(p.iter().map(|p| p.topic.as_str()).collect::<Vec<_>>(), ["iha-govde"], "{got:?}");
        assert_eq!((p[0].role, p[0].kind, p[0].person.as_deref(), p[0].source.as_deref()), (Role::State, Kind::Inferred, None, Some("iha/İHA.md#1234abcd")));
        // Reference, how-to and recipe notes are not a state, whatever the model quotes.
        let notes = "# Bakım\n\nMonstera haftada bir sulanır. Soğanı kavur. Mercimeği ekle. 25 dakika pişir.";
        let raw = answer(&[("Monstera haftada bir sulanır", "monstera", "Monstera sulanıyor."), ("# Bakım", "bakim", "Bakım notları var."), ("Soğanı kavur. Mercimeği ekle. 25 dakika pişir.", "corba", "Çorba yapıyor.")]);
        assert!(changes(&raw, "notes/Bakım.md", "1234abcd", notes, &[], &[]).is_empty());
        // A bare list, someone's news and a daily note's evening are not the user's state; the
        // daily note's real progress is.
        let lists = "# Ev\n\nSüt, yumurta, ekmek, domates. Yönetici aidatı artırmış.";
        let raw = answer(&[("Süt, yumurta, ekmek, domates", "market", "Süt, yumurta, ekmek ve domates alınacak."), ("Yönetici aidatı artırmış", "aidat", "Aidat arttı.")]);
        assert!(changes(&raw, "notes/Ev.md", "1234abcd", lists, &[], &[]).is_empty());
        let day = "# 23 Ekim\n\nTez taslağını danışmanıma gönderdim. Akşam arkadaşlarla yemeğe çıktık.";
        let raw = answer(&[("Tez taslağını danışmanıma gönderdim", "tez", "Tez taslağını danışmanına gönderdi."), ("Akşam arkadaşlarla yemeğe çıktık", "aksam", "Arkadaşlarla yemeğe çıktı.")]);
        let got = changes(&raw, "daily/2026-10-23.md", "1234abcd", day, &[], &[]);
        assert_eq!(added(&got).iter().map(|p| p.topic.as_str()).collect::<Vec<_>>(), ["tez"], "{got:?}");
        // Someone else's news is not written; the note's own subject and the user's state are.
        let team = "# Website\n\nApparently Sam is leaving. Priya got promoted. Website is live now.";
        let raw = answer(&[("Apparently Sam is leaving", "sam", "Sam is leaving."), ("Priya got promoted", "priya", "Priya got promoted."), ("Website is live now", "website", "Website is live now.")]);
        let got = changes(&raw, "notes/Website.md", "1234abcd", team, &[], &[]);
        assert_eq!(added(&got).iter().map(|p| p.topic.as_str()).collect::<Vec<_>>(), ["website"], "{got:?}");
        // Not JSON: nothing changes (the note's lines are not dropped for a broken answer).
        let current = [inferred("iha-govde", "X8 önde.", "iha/İHA.md#0000")];
        assert!(changes("not json", "iha/İHA.md", "1234abcd", text, &current, &[]).is_empty());
        // At most three lines from one note: one note cannot fill the State file.
        let many: Vec<(String, String, String)> =
            ["X8 gövdesini seçtim", "Motor siparişi verildi", "Kanat açıklığı 2 metre", "Talon yedekte"].iter().enumerate().map(|(i, e)| (e.to_string(), format!("t{i}"), format!("Satır {i}."))).collect();
        let many: Vec<(&str, &str, &str)> = many.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
        let text = "X8 gövdesini seçtim. Motor siparişi verildi. Kanat açıklığı 2 metre. Talon yedekte.";
        assert_eq!(added(&changes(&answer(&many), "iha/İHA.md", "1234abcd", text, &[], &[])).len(), 3);
    }

    #[test]
    fn the_state_file_says_when_it_was_updated() {
        let f = Fixture::new();
        f.note("notes/tez.md", "# Tez\n\nTezin ikinci bölümünü yazıyorum. Bütçe onaylandı.");
        let n = f.infer("notes/tez.md", &answer(&[("Tezin ikinci bölümünü yazıyorum", "tez", "Tezin ikinci bölümünü yazıyor."), ("Bütçe onaylandı", "butce", "Bütçe onaylandı.")]), 24);
        assert_eq!(n, 2);
        stamp(&f.vault, n, at(24), Lang::Tr).unwrap();
        let text = f.read("Durum.md");
        let body: Vec<&str> = text.lines().skip_while(|l| !l.starts_with("# ")).collect();
        assert_eq!(&body[..3], ["# Durum", "", "Son güncelleme: 2 kayıt, 24 Eki"], "{text}");
        assert_eq!(f.state().len(), 2, "the line is not a record");
        stamp(&f.vault, 1, at(25), Lang::Tr).unwrap();
        let text = f.read("Durum.md");
        assert_eq!(text.matches("Son güncelleme").count(), 1, "{text}");
        assert!(text.contains("Son güncelleme: 1 kayıt, 25 Eki"), "{text}");
        assert_eq!(f.state().len(), 2);
    }
}
