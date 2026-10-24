//! What the chat may write to memory: the request to the model, its JSON, and the code guards that
//! hold whatever the model says (spec 2026-10-18, "Sohbetten"). Only the user's own words are a
//! source; the assistant's answer never is.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::index;
use crate::lang::Lang;
use crate::qa::lang::{answer_language, is_passing, is_unsure, language_of, name};

use super::apply::{Change, Proposal};
use super::format::{topic_key, Kind, Role};

/// A proposal this close (word-stem Jaccard) to a rejected line is not written. Measured in Task 8.
pub(super) const REJECTED_SIMILARITY: f64 = 0.5;

const SYSTEM: &str = "You keep the user's long-term memory. From the user's message, list only what the user states as true and lasting: their preferences, habits, health, work and routines (file about), how the assistant should answer them (file rules), where their projects and open work stand (file state), and facts about a person the user names (file people). Write nothing that is hedged, negated, asked, wished, planned or conditional, nothing only true today, and nothing the assistant said. For each memory: evidence is the user's exact words; person is the person's name as the user wrote it for file people, otherwise null (the user is never a person); topic is a short kebab-case key for the subject, the same known topic when the memory updates it; text is one short sentence in the language the turn names, without a subject (\"Kahveyi sütsüz içer.\", not \"Kullanıcı kahveyi sütsüz içer.\"). When the user asks to forget or correct something, put its known topic in forget with their words as evidence. Never write a rejected memory again. Answer with empty lists when there is nothing to keep.";

/// Worked examples shown before the message: `(message, known topics, answer)`. They share no case
/// with `tests/nl/memory.jsonl` (nl_eval checks the held-out cases against them).
const EXAMPLES: &[(&str, &[&str], &str)] = &[
    (
        "Gözlük kullanıyorum, miyobum.",
        &[],
        r#"{"memories":[{"evidence":"Gözlük kullanıyorum, miyobum.","file":"about","person":null,"topic":"goz","text":"Miyop, gözlük kullanıyor."}],"forget":[]}"#,
    ),
    (
        "Tarifleri gram olarak ver, ölçü kabıyla uğraşmıyorum.",
        &[],
        r#"{"memories":[{"evidence":"Tarifleri gram olarak ver","file":"rules","person":null,"topic":"tarif-olcusu","text":"Tarifleri gram olarak ister."}],"forget":[]}"#,
    ),
    (
        "Tezin ikinci bölümünü yazıyorum şu an.",
        &[],
        r#"{"memories":[{"evidence":"Tezin ikinci bölümünü yazıyorum","file":"state","person":null,"topic":"tez","text":"Tezin ikinci bölümünü yazıyor."}],"forget":[]}"#,
    ),
    (
        "Kuzenim Ece Ankara'da yaşıyor.",
        &[],
        r#"{"memories":[{"evidence":"Kuzenim Ece Ankara'da yaşıyor.","file":"people","person":"Ece","topic":"ece-sehir","text":"Kuzeni, Ankara'da yaşıyor."}],"forget":[]}"#,
    ),
    ("Galiba yarın yağmur yağacak, şemsiye alsam mı?", &[], r#"{"memories":[],"forget":[]}"#),
    ("Tezle ilgili kaydı sil, bitti o iş.", &["goz", "tez"], r#"{"memories":[],"forget":[{"evidence":"Tezle ilgili kaydı sil","topic":"tez"}]}"#),
    (
        "Burak gitar çalıyor, Zeynep de piyano.",
        &[],
        r#"{"memories":[{"evidence":"Burak gitar çalıyor","file":"people","person":"Burak","topic":"burak-enstruman","text":"Gitar çalıyor."},{"evidence":"Zeynep de piyano","file":"people","person":"Zeynep","topic":"zeynep-enstruman","text":"Piyano çalıyor."}],"forget":[]}"#,
    ),
    ("Tamam, teşekkürler.", &[], r#"{"memories":[],"forget":[]}"#),
    (
        "Ece'nin şehriyle ilgili yazdığını unut, taşındı.",
        &["goz", "ece-sehir"],
        r#"{"memories":[],"forget":[{"evidence":"Ece'nin şehriyle ilgili yazdığını unut","topic":"ece-sehir"}]}"#,
    ),
    (
        "Dedem balık tutmayı çok sever.",
        &[],
        r#"{"memories":[{"evidence":"Dedem balık tutmayı çok sever.","file":"people","person":"Dedem","topic":"dedem-hobi","text":"Balık tutmayı çok sever."}],"forget":[]}"#,
    ),
    (
        "My brother Tom lives in Leeds.",
        &[],
        r#"{"memories":[{"evidence":"My brother Tom lives in Leeds.","file":"people","person":"Tom","topic":"tom-city","text":"Lives in Leeds."}],"forget":[]}"#,
    ),
    (
        "Salı akşamları yoga kursuna gidiyorum.",
        &[],
        r#"{"memories":[{"evidence":"Salı akşamları yoga kursuna gidiyorum.","file":"about","person":null,"topic":"yoga","text":"Salı akşamları yoga kursuna gidiyor."}],"forget":[]}"#,
    ),
    (
        "I prefer tea over coffee.",
        &[],
        r#"{"memories":[{"evidence":"I prefer tea over coffee.","file":"about","person":null,"topic":"drink","text":"Prefers tea over coffee."}],"forget":[]}"#,
    ),
];

/// The example messages, so an evaluation can keep its held-out cases away from them.
pub fn example_messages() -> Vec<&'static str> {
    EXAMPLES.iter().map(|e| e.0).collect()
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "memories": {"type": "array", "items": {"type": "object", "properties": {
                "evidence": {"type": "string"},
                "file": {"type": "string", "enum": ["about", "rules", "state", "people"]},
                "person": {"type": ["string", "null"]},
                "topic": {"type": "string"},
                "text": {"type": "string"}
            }, "required": ["evidence", "file", "person", "topic", "text"]}},
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

/// The user turn: what is known, then the message and the language its memories are written in.
fn user_turn(message: &str, previous: Option<&str>, topics: &[&str], rejected: &[(String, String)], language: &str) -> String {
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
    user.push_str(&format!("The user's message: {message}\nWrite text in {language}."));
    user
}

/// Body for llama-server's `/v1/chat/completions`. `previous` is the user's own earlier message,
/// given only when the message points back at it ("bunu"); never the assistant's answer. `topics`
/// are the active topics, `rejected` the `(topic, text)` lines the user turned down. Memories are
/// written in the message's language (`lang` decides a message that does not tell).
pub fn request(message: &str, previous: Option<&str>, topics: &[String], rejected: &[(String, String)], lang: Lang) -> Value {
    let ui = if lang == Lang::En { "en" } else { "tr" };
    let language = name(answer_language(message, previous, ui));
    let mut messages = vec![json!({"role": "system", "content": SYSTEM})];
    for (msg, known, answer) in EXAMPLES {
        let shown = name(answer_language(msg, None, "tr"));
        messages.push(json!({"role": "user", "content": user_turn(msg, None, known, &[], shown)}));
        messages.push(json!({"role": "assistant", "content": answer}));
    }
    let topics: Vec<&str> = topics.iter().map(String::as_str).collect();
    messages.push(json!({"role": "user", "content": user_turn(message, previous, &topics, rejected, language)}));
    json!({
        "messages": messages,
        "temperature": 0,
        "max_tokens": 600,
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

/// Words that point back at the user's earlier message ("bunu hatırla", "remember that"): only
/// then is that message given to the model and the guard.
const POINTERS: &[&str] = &["bunu", "sunu", "onu", "bunlari", "sunlari", "onlari", "this", "that", "it", "these", "those"];

/// Whether `message` points back at the user's earlier message.
pub fn points_back(message: &str) -> bool {
    index::loose_words(message).iter().any(|w| POINTERS.contains(&w.as_str()))
}

/// The topics of the lines PLA wrote, as the model is shown them (`request`) and `guard` checks a
/// forget against.
pub fn known_topics(recall: &super::Recall) -> Vec<String> {
    let mut topics: Vec<String> = Vec::new();
    for e in recall.about.iter().chain(&recall.rules).chain(&recall.state).chain(&recall.people).filter(|e| e.kind != Kind::Manual) {
        if !topics.contains(&e.topic) {
            topics.push(e.topic.clone());
        }
    }
    topics
}

/// The changes the code lets through, whatever the model said: the quote is the user's (message or
/// `previous`), its sentence states a fact (`qa::lang::is_unsure`) and its clause a lasting one
/// (`is_passing`), it is not a request to forget, the line is in the message's language, a person's
/// line names a person the user mentioned, nothing close to a rejected line comes back, and only a
/// known topic is forgotten.
pub fn guard(raw: Vec<Raw>, message: &str, previous: Option<&str>, topics: &[String], rejected: &[(String, String)]) -> Vec<Change> {
    let sources: Vec<&str> = std::iter::once(message).chain(previous).collect();
    // "bunu unut" points back at the previous message: nothing from it is kept then.
    let forgetting = asks_to_forget(message);
    let mut out = Vec::new();
    for r in raw {
        let Some((at, place)) = sources.iter().enumerate().find_map(|(i, s)| quoted(s, &r.evidence).map(|q| (i, q))) else { continue };
        if let Some(topic) = &r.forget {
            let key = topic_key(topic);
            if let Some(known) = topics.iter().find(|t| topic_key(t) == key) {
                out.push(Change::Forget { topic: known.clone(), reason: r.evidence.trim().to_string() });
            }
            continue;
        }
        let (text, topic) = (r.text.trim(), r.topic.trim());
        if text.is_empty() || topic_key(topic).is_empty() || is_unsure(&place.sentence) || is_passing(&place.clause) || asks_to_forget(&place.sentence) {
            continue;
        }
        if forgetting && at > 0 {
            continue;
        }
        // A line in another language than the user's: their own words are the line.
        let text = if same_language(text, sources[at]) { text } else { r.evidence.trim() };
        let Some(mut role) = role_of(&r.file) else { continue };
        let kin = if role == Role::People { kin_in(&r.evidence) } else { None };
        let person = r.person.as_deref().map(str::trim).filter(|p| !p.is_empty()).or(kin.as_deref());
        let named = person.is_some_and(|p| sources.iter().any(|s| mentions(s, p)));
        // A line about a person the user named belongs under that person, whatever file it was given.
        if named {
            role = Role::People;
        }
        if role == Role::People && !named {
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

/// A line in Turkish for an English message, or the other way round (the model sometimes answers
/// in its examples' language).
pub(super) fn same_language(text: &str, source: &str) -> bool {
    let turkish_letters = |s: &str| s.chars().any(|c| "çğıöşüÇĞİÖŞÜ".contains(c));
    match (language_of(text), language_of(source)) {
        (Some(a), Some(b)) => a == b,
        (Some("tr"), None) | (None, None) => !(turkish_letters(text) && !turkish_letters(source)),
        _ => true,
    }
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

/// Where a quote sits: its sentence (with the end mark) and its clause, both folded with `index::key`.
struct Place {
    sentence: String,
    clause: String,
}

/// Where `quote` sits in `text`: word for word (folded), else as the same words with other endings
/// ("içer" for "içerim"), when the whole text counts as its sentence and clause so no hedge is
/// missed. `None` when the quote is empty or not the user's words.
fn quoted(text: &str, quote: &str) -> Option<Place> {
    let (text, quote) = (index::key(text), index::key(quote.trim()));
    if quote.is_empty() {
        return None;
    }
    let Some(at) = text.find(&quote) else {
        let words = index::loose_words(&text);
        let stem = |w: &String| w.chars().take(4).collect::<String>();
        let same = |q: &String| words.iter().any(|w| w == q || (q.chars().count() >= 3 && w.chars().count() >= 3 && stem(w) == stem(q)));
        let quote_words = index::loose_words(&quote);
        return (!quote_words.is_empty() && quote_words.iter().all(same)).then(|| Place { sentence: text.clone(), clause: text });
    };
    let after = at + quote.len();
    let around = |marks: &[char]| {
        let start = text[..at].rfind(marks).map_or(0, |i| i + 1);
        let end = text[after..].find(marks).map_or(text.len(), |i| after + i + 1);
        text[start..end].to_string()
    };
    Some(Place { sentence: around(&['.', '!', '?', '\n']), clause: around(&['.', '!', '?', '\n', ',', ';']) })
}

/// Kin words, folded: these take a suffix without an apostrophe ("annemin", "kardeşimle").
const KIN: &[&str] = &[
    "anne", "baba", "kardes", "abla", "abi", "agabey", "es", "kuzen", "teyze", "dayi", "amca", "hala", "dede", "nine", "babaanne", "anneanne", "oglu", "kizi", "yegen",
    "eniste", "yenge",
];

/// Whether `text` names `person` as whole words ("Deniz'in" names Deniz, "Denizli" does not); a
/// kin word may carry its suffix ("Annem" is named by "Annemin").
fn mentions(text: &str, person: &str) -> bool {
    let (words, name) = (index::loose_words(text), index::loose_words(person));
    if name.is_empty() {
        return false;
    }
    if words.windows(name.len()).any(|w| w == name.as_slice()) {
        return true;
    }
    let one = name.join(" ");
    let kin = KIN.iter().map(|k| index::loose(k)).find(|k| one.starts_with(k.as_str()));
    kin.filter(|k| one.len() <= k.len() + 3).is_some_and(|k| words.iter().any(|w| w.starts_with(&k) && (w.starts_with(&one) || one.starts_with(w.as_str()))))
}

/// The kin word a quote opens with as the user wrote it, with "my" ("Annemin" → "Annem",
/// "Kardeşimle" → "Kardeşim"): the person of a People line the model left unnamed.
fn kin_in(quote: &str) -> Option<String> {
    for word in quote.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        let folded = index::loose(word);
        let Some(k) = KIN.iter().find(|k| folded.starts_with(*k)) else { continue };
        let rest = &folded[k.len()..];
        let mine = if rest.starts_with('m') { 1 } else if ["im", "um"].iter().any(|e| rest.starts_with(e)) { 2 } else { 0 };
        return Some(word.chars().take(k.chars().count() + mine).collect());
    }
    None
}

/// A request to forget or delete ("bunu unut", "sil şunu", "forget it") is never a fact to keep;
/// "unutma" ("do not forget") is not one.
fn asks_to_forget(sentence: &str) -> bool {
    index::loose_words(sentence).iter().any(|w| (w.starts_with("unut") && !w.starts_with("unutma")) || w == "forget" || w == "sil" || w == "delete")
}

/// Shared word stems (five letters, digits as one sign), Jaccard: the nl-eval measure.
pub(super) fn closeness(a: &str, b: &str) -> f64 {
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
    fn passing_planned_and_forget_sentences_are_dropped() {
        // memory eval v2: each was written as a fact
        for msg in ["Yarın 10'da dişçiye gideceğim.", "Bugün çok yorgunum.", "Bu hafta spora gidemedim.", "Kahve kısmını unut, yanlış anlamışsın.", "Kahve içmeyi bırakmayı düşünüyorum."] {
            assert!(added(&guard(vec![memory("X.", "x", msg)], msg, None, &[], &[])).is_empty(), "{msg}");
        }
        let msg = "Bunu unutma: kahveyi sütsüz içerim.";
        assert_eq!(added(&guard(vec![memory("Kahveyi sütsüz içer.", "kahve", msg)], msg, None, &[], &[])).len(), 1);
    }

    #[test]
    fn a_named_person_goes_under_their_name_and_kin_words_take_suffixes() {
        let msg = "Deniz kahveyi sütsüz içer, Can ise şekerli.";
        let raw = Raw { person: Some("Can".into()), ..memory("Şekerli içer.", "kahve", msg) };
        let got = guard(vec![raw], msg, None, &[], &[]);
        assert_eq!((added(&got)[0].role, added(&got)[0].person.as_deref()), (Role::People, Some("Can")), "file about, person Can");
        let msg = "Annemin doğum günü 3 Mart.";
        for name in ["Annem", "anne"] {
            let raw = Raw { file: "people".into(), person: Some(name.into()), ..memory("Doğum günü 3 Mart.", "dogum-gunu", msg) };
            assert_eq!(added(&guard(vec![raw], msg, None, &[], &[])).len(), 1, "{name}");
        }
        assert!(!mentions("Denizli'ye gittim", "Deniz"));
    }

    #[test]
    fn a_passing_clause_does_not_take_its_sentence_with_it() {
        let msg = "Bugün akşam spor salonuna uğradım, kahveyi de artık sütsüz içiyorum.";
        let got = guard(vec![memory("Salona uğradı.", "spor", "Bugün akşam spor salonuna uğradım"), memory("Kahveyi sütsüz içer.", "kahve", "kahveyi de artık sütsüz içiyorum")], msg, None, &[], &[]);
        assert_eq!(added(&got).iter().map(|p| p.topic.as_str()).collect::<Vec<_>>(), ["kahve"]);
    }

    #[test]
    fn a_quote_with_other_endings_still_counts_but_reads_the_whole_message() {
        let got = guard(vec![memory("Kahveyi sütsüz içer.", "kahve", "Kahveyi sütsüz içer.")], "Kahveyi sütsüz içerim.", None, &[], &[]);
        assert_eq!(added(&got).len(), 1);
        let msg = "Kahveyi sütsüz içerim. Belki çay da denerim.";
        assert!(guard(vec![memory("Kahveyi sütsüz içer.", "kahve", "Kahveyi sütsüz içer.")], msg, None, &[], &[]).is_empty(), "the hedge elsewhere counts");
        assert!(guard(vec![memory("Çay içer.", "cay", "çay içerim")], "Kahveyi sütsüz içerim.", None, &[], &[]).is_empty());
    }

    #[test]
    fn a_line_in_another_language_proposals_and_a_forgotten_previous_are_dropped() {
        let msg = "I work night shifts at the hospital.";
        assert_eq!(added(&guard(vec![memory("Works night shifts at the hospital.", "work", msg)], msg, None, &[], &[])).len(), 1);
        let msg = "tamam, öyle yapalım";
        assert!(guard(vec![memory("Öyle yapalım.", "karar", msg)], msg, None, &[], &[]).is_empty());
        let got = guard(vec![memory("Aselsan'da çalışıyor.", "is-yeri", "Aselsan'da çalışıyorum.")], "Bunu unut, artık orada çalışmıyorum.", Some("Aselsan'da çalışıyorum."), &[], &[]);
        assert!(got.is_empty());
    }

    #[test]
    fn a_wrong_language_line_becomes_the_users_words_and_kin_names_the_person() {
        let msg = "I work night shifts at the hospital.";
        let got = guard(vec![memory("Hastanede gece vardiyasında çalışıyor.", "is", msg)], msg, None, &[], &[]);
        assert_eq!(added(&got)[0].text, msg);
        let msg = "Annemin doğum günü 3 Mart.";
        let raw = Raw { file: "people".into(), ..memory("Doğum günü 3 Mart.", "dogum-gunu", msg) };
        assert_eq!(added(&guard(vec![raw], msg, None, &[], &[]))[0].person.as_deref(), Some("Annem"));
        assert_eq!(kin_in("Kardeşimle konuştum"), Some("Kardeşim".into()));
        assert_eq!(kin_in("Deniz geldi"), None);
    }

    #[test]
    fn only_a_message_that_points_back_takes_the_previous_one() {
        assert!(points_back("bunu hatırla"));
        assert!(points_back("Remember that"));
        assert!(!points_back("Kahveyi sütsüz içerim."));
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
        assert!(user.ends_with("Write text in Turkish."), "records in the message's language: {user}");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["temperature"], 0);
    }
}
