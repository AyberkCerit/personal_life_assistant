//! What the chat may write to memory: the request to the model, its JSON, and the code guards that
//! hold whatever the model says (spec 2026-10-18, "Sohbetten"). Only the user's own words are a
//! source; the assistant's answer never is.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::index;
use crate::lang::Lang;
use crate::qa::lang::{answer_language, is_unsure, name};

use super::apply::{Change, Proposal};
use super::format::{topic_key, Kind, Role};

/// A proposal this close (word-stem Jaccard) to a rejected line is not written. Measured in Task 8.
const REJECTED_SIMILARITY: f64 = 0.5;

const SYSTEM: &str = "You keep the user's long-term memory. Read the user's message and list only facts the user states as true about \
themselves or the people around them that will still matter in later conversations: stable preferences, routines, rules for how the \
assistant should answer, the current state of their projects and open work, and facts about named people. Never write anything \
hedged, negated, asked, wished or conditional, anything the assistant said, or anything that is only true today. For each memory, \
first quote the user's exact words as evidence, then write it as one short third-person sentence in {language}. topic is a short \
stable key in kebab-case; reuse a known topic when the memory updates it. file is about (facts and preferences), rules (how the \
assistant should answer), state (projects and open work) or people (a named person; person is then their name as the user wrote \
it). When the user asks to forget or correct something, put its known topic in forget with the user's words as evidence. Never \
write a rejected memory again. Answer with empty lists when there is nothing to keep.";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "memories": {"type": "array", "items": {"type": "object", "properties": {
                "evidence": {"type": "string"},
                "text": {"type": "string"},
                "topic": {"type": "string"},
                "file": {"type": "string", "enum": ["about", "rules", "state", "people"]},
                "person": {"type": ["string", "null"]}
            }, "required": ["evidence", "text", "topic", "file", "person"]}},
            "forget": {"type": "array", "items": {"type": "object", "properties": {
                "evidence": {"type": "string"},
                "topic": {"type": "string"}
            }, "required": ["evidence", "topic"]}}
        },
        "required": ["memories", "forget"]
    })
}

/// One item of the model's answer: a memory to write, or (`forget`) a known topic to drop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Raw {
    pub text: String,
    pub topic: String,
    /// `about`, `rules`, `state` or `people` (the Turkish file names are read too).
    pub file: String,
    pub person: Option<String>,
    /// The user's words, quoted.
    pub evidence: String,
    pub forget: Option<String>,
}

/// Body for llama-server's `/v1/chat/completions`. `previous` is the user's own earlier message,
/// given only when the message points back at it ("bunu"); never the assistant's answer. `topics`
/// are the active topics, `rejected` the `(topic, text)` lines the user turned down. Memories are
/// written in the message's language (`lang` decides a message that does not tell).
pub fn request(message: &str, previous: Option<&str>, topics: &[String], rejected: &[(String, String)], lang: Lang) -> Value {
    let ui = if lang == Lang::En { "en" } else { "tr" };
    let language = name(answer_language(message, previous, ui));
    let mut user = String::new();
    user.push_str(&format!("Known topics: {}\n", if topics.is_empty() { "(none)".to_string() } else { topics.join(", ") }));
    if !rejected.is_empty() {
        user.push_str("Rejected, never write again:\n");
        for (topic, text) in rejected {
            user.push_str(&format!("- {topic}: {text}\n"));
        }
    }
    if let Some(p) = previous {
        user.push_str(&format!("The user's previous message: {p}\n"));
    }
    user.push_str(&format!("The user's message: {message}"));
    json!({
        "messages": [
            {"role": "system", "content": SYSTEM.replace("{language}", language)},
            {"role": "user", "content": user}
        ],
        "temperature": 0,
        "max_tokens": 400,
        "cache_prompt": true,
        "response_format": {"type": "json_schema", "json_schema": {"name": "memory", "schema": schema()}},
        "chat_template_kwargs": {"enable_thinking": false}
    })
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    memories: Vec<Memory>,
    #[serde(default)]
    forget: Vec<Forget>,
}

#[derive(Deserialize)]
struct Memory {
    #[serde(default)]
    evidence: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    person: Option<String>,
}

#[derive(Deserialize)]
struct Forget {
    #[serde(default)]
    evidence: String,
    topic: String,
}

/// The model's JSON as one list: the memories, then the topics to forget.
pub fn parse(raw: &str) -> Result<Vec<Raw>, String> {
    let a: Answer = serde_json::from_str(raw.trim()).map_err(|e| e.to_string())?;
    let mut out: Vec<Raw> =
        a.memories.into_iter().map(|m| Raw { text: m.text, topic: m.topic, file: m.file, person: m.person, evidence: m.evidence, forget: None }).collect();
    out.extend(a.forget.into_iter().map(|f| Raw { evidence: f.evidence, forget: Some(f.topic), ..Raw::default() }));
    Ok(out)
}

/// The changes the code lets through, whatever the model said: the quote is the user's (message or
/// `previous`), its sentence states a fact (`qa::lang::is_unsure`), a person's line names a person
/// the user mentioned, nothing close to a rejected line comes back, and only a known topic is
/// forgotten.
pub fn guard(raw: Vec<Raw>, message: &str, previous: Option<&str>, topics: &[String], rejected: &[(String, String)]) -> Vec<Change> {
    let sources: Vec<&str> = std::iter::once(message).chain(previous).collect();
    let mut out = Vec::new();
    for r in raw {
        let Some(sentence) = sources.iter().find_map(|s| sentence_of(s, &r.evidence)) else { continue };
        if let Some(topic) = &r.forget {
            let key = topic_key(topic);
            if let Some(known) = topics.iter().find(|t| topic_key(t) == key) {
                out.push(Change::Forget { topic: known.clone(), reason: r.evidence.trim().to_string() });
            }
            continue;
        }
        let (text, topic) = (r.text.trim(), r.topic.trim());
        if text.is_empty() || topic_key(topic).is_empty() || is_unsure(&sentence) {
            continue;
        }
        let Some(role) = role_of(&r.file) else { continue };
        let person = r.person.as_deref().map(str::trim).filter(|p| !p.is_empty());
        if role == Role::People && !person.is_some_and(|p| sources.iter().any(|s| mentions(s, p))) {
            continue;
        }
        if rejected.iter().any(|(_, t)| closeness(text, t) >= REJECTED_SIMILARITY) {
            continue;
        }
        out.push(Change::Add(Proposal {
            text: text.to_string(),
            topic: topic.to_string(),
            role,
            person: if role == Role::People { person.map(str::to_string) } else { None },
            kind: Kind::Said,
            source: None,
        }));
    }
    out
}

fn role_of(file: &str) -> Option<Role> {
    match index::loose(file.trim()).as_str() {
        "about" | "hakkimda" => Some(Role::About),
        "rules" | "kurallar" => Some(Role::Rules),
        "state" | "current" | "durum" => Some(Role::State),
        "people" | "kisiler" => Some(Role::People),
        _ => None,
    }
}

/// The sentence of `text` that holds `quote` (folded with `index::key`), with its end mark, so a
/// hedge the quote left out still counts. `None` when the quote is empty or not there.
fn sentence_of(text: &str, quote: &str) -> Option<String> {
    let (text, quote) = (index::key(text), index::key(quote.trim()));
    if quote.is_empty() {
        return None;
    }
    let at = text.find(&quote)?;
    let marks = ['.', '!', '?', '\n'];
    let start = text[..at].rfind(marks).map_or(0, |i| i + 1);
    let after = at + quote.len();
    let end = text[after..].find(marks).map_or(text.len(), |i| after + i + 1);
    Some(text[start..end].to_string())
}

/// Whether `text` names `person` as whole words ("Deniz'in" names Deniz, "Denizli" does not).
fn mentions(text: &str, person: &str) -> bool {
    let (words, name) = (index::loose_words(text), index::loose_words(person));
    !name.is_empty() && words.windows(name.len()).any(|w| w == name.as_slice())
}

/// Shared word stems (five letters, digits as one sign), Jaccard: the nl-eval measure.
fn closeness(a: &str, b: &str) -> f64 {
    let stems = |s: &str| -> BTreeSet<String> {
        index::key(s)
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| if w.chars().all(|c| c.is_ascii_digit()) { "#".to_owned() } else { w.chars().take(5).collect() })
            .collect()
    };
    let (x, y) = (stems(a), stems(b));
    let union = x.union(&y).count();
    if union == 0 {
        return 0.0;
    }
    x.intersection(&y).count() as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::{Kind, Proposal, Role};

    fn memory(text: &str, topic: &str, evidence: &str) -> Raw {
        Raw { text: text.into(), topic: topic.into(), file: "about".into(), evidence: evidence.into(), ..Raw::default() }
    }

    fn added(changes: &[Change]) -> Vec<&Proposal> {
        changes.iter().filter_map(|c| if let Change::Add(p) = c { Some(p) } else { None }).collect()
    }

    #[test]
    fn evidence_must_be_in_the_message() {
        let msg = "Bu arada, sabahları spor yapıyorum.";
        let ok = guard(vec![memory("Sabahları spor yapıyor.", "spor-zamanı", "SABAHLARI spor yapıyorum")], msg, None, &[], &[]);
        let p = added(&ok);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].role, p[0].kind, p[0].source.as_deref()), (Role::About, Kind::Said, None));
        assert!(guard(vec![memory("Akşamları spor yapıyor.", "spor", "akşamları spor yapıyorum")], msg, None, &[], &[]).is_empty());
        assert!(guard(vec![memory("Sabahları spor yapıyor.", "spor", "")], msg, None, &[], &[]).is_empty(), "no quote, no memory");
        // The user's earlier message may hold it ("bunu hatırla"); nothing else can.
        let earlier = guard(vec![memory("Kahveyi sütsüz içer.", "kahve", "kahveyi sütsüz içerim")], "bunu hatırla", Some("Kahveyi sütsüz içerim."), &[], &[]);
        assert_eq!(added(&earlier).len(), 1);
    }

    #[test]
    fn hedged_negated_and_asked_are_dropped() {
        for msg in ["belki sabah spor yaparım", "kahve içmiyorum değil", "sabah mı spor yapsam?", "spor yapsam iyi olur", "Sanırım, sabahları spor yapıyorum.", "I might run in the mornings", "I don't drink coffee"] {
            let got = guard(vec![memory("Sabah spor.", "spor", msg)], msg, None, &[], &[]);
            assert!(added(&got).is_empty(), "{msg}");
        }
        let msg = "sabahları spor yapıyorum";
        assert_eq!(added(&guard(vec![memory("Sabahları spor yapıyor.", "spor", msg)], msg, None, &[], &[])).len(), 1);
        // The hedge counts in the evidence's sentence even when the quote leaves it out; another
        // sentence's hedge does not.
        let msg = "Belki sabah spor yaparım. Kahveyi sütsüz içerim.";
        let got = guard(vec![memory("Sabah spor.", "spor", "sabah spor yaparım"), memory("Kahveyi sütsüz içer.", "kahve", "Kahveyi sütsüz içerim")], msg, None, &[], &[]);
        assert_eq!(added(&got).iter().map(|p| p.topic.as_str()).collect::<Vec<_>>(), ["kahve"]);
    }

    #[test]
    fn a_rejected_fact_is_not_written_again() {
        let rejected = [("spor-zamani".to_string(), "Akşamları spor yapıyor.".to_string())];
        let msg = "akşamları spor yapıyorum, kahveyi de sütsüz içerim";
        let got = guard(
            vec![memory("Akşamları spor yapar.", "spor", "akşamları spor yapıyorum"), memory("Kahveyi sütsüz içer.", "kahve", "kahveyi de sütsüz içerim")],
            msg,
            None,
            &[],
            &rejected,
        );
        assert_eq!(added(&got).iter().map(|p| p.topic.as_str()).collect::<Vec<_>>(), ["kahve"]);
    }

    #[test]
    fn people_need_a_name() {
        let msg = "Deniz kahveyi sütsüz içer.";
        let person = |name: Option<&str>| Raw { file: "kişiler".into(), person: name.map(Into::into), ..memory("Kahveyi sütsüz içer.", "kahve", msg) };
        assert!(guard(vec![person(None)], msg, None, &[], &[]).is_empty());
        assert!(guard(vec![person(Some(" "))], msg, None, &[], &[]).is_empty());
        assert!(guard(vec![person(Some("Ali"))], msg, None, &[], &[]).is_empty(), "a name the user did not say");
        let got = guard(vec![person(Some("deniz"))], msg, None, &[], &[]);
        let p = added(&got);
        assert_eq!((p[0].role, p[0].person.as_deref()), (Role::People, Some("deniz")));
        let msg = "Deniz'in doğum günü 3 Mart.";
        let raw = Raw { file: "people".into(), person: Some("Deniz".into()), ..memory("Doğum günü 3 Mart.", "doğum-günü", msg) };
        assert_eq!(added(&guard(vec![raw], msg, None, &[], &[])).len(), 1, "a name with a suffix");
    }

    #[test]
    fn forget_only_known_topics() {
        let forget = |topic: &str, evidence: &str| Raw { forget: Some(topic.into()), evidence: evidence.into(), ..Raw::default() };
        let msg = "bunu unut, artık öyle değil";
        let topics = ["kahve".to_string(), "spor-zamani".to_string()];
        assert!(guard(vec![forget("uyku", msg)], msg, None, &topics, &[]).is_empty());
        assert!(guard(vec![forget("spor zamanı", "yanlış anladın")], msg, None, &topics, &[]).is_empty(), "the quote must be the user's");
        let got = guard(vec![forget("Spor Zamanı", "artık öyle değil")], msg, None, &topics, &[]);
        assert_eq!(got, [Change::Forget { topic: "spor-zamani".into(), reason: "artık öyle değil".into() }]);
    }

    #[test]
    fn parse_reads_both_lists() {
        let raw = r#"{"memories":[{"evidence":"sabahları spor yapıyorum","text":"Sabahları spor yapıyor.","topic":"spor-zamanı","file":"about","person":null}],"forget":[{"evidence":"bunu unut","topic":"kahve"}]}"#;
        let got = parse(raw).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].topic.as_str(), got[0].forget.as_deref()), ("spor-zamanı", None));
        assert_eq!((got[1].forget.as_deref(), got[1].evidence.as_str()), (Some("kahve"), "bunu unut"));
        assert!(parse("not json").is_err());
    }

    #[test]
    fn the_request_carries_the_message_topics_and_rejected() {
        let body = request("sabahları spor yapıyorum", None, &["kahve".into()], &[("uyku".into(), "Gece yarısı uyur.".into())], Lang::En);
        let user = body["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
        assert!(user.contains("sabahları spor yapıyorum") && user.contains("kahve") && user.contains("Gece yarısı uyur."), "{user}");
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("Turkish"), "records in the message's language");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["temperature"], 0);
    }
}
