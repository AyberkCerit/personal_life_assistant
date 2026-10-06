//! The note index in `cache.db` (SRS §3.3, M3): titles, aliases and tags, wikilinks with their line,
//! and full-text search. Rebuildable from the vault at any time; it never changes a note.

use std::path::Path;

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

/// How a link target or note name is compared: lower-case, `/` separators, no `.md`, no `#heading`.
pub fn normalize_name(name: &str) -> String {
    let name = name.split('#').next().unwrap_or_default().trim().replace('\\', "/");
    let name = match name.len().checked_sub(3) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".md") => &name[..at],
        _ => &name[..],
    };
    fold(name.trim())
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

fn unquote(s: &str) -> String {
    s.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_owned()
}

/// The values of `key` (or its singular) in simple YAML: `[a, b]`, a block list, or one scalar.
fn yaml_list(front: &[&str], keys: &[&str]) -> Vec<String> {
    for (i, line) in front.iter().enumerate() {
        let Some((key, value)) = line.split_once(':') else { continue };
        if line.starts_with(char::is_whitespace) || !keys.contains(&key.trim()) {
            continue;
        }
        let value = value.trim();
        let items: Vec<String> = if let Some(inner) = value.strip_prefix('[') {
            let Some(inner) = inner.strip_suffix(']') else { return Vec::new() };
            inner.split(',').map(unquote).collect()
        } else if value.is_empty() {
            front[i + 1..]
                .iter()
                .take_while(|l| l.trim_start().starts_with("- ") || l.trim().is_empty())
                .filter_map(|l| l.trim_start().strip_prefix("- ").map(unquote))
                .collect()
        } else {
            vec![unquote(value)]
        };
        return items.into_iter().filter(|s| !s.is_empty()).collect();
    }
    Vec::new()
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
        let after_word = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '#' || chars[i - 1] == '&');
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
        if !tag.is_empty() && !tag.chars().all(|c| c.is_ascii_digit()) {
            push_unique(out, fold(&tag));
        }
        i = end.max(i + 1);
    }
}

fn links_in(line: &str, number: usize, original: &str, out: &mut Vec<Link>) {
    let mut rest = line;
    let mut offset = 0;
    while let Some(open) = rest.find("[[") {
        let Some(close) = rest[open + 2..].find("]]") else { break };
        let inner = &rest[open + 2..open + 2 + close];
        let embed = line[..offset + open].ends_with('!');
        let target = normalize_name(inner.split('|').next().unwrap_or_default());
        if !embed && !target.is_empty() {
            out.push(Link { target, line: number, line_text: original.trim_end().to_owned() });
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
    let (front, body_start) = split_frontmatter(&lines).unwrap_or((Vec::new(), 0));
    let aliases = yaml_list(&front, &["aliases", "alias"]);
    let mut tags: Vec<String> = Vec::new();
    for t in yaml_list(&front, &["tags", "tag"]) {
        let t = fold(t.trim_start_matches('#'));
        if !t.is_empty() {
            push_unique(&mut tags, t);
        }
    }
    let mut links = Vec::new();
    let mut fenced = false;
    for (i, line) in lines.iter().enumerate().skip(body_start) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
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
    let tx = conn.unchecked_transaction()?;
    delete_rows(&tx, rel)?;
    tx.execute(
        "INSERT INTO note_index (note_path, title, aliases, tags, mtime, size, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            rel,
            note.title,
            serde_json::to_string(&note.aliases).expect("strings serialize"),
            serde_json::to_string(&note.tags).expect("strings serialize"),
            mtime,
            size,
            content_hash(text.as_bytes())
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
    let mut report = SyncReport::default();
    let files = markdown_files(root);
    let indexed: Vec<String> = conn.prepare("SELECT note_path FROM note_index")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for gone in indexed.iter().filter(|p| files.binary_search(p).is_err()) {
        remove_note(conn, gone)?;
        report.removed += 1;
    }
    for rel in &files {
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
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect();
    let mut query = words.join(" ");
    if query.is_empty() {
        return None;
    }
    query.push('*');
    Some(query)
}

/// SQL that is true when `folder` is the note's folder or one above it (no LIKE wildcards involved).
const IN_FOLDER: &str = "(?2 IS NULL OR substr(n.note_path, 1, length(?2) + 1) = ?2 || '/')";
const HAS_TAG: &str = "(?3 IS NULL OR EXISTS (SELECT 1 FROM json_each(n.tags) WHERE value = ?3 OR substr(value, 1, length(?3) + 1) = ?3 || '/'))";

/// FR-EDT-014/015: ranked full-text results; `folder` and `tag` narrow them. A tag alone lists its notes.
pub fn search(conn: &Connection, text: &str, folder: Option<&str>, tag: Option<&str>, limit: usize) -> rusqlite::Result<Vec<SearchHit>> {
    let folder = folder.map(|f| f.trim_matches('/').to_owned()).filter(|f| !f.is_empty());
    let tag = tag.map(|t| fold(t.trim().trim_start_matches('#'))).filter(|t| !t.is_empty());
    let words: Vec<String> = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(fold).collect();
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
    Ok(rows
        .into_iter()
        .map(|(note_path, title, snippet, body)| {
            let line_text = body.lines().find(|l| {
                let l = fold(l);
                words.iter().any(|w| l.contains(w.as_str()))
            });
            SearchHit { note_path, title, snippet, line_text: line_text.map(str::to_owned) }
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
    if let Some(p) = pick(all.iter().filter(|n| fold(&n.1) == target).map(|n| &n.0).collect()) {
        return Ok(Some(p));
    }
    Ok(pick(all.iter().filter(|n| n.2.iter().any(|a| fold(a) == target)).map(|n| &n.0).collect()))
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
    let mut candidates = vec![normalize_name(rel), fold(&title)];
    candidates.extend(aliases.iter().map(|a| fold(a)));
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
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let lists: Vec<String> = conn.prepare("SELECT tags FROM note_index")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for list in lists {
        for tag in serde_json::from_str::<Vec<String>>(&list).unwrap_or_default() {
            *counts.entry(tag).or_default() += 1;
        }
    }
    Ok(counts.into_iter().map(|(tag, count)| TagCount { tag, count }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_compared_without_case_extension_or_heading() {
        assert_eq!(normalize_name("Projeler\\PLA Planı.md#Hedefler"), "projeler/pla planı");
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
        assert_eq!(found, vec![("proje planı", 1), ("notes/fikirler", 1), ("günlük", 4), ("boş", 4)]);
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
}
