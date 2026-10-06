//! The note index in `cache.db` (SRS §3.3, M3): titles, aliases and tags, wikilinks with their line,
//! and full-text search. Rebuildable from the vault at any time; it never changes a note.

use std::path::Path;

use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::pipeline::blocks::content_hash;

/// A wikilink as written in a note: its normalised target and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Lower-case, `/` separators, without `.md` and without `#heading`.
    pub target: String,
    /// 1-based line number.
    pub line: usize,
    pub line_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedNote {
    pub title: String,
    pub aliases: Vec<String>,
    /// Lower-case, without `#`; nested tags keep their `/`.
    pub tags: Vec<String>,
    pub links: Vec<Link>,
    /// The text without frontmatter, for full-text search.
    pub body: String,
}

/// Lower-case the way note names and tags are compared: `İ` becomes `i` (not `i̇`), the rest as Unicode.
fn fold(s: &str) -> String {
    s.replace('İ', "i").to_lowercase()
}

/// How names and tags are matched: like `fold`, and Turkish `ı` equals `i`, so `Işık`, `ışık` and
/// `ISIK` are one (links final review I2).
pub fn key(s: &str) -> String {
    fold(s).replace('ı', "i")
}

/// `key` without the Turkish diacritics FTS5 also drops (ç ğ ş ö ü), for finding the matching line.
fn loose(s: &str) -> String {
    key(s).chars().map(|c| match c {
        'ç' => 'c',
        'ğ' => 'g',
        'ş' => 's',
        'ö' => 'o',
        'ü' => 'u',
        'â' => 'a',
        'î' => 'i',
        'û' => 'u',
        c => c,
    }).collect()
}

/// How a link target or note name is compared: lower-case, `/` separators, no `.md`, no `#heading`.
pub fn normalize_name(name: &str) -> String {
    let name = name.split('#').next().unwrap_or_default().trim().replace('\\', "/");
    key(strip_md(name.trim()).trim())
}

/// `name` without a final `.md` in any case.
pub fn strip_md(name: &str) -> &str {
    match name.len().checked_sub(3) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".md") => &name[..at],
        _ => name,
    }
}

/// The frontmatter's lines (without the `---` fences) and the index of the first body line, or
/// `None` when there is no frontmatter or it never closes (FR-EDT-007: then there is none).
fn split_frontmatter<'a>(lines: &[&'a str]) -> Option<(Vec<&'a str>, usize)> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return None;
    }
    let end = lines.iter().enumerate().skip(1).find(|(_, l)| matches!(l.trim_end(), "---" | "..."))?.0;
    Some((lines[1..end].to_vec(), end + 1))
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// `text` with inline code spans blanked out, so their `#` and `[[` do not count.
fn without_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for c in line.chars() {
        if c == '`' {
            in_code = !in_code;
            out.push(' ');
        } else {
            out.push(if in_code { ' ' } else { c });
        }
    }
    out
}

fn tags_in(line: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let after_word = i > 0 && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '#' | '&' | '/'));
        if chars[i] != '#' || after_word {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < chars.len() && (chars[end].is_alphanumeric() || matches!(chars[end], '_' | '-' | '/')) {
            end += 1;
        }
        let tag: String = chars[start..end].iter().collect::<String>().trim_end_matches('/').to_owned();
        let valid = !tag.is_empty() && !tag.starts_with('/') && !tag.chars().all(|c| c.is_ascii_digit());
        if valid {
            push_unique(out, fold(&tag));
        }
        i = end.max(i + 1);
    }
}

/// Up to `CONTEXT` characters each side of a link: a megabyte line is not stored per link (I5).
const CONTEXT: usize = 100;

fn links_in(line: &str, number: usize, original: &str, out: &mut Vec<Link>) {
    let original: Vec<char> = original.trim_end().chars().collect();
    let mut rest = line;
    let mut offset = 0;
    while let Some(open) = rest.find("[[") {
        let Some(close) = rest[open + 2..].find("]]") else { break };
        let inner = rest[open + 2..open + 2 + close].replace("\\|", "|"); // a pipe escaped in a table
        let start = offset + open;
        let embed = line[..start].ends_with('!');
        let target = normalize_name(inner.split('|').next().unwrap_or_default());
        if !embed && !target.is_empty() {
            // `line` and `original` have the same characters (code spans are blanked one for one)
            let at = line[..start].chars().count();
            let end = at + rest[open..open + 2 + close + 2].chars().count();
            let from = at.saturating_sub(CONTEXT);
            let to = (end + CONTEXT).min(original.len());
            let mut text: String = original[from..to].iter().collect();
            if from > 0 {
                text.insert(0, '…');
            }
            if to < original.len() {
                text.push('…');
            }
            out.push(Link { target, line: number, line_text: text });
        }
        let used = open + 2 + close + 2;
        offset += used;
        rest = &rest[used..];
    }
}

pub fn parse_note(rel: &str, text: &str) -> ParsedNote {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let title = std::path::Path::new(rel).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let body_start = split_frontmatter(&lines).map_or(0, |(_, start)| start);
    // FR-EDT-006/007: the same reading as the properties strip; invalid YAML gives none (its lines
    // are still not the body)
    let front = crate::media::read_frontmatter(text);
    let aliases = front.aliases;
    let mut tags: Vec<String> = Vec::new();
    for t in front.tags {
        push_unique(&mut tags, fold(&t));
    }
    let mut links = Vec::new();
    let mut fence: Option<&str> = None; // the marker that opened the code block
    for (i, line) in lines.iter().enumerate().skip(body_start) {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (None, Some(m)) => {
                fence = Some(m);
                continue;
            }
            (Some(open), Some(m)) if open == m => {
                fence = None;
                continue;
            }
            (Some(_), _) => continue,
            (None, None) => {}
        }
        let plain = without_inline_code(line);
        tags_in(&plain, &mut tags);
        links_in(&plain, i + 1, line, &mut links);
    }
    ParsedNote { title, aliases, tags, links, body: lines[body_start..].join("\n") }
}

// ---------------------------------------------------------------- storage

/// Puts one note into the index, replacing what was there (one transaction).
pub fn index_note(conn: &Connection, rel: &str, text: &str, mtime: i64, size: i64) -> rusqlite::Result<()> {
    let note = parse_note(rel, text);
    let tag_keys: Vec<String> = note.tags.iter().map(|t| key(t)).collect();
    let tx = conn.unchecked_transaction()?;
    delete_rows(&tx, rel)?;
    tx.execute(
        "INSERT INTO note_index (note_path, title, aliases, tags, mtime, size, content_hash, tag_keys) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            rel,
            note.title,
            serde_json::to_string(&note.aliases).expect("strings serialize"),
            serde_json::to_string(&note.tags).expect("strings serialize"),
            mtime,
            size,
            content_hash(text.as_bytes()),
            serde_json::to_string(&tag_keys).expect("strings serialize")
        ],
    )?;
    tx.execute("INSERT INTO note_fts (note_path, title, body) VALUES (?1, ?2, ?3)", params![rel, note.title, note.body])?;
    for link in &note.links {
        tx.execute("INSERT INTO link (source_path, target_path, line_text, line) VALUES (?1, ?2, ?3, ?4)", params![rel, link.target, link.line_text, link.line as i64])?;
    }
    tx.commit()
}

fn delete_rows(conn: &Connection, rel: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM note_index WHERE note_path = ?1", [rel])?;
    conn.execute("DELETE FROM note_fts WHERE note_path = ?1", [rel])?;
    conn.execute("DELETE FROM link WHERE source_path = ?1", [rel])?;
    Ok(())
}

pub fn remove_note(conn: &Connection, rel: &str) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    delete_rows(&tx, rel)?;
    tx.commit()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub indexed: usize,
    pub unchanged: usize,
    pub removed: usize,
    /// Not UTF-8 or not readable: left out of the index.
    pub unreadable: usize,
}

fn is_hidden(rel: &str) -> bool {
    rel.split('/').any(|part| part.starts_with('.'))
}

/// Every `.md` file of the vault outside hidden folders, as `/`-separated relative paths. Links and
/// junctions are not followed.
fn markdown_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(kind) = e.file_type() else { continue };
            if name.starts_with('.') || kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(e.path());
            } else if name.to_ascii_lowercase().ends_with(".md") {
                if let Ok(rel) = e.path().strip_prefix(root) {
                    out.push(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
                }
            }
        }
    }
    out.sort();
    out
}

/// A note for `target` straight from the disk, for when the index does not know it yet (at the
/// first open, during a long sync, or a note made a moment ago): path, then name; shortest wins.
pub fn find_on_disk(root: &Path, target: &str) -> Option<String> {
    let wanted = normalize_name(target);
    let name = wanted.rsplit('/').next().unwrap_or_default().to_owned();
    if wanted.is_empty() {
        return None;
    }
    let files = markdown_files(root);
    let shortest = |hits: Vec<&String>| hits.into_iter().min_by_key(|p| (p.len(), (*p).clone())).cloned();
    shortest(files.iter().filter(|p| normalize_name(p) == wanted).collect()).or_else(|| {
        shortest(files.iter().filter(|p| normalize_name(p).rsplit('/').next() == Some(name.as_str()) && !wanted.contains('/')).collect())
    })
}

fn stamp(meta: &std::fs::Metadata) -> (i64, i64) {
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
    (mtime, meta.len() as i64)
}

enum Refresh {
    Indexed,
    Unchanged,
    Removed,
    Unreadable,
}

/// Re-reads one note if its size, time or content changed; removes it when it is gone or unreadable.
fn refresh(conn: &Connection, root: &Path, rel: &str, known: Option<(i64, i64, String)>) -> rusqlite::Result<Refresh> {
    let path = root.join(rel);
    let Ok(meta) = std::fs::metadata(&path).map_err(drop).and_then(|m| if m.is_file() { Ok(m) } else { Err(()) }) else {
        if known.is_some() {
            remove_note(conn, rel)?;
            return Ok(Refresh::Removed);
        }
        return Ok(Refresh::Unchanged);
    };
    let (mtime, size) = stamp(&meta);
    if known.as_ref().is_some_and(|(m, s, _)| *m == mtime && *s == size) {
        return Ok(Refresh::Unchanged);
    }
    let Ok(text) = std::fs::read(&path).map_err(drop).and_then(|b| String::from_utf8(b).map_err(drop)) else {
        if known.is_some() {
            remove_note(conn, rel)?;
        }
        return Ok(Refresh::Unreadable);
    };
    if known.as_ref().is_some_and(|(_, _, hash)| *hash == content_hash(text.as_bytes())) {
        conn.execute("UPDATE note_index SET mtime = ?2, size = ?3 WHERE note_path = ?1", params![rel, mtime, size])?;
        return Ok(Refresh::Unchanged);
    }
    index_note(conn, rel, &text, mtime, size)?;
    Ok(Refresh::Indexed)
}

fn known_stamp(conn: &Connection, rel: &str) -> rusqlite::Result<Option<(i64, i64, String)>> {
    conn.query_row("SELECT mtime, size, content_hash FROM note_index WHERE note_path = ?1", [rel], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()
}

/// At vault open: re-reads what changed since the last run and drops what is gone (Review Focus 3).
pub fn sync_vault(conn: &Connection, root: &Path) -> rusqlite::Result<SyncReport> {
    sync_vault_until(conn, root, &AtomicBool::new(false))
}

/// `sync_vault` that stops between two notes once `stop` is set (a vault switch must not wait, I4).
pub fn sync_vault_until(conn: &Connection, root: &Path, stop: &AtomicBool) -> rusqlite::Result<SyncReport> {
    let mut report = SyncReport::default();
    if stop.load(Ordering::SeqCst) {
        return Ok(report);
    }
    let files = markdown_files(root);
    let indexed: Vec<String> = conn.prepare("SELECT note_path FROM note_index")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for gone in indexed.iter().filter(|p| files.binary_search(p).is_err()) {
        remove_note(conn, gone)?;
        report.removed += 1;
    }
    for rel in &files {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match refresh(conn, root, rel, known_stamp(conn, rel)?)? {
            Refresh::Indexed => report.indexed += 1,
            Refresh::Unchanged => report.unchanged += 1,
            Refresh::Removed => report.removed += 1,
            Refresh::Unreadable => report.unreadable += 1,
        }
    }
    Ok(report)
}

/// A watcher event for one path: re-index, or remove when it is gone. Returns whether the index changed.
pub fn touch(conn: &Connection, root: &Path, rel: &str) -> rusqlite::Result<bool> {
    if is_hidden(rel) || !rel.to_ascii_lowercase().ends_with(".md") {
        return Ok(false);
    }
    Ok(matches!(refresh(conn, root, rel, known_stamp(conn, rel)?)?, Refresh::Indexed | Refresh::Removed))
}

// ---------------------------------------------------------------- queries

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    pub note_path: String,
    pub title: String,
    /// Matched words between U+0002 and U+0003; plain text otherwise (Review Focus 5).
    pub snippet: String,
    /// The first line holding a searched word, to select when the note opens.
    pub line_text: Option<String>,
}

/// User text as an FTS5 query: every word quoted (so no operator survives), the last one a prefix.
fn fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(fold).collect();
    let last = words.len().checked_sub(1)?;
    let groups: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(n, w)| {
            let star = if n == last { "*" } else { "" };
            let variants: Vec<String> = i_variants(w).into_iter().map(|v| format!("\"{}\"{star}", v.replace('"', "\"\""))).collect();
            format!("({})", variants.join(" OR "))
        })
        .collect();
    Some(groups.join(" AND "))
}

/// The spellings of `word` with each `i` or `ı` either way: FTS5 keeps `ı` apart from `i`, but a
/// Turkish word may be written with an `I` that folds to `i` (`Işık` → `işık`). At most 16.
fn i_variants(word: &str) -> Vec<String> {
    let positions: Vec<usize> = word.char_indices().filter(|(_, c)| matches!(c, 'i' | 'ı')).map(|(i, _)| i).take(4).collect();
    let mut out: Vec<String> = Vec::new();
    for mask in 0..(1u32 << positions.len()) {
        let mut v = String::with_capacity(word.len());
        for (i, c) in word.char_indices() {
            match positions.iter().position(|p| *p == i) {
                Some(bit) => v.push(if mask & (1 << bit) == 0 { 'i' } else { 'ı' }),
                None => v.push(c),
            }
        }
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// SQL that is true when `folder` is the note's folder or one above it (no LIKE wildcards involved).
const IN_FOLDER: &str = "(?2 IS NULL OR substr(n.note_path, 1, length(?2) + 1) = ?2 || '/')";
const HAS_TAG: &str = "(?3 IS NULL OR EXISTS (SELECT 1 FROM json_each(n.tag_keys) WHERE value = ?3 OR substr(value, 1, length(?3) + 1) = ?3 || '/'))";

/// FR-EDT-014/015: ranked full-text results; `folder` and `tag` narrow them. A tag alone lists its notes.
/// A note's text for the assistant's context (FR-MEM-009).
#[derive(Debug, Clone, PartialEq)]
pub struct NoteText {
    pub note_path: String,
    pub title: String,
    pub body: String,
    /// Last change, milliseconds since the epoch.
    pub mtime: i64,
}

/// Words that say nothing about what is asked (Turkish and English question words and fillers).
const STOPWORDS: &[&str] = &[
    "ben", "bana", "beni", "benim", "bir", "bu", "şu", "ve", "ile", "için", "ama", "gibi", "kadar", "daha", "çok", "mi", "mı", "mu", "mü", "ne",
    "neler", "nedir", "nasıl", "neden", "niye", "hangi", "kaç", "var", "yok", "olan", "oldu", "olarak", "da", "de", "ki", "the", "and", "for",
    "what", "which", "how", "why", "when", "where", "who", "did", "does", "was", "were", "are", "have", "has", "with", "about", "from", "that",
    "this", "you", "your", "my", "me", "can", "will",
];

/// The question's words for retrieval: folded, at least three letters, no filler words.
fn retrieval_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in text.split(|c: char| !(c.is_alphanumeric() || c == '_')).map(fold) {
        if w.chars().count() >= 3 && !STOPWORDS.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// FR-MEM-005 (keyword half): notes sharing any of the question's words, best first. Unlike
/// `search`, a note does not need every word (a question is not a search box).
pub fn retrieve(conn: &Connection, question: &str, limit: usize) -> rusqlite::Result<Vec<NoteText>> {
    let words = retrieval_words(question);
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let query = words
        .iter()
        .map(|w| {
            let variants: Vec<String> = i_variants(w).into_iter().map(|v| format!("\"{}\"*", v.replace('"', "\"\""))).collect();
            format!("({})", variants.join(" OR "))
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    conn.prepare(
        "SELECT f.note_path, n.title, f.body, n.mtime FROM note_fts f JOIN note_index n ON n.note_path = f.note_path
         WHERE note_fts MATCH ?1 ORDER BY bm25(note_fts, 0.0, 8.0, 1.0) LIMIT ?2",
    )?
    .query_map(params![query, limit as i64], |r| Ok(NoteText { note_path: r.get(0)?, title: r.get(1)?, body: r.get(2)?, mtime: r.get(3)? }))?
    .collect()
}

/// FR-MEM-009: the notes changed since `since_ms` (milliseconds since the epoch), newest first.
pub fn recent(conn: &Connection, since_ms: i64, limit: usize) -> rusqlite::Result<Vec<NoteText>> {
    conn.prepare(
        "SELECT n.note_path, n.title, f.body, n.mtime FROM note_index n JOIN note_fts f ON f.note_path = n.note_path
         WHERE n.mtime >= ?1 ORDER BY n.mtime DESC LIMIT ?2",
    )?
    .query_map(params![since_ms, limit as i64], |r| Ok(NoteText { note_path: r.get(0)?, title: r.get(1)?, body: r.get(2)?, mtime: r.get(3)? }))?
    .collect()
}

pub fn search(conn: &Connection, text: &str, folder: Option<&str>, tag: Option<&str>, limit: usize) -> rusqlite::Result<Vec<SearchHit>> {
    let folder = folder.map(|f| f.trim_matches('/').to_owned()).filter(|f| !f.is_empty());
    let tag = tag.map(|t| key(t.trim().trim_start_matches('#'))).filter(|t| !t.is_empty());
    let words: Vec<String> = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(loose).collect();
    let rows: Vec<(String, String, String, String)> = match fts_query(text) {
        Some(query) => conn
            .prepare(&format!(
                "SELECT f.note_path, n.title, snippet(note_fts, 2, char(2), char(3), '…', 12), f.body
                 FROM note_fts f JOIN note_index n ON n.note_path = f.note_path
                 WHERE note_fts MATCH ?1 AND {IN_FOLDER} AND {HAS_TAG}
                 ORDER BY bm25(note_fts, 0.0, 8.0, 1.0) LIMIT ?4"
            ))?
            .query_map(params![query, folder, tag, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?,
        None if tag.is_some() || folder.is_some() => conn
            .prepare(&format!(
                "SELECT n.note_path, n.title, substr(f.body, 1, 120), f.body
                 FROM note_index n JOIN note_fts f ON f.note_path = n.note_path
                 WHERE ?1 IS NULL AND {IN_FOLDER} AND {HAS_TAG} ORDER BY n.mtime DESC LIMIT ?4"
            ))?
            .query_map(params![None::<String>, folder, tag, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    // The line to select: one holding a word that starts like a searched one (M2), loosely compared.
    let matches_line = |l: &str| {
        l.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(loose).any(|w| words.iter().any(|q| w.starts_with(q.as_str())))
    };
    Ok(rows
        .into_iter()
        .map(|(note_path, title, snippet, body)| {
            let line_text = body.lines().find(|l| matches_line(l)).map(str::to_owned);
            SearchHit { note_path, title, snippet, line_text }
        })
        .collect())
}

/// Every note's path, title and aliases: what links resolve against and quick open searches.
/// A note as links and quick open see it: path, title, aliases, last change.
type Name = (String, String, Vec<String>, i64);

fn names(conn: &Connection) -> rusqlite::Result<Vec<Name>> {
    conn.prepare("SELECT note_path, title, aliases, mtime FROM note_index")?
        .query_map([], |r| {
            let aliases: String = r.get(2)?;
            Ok((r.get(0)?, r.get(1)?, serde_json::from_str(&aliases).unwrap_or_default(), r.get(3)?))
        })?
        .collect()
}

/// FR-EDT-008: like Obsidian, the exact vault path first, then the file name, then an alias; among
/// several notes of one name the shortest path wins.
pub fn resolve_link(conn: &Connection, target: &str) -> rusqlite::Result<Option<String>> {
    let target = normalize_name(target);
    if target.is_empty() {
        return Ok(None);
    }
    let all = names(conn)?;
    let pick = |hits: Vec<&String>| hits.into_iter().min_by_key(|p| (p.len(), (*p).clone())).cloned();
    if let Some(p) = pick(all.iter().map(|n| &n.0).filter(|p| normalize_name(p) == target).collect()) {
        return Ok(Some(p));
    }
    if let Some(p) = pick(all.iter().filter(|n| key(&n.1) == target).map(|n| &n.0).collect()) {
        return Ok(Some(p));
    }
    Ok(pick(all.iter().filter(|n| n.2.iter().any(|a| key(a) == target)).map(|n| &n.0).collect()))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Backlink {
    pub source_path: String,
    pub source_title: String,
    pub line: usize,
    pub line_text: String,
}

/// FR-EDT-011: every line of another note whose link resolves to `rel`.
pub fn backlinks(conn: &Connection, rel: &str) -> rusqlite::Result<Vec<Backlink>> {
    let Some((_, title, aliases, _)) = names(conn)?.into_iter().find(|n| n.0 == rel) else { return Ok(Vec::new()) };
    let mut candidates = vec![normalize_name(rel), key(&title)];
    candidates.extend(aliases.iter().map(|a| key(a)));
    let mut out = Vec::new();
    let mut resolved: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT l.source_path, COALESCE(n.title, ''), l.line, COALESCE(l.line_text, ''), l.target_path
         FROM link l LEFT JOIN note_index n ON n.note_path = l.source_path
         WHERE l.target_path = ?1 AND l.source_path <> ?2 ORDER BY l.source_path, l.line",
    )?;
    for name in candidates.iter().collect::<std::collections::BTreeSet<_>>() {
        let rows = stmt.query_map(params![name, rel], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)))?;
        for row in rows {
            let (source_path, source_title, line, line_text, target) = row?;
            let goes_to = match resolved.get(&target) {
                Some(p) => p.clone(),
                None => {
                    let p = resolve_link(conn, &target)?;
                    resolved.insert(target, p.clone());
                    p
                }
            };
            if goes_to.as_deref() == Some(rel) {
                out.push(Backlink { source_path, source_title, line: line as usize, line_text });
            }
        }
    }
    out.sort_by(|a, b| (a.source_path.as_str(), a.line).cmp(&(b.source_path.as_str(), b.line)));
    out.dedup();
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuickHit {
    pub note_path: String,
    pub title: String,
    /// The alias that matched, when it was an alias.
    pub alias: Option<String>,
}

/// How well `query` matches `name`: starts with it, contains it, or has its letters in order.
fn fuzzy(query: &str, name: &str) -> Option<f64> {
    let name = fold(name);
    if name.starts_with(query) {
        return Some(3.0 + query.len() as f64 / name.len().max(1) as f64);
    }
    if name.contains(query) {
        return Some(2.0 + query.len() as f64 / name.len().max(1) as f64);
    }
    let mut letters = name.chars();
    query.chars().all(|q| letters.any(|c| c == q)).then(|| 1.0 + query.chars().count() as f64 / name.chars().count().max(1) as f64)
}

/// FR-EDT-005: notes by title, alias or path; with nothing typed, the latest changed.
pub fn quick_open(conn: &Connection, text: &str, limit: usize) -> rusqlite::Result<Vec<QuickHit>> {
    let query = fold(text.trim());
    let mut all = names(conn)?;
    if query.is_empty() {
        all.sort_by(|a, b| b.3.cmp(&a.3).then(a.0.cmp(&b.0)));
        return Ok(all.into_iter().take(limit).map(|(note_path, title, _, _)| QuickHit { note_path, title, alias: None }).collect());
    }
    let mut scored: Vec<(f64, QuickHit)> = all
        .into_iter()
        .filter_map(|(note_path, title, aliases, _)| {
            let by_title = fuzzy(&query, &title).map(|s| (s, None));
            let by_alias = aliases.iter().filter_map(|a| fuzzy(&query, a).map(|s| (s * 0.95, Some(a.clone())))).max_by(|a, b| a.0.total_cmp(&b.0));
            let by_path = fuzzy(&query, &note_path).map(|s| (s * 0.8, None));
            let (score, alias) = [by_title, by_alias, by_path].into_iter().flatten().max_by(|a, b| a.0.total_cmp(&b.0))?;
            Some((score, QuickHit { note_path, title, alias }))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.note_path.len().cmp(&b.1.note_path.len())).then(a.1.note_path.cmp(&b.1.note_path)));
    Ok(scored.into_iter().take(limit).map(|(_, h)| h).collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub count: usize,
}

/// FR-EDT-012: every tag with the number of notes that carry it, alphabetical.
pub fn list_tags(conn: &Connection) -> rusqlite::Result<Vec<TagCount>> {
    // Counted by key, shown as first written (`#Işık` and `#ışık` are one tag).
    let mut counts: std::collections::BTreeMap<String, (String, usize)> = std::collections::BTreeMap::new();
    let lists: Vec<String> = conn.prepare("SELECT tags FROM note_index ORDER BY note_path")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for list in lists {
        let mut seen = std::collections::BTreeSet::new();
        for tag in serde_json::from_str::<Vec<String>>(&list).unwrap_or_default() {
            let k = key(&tag);
            if seen.insert(k.clone()) {
                counts.entry(k).or_insert_with(|| (tag, 0)).1 += 1;
            }
        }
    }
    Ok(counts.into_values().map(|(tag, count)| TagCount { tag, count }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_compared_without_case_extension_or_heading() {
        assert_eq!(normalize_name("Projeler\\PLA Planı.md#Hedefler"), "projeler/pla plani");
        assert_eq!(normalize_name("  Günlük  "), "günlük");
        assert_eq!(normalize_name("İstanbul"), "istanbul", "Turkish capitals lower-case like Obsidian's search");
    }

    #[test]
    fn titles_aliases_and_tags_come_from_the_file_name_and_frontmatter() {
        // FR-EDT-006/012
        let text = "---\naliases: [PLA, Asistan]\ntags:\n  - Proje\n  - proje/pla\n---\n# Başlık\n\nBugün #Çalışma ve #proje/pla notları. Renk #ff0000 değil, #123 de değil.\n";
        let n = parse_note("notes/Kişisel Asistan.md", text);
        assert_eq!(n.title, "Kişisel Asistan");
        assert_eq!(n.aliases, vec!["PLA", "Asistan"]);
        assert_eq!(n.tags, vec!["proje", "proje/pla", "çalışma", "ff0000"]);
        assert!(n.body.starts_with("# Başlık"), "frontmatter is not searched");
        let scalar = parse_note("a.md", "---\ntags: tek\naliases: Bir İsim\n---\nmetin");
        assert_eq!((scalar.tags, scalar.aliases), (vec!["tek".to_owned()], vec!["Bir İsim".to_owned()]));
    }

    #[test]
    fn broken_frontmatter_counts_as_none() {
        // FR-EDT-007
        let n = parse_note("a.md", "---\ntags: [açık\nbitmedi\n");
        assert!(n.tags.is_empty());
        assert!(n.body.starts_with("---"), "the whole text stays searchable");
    }

    #[test]
    fn tags_in_code_and_headings_are_not_tags() {
        let n = parse_note("a.md", "# Başlık\n## Alt\n`#kod` içinde\n```\n#blok\n```\nmetin#bitişik ve (#parantez)\n");
        assert_eq!(n.tags, vec!["parantez"]);
    }

    #[test]
    fn wikilinks_are_found_with_their_line_and_embeds_are_not_links() {
        // FR-EDT-008
        let text = "İlk satır [[Proje Planı]] ve [[notes/Fikirler|fikirler]].\n\n![[resim.png]]\nBaşlığa [[Günlük#Sabah]] ve [[ boş ]] ile [[]].\n";
        let n = parse_note("a.md", text);
        let found: Vec<(&str, usize)> = n.links.iter().map(|l| (l.target.as_str(), l.line)).collect();
        assert_eq!(found, vec![("proje plani", 1), ("notes/fikirler", 1), ("günlük", 4), ("boş", 4)]);
        assert_eq!(n.links[0].line_text, "İlk satır [[Proje Planı]] ve [[notes/Fikirler|fikirler]].");
    }

    fn db() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = crate::db::open_databases(&tmp.path().join(".data")).unwrap().cache;
        (tmp, conn)
    }

    fn put(conn: &Connection, rel: &str, text: &str) {
        index_note(conn, rel, text, 1, text.len() as i64).unwrap();
    }

    #[test]
    fn search_finds_turkish_words_ranks_titles_and_marks_snippets() {
        // FR-EDT-014, Review Focus 4/5
        let (_t, conn) = db();
        put(&conn, "notes/Dişçi.md", "Kontrol randevusu.");
        put(&conn, "daily/2026-10-05.md", "Sabah koşu.\nYarın Dişçi randevusu var <b>kalın</b>.\nAkşam kitap.");
        put(&conn, "notes/Başka.md", "Hiç ilgisi yok.");
        let hits = search(&conn, "dişçi", None, None, 20).unwrap();
        assert_eq!(hits.iter().map(|h| h.note_path.as_str()).collect::<Vec<_>>(), vec!["notes/Dişçi.md", "daily/2026-10-05.md"], "a title match ranks first");
        let daily = &hits[1];
        assert!(daily.snippet.contains("\u{2}Dişçi\u{3}"), "{:?}", daily.snippet);
        assert!(daily.snippet.contains("<b>kalın</b>"), "markup stays text; the UI never renders it as HTML");
        assert_eq!(daily.line_text.as_deref(), Some("Yarın Dişçi randevusu var <b>kalın</b>."), "the line to select on open");
        assert_eq!(search(&conn, "randev", None, None, 20).unwrap().len(), 2, "the last word is a prefix");
        assert_eq!(search(&conn, "dişçi kitap", None, None, 20).unwrap().len(), 1, "all words must match");
        assert!(search(&conn, "   ", None, None, 20).unwrap().is_empty());
    }

    #[test]
    fn any_typed_query_is_safe() {
        // Review Focus 2: FTS syntax in user input is plain text
        let (_t, conn) = db();
        put(&conn, "a.md", "a OR b AND (c) NEAR title:x \"tırnak\" * - ^");
        for q in ["\"", "a OR", "(c", "title:x", "*", "-", "NEAR(", "^", "\"tırnak", "a\"b", "'; DROP TABLE link; --"] {
            assert!(search(&conn, q, None, None, 20).is_ok(), "{q}");
        }
        assert_eq!(search(&conn, "tırnak", None, None, 20).unwrap().len(), 1);
    }

    #[test]
    fn search_filters_by_folder_and_by_tag_including_nested_tags() {
        // FR-EDT-015
        let (_t, conn) = db();
        put(&conn, "projeler/pla.md", "rapor #proje/pla");
        put(&conn, "projeler_eski/x.md", "rapor #proje");
        put(&conn, "notes/y.md", "rapor #kişisel");
        let paths = |hits: Vec<SearchHit>| hits.into_iter().map(|h| h.note_path).collect::<Vec<_>>();
        assert_eq!(paths(search(&conn, "rapor", Some("projeler"), None, 20).unwrap()), vec!["projeler/pla.md"], "a folder, not a name prefix");
        let mut by_tag = paths(search(&conn, "rapor", None, Some("proje"), 20).unwrap());
        by_tag.sort();
        assert_eq!(by_tag, vec!["projeler/pla.md", "projeler_eski/x.md"], "#proje includes #proje/pla");
        assert_eq!(paths(search(&conn, "rapor", None, Some("Kişisel"), 20).unwrap()), vec!["notes/y.md"]);
        assert_eq!(paths(search(&conn, "", None, Some("proje/pla"), 20).unwrap()), vec!["projeler/pla.md"], "a tag alone lists its notes");
    }

    #[test]
    fn a_vault_sync_reads_only_what_changed_and_skips_hidden_and_unreadable_notes() {
        // Review Focus 3
        let (tmp, conn) = db();
        let root = tmp.path().join("kasa");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::create_dir_all(root.join(".obsidian")).unwrap();
        std::fs::write(root.join("notes/a.md"), "elma").unwrap();
        std::fs::write(root.join("notes/b.md"), "armut").unwrap();
        std::fs::write(root.join(".obsidian/c.md"), "gizli").unwrap();
        std::fs::write(root.join("notes/eski.md"), b"Yar\xfdn").unwrap();
        let first = sync_vault(&conn, &root).unwrap();
        assert_eq!((first.indexed, first.removed, first.unreadable), (2, 0, 1));
        assert!(search(&conn, "gizli", None, None, 20).unwrap().is_empty());

        let again = sync_vault(&conn, &root).unwrap();
        assert_eq!((again.indexed, again.unchanged), (0, 2), "nothing changed, nothing re-read");

        std::fs::write(root.join("notes/a.md"), "elma ve kiraz").unwrap();
        std::fs::remove_file(root.join("notes/b.md")).unwrap();
        let later = sync_vault(&conn, &root).unwrap();
        assert_eq!((later.indexed, later.removed), (1, 1));
        assert_eq!(search(&conn, "kiraz", None, None, 20).unwrap().len(), 1);
        assert!(search(&conn, "armut", None, None, 20).unwrap().is_empty());

        assert!(!touch(&conn, &root, "notes/a.md").unwrap(), "unchanged content: no work");
        std::fs::remove_file(root.join("notes/a.md")).unwrap();
        assert!(touch(&conn, &root, "notes/a.md").unwrap());
        assert!(search(&conn, "kiraz", None, None, 20).unwrap().is_empty(), "a vanished note leaves the index");
        assert!(!touch(&conn, &root, ".obsidian/c.md").unwrap(), "hidden paths are never indexed");
    }

    #[test]
    fn quick_open_matches_titles_aliases_and_paths_loosely() {
        // FR-EDT-005
        let (_t, conn) = db();
        index_note(&conn, "notes/Proje Planı.md", "x", 30, 1).unwrap();
        index_note(&conn, "notes/Plan B.md", "x", 20, 1).unwrap();
        index_note(&conn, "daily/2026-10-05.md", "---\naliases: [Pazartesi]\n---\nx", 10, 1).unwrap();
        let names = |q: &str| quick_open(&conn, q, 10).unwrap().into_iter().map(|h| h.note_path).collect::<Vec<_>>();
        assert_eq!(names("plan")[0], "notes/Plan B.md", "a name that starts with it first");
        assert!(names("prjpln").contains(&"notes/Proje Planı.md".to_owned()), "letters in order");
        assert_eq!(names("pazar"), vec!["daily/2026-10-05.md"], "by alias");
        assert_eq!(quick_open(&conn, "pazar", 10).unwrap()[0].alias.as_deref(), Some("Pazartesi"));
        assert_eq!(names(""), vec!["notes/Proje Planı.md", "notes/Plan B.md", "daily/2026-10-05.md"], "nothing typed: the latest changed");
        assert!(names("zzzz").is_empty());
    }

    #[test]
    fn links_resolve_like_obsidian_and_backlinks_show_their_line() {
        // FR-EDT-008/011
        let (_t, conn) = db();
        put(&conn, "notes/PLA.md", "---\naliases: [Asistan]\n---\nana not");
        put(&conn, "projeler/eski/PLA.md", "başka PLA");
        put(&conn, "daily/a.md", "Bugün [[PLA]] üzerinde çalıştım.\nSonra [[Asistan|asistana]] baktım.");
        put(&conn, "daily/b.md", "[[projeler/eski/PLA]] arşivde.\n[[Yok Böyle Not]]");
        put(&conn, "notes/PLA2.md", "Kendine bağlantı [[PLA2]].");
        assert_eq!(resolve_link(&conn, "pla").unwrap().as_deref(), Some("notes/PLA.md"), "the shortest path wins a tie");
        assert_eq!(resolve_link(&conn, "Projeler/Eski/PLA.md").unwrap().as_deref(), Some("projeler/eski/PLA.md"));
        assert_eq!(resolve_link(&conn, "asistan").unwrap().as_deref(), Some("notes/PLA.md"), "by alias");
        assert_eq!(resolve_link(&conn, "Yok Böyle Not").unwrap(), None);
        let back = backlinks(&conn, "notes/PLA.md").unwrap();
        assert_eq!(back.iter().map(|b| (b.source_path.as_str(), b.line)).collect::<Vec<_>>(), vec![("daily/a.md", 1), ("daily/a.md", 2)]);
        assert_eq!(back[1].line_text, "Sonra [[Asistan|asistana]] baktım.");
        assert_eq!(backlinks(&conn, "projeler/eski/PLA.md").unwrap().iter().map(|b| b.source_path.as_str()).collect::<Vec<_>>(), vec!["daily/b.md"]);
        assert!(backlinks(&conn, "notes/PLA2.md").unwrap().is_empty(), "a note linking itself is not a backlink");
        remove_note(&conn, "daily/a.md").unwrap();
        assert!(backlinks(&conn, "notes/PLA.md").unwrap().is_empty());
    }

    #[test]
    fn the_tag_list_counts_notes_per_tag() {
        // FR-EDT-012
        let (_t, conn) = db();
        put(&conn, "a.md", "#proje #proje/pla");
        put(&conn, "b.md", "---\ntags: [proje]\n---\n#Proje tekrar");
        put(&conn, "c.md", "#okuma");
        let tags = list_tags(&conn).unwrap();
        assert_eq!(
            tags.iter().map(|t| (t.tag.as_str(), t.count)).collect::<Vec<_>>(),
            vec![("okuma", 1), ("proje", 2), ("proje/pla", 1)]
        );
    }

    #[test]
    fn turkish_dotted_and_dotless_i_match_in_search_tags_and_links() {
        // links final review I2, Review Focus 4
        let (_t, conn) = db();
        put(&conn, "notes/Işık.md", "Işık ILIK bir sabah. #Işık");
        put(&conn, "notes/b.md", "Bugün ışık güzeldi #ışık ve [[ışık]] notuna baktım.");
        for q in ["ışık", "işık", "ISIK", "Işık", "ılık", "ılı"] {
            assert!(!search(&conn, q, None, None, 20).unwrap().is_empty(), "{q}");
        }
        assert_eq!(search(&conn, "ışık", None, None, 20).unwrap().len(), 2);
        let tags = list_tags(&conn).unwrap();
        assert_eq!(tags.len(), 1, "#Işık and #ışık are one tag: {tags:?}");
        assert_eq!(tags[0].count, 2);
        assert_eq!(search(&conn, "", None, Some("IŞIK"), 20).unwrap().len(), 2);
        assert_eq!(resolve_link(&conn, "ışık").unwrap().as_deref(), Some("notes/Işık.md"));
        assert_eq!(backlinks(&conn, "notes/Işık.md").unwrap().len(), 1);
    }

    #[test]
    fn crlf_notes_parse_like_lf_notes() {
        let n = parse_note("a.md", "---\r\ntags: [x]\r\n---\r\nSatır [[B]]\r\n#etiket\r\n");
        assert_eq!(n.tags, vec!["x", "etiket"]);
        assert_eq!(n.links[0].line_text, "Satır [[B]]");
    }

    #[test]
    fn parsing_edge_cases_from_review() {
        // URL fragments, comma lists, YAML comments, escaped pipes, fences of the other kind
        let n = parse_note("a.md", "---\ntags: a, b # yorum\n---\nhttps://site.com/#/route\n| [[Tablo\\|takma]] |\n~~~\n```\n#kodda\n~~~\n");
        assert_eq!(n.tags, vec!["a", "b"]);
        assert_eq!(n.links.iter().map(|l| l.target.as_str()).collect::<Vec<_>>(), vec!["tablo"]);
    }

    #[test]
    fn a_link_keeps_only_the_text_around_it() {
        // links final review I5: a megabyte line must not be stored once per link
        let long = format!("{}[[A]]{}", "x".repeat(5000), "y".repeat(5000));
        let n = parse_note("a.md", &long);
        assert!(n.links[0].line_text.chars().count() <= 210, "{}", n.links[0].line_text.len());
        assert!(n.links[0].line_text.contains("[[A]]"));
    }

    #[test]
    fn a_note_on_disk_is_found_even_before_the_index_knows_it() {
        // links final review I1: the first open after an upgrade, or a large vault still syncing
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("notes/alt")).unwrap();
        std::fs::create_dir_all(root.join(".trash")).unwrap();
        std::fs::write(root.join("notes/alt/Proje Planı.md"), "x").unwrap();
        std::fs::write(root.join(".trash/Silinen.md"), "x").unwrap();
        assert_eq!(find_on_disk(root, "proje planı").as_deref(), Some("notes/alt/Proje Planı.md"));
        assert_eq!(find_on_disk(root, "Notes/Alt/Proje Planı.md").as_deref(), Some("notes/alt/Proje Planı.md"));
        assert_eq!(find_on_disk(root, "Silinen"), None, "hidden folders do not count");
        assert_eq!(find_on_disk(root, "Yok"), None);
    }

    #[test]
    fn a_sync_can_be_stopped_between_notes() {
        // links final review I4: switching vaults must not wait for a long first index
        let (tmp, conn) = db();
        let root = tmp.path().join("kasa");
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..20 {
            std::fs::write(root.join(format!("n{i}.md")), "x").unwrap();
        }
        let stop = std::sync::atomic::AtomicBool::new(true);
        let report = sync_vault_until(&conn, &root, &stop).unwrap();
        assert_eq!(report.indexed, 0, "stopped before the first note");
    }
}
