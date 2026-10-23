//! Reads the memory files and builds the two prompt blocks: the fixed memory and the people the
//! conversation mentions. Reading never writes: a missing folder is an empty memory.

use crate::index;
use crate::qa::Turn;
use crate::util::tokens;
use crate::vault::Vault;

use super::format::{parse_doc, Block, Entry, Kind, Role};

/// The fixed memory block never exceeds this many tokens.
pub const MEMORY_TOKENS: usize = 600;
/// The block with the people a conversation mentions never exceeds this many tokens.
pub const PEOPLE_TOKENS: usize = 300;

const MEMORY_SECTIONS: [(&str, usize); 3] = [("About the user:", 0), ("Rules from the user:", 1), ("Current state:", 2)];

#[derive(Debug, Clone, Default)]
pub struct Recall {
    pub about: Vec<Entry>,
    pub rules: Vec<Entry>,
    pub state: Vec<Entry>,
    /// Entries of the People file; `person` holds the `## Name` they sit under.
    pub people: Vec<Entry>,
    /// `(name, alias)` from `## [[Name|Alias]]` headings, so the alias also matches a mention.
    pub aliases: Vec<(String, String)>,
    /// Unreadable files, duplicated language files, and a memory block that had to be cut.
    pub warnings: Vec<String>,
}

pub fn load(vault: &Vault) -> Recall {
    let mut r = Recall::default();
    let dir = vault.root.join(&vault.config.folders.memory);
    let Ok(read) = std::fs::read_dir(&dir) else { return r };
    let mut files: Vec<(Role, String)> = read
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            Role::from_file_name(&name).filter(|_| name.to_lowercase().ends_with(".md")).map(|role| (role, name))
        })
        .filter(|(role, _)| *role != Role::Archive)
        .collect();
    files.sort_by(|a, b| a.1.cmp(&b.1));
    let mut warned: Vec<Role> = Vec::new();
    for (role, name) in &files {
        if files.iter().filter(|(other, _)| other == role).count() > 1 && !warned.contains(role) {
            warned.push(*role);
            r.warnings.push(format!("memory: both language files exist for the same role (including {name}); both are read"));
        }
        let text = match std::fs::read_to_string(dir.join(name)) {
            Ok(t) => t,
            Err(e) => {
                r.warnings.push(format!("memory: cannot read {name}: {e}"));
                continue;
            }
        };
        let doc = parse_doc(*role, &text);
        if *role == Role::People {
            for b in &doc.blocks {
                if let Block::Person { raw, name } = b {
                    if let Some(alias) = heading_alias(raw) {
                        r.aliases.push((name.clone(), alias));
                    }
                }
            }
        }
        let entries = doc.entries();
        match role {
            Role::About => r.about.extend(entries),
            Role::Rules => r.rules.extend(entries),
            Role::State => r.state.extend(entries),
            Role::People => r.people.extend(entries),
            Role::Archive => {}
        }
    }
    let cut = build_block(&r).1;
    if cut > 0 {
        r.warnings.push(format!("memory: {cut} line(s) did not fit the {MEMORY_TOKENS}-token budget and are left out of the prompt"));
    }
    r
}

/// `[[Name|Alias]]` in a `## ` heading.
fn heading_alias(raw: &str) -> Option<String> {
    let inner = raw.trim().strip_prefix("##")?.trim().strip_prefix("[[")?.split_once("]]")?.0;
    let alias = inner.split_once('|')?.1.trim();
    (!alias.is_empty()).then(|| alias.to_string())
}

fn line(e: &Entry) -> String {
    let date = e.date.format("%Y-%m-%d");
    match e.kind {
        Kind::Manual => format!("- {}", e.text),
        Kind::Said => format!("- {} (said by the user, {date})", e.text),
        Kind::Inferred => {
            let path = e.source.as_deref().unwrap_or("").split('#').next().unwrap_or("");
            let file = path.rsplit('/').next().unwrap_or(path);
            format!("- According to your notes ([[{}]], {date}): {}", file.strip_suffix(".md").unwrap_or(file), e.text)
        }
    }
}

/// Indices in keep-first order: manual lines first (file order; their date is unknown), then the
/// rest newest first.
fn priority(entries: &[&Entry]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    idx.sort_by(|&a, &b| {
        let (x, y) = (entries[a], entries[b]);
        match (x.kind == Kind::Manual, y.kind == Kind::Manual) {
            (true, true) => a.cmp(&b),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => y.date.cmp(&x.date).then(a.cmp(&b)),
        }
    });
    idx
}

/// The memory block text and how many lines it had to leave out.
fn build_block(r: &Recall) -> (String, usize) {
    let lists = [&r.about, &r.rules, &r.state];
    let all: Vec<(usize, &Entry)> = lists.iter().enumerate().flat_map(|(s, l)| l.iter().map(move |e| (s, e))).collect();
    let refs: Vec<&Entry> = all.iter().map(|(_, e)| *e).collect();
    let mut keep = vec![false; all.len()];
    let render = |keep: &[bool]| -> String {
        let mut out = String::new();
        for (title, s) in MEMORY_SECTIONS {
            let lines: Vec<String> = all.iter().zip(keep).filter(|((sec, _), k)| *sec == s && **k).map(|((_, e), _)| line(e)).collect();
            if lines.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(title);
            out.push('\n');
            out.push_str(&lines.join("\n"));
            out.push('\n');
        }
        out
    };
    for i in priority(&refs) {
        keep[i] = true;
        if tokens(&render(&keep)) > MEMORY_TOKENS {
            keep[i] = false;
        }
    }
    let dropped = keep.iter().filter(|k| !**k).count();
    (render(&keep), dropped)
}

/// About / rules / state as prompt text within `MEMORY_TOKENS`; empty when there is nothing. Lines
/// that do not fit are left out whole (`load` reports the cut in `Recall.warnings`).
pub fn block(r: &Recall) -> String {
    build_block(r).0.trim_end().to_string()
}

/// Where `name` first appears in `text` as a whole word (Turkish-folded); an apostrophe suffix
/// (`Deniz'in`) still counts.
fn mention_pos(name: &str, text: &str) -> Option<usize> {
    let needle = index::key(name);
    if needle.is_empty() {
        return None;
    }
    let hay = index::key(text);
    let mut from = 0;
    while let Some(p) = hay[from..].find(&needle) {
        let start = from + p;
        let end = start + needle.len();
        let before_ok = hay[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after_ok = hay[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return Some(hay[..start].chars().count());
        }
        from = start + hay[start..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// The people named in the question or the last two turns, with what is known about them, within
/// `PEOPLE_TOKENS`. Empty when nobody matches.
pub fn people_block(r: &Recall, question: &str, turns: &[Turn]) -> String {
    let mut sources: Vec<String> = vec![question.to_string()];
    for t in turns.iter().rev().take(2) {
        sources.push(format!("{} {}", t.question, t.answer));
    }
    let mut names: Vec<&str> = Vec::new();
    for n in r.people.iter().filter_map(|e| e.person.as_deref()) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let mut hits: Vec<((usize, usize), &str)> = Vec::new();
    for n in names {
        let aliases: Vec<&str> = r.aliases.iter().filter(|(name, _)| name == n).map(|(_, a)| a.as_str()).collect();
        let found = sources.iter().enumerate().find_map(|(si, s)| {
            std::iter::once(n).chain(aliases.iter().copied()).filter_map(|c| mention_pos(c, s)).min().map(|p| (si, p))
        });
        if let Some(pos) = found {
            hits.push((pos, n));
        }
    }
    hits.sort_by_key(|(pos, _)| *pos);

    let mut out = String::from("About people mentioned:");
    let mut any = false;
    for (_, name) in hits {
        let theirs: Vec<&Entry> = r.people.iter().filter(|e| e.person.as_deref() == Some(name)).collect();
        let mut chunk = String::new();
        for i in priority(&theirs) {
            let candidate = if chunk.is_empty() { format!("\n{name}:\n{}", line(theirs[i])) } else { format!("{chunk}\n{}", line(theirs[i])) };
            if tokens(&format!("{out}{candidate}")) <= PEOPLE_TOKENS {
                chunk = candidate;
            }
        }
        if !chunk.is_empty() {
            out.push_str(&chunk);
            any = true;
        }
    }
    if any {
        out
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::open_vault;

    fn vault_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
        let tmp = tempfile::tempdir().unwrap();
        let v = open_vault(tmp.path()).unwrap();
        if !files.is_empty() {
            let dir = tmp.path().join(&v.config.folders.memory);
            std::fs::create_dir_all(&dir).unwrap();
            for (n, c) in files {
                std::fs::write(dir.join(n), c).unwrap();
            }
        }
        (tmp, v)
    }

    fn turn(q: &str, a: &str) -> Turn {
        Turn { turn_id: "t".into(), question: q.into(), answer: a.into(), tools: Vec::new(), created_at: String::new(), status: "done".into(), new_topic: false }
    }

    #[test]
    fn no_memory_folder_is_an_empty_memory() {
        let (tmp, v) = vault_with(&[]);
        let r = load(&v);
        assert!(r.about.is_empty() && r.warnings.is_empty());
        assert_eq!(block(&r), "");
        assert_eq!(people_block(&r, "Deniz", &[]), "");
        assert!(!tmp.path().join(&v.config.folders.memory).exists(), "reading creates nothing");
    }

    #[test]
    fn the_block_labels_each_kind() {
        let md = "- Sabah spor. <!-- pla:id=a konu=spor tür=söz tarih=2026-10-18 -->\n- X8 önde. <!-- pla:id=b konu=x tür=çıkarım kaynak=iha/Skywalker X8.md#9f tarih=2026-10-10 -->\n- Çay sever.\n";
        let (_t, v) = vault_with(&[("Hakkımda.md", md), ("Notes.md", "- yok\n"), ("Arşiv.md", "- arşiv\n")]);
        let r = load(&v);
        assert_eq!(r.about.len(), 3);
        let b = block(&r);
        assert!(b.starts_with("About the user:\n"), "{b}");
        assert!(b.contains("- Sabah spor. (said by the user, 2026-10-18)"), "{b}");
        assert!(b.contains("- According to your notes ([[Skywalker X8]], 2026-10-10): X8 önde."), "{b}");
        assert!(b.ends_with("- Çay sever."), "{b}");
        assert!(!b.contains("arşiv") && !b.contains("yok"));
    }

    #[test]
    fn inferred_lines_carry_their_hedge() {
        let md = "- Sabah spor. <!-- pla:id=a konu=spor tür=söz tarih=2026-10-18 -->
- X8 önde. <!-- pla:id=b konu=x tür=çıkarım kaynak=iha/Skywalker X8.md#9f tarih=2026-10-10 -->
- Çay sever.
";
        let r = load(&vault_with(&[("Hakkımda.md", md)]).1);
        let b = block(&r);
        let hedged: Vec<&str> = b.lines().filter(|l| l.starts_with("- According to your notes (")).collect();
        assert_eq!(hedged.len(), 1, "{b}");
        assert_eq!(hedged[0], "- According to your notes ([[Skywalker X8]], 2026-10-10): X8 önde.", "{b}");
        assert!(b.contains("- Sabah spor. (said by the user, 2026-10-18)") && b.contains("- Çay sever."), "{b}");
    }

    #[test]
    fn the_budget_keeps_manual_lines_then_the_newest() {
        let mut md = String::from("- Elle yazılmış önemli satır.\n");
        for d in 1..=28 {
            md.push_str(&format!("- Söz numarası {d} ve biraz uzun bir cümle daha ekleyelim burada. <!-- pla:id=i{d} konu=k{d} tür=söz tarih=2026-09-{d:02} -->\n"));
        }
        let r = load(&vault_with(&[("Hakkımda.md", &md)]).1);
        let b = block(&r);
        assert!(tokens(&b) <= MEMORY_TOKENS && b.contains("Elle yazılmış önemli satır."), "{b}");
        assert!(b.contains("Söz numarası 28 ") && !b.contains("Söz numarası 1 "), "{b}");
        assert!(r.warnings.iter().any(|w| w.contains("did not fit")));
    }

    #[test]
    fn sections_come_in_order_and_empty_ones_are_omitted() {
        let (_t, v) = vault_with(&[("Durum.md", "- Proje X\n"), ("Rules.md", "- Kısa yaz\n")]);
        let b = block(&load(&v));
        assert_eq!(b, "Rules from the user:\n- Kısa yaz\n\nCurrent state:\n- Proje X");
    }

    #[test]
    fn an_oversized_manual_file_is_cut_in_the_prompt_not_on_disk() {
        let body: String = (0..400).map(|i| format!("- elle yazılmış satır numarası {i} burada\n")).collect();
        assert!(body.chars().count() > 10_000);
        let (t, v) = vault_with(&[("Hakkımda.md", &body)]);
        let path = t.path().join(&v.config.folders.memory).join("Hakkımda.md");
        let before = std::fs::read(&path).unwrap();
        let r = load(&v);
        assert!(tokens(&block(&r)) <= MEMORY_TOKENS);
        assert!(!block(&r).is_empty());
        assert!(!r.warnings.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn both_language_files_are_read_with_a_warning() {
        let (_t, v) = vault_with(&[("Hakkımda.md", "- bir\n"), ("About me.md", "- iki\n")]);
        let r = load(&v);
        assert_eq!(r.about.len(), 2);
        assert_eq!(r.warnings.len(), 1);
    }

    fn people() -> Recall {
        let md = "# Kişiler\n\n## Can\n\n- Arkadaşı.\n\n## Deniz\n\n- Kız kardeşi.\n\n## [[Prof. Aydın|Hoca]]\n\n- Danışman.\n";
        load(&vault_with(&[("Kişiler.md", md)]).1)
    }

    #[test]
    fn people_match_whole_names_only() {
        let r = people();
        let b = people_block(&r, "Deniz'in doğum günü ne zaman?", &[]);
        assert!(b.starts_with("About people mentioned:\nDeniz:\n- Kız kardeşi."), "{b}");
        assert!(!b.contains("Can"));
        assert_eq!(people_block(&r, "canım sıkıldı", &[]), "");
        assert_eq!(people_block(&r, "Canan geldi", &[]), "");
        let last = [turn("kim geldi", "Can geldi")];
        assert!(people_block(&r, "ne dedi?", &last).contains("Can:\n- Arkadaşı."));
        assert!(people_block(&r, "Can’a sor", &[]).contains("Can:"));
    }

    #[test]
    fn aliases_phrases_and_mention_order() {
        let r = people();
        assert!(people_block(&r, "Hoca ne dedi", &[]).contains("Prof. Aydın:"));
        assert!(people_block(&r, "prof. aydın ile görüştüm", &[]).contains("Prof. Aydın:"));
        let b = people_block(&r, "Deniz ve Can", &[]);
        assert!(b.find("Deniz:").unwrap() < b.find("Can:").unwrap());
        // Only the last two turns count.
        let recent = [turn("a", "b"), turn("Can?", "x"), turn("y", "z")];
        assert!(people_block(&r, "ne", &recent).contains("Can:"));
        let old = [turn("Can?", "x"), turn("a", "b"), turn("y", "z")];
        assert_eq!(people_block(&r, "ne", &old), "");
    }

    #[test]
    fn people_stay_within_their_share() {
        let mut md = String::from("# Kişiler\n");
        let mut q = String::new();
        for i in 0..10u8 {
            let name = format!("Kişi{}", (b'A' + i) as char);
            md.push_str(&format!("\n## {name}\n\n"));
            for j in 0..6 {
                md.push_str(&format!("- Bu kişi hakkında oldukça uzun bir bilgi satırı numara {j}.\n"));
            }
            q.push_str(&name);
            q.push(' ');
        }
        let r = load(&vault_with(&[("Kişiler.md", &md)]).1);
        let b = people_block(&r, &q, &[]);
        assert!(!b.is_empty() && tokens(&b) <= PEOPLE_TOKENS, "{} tokens", tokens(&b));
    }
}
