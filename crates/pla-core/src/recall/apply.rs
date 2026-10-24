//! Writes memory changes into the files. The files stay the source: every write is rebuilt on what
//! is on disk right before it, and a line the user changed meanwhile is never touched.
//!
//! A topic is one across the active files. `Manual` lines are never replaced, moved or forgotten;
//! a said line replaces said and inferred ones; an inference replaces only an inference. A line
//! that leaves an active file goes to the archive under the heading of its reason, never away.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, FixedOffset, NaiveDate};
use rusqlite::{params, Connection};

use crate::fs_atomic::write_atomic;
use crate::index;
use crate::lang::Lang;
use crate::util::{content_hash, tokens};
use crate::vault::{walk_vault, Vault};

use super::format::{parse_doc, render, render_doc, topic_key, Block, Doc, Entry, Kind, Role};
use super::load::PEOPLE_TOKENS;

/// `Forget` reasons of the State inference: the note a line came from changed or went away. Such
/// a forget drops only inferences, as outdated; the user rejected nothing.
pub const SOURCE_CHANGED: &str = "source changed";
pub const SOURCE_DELETED: &str = "source deleted";

/// How often a write is rebuilt because a file changed under it before PLA gives up.
const ATTEMPTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub text: String,
    pub topic: String,
    pub role: Role,
    /// The `## Name` section, required for `Role::People`.
    pub person: Option<String>,
    pub kind: Kind,
    /// `path/to/note.md#blockid`, required for an inference.
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add(Proposal),
    Forget { topic: String, reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum RecallError {
    #[error("memory: cannot read {file}: {error}")]
    Unreadable { file: String, error: std::io::Error },
    #[error("memory: {file} kept changing while PLA wrote to it")]
    Busy { file: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// Writes `changes` into the memory files and records them in `memory_entry`. Returns the new
/// lines written; `wrote` gets the vault path of every file written (for `self_writes`). An
/// unreadable memory file stops the whole write: nothing is overwritten.
pub fn apply(vault: &Vault, db: &Connection, changes: &[Change], now: DateTime<FixedOffset>, lang: Lang, wrote: &dyn Fn(&str)) -> Result<Vec<Entry>, RecallError> {
    apply_with(vault, db, changes, now, lang, wrote, &mut || {})
}

/// `between` runs after the new text is built and before the files are read again (tests edit a
/// file there, as the user would in Obsidian).
fn apply_with(
    vault: &Vault,
    db: &Connection,
    changes: &[Change],
    now: DateTime<FixedOffset>,
    lang: Lang,
    wrote: &dyn Fn(&str),
    between: &mut dyn FnMut(),
) -> Result<Vec<Entry>, RecallError> {
    let dir = vault.root.join(&vault.config.folders.memory);
    let mut known: HashSet<String> = HashSet::new();
    {
        let mut st = db.prepare("SELECT id FROM memory_entry")?;
        for id in st.query_map([], |r| r.get::<_, String>(0))? {
            known.insert(id?);
        }
    }
    let mut first: Option<HashMap<String, Entry>> = None;
    let mut busy = String::new();
    for _ in 0..ATTEMPTS {
        let files = read_files(&dir)?;
        let before = fingerprint(&files);
        let seen: HashMap<String, Entry> = files.iter().flat_map(|f| f.doc.entries()).map(|e| (e.id.clone(), e)).collect();
        // A line that differs from the first read was changed by the user while PLA worked.
        let pinned: HashSet<String> = match &first {
            Some(old) => seen.iter().filter(|(id, e)| old.get(*id).is_some_and(|o| o != *e)).map(|(id, _)| id.clone()).collect(),
            None => HashSet::new(),
        };
        first.get_or_insert(seen);
        let mut plan = Plan { files, pinned: &pinned, vault, lang, today: now.date_naive(), stamp: now.to_rfc3339(), known: known.clone(), out: Outcome::default() };
        for c in changes {
            match c {
                Change::Add(p) => plan.add(p),
                Change::Forget { topic, reason } => plan.forget(topic, reason),
            }
        }
        plan.compact();
        between();
        let after = fingerprint(&read_files(&dir)?);
        if after != before {
            busy = after.iter().find(|f| !before.contains(f)).or_else(|| before.iter().find(|f| !after.contains(f))).map(|f| f.0.clone()).unwrap_or_default();
            continue;
        }
        return plan.write(&dir, db, wrote);
    }
    Err(RecallError::Busy { file: busy })
}

/// Brings `memory_entry` in line with the files: a PLA line missing from every file was deleted
/// by the user and becomes `rejected`; one PLA moved to the archive is `archived`, not rejected.
/// An unreadable file stops it, so its lines are not taken for deleted ones.
pub fn reconcile(vault: &Vault, db: &Connection) -> Result<(), RecallError> {
    let mut files = read_files(&vault.root.join(&vault.config.folders.memory))?;
    // Active files first: a line copied back from the archive is active again.
    files.sort_by_key(|f| f.role == Role::Archive);
    let mut known: HashMap<String, (String, String)> = HashMap::new();
    {
        let mut st = db.prepare("SELECT id, state, text FROM memory_entry")?;
        for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (id, state, text) = row?;
            known.insert(id, (state, text));
        }
    }
    let at = chrono::Local::now().fixed_offset().to_rfc3339();
    crate::db::atomically(db, "recall_reconcile", || -> Result<(), RecallError> {
        let mut present: HashSet<String> = HashSet::new();
        for f in &files {
            for e in f.doc.entries().into_iter().filter(|e| e.kind != Kind::Manual) {
                if !present.insert(e.id.clone()) {
                    continue;
                }
                let row = known.get(&e.id);
                if f.role != Role::Archive {
                    if row.is_none_or(|(state, text)| state != "active" || *text != e.text) {
                        upsert(db, &e, f.role, "active", None, &at)?;
                    }
                } else {
                    match row.map(|(state, _)| state.as_str()) {
                        None => upsert(db, &e, Role::Archive, "archived", None, &at)?,
                        Some("active") => {
                            db.execute("UPDATE memory_entry SET state = 'archived', changed_at = ?2 WHERE id = ?1", params![e.id, at])?;
                        }
                        Some(_) => {}
                    }
                }
            }
        }
        for (id, (state, _)) in &known {
            if state == "active" && !present.contains(id) {
                db.execute("UPDATE memory_entry SET state = 'rejected', reason = 'deleted by the user', changed_at = ?2 WHERE id = ?1", params![id, at])?;
            }
        }
        Ok(())
    })
}

/// `(topic, text)` of every rejected line, oldest first: what the writers must not write again.
pub fn rejected(db: &Connection) -> Vec<(String, String)> {
    let Ok(mut st) = db.prepare("SELECT topic, text FROM memory_entry WHERE state = 'rejected' ORDER BY changed_at, id") else { return Vec::new() };
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
}

/// The visible-text limit of a role file in characters; People sections are held to tokens.
fn char_limit(role: Role) -> Option<usize> {
    match role {
        Role::About | Role::State => Some(800),
        Role::Rules => Some(600),
        Role::People | Role::Archive => None,
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::About => "about",
        Role::Rules => "rules",
        Role::State => "state",
        Role::People => "people",
        Role::Archive => "archive",
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Said => "said",
        Kind::Inferred => "inferred",
        Kind::Manual => "manual",
    }
}

fn upsert(db: &Connection, e: &Entry, role: Role, state: &str, reason: Option<&str>, at: &str) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO memory_entry (id, topic, role, kind, text, state, reason, changed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET topic = excluded.topic, role = excluded.role, kind = excluded.kind, text = excluded.text,
           state = excluded.state, reason = excluded.reason, changed_at = excluded.changed_at",
        params![e.id, e.topic, role_name(role), kind_name(e.kind), e.text, state, reason, at],
    )?;
    Ok(())
}

/// Why a line left an active file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    Outdated,
    Overflow,
    Rejected,
}

impl Why {
    fn heading(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Why::Outdated, Lang::Tr) => "Eskiyenler",
            (Why::Overflow, Lang::Tr) => "Taşanlar",
            (Why::Rejected, Lang::Tr) => "Reddedilenler",
            (Why::Outdated, Lang::En) => "Outdated",
            (Why::Overflow, Lang::En) => "Over the limit",
            (Why::Rejected, Lang::En) => "Rejected",
        }
    }

    /// The indented note under the archived line.
    fn note(self, lang: Lang, reason: &str) -> String {
        match (self, lang) {
            (Why::Outdated, Lang::Tr) => "Eskidi".to_string(),
            (Why::Overflow, Lang::Tr) => "Sınırdan taştı".to_string(),
            (Why::Rejected, Lang::Tr) => format!("Reddedildi: {reason}"),
            (Why::Outdated, Lang::En) => "Outdated".to_string(),
            (Why::Overflow, Lang::En) => "Over the limit".to_string(),
            (Why::Rejected, Lang::En) => format!("Rejected: {reason}"),
        }
    }

    fn state(self) -> &'static str {
        if self == Why::Rejected { "rejected" } else { "archived" }
    }
}

/// A memory file as read; `hash` is `None` for one PLA is about to create.
struct File {
    role: Role,
    name: String,
    hash: Option<String>,
    doc: Doc,
    dirty: bool,
}

/// The role files of the memory folder; none when it does not exist yet.
fn read_files(dir: &Path) -> Result<Vec<File>, RecallError> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(RecallError::Unreadable { file: dir.display().to_string(), error }),
    };
    let mut files = Vec::new();
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(role) = Role::from_file_name(&name).filter(|_| name.to_lowercase().ends_with(".md")) else { continue };
        if !e.path().is_file() {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).map_err(|error| RecallError::Unreadable { file: name.clone(), error })?;
        files.push(File { role, hash: Some(content_hash(text.as_bytes())), doc: parse_doc(role, &text), name, dirty: false });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn fingerprint(files: &[File]) -> Vec<(String, String)> {
    files.iter().filter_map(|f| f.hash.clone().map(|h| (f.name.clone(), h))).collect()
}

/// The topic as stored: folded, and for a person prefixed with their name, so two people's
/// "kahve" stay two topics.
fn stored_topic(role: Role, topic: &str, person: Option<&str>) -> String {
    let key = topic_key(topic);
    match (role, person) {
        (Role::People, Some(p)) => {
            let pk = topic_key(p);
            if key == pk || key.starts_with(&format!("{pk}-")) { key } else { format!("{pk}-{key}") }
        }
        _ => key,
    }
}

fn is_heading(b: &Block) -> bool {
    match b {
        Block::Person { .. } => true,
        Block::Raw(l) => l.starts_with("# ") || l.starts_with("## "),
        Block::Entry(_) => false,
    }
}

fn is_blank(b: Option<&Block>) -> bool {
    matches!(b, Some(Block::Raw(l)) if l.trim().is_empty())
}

/// Puts `entry` after the last line of the section whose heading is at `head`.
fn insert_in_section(blocks: &mut Vec<Block>, head: usize, entry: Entry) {
    let end = blocks[head + 1..].iter().position(is_heading).map_or(blocks.len(), |p| head + 1 + p);
    let at = match blocks[head + 1..end].iter().rposition(|b| matches!(b, Block::Entry(_))) {
        Some(p) => head + 2 + p,
        None if is_blank(blocks.get(head + 1)) => head + 2,
        None => {
            blocks.insert(head + 1, Block::Raw(String::new()));
            head + 2
        }
    };
    blocks.insert(at, Block::Entry(entry));
    if blocks.get(at + 1).is_some_and(is_heading) {
        blocks.insert(at + 1, Block::Raw(String::new()));
    }
}

/// A new section at the end; returns its heading's index.
fn append_section(blocks: &mut Vec<Block>, heading: Block) -> usize {
    if !blocks.is_empty() && !is_blank(blocks.last()) {
        blocks.push(Block::Raw(String::new()));
    }
    blocks.push(heading);
    blocks.push(Block::Raw(String::new()));
    blocks.len() - 2
}

/// The file stem of the vault note named `name` (memory folder aside), if there is one.
fn person_note(vault: &Vault, name: &str) -> Option<String> {
    let key = index::key(name);
    let memory = format!("{}/", vault.config.folders.memory.trim_end_matches('/'));
    walk_vault(&vault.root).filter(|w| !w.is_dir && !w.rel.starts_with(&memory)).find_map(|w| {
        let stem = w.rel.rsplit('/').next()?.strip_suffix(".md")?;
        (index::key(stem) == key).then(|| stem.to_string())
    })
}

#[derive(Default)]
struct Outcome {
    added: Vec<(Role, Entry)>,
    /// `(role it left, line, why, reason)`.
    moved: Vec<(Role, Entry, Why, String)>,
}

/// The changes applied to one read of the files, in memory.
struct Plan<'a> {
    files: Vec<File>,
    pinned: &'a HashSet<String>,
    vault: &'a Vault,
    lang: Lang,
    today: NaiveDate,
    stamp: String,
    /// Every id in use (files, database, this write), so a new one is unique.
    known: HashSet<String>,
    out: Outcome,
}

impl Plan<'_> {
    /// `(file, block, entry)` of every line on `key` in the active files.
    fn find(&self, key: &str) -> Vec<(usize, usize, Entry)> {
        let mut found = Vec::new();
        for (f, file) in self.files.iter().enumerate().filter(|(_, f)| f.role != Role::Archive) {
            for (b, block) in file.doc.blocks.iter().enumerate() {
                if let Block::Entry(e) = block {
                    if topic_key(&e.topic) == key {
                        found.push((f, b, e.clone()));
                    }
                }
            }
        }
        found
    }

    /// The file a role is written to: the one in `lang`, else the other language's, else a new one.
    fn target(&mut self, role: Role) -> usize {
        let name = role.file_name(self.lang);
        if let Some(i) = self.files.iter().position(|f| f.name == name).or_else(|| self.files.iter().position(|f| f.role == role)) {
            return i;
        }
        let doc = parse_doc(role, &render(role, &[], self.lang));
        self.files.push(File { role, name: name.to_string(), hash: None, doc, dirty: false });
        self.files.len() - 1
    }

    fn new_id(&mut self, key: &str, text: &str) -> String {
        for n in 0.. {
            let id: String = content_hash(format!("{key}\n{text}\n{}\n{n}", self.stamp).as_bytes()).chars().take(8).collect();
            let in_files = self.files.iter().any(|f| f.doc.blocks.iter().any(|b| matches!(b, Block::Entry(e) if e.id == id)));
            if !in_files && self.known.insert(id.clone()) {
                return id;
            }
        }
        unreachable!("ids run out")
    }

    fn add(&mut self, p: &Proposal) {
        let text = p.text.trim();
        let person = p.person.as_deref().map(str::trim).filter(|s| !s.is_empty() && p.role == Role::People);
        if text.is_empty()
            || p.role == Role::Archive
            || p.kind == Kind::Manual
            || (p.role == Role::People && person.is_none())
            || (p.kind == Kind::Inferred && p.source.is_none())
        {
            return;
        }
        let key = stored_topic(p.role, &p.topic, person);
        if key.is_empty() {
            return;
        }
        let mut found = self.find(&key);
        let held = |e: &Entry| e.kind == Kind::Manual || self.pinned.contains(&e.id) || (p.kind == Kind::Inferred && e.kind == Kind::Said);
        if found.iter().any(|(_, _, e)| held(e)) {
            return;
        }
        // Said again in the same words: the line stays, only its date moves.
        if let [(f, b, e)] = found.as_slice() {
            if e.kind == p.kind && e.text == text && e.source == p.source && self.files[*f].role == p.role {
                if let Block::Entry(line) = &mut self.files[*f].doc.blocks[*b] {
                    if line.date != self.today {
                        line.date = self.today;
                        self.files[*f].dirty = true;
                    }
                }
                return;
            }
        }
        let id = self.new_id(&key, text);
        let mut entry = Entry { id, text: text.to_string(), kind: p.kind, topic: key, source: p.source.clone(), date: self.today, person: None, notes: Vec::new() };
        let target = self.target(p.role);
        let same_person = |e: &Entry| p.role != Role::People || e.person.as_deref().map(index::key) == person.map(index::key);
        let in_place = found.iter().find(|(f, _, e)| *f == target && same_person(e)).map(|(f, b, _)| (*f, *b));
        // From the last block back, so the indices left to visit stay valid.
        found.sort_by_key(|x| std::cmp::Reverse((x.0, x.1)));
        for (f, b, old) in found {
            if Some((f, b)) == in_place {
                entry.person = old.person.clone();
                self.files[f].doc.blocks[b] = Block::Entry(entry.clone());
            } else {
                self.files[f].doc.blocks.remove(b);
            }
            self.files[f].dirty = true;
            let role = self.files[f].role;
            self.retire(role, old, Why::Outdated, "replaced");
        }
        if in_place.is_none() {
            entry = self.insert(target, entry, person);
        }
        self.out.added.push((p.role, entry));
    }

    fn forget(&mut self, topic: &str, reason: &str) {
        // A note that changed or went away takes only its inferences with it, and rejects nothing.
        let from_source = reason == SOURCE_CHANGED || reason == SOURCE_DELETED;
        let why = if from_source { Why::Outdated } else { Why::Rejected };
        let mut found: Vec<_> = self
            .find(&topic_key(topic))
            .into_iter()
            .filter(|(_, _, e)| e.kind != Kind::Manual && !self.pinned.contains(&e.id) && (!from_source || e.kind == Kind::Inferred))
            .collect();
        found.sort_by_key(|x| std::cmp::Reverse((x.0, x.1)));
        for (f, b, old) in found {
            self.files[f].doc.blocks.remove(b);
            self.files[f].dirty = true;
            let role = self.files[f].role;
            self.retire(role, old, why, reason);
        }
    }

    /// Writes `entry` into file `f` (under the person's section for People); returns it as written.
    fn insert(&mut self, f: usize, mut entry: Entry, person: Option<&str>) -> Entry {
        let section = person.map(|name| {
            let key = index::key(name);
            let found = self.files[f].doc.blocks.iter().position(|b| matches!(b, Block::Person { name, .. } if index::key(name) == key));
            (name, found)
        });
        let blocks = &mut self.files[f].doc.blocks;
        match section {
            Some((_, Some(head))) => {
                if let Block::Person { name, .. } = &blocks[head] {
                    entry.person = Some(name.clone());
                }
                insert_in_section(blocks, head, entry.clone());
            }
            Some((name, None)) => {
                let note = person_note(self.vault, name);
                let raw = match &note {
                    Some(stem) => format!("## [[{stem}]]"),
                    None => format!("## {name}"),
                };
                let name = note.unwrap_or_else(|| name.to_string());
                entry.person = Some(name.clone());
                let head = append_section(blocks, Block::Person { raw, name });
                insert_in_section(blocks, head, entry.clone());
            }
            None => match blocks.iter().rposition(|b| matches!(b, Block::Entry(_))) {
                Some(p) => blocks.insert(p + 1, Block::Entry(entry.clone())),
                None => {
                    if !blocks.is_empty() && !is_blank(blocks.last()) {
                        blocks.push(Block::Raw(String::new()));
                    }
                    blocks.push(Block::Entry(entry.clone()));
                }
            },
        }
        self.files[f].dirty = true;
        entry
    }

    /// Moves a line that left an active file into the archive. A line added by this same write
    /// just disappears: it was never on disk.
    fn retire(&mut self, from: Role, old: Entry, why: Why, reason: &str) {
        if let Some(i) = self.out.added.iter().position(|(_, a)| a.id == old.id) {
            self.out.added.remove(i);
            return;
        }
        let mut line = old.clone();
        let who = old.person.as_deref().map(|p| format!(" · {p}")).unwrap_or_default();
        line.notes.push(format!("  - {}{who} · {}", why.note(self.lang, reason), self.today.format("%Y-%m-%d")));
        line.person = None;
        let archive = self.target(Role::Archive);
        let heading = format!("## {}", why.heading(self.lang));
        let blocks = &mut self.files[archive].doc.blocks;
        let head = match blocks.iter().position(|b| matches!(b, Block::Raw(l) if l.trim() == heading)) {
            Some(h) => h,
            None => append_section(blocks, Block::Raw(heading)),
        };
        insert_in_section(blocks, head, line);
        self.files[archive].dirty = true;
        self.out.moved.push((from, old, why, reason.to_string()));
    }

    /// Keeps every changed file within its limit: State drops its oldest lines; About and Rules
    /// their oldest inference, then their oldest said line; a People section the same within
    /// `PEOPLE_TOKENS`. Manual lines, lines the user just changed and this write's lines stay.
    fn compact(&mut self) {
        for f in 0..self.files.len() {
            if !self.files[f].dirty {
                continue;
            }
            while let Some(b) = self.over(f) {
                if let Block::Entry(old) = self.files[f].doc.blocks.remove(b) {
                    let role = self.files[f].role;
                    self.retire(role, old, Why::Overflow, "over the limit");
                }
            }
        }
    }

    /// The block to move out of file `f` next, if it is over its limit.
    fn over(&self, f: usize) -> Option<usize> {
        let file = &self.files[f];
        let entries: Vec<(usize, &Entry)> = file.doc.blocks.iter().enumerate().filter_map(|(i, b)| if let Block::Entry(e) = b { Some((i, e)) } else { None }).collect();
        let groups: Vec<Vec<(usize, &Entry)>> = match file.role {
            Role::People => {
                let mut names: Vec<Option<String>> = Vec::new();
                for (_, e) in &entries {
                    if !names.contains(&e.person) {
                        names.push(e.person.clone());
                    }
                }
                names.iter().map(|n| entries.iter().filter(|(_, e)| e.person == *n).copied().collect()).collect()
            }
            Role::Archive => return None,
            _ => vec![entries],
        };
        for g in groups {
            let over = match char_limit(file.role) {
                Some(limit) => g.iter().map(|(_, e)| e.text.chars().count()).sum::<usize>() > limit,
                None => tokens(&g.iter().map(|(_, e)| e.text.as_str()).collect::<Vec<_>>().join("\n")) > PEOPLE_TOKENS,
            };
            if !over {
                continue;
            }
            let movable = g.iter().filter(|(_, e)| e.kind != Kind::Manual && !self.pinned.contains(&e.id) && !self.out.added.iter().any(|(_, a)| a.id == e.id));
            let victim = if file.role == Role::State {
                movable.min_by_key(|(_, e)| e.date)
            } else {
                movable.min_by_key(|(_, e)| (e.kind != Kind::Inferred, e.date))
            };
            if let Some((b, _)) = victim {
                return Some(*b);
            }
        }
        None
    }

    /// Writes the changed files (the archive first, so a crash between two writes loses no line),
    /// then records the lines in `memory_entry`.
    fn write(mut self, dir: &Path, db: &Connection, wrote: &dyn Fn(&str)) -> Result<Vec<Entry>, RecallError> {
        self.files.sort_by_key(|f| f.role != Role::Archive);
        let folder = self.vault.config.folders.memory.trim_end_matches('/');
        for file in self.files.iter().filter(|f| f.dirty) {
            std::fs::create_dir_all(dir)?;
            write_atomic(&dir.join(&file.name), render_doc(&file.doc, self.lang).as_bytes())?;
            wrote(&format!("{folder}/{}", file.name));
        }
        let at = self.stamp.clone();
        crate::db::atomically(db, "recall_apply", || -> rusqlite::Result<()> {
            for (role, e) in &self.out.added {
                upsert(db, e, *role, "active", None, &at)?;
            }
            for (role, e, why, reason) in &self.out.moved {
                upsert(db, e, *role, why.state(), Some(reason), &at)?;
            }
            Ok(())
        })?;
        Ok(self.out.added.into_iter().map(|(_, e)| e).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::format::{parse, render};
    use crate::vault::open_vault;
    use chrono::NaiveDate;

    struct Fixture {
        _tmp: tempfile::TempDir,
        _data: tempfile::TempDir,
        vault: Vault,
        db: Connection,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let data = tempfile::tempdir().unwrap();
            let vault = open_vault(tmp.path()).unwrap();
            let dir = tmp.path().join(&vault.config.folders.memory);
            for (n, c) in files {
                let path = if n.contains('/') { tmp.path().join(n) } else { dir.join(n) };
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, c).unwrap();
            }
            let db = crate::db::open_databases(data.path()).unwrap().pla;
            Fixture { _tmp: tmp, _data: data, vault, db }
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            self.vault.root.join(&self.vault.config.folders.memory).join(name)
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.path(name)).unwrap_or_default()
        }

        fn apply(&self, changes: &[Change], day: u32) -> Vec<Entry> {
            apply(&self.vault, &self.db, changes, at(day), Lang::Tr, &|_| {}).unwrap()
        }

        fn state(&self, id: &str) -> (String, Option<String>) {
            self.db.query_row("SELECT state, reason FROM memory_entry WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
        }
    }

    /// 2026-10-`day`, 10:00 Istanbul.
    fn at(day: u32) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(&format!("2026-10-{day:02}T10:00:00+03:00")).unwrap()
    }

    fn add(role: Role, kind: Kind, topic: &str, text: &str) -> Change {
        let source = (kind == Kind::Inferred).then(|| "iha/Skywalker X8.md#9f2e".to_string());
        Change::Add(Proposal { text: text.into(), topic: topic.into(), role, person: None, kind, source })
    }

    fn said(topic: &str, text: &str) -> Change {
        add(Role::About, Kind::Said, topic, text)
    }

    fn line(id: &str, topic: &str, text: &str, kind: Kind, date: &str) -> Entry {
        let source = (kind == Kind::Inferred).then(|| "iha/Skywalker X8.md#9f2e".to_string());
        Entry { id: id.into(), text: text.into(), kind, topic: topic.into(), source, date: date.parse::<NaiveDate>().unwrap(), person: None, notes: Vec::new() }
    }

    /// The text of the entries under `## heading` in the archive.
    fn archived_under(f: &Fixture, heading: &str) -> Vec<String> {
        let text = f.read("Arşiv.md");
        let section = text.split(&format!("## {heading}\n")).nth(1).unwrap_or("");
        let section = section.split("\n## ").next().unwrap_or("");
        parse(Role::Archive, section).into_iter().map(|e| e.text).collect()
    }

    #[test]
    fn the_same_topic_said_again_replaces_the_line() {
        let f = Fixture::new(&[]);
        let first = f.apply(&[said("Spor Zamanı", "Sabahları spor yapıyor.")], 18);
        let second = f.apply(&[said("spor-zamani", "Akşamları spor yapıyor.")], 23);
        let about = parse(Role::About, &f.read("Hakkımda.md"));
        assert_eq!(about.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["Akşamları spor yapıyor."]);
        assert_eq!(about[0].date.to_string(), "2026-10-23");
        assert_eq!(archived_under(&f, "Eskiyenler"), ["Sabahları spor yapıyor."]);
        assert_eq!(f.state(&first[0].id).0, "archived");
        assert_eq!(f.state(&second[0].id).0, "active");
        // Said in other words a third time on the same day: still one line.
        f.apply(&[said("Spor zamanı", "Akşamları spor yapıyor.")], 24);
        assert_eq!(parse(Role::About, &f.read("Hakkımda.md")).len(), 1);
    }

    #[test]
    fn said_beats_inferred_and_inferred_never_beats_said() {
        let f = Fixture::new(&[]);
        f.apply(&[add(Role::State, Kind::Inferred, "iha-gövde", "X8 önde.")], 10);
        f.apply(&[add(Role::State, Kind::Inferred, "iha gövde", "X8 hâlâ önde.")], 11);
        assert_eq!(parse(Role::State, &f.read("Durum.md"))[0].text, "X8 hâlâ önde.", "an inference replaces an inference");
        // The topic is one across all files: a said line in About displaces the inference in State.
        f.apply(&[add(Role::About, Kind::Said, "İHA gövde", "Talon'u seçtim.")], 12);
        assert!(parse(Role::State, &f.read("Durum.md")).is_empty());
        assert_eq!(parse(Role::About, &f.read("Hakkımda.md"))[0].text, "Talon'u seçtim.");
        let state_before = f.read("Durum.md");
        let added = f.apply(&[add(Role::State, Kind::Inferred, "IHA-gövde", "X8 yine önde.")], 13);
        assert!(added.is_empty());
        assert_eq!(f.read("Durum.md"), state_before);
        assert_eq!(archived_under(&f, "Eskiyenler"), ["X8 önde.", "X8 hâlâ önde."]);
    }

    #[test]
    fn manual_lines_are_never_replaced_or_moved() {
        let long = "Uzun bir elle yazılmış satır, sınırı tek başına zorlayacak kadar uzun bir açıklama içerir.".repeat(2);
        let manual = format!("# Hakkımda\n\n- Kahveyi sütsüz içer.\n- {long}\n- {long}\n- {long}\n- {long}\n- {long}\n");
        let f = Fixture::new(&[("Hakkımda.md", &manual)]);
        let manual_topic = parse(Role::About, &manual)[0].topic.clone();
        assert!(f.apply(&[said(&manual_topic, "Kahveyi şekerli içer.")], 20).is_empty());
        f.apply(&[Change::Forget { topic: manual_topic, reason: "yanlış".into() }], 20);
        assert_eq!(f.read("Hakkımda.md"), manual, "untouched, not even re-rendered");
        // Manual lines alone are over 800 characters: the PLA lines move out, oldest said first.
        f.apply(&[said("spor", "Sabahları spor yapıyor.")], 21);
        f.apply(&[said("uyku", "Gece yarısı uyur.")], 22);
        let about = f.read("Hakkımda.md");
        assert!(about.contains("- Kahveyi sütsüz içer.\n") && about.matches(&long).count() == 5, "{about}");
        assert!(!about.contains("Sabahları spor"), "{about}");
        assert_eq!(archived_under(&f, "Taşanlar"), ["Sabahları spor yapıyor."]);
        assert!(!f.read("Arşiv.md").contains("sütsüz"));
    }

    #[test]
    fn a_line_the_user_changed_meanwhile_is_left_alone() {
        let start = render(Role::About, &[line("s1", "spor-zamani", "Sabahları spor yapıyor.", Kind::Said, "2026-10-18")], Lang::Tr);
        let f = Fixture::new(&[("Hakkımda.md", &start)]);
        let path = f.path("Hakkımda.md");
        let mut edited = false;
        let changes = [said("spor zamanı", "Akşamları spor yapıyor."), said("kahve", "Kahveyi sütsüz içer.")];
        let added = apply_with(&f.vault, &f.db, &changes, at(23), Lang::Tr, &|_| {}, &mut || {
            if !edited {
                edited = true;
                let text = std::fs::read_to_string(&path).unwrap().replace("Sabahları spor", "Sabah 7'de spor");
                std::fs::write(&path, format!("{text}- Elle eklenen satır.\n")).unwrap();
            }
        })
        .unwrap();
        let about = f.read("Hakkımda.md");
        assert!(about.contains("Sabah 7'de spor yapıyor."), "the user's change stays: {about}");
        assert!(about.contains("- Elle eklenen satır.\n"), "the user's new line stays: {about}");
        assert!(about.contains("Kahveyi sütsüz içer."), "the other change is applied on top: {about}");
        assert!(!about.contains("Akşamları"), "{about}");
        assert!(!f.read("Arşiv.md").contains("Sabah"));
        assert_eq!(added.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["Kahveyi sütsüz içer."]);
    }

    #[test]
    fn over_the_limit_the_oldest_moves_to_the_archive() {
        let f = Fixture::new(&[]);
        for day in 1..=12 {
            let text = format!("{day}. iş: bitirme projesinin bu bölümü için yapılacakların kısa ama dolu bir özeti burada durur.");
            f.apply(&[add(Role::State, Kind::Said, &format!("is-{day}"), &text)], day);
        }
        let state = parse(Role::State, &f.read("Durum.md"));
        assert!(state.iter().map(|e| e.text.chars().count()).sum::<usize>() <= 800);
        assert!(state.iter().any(|e| e.text.starts_with("12. iş")));
        let moved = archived_under(&f, "Taşanlar");
        assert!(moved[0].starts_with("1. iş"), "{moved:?}");
        assert!(!state.iter().any(|e| e.text.starts_with("1. iş")));

        // About: the oldest inference goes before an even older said line.
        let text = |c: char| c.to_string().repeat(390);
        let about = render(
            Role::About,
            &[line("old", "eski", &text('a'), Kind::Said, "2026-09-01"), line("inf", "cikarim", &text('b'), Kind::Inferred, "2026-10-01")],
            Lang::Tr,
        );
        let f = Fixture::new(&[("Hakkımda.md", &about)]);
        f.apply(&[said("yeni", &"c".repeat(100))], 20);
        let kept: Vec<String> = parse(Role::About, &f.read("Hakkımda.md")).into_iter().map(|e| e.id).collect();
        assert_eq!(kept.len(), 2);
        assert!(kept.contains(&"old".to_string()) && !kept.contains(&"inf".to_string()));
        assert_eq!(archived_under(&f, "Taşanlar"), [text('b')]);
    }

    #[test]
    fn forget_rejects_and_archives_with_its_reason() {
        let f = Fixture::new(&[]);
        let added = f.apply(&[said("Spor Zamanı", "Sabahları spor yapıyor.")], 18);
        f.apply(&[Change::Forget { topic: "spor zamani".into(), reason: "artık öyle değil".into() }], 23);
        assert!(parse(Role::About, &f.read("Hakkımda.md")).is_empty());
        assert_eq!(archived_under(&f, "Reddedilenler"), ["Sabahları spor yapıyor."]);
        assert!(f.read("Arşiv.md").contains("artık öyle değil"));
        assert_eq!(f.state(&added[0].id), ("rejected".to_string(), Some("artık öyle değil".to_string())));
        assert_eq!(rejected(&f.db), [("spor-zamani".to_string(), "Sabahları spor yapıyor.".to_string())]);
        // An unknown topic forgets nothing.
        let before = (f.read("Hakkımda.md"), f.read("Arşiv.md"));
        f.apply(&[Change::Forget { topic: "yok".into(), reason: "x".into() }], 24);
        assert_eq!((f.read("Hakkımda.md"), f.read("Arşiv.md")), before);
    }

    #[test]
    fn a_line_the_user_deleted_becomes_rejected_but_a_line_pla_moved_does_not() {
        let f = Fixture::new(&[]);
        let old = f.apply(&[said("spor", "Sabahları spor yapıyor.")], 18);
        f.apply(&[said("spor", "Akşamları spor yapıyor."), said("kahve", "Kahveyi sütsüz içer.")], 20);
        let about = f.read("Hakkımda.md");
        let kept: String = about.lines().filter(|l| !l.contains("Kahveyi")).map(|l| format!("{l}\n")).collect();
        std::fs::write(f.path("Hakkımda.md"), kept).unwrap();
        reconcile(&f.vault, &f.db).unwrap();
        assert_eq!(rejected(&f.db), [("kahve".to_string(), "Kahveyi sütsüz içer.".to_string())]);
        assert_eq!(f.state(&old[0].id).0, "archived");
        reconcile(&f.vault, &f.db).unwrap();
        assert_eq!(rejected(&f.db).len(), 1, "reconciling again changes nothing");
    }

    #[test]
    fn an_unreadable_file_is_not_overwritten() {
        let f = Fixture::new(&[]);
        f.apply(&[said("spor", "Sabahları spor yapıyor.")], 18);
        let mut bytes = std::fs::read(f.path("Hakkımda.md")).unwrap();
        bytes.extend_from_slice(b"- bozuk \xff\xfe\n");
        std::fs::write(f.path("Hakkımda.md"), &bytes).unwrap();
        let err = apply(&f.vault, &f.db, &[said("kahve", "Kahveyi sütsüz içer.")], at(20), Lang::Tr, &|_| {}).unwrap_err();
        assert!(matches!(err, RecallError::Unreadable { .. }), "{err}");
        assert_eq!(std::fs::read(f.path("Hakkımda.md")).unwrap(), bytes);
        assert!(!f.path("Arşiv.md").exists());
        // Its lines are not taken for deleted ones either.
        assert!(reconcile(&f.vault, &f.db).is_err());
        assert!(rejected(&f.db).is_empty());
    }

    #[test]
    fn a_new_person_links_to_their_note() {
        let f = Fixture::new(&[("kisiler/Deniz.md", "# Deniz\n")]);
        let person = |name: &str, topic: &str, text: &str| {
            Change::Add(Proposal { text: text.into(), topic: topic.into(), role: Role::People, person: Some(name.into()), kind: Kind::Said, source: None })
        };
        let seen = std::cell::RefCell::new(Vec::<String>::new());
        apply(&f.vault, &f.db, &[person("Deniz", "kahve", "Kahveyi sütsüz içer."), person("Annem", "kahve", "Çay sever.")], at(20), Lang::Tr, &|p| {
            seen.borrow_mut().push(p.to_string())
        })
        .unwrap();
        f.apply(&[person("deniz", "doğum günü", "Doğum günü 3 Mart.")], 21);
        let text = f.read("Kişiler.md");
        assert_eq!(text.matches("## [[Deniz]]\n").count(), 1, "{text}");
        assert!(text.contains("## Annem\n"), "{text}");
        let people = parse(Role::People, &text);
        let of = |n: &str| people.iter().filter(|e| e.person.as_deref() == Some(n)).map(|e| e.text.as_str()).collect::<Vec<_>>();
        assert_eq!(of("Deniz"), ["Kahveyi sütsüz içer.", "Doğum günü 3 Mart."]);
        assert_eq!(of("Annem"), ["Çay sever."], "two people may share a topic word");
        assert_eq!(seen.into_inner(), ["memory/Kişiler.md"]);
    }
}
