//! The memory file format: one list item per record, with a machine comment after the visible text.
//!
//! `- {text} _({label} · {day month})_ <!-- pla:id=… konu=… tür=söz|çıkarım kaynak=… tarih=YYYY-MM-DD -->`
//!
//! A line without a valid comment is the user's own (`Manual`) and is never changed automatically.

use chrono::{Datelike, NaiveDate};

use crate::index;
use crate::lang::Lang;
use crate::util::content_hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Said by the user (`söz`).
    Said,
    /// Inferred from a note (`çıkarım`).
    Inferred,
    /// Written by hand (`elle`): no comment, never changed automatically.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    About,
    Rules,
    State,
    People,
    Archive,
}

impl Role {
    const ALL: [Role; 5] = [Role::About, Role::Rules, Role::State, Role::People, Role::Archive];

    fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Role::About, Lang::Tr) => "Hakkımda",
            (Role::Rules, Lang::Tr) => "Kurallar",
            (Role::State, Lang::Tr) => "Durum",
            (Role::People, Lang::Tr) => "Kişiler",
            (Role::Archive, Lang::Tr) => "Arşiv",
            (Role::About, Lang::En) => "About me",
            (Role::Rules, Lang::En) => "Rules",
            (Role::State, Lang::En) => "Current",
            (Role::People, Lang::En) => "People",
            (Role::Archive, Lang::En) => "Archive",
        }
    }

    /// The file name inside the memory folder, with `.md`.
    pub fn file_name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Role::About, Lang::Tr) => "Hakkımda.md",
            (Role::Rules, Lang::Tr) => "Kurallar.md",
            (Role::State, Lang::Tr) => "Durum.md",
            (Role::People, Lang::Tr) => "Kişiler.md",
            (Role::Archive, Lang::Tr) => "Arşiv.md",
            (Role::About, Lang::En) => "About me.md",
            (Role::Rules, Lang::En) => "Rules.md",
            (Role::State, Lang::En) => "Current.md",
            (Role::People, Lang::En) => "People.md",
            (Role::Archive, Lang::En) => "Archive.md",
        }
    }

    /// Either language, any case, with or without `.md`.
    pub fn from_file_name(name: &str) -> Option<Role> {
        let stem = name.strip_suffix(".md").or_else(|| name.strip_suffix(".MD")).unwrap_or(name);
        let wanted = index::key(stem);
        Role::ALL
            .into_iter()
            .find(|r| [Lang::Tr, Lang::En].iter().any(|l| index::key(r.name(*l)) == wanted))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub kind: Kind,
    pub topic: String,
    /// `path/to/note.md#blockid` for an inference.
    pub source: Option<String>,
    pub date: NaiveDate,
    /// The `## Name` section of the People file.
    pub person: Option<String>,
}

/// Turkish-folded, whitespace and `-`/`_` runs collapsed into one `-`.
pub fn topic_key(raw: &str) -> String {
    let folded = index::key(raw);
    let mut out = String::new();
    let mut pending = false;
    for c in folded.chars() {
        if c.is_whitespace() || c == '-' || c == '_' {
            pending = true;
        } else {
            if pending && !out.is_empty() {
                out.push('-');
            }
            pending = false;
            out.push(c);
        }
    }
    out
}

const KEYS: [&str; 5] = ["id", "konu", "tür", "kaynak", "tarih"];

/// Reads `key=value` pairs; a value runs until the next ` key=` or the end (values may hold spaces).
fn parse_pairs(body: &str) -> Vec<(&'static str, String)> {
    let padded = format!(" {body}");
    let mut marks: Vec<(usize, &'static str)> = Vec::new();
    for key in KEYS {
        let needle = format!(" {key}=");
        let mut from = 0;
        while let Some(i) = padded[from..].find(&needle) {
            marks.push((from + i, key));
            from += i + needle.len();
        }
    }
    marks.sort();
    marks
        .iter()
        .enumerate()
        .map(|(n, (pos, key))| {
            let start = pos + key.len() + 2;
            let end = marks.get(n + 1).map_or(padded.len(), |m| m.0);
            (*key, padded[start..end].trim().to_string())
        })
        .collect()
}

type Parsed = (String, String, Kind, Option<String>, NaiveDate);

/// `None` when the comment is not ours or lacks a required field (id, konu, tür, valid tarih).
fn parse_comment(comment: &str) -> Option<Parsed> {
    let pairs = parse_pairs(comment.strip_prefix("pla:")?);
    let get = |k: &str| pairs.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    let kind = match get("tür")?.as_str() {
        "söz" => Kind::Said,
        "çıkarım" => Kind::Inferred,
        _ => return None,
    };
    let date = NaiveDate::parse_from_str(&get("tarih")?, "%Y-%m-%d").ok()?;
    Some((get("id")?, get("konu")?, kind, get("kaynak"), date))
}

/// Cuts the trailing visible ` _(label · date)_` off the text.
fn strip_suffix(text: &str) -> &str {
    let text = text.trim_end();
    if text.ends_with(")_") {
        if let Some(i) = text.rfind(" _(") {
            return text[..i].trim_end();
        }
    }
    text
}

fn parse_line(body: &str, person: &Option<String>) -> Entry {
    let body = body.trim();
    if let (Some(open), true) = (body.rfind("<!--"), body.ends_with("-->")) {
        let comment = body[open + 4..body.len() - 3].trim();
        if let Some((id, topic, kind, source, date)) = parse_comment(comment) {
            return Entry { id, text: strip_suffix(&body[..open]).to_string(), kind, topic, source, date, person: person.clone() };
        }
    }
    // A hand-written line has no date; the minimum date marks "unknown".
    let id: String = content_hash(topic_key(body).as_bytes()).chars().take(8).collect();
    Entry { id, text: body.to_string(), kind: Kind::Manual, topic: topic_key(body), source: None, date: NaiveDate::MIN, person: person.clone() }
}

pub fn parse(role: Role, text: &str) -> Vec<Entry> {
    let mut person: Option<String> = None;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(h) = line.strip_prefix("## ") {
            if role == Role::People {
                person = Some(h.trim().trim_start_matches("[[").trim_end_matches("]]").trim().to_string());
            }
        } else if let Some(body) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            if !body.trim().is_empty() {
                out.push(parse_line(body, &person));
            }
        }
    }
    out
}

fn visible_date(date: NaiveDate, lang: Lang) -> String {
    const TR: [&str; 12] = ["Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara"];
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let months = if lang == Lang::Tr { TR } else { EN };
    format!("{} {}", date.day(), months[date.month0() as usize])
}

fn render_line(e: &Entry, lang: Lang) -> String {
    let (label, kind) = match e.kind {
        Kind::Manual => return format!("- {}", e.text),
        Kind::Said => (if lang == Lang::Tr { "senin sözün" } else { "you said" }.to_string(), "söz"),
        Kind::Inferred => {
            let path = e.source.as_deref().unwrap_or("").split('#').next().unwrap_or("");
            let file = path.rsplit('/').next().unwrap_or(path);
            (format!("[[{}]]", file.strip_suffix(".md").unwrap_or(file)), "çıkarım")
        }
    };
    let source = e.source.as_ref().map(|s| format!(" kaynak={s}")).unwrap_or_default();
    format!(
        "- {} _({} · {})_ <!-- pla:id={} konu={} tür={}{} tarih={} -->",
        e.text,
        label,
        visible_date(e.date, lang),
        e.id,
        e.topic,
        kind,
        source,
        e.date.format("%Y-%m-%d")
    )
}

pub fn render(role: Role, entries: &[Entry], lang: Lang) -> String {
    let mut out = format!("---\npla_generated: true\n---\n# {}\n\n", role.name(lang));
    if role == Role::People {
        let mut names: Vec<&str> = Vec::new();
        for p in entries.iter().filter_map(|e| e.person.as_deref()) {
            if !names.contains(&p) {
                names.push(p);
            }
        }
        // Lines before any person heading come first.
        for e in entries.iter().filter(|e| e.person.is_none()) {
            out.push_str(&render_line(e, lang));
            out.push('\n');
        }
        for name in names {
            out.push_str(&format!("\n## {name}\n\n"));
            for e in entries.iter().filter(|e| e.person.as_deref() == Some(name)) {
                out.push_str(&render_line(e, lang));
                out.push('\n');
            }
        }
    } else {
        for e in entries {
            out.push_str(&render_line(e, lang));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAID: &str = "- Sabahları spor yapıyor. _(senin sözün · 18 Eki)_ <!-- pla:id=k7f3 konu=spor-zamanı tür=söz tarih=2026-10-18 -->";
    const INFERRED: &str = "- İHA gövde seçiminde X8 önde. _([[Skywalker X8]] · 10 Eki)_ <!-- pla:id=a91c konu=iha-gövde tür=çıkarım kaynak=iha/Skywalker X8.md#9f2e tarih=2026-10-10 -->";

    #[test]
    fn a_line_round_trips() {
        let entries = parse(Role::About, &format!("{SAID}\n{INFERRED}\n"));
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].id.as_str(), entries[0].topic.as_str(), entries[0].kind, &entries[0].source), ("k7f3", "spor-zamanı", Kind::Said, &None));
        assert_eq!(entries[0].text, "Sabahları spor yapıyor.");
        assert_eq!(entries[1].kind, Kind::Inferred);
        assert_eq!(entries[1].source.as_deref(), Some("iha/Skywalker X8.md#9f2e"));
        let out = render(Role::About, &entries, Lang::Tr);
        assert!(out.contains(&format!("{SAID}\n{INFERRED}\n")), "{out}");
        assert_eq!(parse(Role::About, &out), entries);
    }

    #[test]
    fn a_line_without_its_comment_is_manual() {
        let e = &parse(Role::Rules, "- Kahveyi sütsüz içer.\n")[0];
        assert_eq!(e.kind, Kind::Manual);
        assert_eq!(e.id.len(), 8);
        assert_eq!(e.id, parse(Role::Rules, "- Kahveyi sütsüz içer.")[0].id);
        assert_eq!(render(Role::Rules, std::slice::from_ref(e), Lang::Tr).lines().last(), Some("- Kahveyi sütsüz içer."));
    }

    #[test]
    fn a_broken_comment_counts_as_manual_text() {
        let e = &parse(Role::About, "- Bir şey <!-- pla:id= -->\n")[0];
        assert_eq!(e.kind, Kind::Manual);
        assert!(e.text.contains("<!-- pla:id= -->"));
    }

    #[test]
    fn people_sections_carry_the_name() {
        let md = "# Kişiler\n\n## [[Deniz]]\n\n- Kız kardeşi.\n\n## Annem\n\n- Çay sever.\n";
        let e = parse(Role::People, md);
        assert_eq!(e[0].person.as_deref(), Some("Deniz"));
        assert_eq!(e[1].person.as_deref(), Some("Annem"));
        let out = render(Role::People, &e, Lang::Tr);
        assert_eq!(parse(Role::People, &out), e);
    }

    #[test]
    fn topic_key_folds_turkish() {
        let a = topic_key("Spor Zamanı");
        assert_eq!(a, "spor-zamani");
        assert_eq!(a, topic_key("spor-zamani"));
        assert_eq!(a, topic_key("SPOR  ZAMANI"));
    }

    #[test]
    fn the_frontmatter_marks_it_generated() {
        assert!(crate::notes::is_generated(&render(Role::State, &[], Lang::En)));
    }

    #[test]
    fn file_names_are_found_in_both_languages() {
        assert_eq!(Role::from_file_name("kişiler.md"), Some(Role::People));
        assert_eq!(Role::from_file_name("About me.md"), Some(Role::About));
        assert_eq!(Role::from_file_name("Notes.md"), None);
        assert_eq!(Role::Archive.file_name(Lang::Tr), "Arşiv.md");
    }

    #[test]
    fn english_dates_and_labels() {
        let e = parse(Role::About, SAID);
        let out = render(Role::About, &e, Lang::En);
        assert!(out.contains("_(you said · 18 Oct)_"), "{out}");
    }
}
