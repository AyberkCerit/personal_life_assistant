//! Daily summaries (FR-MEM-006/007/008/011): a few lines per past day of notes, written by the model
//! in the maintenance window and kept in cache.db. They only help find a day; the assistant reads
//! that day's raw notes. Notes and pla.db are never written here.

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, NaiveDate, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

use crate::index::NoteText;
use crate::llm::LlmError;
use crate::memory::{self, Embedder};
use crate::qa::context::tokens;
use crate::qa::ChatModel;
use crate::vault::Folders;

/// Owner decision A: the first maintenance summarises the last 30 days, each later one at most 20.
pub const FIRST_WINDOW_DAYS: i64 = 30;
pub const PER_RUN: usize = 20;
/// What the model reads for one day (its window is 8 192 tokens).
const INPUT_TOKENS: usize = 5000;

/// A day's notes: its daily note and the other notes last changed that day (reports and
/// templates left out, FR-MEM-011), with a signature that changes when any of them does.
#[derive(Debug, Clone, PartialEq)]
pub struct DaySources {
    pub date: NaiveDate,
    pub notes: Vec<NoteText>,
    pub signature: String,
}

fn excluded(rel: &str, folders: &Folders) -> bool {
    [&folders.reports, &folders.templates].iter().any(|f| rel.starts_with(&format!("{}/", f.trim_matches('/'))))
}

/// The local day a modification time falls on.
pub fn day_of(ms: i64) -> Option<NaiveDate> {
    Local.timestamp_millis_opt(ms).single().map(|t| t.date_naive())
}

fn day_bounds(date: NaiveDate) -> (i64, i64) {
    // where midnight does not exist (a DST jump at 00:00), the day starts at its first real hour
    let start = |d: NaiveDate| {
        (0..3)
            .find_map(|h| Local.from_local_datetime(&d.and_hms_opt(h, 0, 0).expect("an hour")).earliest())
            .map_or(0, |t| t.timestamp_millis())
    };
    (start(date), start(date + Duration::days(1)))
}

/// The date of a daily note path (`daily/2026/2026-10-06.md`), if it is one.
pub fn daily_note_date(folders: &Folders, rel: &str) -> Option<NaiveDate> {
    let rest = rel.strip_prefix(&format!("{}/", folders.daily.trim_matches('/')))?;
    let (_year, file) = rest.split_once('/')?;
    let stem = file.strip_suffix(".md")?;
    let date = NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()?;
    (crate::fileops::daily_note_rel(folders, date) == rel).then_some(date)
}

const NOTE: &str = "SELECT n.note_path, n.title, '', n.mtime, n.content_hash FROM note_index n";

fn note_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<(NoteText, String)> {
    Ok((NoteText { note_path: r.get(0)?, title: r.get(1)?, body: r.get(2)?, mtime: r.get(3)? }, r.get(4)?))
}

/// A day's notes from `note_index` alone (an index on `mtime`, the primary key): no bodies are read,
/// so a whole maintenance plan stays cheap on a large vault (final review I1).
fn day_notes(conn: &Connection, folders: &Folders, date: NaiveDate) -> rusqlite::Result<Vec<(NoteText, String)>> {
    let (from, to) = day_bounds(date);
    let daily = crate::fileops::daily_note_rel(folders, date);
    let mut rows: Vec<(NoteText, String)> = conn
        .prepare(&format!("{NOTE} WHERE n.mtime >= ?1 AND n.mtime < ?2 ORDER BY n.note_path"))?
        .query_map(params![from, to], note_row)?
        .collect::<Result<_, _>>()?;
    // a daily note belongs to its own date, whenever it was last changed
    rows.retain(|(n, _)| daily_note_date(folders, &n.note_path).is_none_or(|d| d == date) && !excluded(&n.note_path, folders));
    if !rows.iter().any(|(n, _)| n.note_path == daily) {
        if let Some(row) = conn.query_row(&format!("{NOTE} WHERE n.note_path = ?1"), [&daily], note_row).optional()? {
            rows.insert(0, row);
        }
    }
    Ok(rows)
}

fn signature_of(rows: &[(NoteText, String)]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (n, hash) in rows {
        hasher.update(format!("{}:{hash}\n", n.note_path).as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// What a day's summary must match to be current.
pub fn signature(conn: &Connection, folders: &Folders, date: NaiveDate) -> rusqlite::Result<String> {
    Ok(signature_of(&day_notes(conn, folders, date)?))
}

/// The day's notes with their text (from their chunks), for the model.
pub fn sources(conn: &Connection, folders: &Folders, date: NaiveDate) -> rusqlite::Result<DaySources> {
    let mut rows = day_notes(conn, folders, date)?;
    let signature = signature_of(&rows);
    for (n, _) in rows.iter_mut() {
        let parts: Vec<(String, String)> =
            conn.prepare("SELECT heading, text FROM chunk WHERE note_path = ?1 ORDER BY ord")?.query_map([&n.note_path], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        let mut last = String::new();
        let mut body = String::new();
        for (heading, text) in parts {
            if !heading.is_empty() && heading != last {
                body.push_str(&format!("## {heading}\n"));
                last = heading;
            }
            body.push_str(&text);
            body.push_str("\n\n");
        }
        n.body = body;
    }
    Ok(DaySources { date, signature, notes: rows.into_iter().map(|(n, _)| n).collect() })
}

/// Every past day that has notes: last-change days and daily-note dates, newest first.
pub fn note_days(conn: &Connection, folders: &Folders, today: NaiveDate) -> rusqlite::Result<Vec<NaiveDate>> {
    let rows: Vec<(String, i64)> = conn.prepare("SELECT note_path, mtime FROM note_index")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut days: Vec<NaiveDate> = Vec::new();
    for (rel, mtime) in rows {
        if excluded(&rel, folders) {
            continue;
        }
        let day = daily_note_date(folders, &rel).or_else(|| day_of(mtime));
        if let Some(d) = day.filter(|d| *d < today) {
            if !days.contains(&d) {
                days.push(d);
            }
        }
    }
    days.sort_by(|a, b| b.cmp(a));
    Ok(days)
}

/// FR-MEM-006, owner decision A: the days this maintenance summarises: stale ones first, then
/// missing ones newest first: the last 30 days the first time, at most 20 days each later time.
/// A summary whose day has no notes any more goes.
pub fn plan(conn: &Connection, folders: &Folders, today: NaiveDate) -> rusqlite::Result<Vec<NaiveDate>> {
    let days = note_days(conn, folders, today)?;
    let stored: Vec<(String, String)> =
        conn.prepare("SELECT date, signature FROM daily_summary")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    for (date, _) in &stored {
        let gone = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or(true, |d| !days.contains(&d));
        if gone {
            delete(conn, date)?;
        }
    }
    let first_time = stored.is_empty();
    let mut stale = Vec::new();
    let mut missing = Vec::new();
    for d in days {
        let key = d.format("%Y-%m-%d").to_string();
        match stored.iter().find(|(date, _)| *date == key) {
            Some((_, sig)) => {
                if *sig != signature(conn, folders, d)? {
                    stale.push(d);
                }
            }
            None if first_time && d < today - Duration::days(FIRST_WINDOW_DAYS) => {}
            None => missing.push(d),
        }
    }
    stale.extend(missing);
    stale.truncate(if first_time { FIRST_WINDOW_DAYS as usize } else { PER_RUN });
    Ok(stale)
}

const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const DAYS_TR: [&str; 7] = ["Pazartesi", "Salı", "Çarşamba", "Perşembe", "Cuma", "Cumartesi", "Pazar"];

/// The request that summarises one day.
pub fn request(day: &DaySources) -> Value {
    let share = INPUT_TOKENS / day.notes.len().max(1);
    let mut text = format!("Day: {} ({})\n\n", day.date, DAYS[day.date.weekday().num_days_from_monday() as usize]);
    for n in &day.notes {
        let mut body: String = n.body.trim().to_owned();
        while tokens(&body) > share && !body.is_empty() {
            body = body.chars().take(body.chars().count() * 9 / 10).collect();
        }
        text.push_str(&format!("Note [[{}]]:\n{}\n\n", n.title, body));
    }
    json!({
        "messages": [
            { "role": "system", "content": "You summarise one day of the user's own notes for their personal search. Write 2 to 5 short bullet points, each starting with \"- \", in the language the notes are written in. Use only what the notes say: never add, guess or judge. After each point name the note it comes from as [[Note title]]. No heading, no introduction." },
            { "role": "user", "content": text }
        ],
        "temperature": 0.2,
        "max_tokens": 350,
        "cache_prompt": false,
        "chat_template_kwargs": { "enable_thinking": false }
    })
}

/// FR-MEM-006: the model's summary of `day`.
pub fn summarize(model: &mut dyn ChatModel, day: &DaySources, cancel: &AtomicBool) -> Result<String, LlmError> {
    Ok(model.complete(&request(day), cancel)?.trim().to_owned())
}

fn day_number(date: NaiveDate) -> i64 {
    date.num_days_from_ce() as i64
}

fn delete(conn: &Connection, date: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM daily_summary WHERE date = ?1", [date])?;
    conn.execute("DELETE FROM summary_fts WHERE date = ?1", [date])?;
    if let Ok(d) = NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        conn.execute("DELETE FROM summary_vec WHERE day = ?1", [day_number(d)])?;
    }
    Ok(())
}

/// Keeps `text` as `day`'s summary (cache.db only, FR-MEM-007).
pub fn save(conn: &Connection, day: &DaySources, text: &str, now: DateTime<FixedOffset>) -> rusqlite::Result<()> {
    let key = day.date.format("%Y-%m-%d").to_string();
    let paths: Vec<&str> = day.notes.iter().map(|n| n.note_path.as_str()).collect();
    let tx = conn.unchecked_transaction()?;
    delete(&tx, &key)?;
    tx.execute(
        "INSERT INTO daily_summary (date, summary_text, source_paths_json, generated_at, stale, signature) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
        params![key, text, serde_json::to_string(&paths).unwrap_or_default(), now.to_rfc3339(), day.signature],
    )?;
    // the weekday's names are searchable too ("salı", "Tuesday")
    let n = day.date.weekday().num_days_from_monday() as usize;
    let names = format!("{} {}", DAYS[n], DAYS_TR[n]);
    tx.execute("INSERT INTO summary_fts (date, text) VALUES (?1, ?2)", params![key, format!("{key} {names}\n{text}")])?;
    tx.commit()
}

/// A day's summary as the note's box shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DaySummary {
    pub date: String,
    pub text: String,
    pub sources: Vec<String>,
    pub generated_at: String,
    /// The day's notes changed since: the next maintenance writes it again.
    pub stale: bool,
}

pub fn get(conn: &Connection, folders: &Folders, date: NaiveDate) -> rusqlite::Result<Option<DaySummary>> {
    let key = date.format("%Y-%m-%d").to_string();
    let row: Option<(String, String, String, String)> = conn
        .query_row("SELECT summary_text, source_paths_json, generated_at, signature FROM daily_summary WHERE date = ?1", [&key], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    let Some((text, paths, generated_at, signature)) = row else { return Ok(None) };
    let stale = self::signature(conn, folders, date)? != signature;
    Ok(Some(DaySummary { date: key, text, sources: serde_json::from_str(&paths).unwrap_or_default(), generated_at, stale }))
}

/// FR-MEM-008: summaries get vectors too, so a question can find its day by meaning.
pub fn embed_pending(conn: &Connection, embedder: &mut dyn Embedder, until: Instant, stop: &AtomicBool) -> Result<usize, memory::EmbedError> {
    let model = embedder.model_id().to_owned();
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT date, summary_text FROM daily_summary WHERE embedded_model IS NOT ?1 ORDER BY date DESC LIMIT 32")?
        .query_map([&model], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut done = 0;
    for chunk in rows.chunks(8) {
        if Instant::now() >= until || stop.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let texts: Vec<String> = chunk.iter().map(|(d, t)| memory::document_text(d, "", t)).collect();
        let vectors = embedder.embed(&texts, stop)?;
        let tx = conn.unchecked_transaction()?;
        for ((date, _), v) in chunk.iter().zip(vectors) {
            let Ok(d) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else { continue };
            tx.execute("DELETE FROM summary_vec WHERE day = ?1", [day_number(d)])?;
            if v.len() == memory::DIMENSIONS {
                let blob: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
                tx.execute("INSERT INTO summary_vec (day, embedding) VALUES (?1, ?2)", params![day_number(d), blob])?;
            }
            tx.execute("UPDATE daily_summary SET embedded_model = ?2 WHERE date = ?1", params![date, model])?;
            done += 1;
        }
        tx.commit()?;
    }
    Ok(done)
}

/// FR-MEM-008: the days whose summaries match `question` best (keywords, and the vector when
/// there is one), joined like the chunk search.
pub fn find_days(conn: &Connection, question: &str, query: crate::qa::context::Memory<'_>, limit: usize) -> rusqlite::Result<Vec<NaiveDate>> {
    let mut scores: Vec<(NaiveDate, f64)> = Vec::new();
    let mut add = |d: NaiveDate, rank: usize| match scores.iter_mut().find(|(x, _)| *x == d) {
        Some((_, s)) => *s += 1.0 / (60.0 + rank as f64),
        None => scores.push((d, 1.0 / (60.0 + rank as f64))),
    };
    if let Some(q) = crate::index::or_query(question) {
        let dates: Vec<String> = conn
            .prepare("SELECT date FROM summary_fts WHERE summary_fts MATCH ?1 ORDER BY rank LIMIT 10")?
            .query_map([q], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for (i, d) in dates.iter().enumerate() {
            if let Ok(d) = NaiveDate::parse_from_str(d, "%Y-%m-%d") {
                add(d, i + 1);
            }
        }
    }
    if let Some((vector, model)) = query {
        let blob: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
        let days: Vec<i64> =
            conn.prepare("SELECT day FROM summary_vec WHERE embedding MATCH ?1 AND k = 10 ORDER BY distance")?.query_map([blob], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for (i, n) in days.into_iter().enumerate() {
            let Some(d) = NaiveDate::from_num_days_from_ce_opt(n as i32) else { continue };
            let same_model: Option<String> =
                conn.query_row("SELECT embedded_model FROM daily_summary WHERE date = ?1", [d.format("%Y-%m-%d").to_string()], |r| r.get(0)).optional()?.flatten();
            if same_model.as_deref() == Some(model) {
                add(d, i + 1);
            }
        }
    }
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(scores.into_iter().take(limit).map(|(d, _)| d).collect())
}

/// The notes a stored summary came from.
pub fn source_paths(conn: &Connection, date: NaiveDate) -> rusqlite::Result<Vec<String>> {
    let paths: Option<String> = conn
        .query_row("SELECT source_paths_json FROM daily_summary WHERE date = ?1", [date.format("%Y-%m-%d").to_string()], |r| r.get(0))
        .optional()?;
    Ok(paths.and_then(|p| serde_json::from_str(&p).ok()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::index_note;

    fn cache() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = crate::db::open_databases(tmp.path()).unwrap().cache;
        (tmp, conn)
    }

    fn noon(d: NaiveDate) -> i64 {
        Local.from_local_datetime(&d.and_hms_opt(12, 0, 0).unwrap()).earliest().unwrap().timestamp_millis()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-10-08T09:00:00+03:00").unwrap()
    }

    struct Says(&'static str);
    impl ChatModel for Says {
        fn complete(&mut self, body: &Value, _: &AtomicBool) -> Result<String, LlmError> {
            assert!(body["messages"][1]["content"].as_str().unwrap().starts_with("Day: "));
            Ok(self.0.to_owned())
        }
        fn stream(&mut self, _: &Value, _: &AtomicBool, _: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
            unreachable!()
        }
    }

    #[test]
    fn a_day_is_its_daily_note_and_the_notes_changed_that_day() {
        // FR-MEM-006/011
        let (_t, conn) = cache();
        let f = Folders::default();
        index_note(&conn, "daily/2026/2026-10-06.md", "# 2026-10-06\n\nDişçi.", noon(d("2026-10-07")), 1).unwrap(); // edited the next day
        index_note(&conn, "notes/Plan.md", "# Plan\n\nKarar.", noon(d("2026-10-06")), 1).unwrap();
        index_note(&conn, "reports/weekly/w41.md", "# Rapor", noon(d("2026-10-06")), 1).unwrap();
        index_note(&conn, "templates/Günlük.md", "# {{date}}", noon(d("2026-10-06")), 1).unwrap();
        let day = sources(&conn, &f, d("2026-10-06")).unwrap();
        let paths: Vec<&str> = day.notes.iter().map(|n| n.note_path.as_str()).collect();
        assert_eq!(paths, ["daily/2026/2026-10-06.md", "notes/Plan.md"]);
        assert!(sources(&conn, &f, d("2026-10-07")).unwrap().notes.is_empty(), "a daily note belongs to its own date");
        let before = day.signature.clone();
        index_note(&conn, "notes/Plan.md", "# Plan\n\nYeni karar.", noon(d("2026-10-06")), 1).unwrap();
        assert_ne!(sources(&conn, &f, d("2026-10-06")).unwrap().signature, before, "a change makes it stale");
    }

    #[test]
    fn the_first_run_takes_thirty_days_and_later_runs_twenty() {
        // owner decision A
        let (_t, conn) = cache();
        let f = Folders::default();
        let today = d("2026-10-08");
        for i in 1..=60 {
            let day = today - Duration::days(i);
            index_note(&conn, &format!("notes/n{i}.md"), &format!("# N{i}\n\nmetin"), noon(day), 1).unwrap();
        }
        let first = plan(&conn, &f, today).unwrap();
        assert_eq!(first.len(), 30);
        assert_eq!((first[0], first[29]), (d("2026-10-07"), d("2026-09-08")), "newest first, last 30 days");
        let mut model = Says("- Bir şey [[N1]]");
        for day in &first {
            let src = sources(&conn, &f, *day).unwrap();
            save(&conn, &src, &summarize(&mut model, &src, &AtomicBool::new(false)).unwrap(), now()).unwrap();
        }
        let second = plan(&conn, &f, today).unwrap();
        assert_eq!(second.len(), 20);
        assert_eq!(second[0], d("2026-09-07"), "then further back");
        // a note changed on a summarised day: that day comes first
        index_note(&conn, "notes/n3.md", "# N3\n\nyeni metin", noon(today - Duration::days(3)), 1).unwrap();
        assert_eq!(plan(&conn, &f, today).unwrap()[0], today - Duration::days(3));
        // today is never summarised
        index_note(&conn, "notes/bugun.md", "# Bugün", noon(today), 1).unwrap();
        assert!(!plan(&conn, &f, today).unwrap().contains(&today));
    }

    #[test]
    fn a_summary_shows_when_it_is_stale_and_goes_with_its_notes() {
        let (_t, conn) = cache();
        let f = Folders::default();
        index_note(&conn, "notes/a.md", "# A\n\nCuma dişçi.", noon(d("2026-10-05")), 1).unwrap();
        let src = sources(&conn, &f, d("2026-10-05")).unwrap();
        save(&conn, &src, "- Cuma dişçi randevusu [[A]]", now()).unwrap();
        let s = get(&conn, &f, d("2026-10-05")).unwrap().unwrap();
        assert_eq!((s.stale, s.sources.clone()), (false, vec!["notes/a.md".to_owned()]));
        index_note(&conn, "notes/a.md", "# A\n\nCuma dişçi, saat 14.", noon(d("2026-10-05")), 1).unwrap();
        assert!(get(&conn, &f, d("2026-10-05")).unwrap().unwrap().stale);
        crate::index::remove_note(&conn, "notes/a.md").unwrap();
        plan(&conn, &f, d("2026-10-08")).unwrap();
        assert!(get(&conn, &f, d("2026-10-05")).unwrap().is_none(), "no notes, no summary");
    }

    #[test]
    fn a_question_finds_its_day_through_the_summary() {
        // FR-MEM-008
        let (_t, conn) = cache();
        let f = Folders::default();
        index_note(&conn, "notes/a.md", "# A\n\nmetin", noon(d("2026-10-06")), 1).unwrap();
        index_note(&conn, "notes/b.md", "# B\n\nmetin", noon(d("2026-10-05")), 1).unwrap();
        save(&conn, &sources(&conn, &f, d("2026-10-06")).unwrap(), "- Sunum hazırlandı [[A]]", now()).unwrap();
        save(&conn, &sources(&conn, &f, d("2026-10-05")).unwrap(), "- Market alışverişi [[B]]", now()).unwrap();
        assert_eq!(find_days(&conn, "sunum ne zaman hazırlandı", None, 2).unwrap(), [d("2026-10-06")]);
        assert_eq!(find_days(&conn, "Monday", None, 2).unwrap(), [d("2026-10-05")], "weekday names are searchable");
        assert_eq!(find_days(&conn, "pazartesi ne yaptım", None, 2).unwrap(), [d("2026-10-05")], "in Turkish too");
        assert_eq!(source_paths(&conn, d("2026-10-06")).unwrap(), ["notes/a.md"]);
        assert_eq!(daily_note_date(&f, "daily/2026/2026-10-06.md"), Some(d("2026-10-06")));
        assert_eq!(daily_note_date(&f, "notes/2026-10-06.md"), None);
    }
}
