//! FR-MEM-009/010: what the model reads for one question, inside its 8 192-token window: about 40%
//! the last 48 hours of notes (cut from the oldest) together with the memory block, 40% notes found
//! for the question, 20% tool results. The memory block (at most `MEMORY_TOKENS`) is paid for by
//! the recent share, which is smaller by that constant, so the found share loses nothing to it. The
//! recent share is the same for every question and any memory content, so the model server can
//! reuse it (M6 performance); the question, the follow-up turns (decision A) and the people the
//! conversation mentions come out of the found share.

use chrono::{DateTime, FixedOffset, TimeZone};
use rusqlite::Connection;

use super::Turn;
use crate::index::{self, NoteText};
use crate::recall::{self, Recall, Role};
use crate::util::tokens;
use crate::vault::is_under;

/// The model's window (`ServerConfig::ctx_size`).
pub const WINDOW: usize = 8192;
/// Kept for the instructions (rules, calendar, tools, examples: about 1 300), the message
/// wrappers and the 900-token answer (final review I5: 2 048 was too thin).
pub const RESERVED: usize = 3000;
pub const ANSWER_TOKENS: usize = 900;
/// The last 48 hours' share, whatever the question or the memory: 40% of the room less the memory
/// block's maximum, which sits right after it.
pub const RECENT_TOKENS: usize = (WINDOW - RESERVED) * 2 / 5 - recall::MEMORY_TOKENS;

/// `s` cut to about `budget` tokens, at a line end when there is one near.
fn cut(s: &str, budget: usize) -> String {
    if tokens(s) <= budget {
        return s.to_owned();
    }
    // measured, not guessed: digits count one token each, so the character count alone can overshoot
    let mut max_chars = budget * 3;
    loop {
        let head: String = s.chars().take(max_chars).collect();
        let out = match head.rfind('\n') {
            Some(i) if i > head.len() / 2 => format!("{}\n…", &head[..i]),
            _ => format!("{head}…"),
        };
        if tokens(&out) <= budget || max_chars == 0 {
            return out;
        }
        max_chars = max_chars * 9 / 10;
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Context {
    /// The last 48 hours of notes, newest first, as text for the model.
    pub recent: String,
    /// What the memory files say about the user (`recall::block`), after the recent notes. Empty
    /// when there is no memory. Its tokens come out of the found share.
    pub memory: String,
    /// The people the question and the last turns mention (`recall::people_block`), without a
    /// heading. Comes out of the found share too.
    pub people: String,
    /// Paragraphs of notes found for the question.
    pub found: String,
    /// The follow-up turns that fit, oldest first.
    pub turns: Vec<Turn>,
    /// The notes the found paragraphs came from, best first (FR-QA-010).
    pub sources: Vec<String>,
    /// Tokens left for tool results (FR-MEM-009: 20%).
    pub tool_budget: usize,
}

fn stamp(ms: i64) -> String {
    chrono::Local.timestamp_millis_opt(ms).single().map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

/// One note for the model. A plain header: the model echoed a "(path, time)" one into its answers.
fn note_block(n: &NoteText, body: &str) -> String {
    let day = stamp(n.mtime).split(' ').next().unwrap_or_default().to_owned();
    format!("Note [[{}]], last changed {day}:\n{}\n\n", n.title, body.trim())
}

/// A chunk found for the question, as the model reads it. Its section is a Markdown heading inside
/// the text: written next to the note's name, the model cited the section as if it were a note.
fn chunk_block(h: &crate::memory::ChunkHit) -> String {
    let day = stamp(h.mtime).split(' ').next().unwrap_or_default().to_owned();
    let under = if h.heading.is_empty() { String::new() } else { format!("## {}\n", h.heading) };
    format!("Note [[{}]], last changed {day}:\n{under}{}\n\n", h.title, h.text.trim())
}

/// What semantic memory adds to a question: its vector and the model that made it (FR-MEM-005).
pub type Memory<'a> = Option<(&'a [f32], &'a str)>;

/// FR-MEM-010: the last 48 hours in `RECENT_TOKENS`, chosen newest first: what does not fit is the
/// oldest, the last one that fits is cut. Written oldest to newest: the note being written is the
/// newest, and saving it then changes only the end of what the model server keeps (M6). Also the
/// notes that went in whole.
fn recent_notes(cache: &Connection, now: DateTime<FixedOffset>, memory_folder: &str) -> rusqlite::Result<(String, Vec<String>)> {
    let since = now.timestamp_millis() - 48 * 3600 * 1000;
    let mut budget = RECENT_TOKENS;
    let mut blocks: Vec<String> = Vec::new();
    let mut in_recent: Vec<String> = Vec::new();
    // the active memory files are in the prompt through the memory block already
    for n in index::recent(cache, since, 40)?.into_iter().filter(|n| !is_active_memory(&n.note_path, memory_folder)) {
        let block = note_block(&n, &n.body);
        let cost = tokens(&block);
        if cost <= budget {
            budget -= cost;
            blocks.push(block);
            in_recent.push(n.note_path.clone());
        } else {
            if budget > 60 {
                blocks.push(cut(&block, budget));
            }
            break;
        }
    }
    blocks.reverse();
    Ok((blocks.concat(), in_recent))
}

/// What every question starts with (M6 performance: read ahead when the panel opens): the last 48
/// hours and the memory block, which the conversation's snapshot keeps the same.
pub fn shared(cache: &Connection, now: DateTime<FixedOffset>, recall: &Recall, memory_folder: &str) -> rusqlite::Result<Context> {
    Ok(Context { recent: recent_notes(cache, now, memory_folder)?.0, memory: recall::block(recall), ..Default::default() })
}

/// Whether `path` is one of the active memory files, which the prompt carries whole: finding them
/// again as chunks would only repeat them. The Archive is not active, so it stays searchable.
fn is_active_memory(path: &str, memory_folder: &str) -> bool {
    is_under(path, memory_folder) && path.rsplit(['/', '\\']).next().and_then(Role::from_file_name).is_some_and(|r| r != Role::Archive)
}

/// How many of the best notes found lend their links, and how many linked notes come in
/// (`research/retrieval-eval`: 4 seeds found nothing 2 did not, for 100 more tokens).
const SEEDS: usize = 2;
pub const LINKED: usize = 3;

/// The hits with the notes the best `SEEDS` notes among them link to: of each such note not found
/// already its closest chunk, the closest `limit` of them, right after the seeds' chunks. A note's
/// own links are the user's map of what belongs together, and a fact one link away rarely shares the
/// question's words (the project note names it, its decisions note answers it). Only the links a
/// note makes: a hub's backlinks are mostly daily notes that mention it, and taking them too gained
/// nothing in the retrieval eval.
fn with_linked(cache: &Connection, question: &str, memory: Memory<'_>, hits: Vec<crate::memory::ChunkHit>, limit: usize, memory_folder: &str) -> rusqlite::Result<Vec<crate::memory::ChunkHit>> {
    let mut seeds: Vec<&str> = Vec::new();
    let mut at = hits.len();
    for (i, h) in hits.iter().enumerate() {
        if !seeds.contains(&h.note_path.as_str()) {
            if seeds.len() == SEEDS {
                at = i;
                break;
            }
            seeds.push(&h.note_path);
        }
    }
    let mut candidates: Vec<String> = Vec::new();
    for list in index::linked_notes(cache, &seeds, false)? {
        for p in list {
            if !hits.iter().any(|h| h.note_path == p) && !candidates.contains(&p) && !is_active_memory(&p, memory_folder) {
                candidates.push(p);
            }
        }
    }
    let mut extra = Vec::new();
    for p in &candidates {
        extra.extend(crate::memory::best_chunk(cache, p, question, memory)?);
    }
    extra.sort_by(|a, b| b.score.total_cmp(&a.score)); // stable: ties keep the seeds' order
    extra.truncate(limit);
    let mut out = hits;
    out.splice(at..at, extra);
    Ok(out)
}

/// Builds the context for `question` with the earlier `turns` (oldest first).
#[allow(clippy::too_many_arguments)]
pub fn build(cache: &Connection, question: &str, turns: &[Turn], now: DateTime<FixedOffset>, memory: Memory<'_>, reports: &str, recall: &Recall, memory_folder: &str) -> rusqlite::Result<Context> {
    build_with(cache, question, turns, now, memory, reports, LINKED, recall, memory_folder)
}

/// `build` with at most `linked` notes brought in by links (0: none), for measuring what they add.
#[allow(clippy::too_many_arguments)]
pub fn build_with(cache: &Connection, question: &str, turns: &[Turn], now: DateTime<FixedOffset>, memory: Memory<'_>, reports: &str, linked: usize, recall: &Recall, memory_folder: &str) -> rusqlite::Result<Context> {
    let room = WINDOW - RESERVED - tokens(question).min(700);
    let tool_budget = room / 5;
    let mut found_budget = room - RECENT_TOKENS - tool_budget;

    // the memory block is paid for by the smaller recent share; the people the conversation names
    // come out of the found share, by the tokens they use (at most PEOPLE_TOKENS)
    let memory_text = recall::block(recall);
    let people = recall::people_block(recall, question, turns);
    found_budget = found_budget.saturating_sub(tokens(&people));

    // Follow-ups first (from the found share), newest kept when they do not all fit.
    let mut kept: Vec<Turn> = Vec::new();
    for t in turns.iter().rev() {
        let cost = tokens(&t.question) + tokens(&t.answer) + 8;
        if cost > found_budget {
            break;
        }
        found_budget -= cost;
        kept.insert(0, t.clone());
    }

    let (recent, in_recent) = recent_notes(cache, now, memory_folder)?;

    // FR-MEM-005: chunks from the hybrid search (keywords alone until the embedding model is there)
    let mut found = String::new();
    let mut sources: Vec<String> = Vec::new();
    // a third of the share is kept for the days the summaries point at: the chunks would often
    // fill it all, and a vague "what did I do that day" needs those days most (final review I4)
    let days = crate::summary::find_days(cache, question, memory, 2)?;
    let day_share = if days.is_empty() { 0 } else { found_budget / 3 };
    found_budget -= day_share;
    let mut hits = crate::memory::hybrid(cache, question, memory, reports, 12)?;
    // before the links: a memory file must neither lend its links nor take a seed slot
    hits.retain(|h| !is_active_memory(&h.note_path, memory_folder));
    if linked > 0 {
        hits = with_linked(cache, question, memory, hits, linked, memory_folder)?;
    }
    for h in hits {
        if in_recent.contains(&h.note_path) {
            if !sources.contains(&h.note_path) {
                sources.push(h.note_path.clone()); // already in full above
            }
            continue;
        }
        let block = chunk_block(&h);
        let cost = tokens(&block);
        if cost > found_budget {
            continue;
        }
        found_budget -= cost;
        found.push_str(&block);
        if !sources.contains(&h.note_path) {
            sources.push(h.note_path);
        }
    }
    // FR-MEM-008: a day's summary only points at the day; its notes' raw text is what goes in
    found_budget += day_share;
    for day in days {
        for path in crate::summary::source_paths(cache, day)? {
            if sources.contains(&path) || in_recent.contains(&path) || is_active_memory(&path, memory_folder) {
                continue;
            }
            let hits: Vec<crate::memory::ChunkHit> = cache
                .prepare(
                    "SELECT c.note_path, coalesce(n.title, ''), c.heading, c.text, coalesce(n.mtime, 0) FROM chunk c
                     LEFT JOIN note_index n ON n.note_path = c.note_path WHERE c.note_path = ?1 ORDER BY c.ord LIMIT 2",
                )?
                .query_map([&path], |r| {
                    Ok(crate::memory::ChunkHit { note_path: r.get(0)?, title: r.get(1)?, heading: r.get(2)?, text: r.get(3)?, mtime: r.get(4)?, score: 0.0 })
                })?
                .collect::<Result<_, _>>()?;
            for h in hits {
                let block = chunk_block(&h);
                let cost = tokens(&block);
                if cost > found_budget {
                    continue;
                }
                found_budget -= cost;
                found.push_str(&block);
                if !sources.contains(&h.note_path) {
                    sources.push(h.note_path);
                }
            }
        }
    }
    Ok(Context { recent, memory: memory_text, people, found, turns: kept, sources, tool_budget })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = crate::db::open_databases(tmp.path()).unwrap().cache;
        (tmp, conn)
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-10-06T12:00:00+03:00").unwrap()
    }

    fn turn(q: &str, a: &str) -> Turn {
        Turn { turn_id: q.into(), question: q.into(), answer: a.into(), tools: Vec::new(), created_at: String::new(), status: "done".into(), new_topic: false }
    }

    #[test]
    fn recent_notes_and_found_paragraphs_fill_their_shares() {
        // FR-MEM-009
        let (_t, conn) = cache();
        let ms = now().timestamp_millis();
        index::index_note(&conn, "daily/bugün.md", "# Bugün\n\nDişçi randevusu yarın 10'da.", ms - 3_600_000, 10).unwrap();
        index::index_note(&conn, "notes/PLA.md", "# PLA\n\nGiriş.\n\nKarar: Tauri ve Svelte kullanılacak.\n\nBaşka konu.", ms - 30 * 86_400_000, 10).unwrap();
        let ctx = build(&conn, "PLA projesinde hangi kararları aldık?", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(ctx.recent.contains("Dişçi randevusu"), "the last 48 hours");
        assert!(!ctx.recent.contains("Tauri"), "an old note is not recent");
        assert!(ctx.found.contains("Karar: Tauri"), "the chunk that matters");
        assert_eq!(ctx.sources, ["notes/PLA.md"]);
        assert_eq!(ctx.tool_budget, (WINDOW - RESERVED - tokens("PLA projesinde hangi kararları aldık?")) / 5);
    }

    #[test]
    fn a_summary_brings_its_days_raw_notes_not_itself() {
        // FR-MEM-008: the summary finds the day, the answer reads the notes
        let (_t, conn) = cache();
        let old = now().timestamp_millis() - 9 * 86_400_000;
        index::index_note(&conn, "notes/Toplantı.md", "# Toplantı\n\nBütçe kararı: 40 bin.", old, 10).unwrap();
        let day = crate::summary::day_of(old).unwrap();
        let src = crate::summary::sources(&conn, &crate::vault::Folders::default(), day).unwrap();
        crate::summary::save(&conn, &src, "- Yönetim kurulu buluştu [[Toplantı]]", now()).unwrap();
        let ctx = build(&conn, "yönetim kurulu ne dedi", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(ctx.found.contains("Bütçe kararı: 40 bin"), "the raw note: {}", ctx.found);
        assert!(!ctx.found.contains("Yönetim kurulu buluştu"), "not the summary");
        assert_eq!(ctx.sources, ["notes/Toplantı.md"]);
    }

    #[test]
    fn a_note_found_brings_the_notes_it_links_to() {
        // the question names the project; the answer sits one link away, in words it does not share
        let (_t, conn) = cache();
        let old = now().timestamp_millis() - 30 * 86_400_000;
        index::index_note(&conn, "notes/PLA.md", "# PLA\n\nYerel asistan projesi. Mimari: [[Kararlar]].", old, 10).unwrap();
        index::index_note(&conn, "notes/Kararlar.md", "# Kararlar\n\nÖnbellek tek bir SQLite dosyasında.", old, 10).unwrap();
        index::index_note(&conn, "notes/Mutfak.md", "# Mutfak\n\nMercimek çorbası.", old, 10).unwrap();
        let q = "PLA'da veritabanı için ne seçtik?";
        let without = build_with(&conn, q, &[], now(), None, "reports", 0, &Recall::default(), "memory").unwrap();
        assert!(!without.found.contains("SQLite"), "words alone miss it: {}", without.found);
        let with = build(&conn, q, &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(with.found.contains("SQLite"), "{}", with.found);
        assert_eq!(with.sources, ["notes/PLA.md", "notes/Kararlar.md"], "after the note that led to it");
        assert!(!with.found.contains("Mercimek"), "an unlinked note does not come along");
    }

    #[test]
    fn too_much_recent_text_is_cut_from_the_oldest() {
        // FR-MEM-010
        let (_t, conn) = cache();
        let ms = now().timestamp_millis();
        let long = "kelime ".repeat(2500); // ~4 300 tokens, more than the recent share
        index::index_note(&conn, "notes/eski.md", &format!("# Eski\n\nESKİ {long}"), ms - 40 * 3_600_000, 10).unwrap();
        index::index_note(&conn, "notes/yeni.md", &format!("# Yeni\n\nYENİ {long}"), ms - 3_600_000, 10).unwrap();
        let ctx = build(&conn, "ne yaptım", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(ctx.recent.contains("YENİ") && !ctx.recent.contains("ESKİ"), "the newest stays, the oldest goes");
        assert!(tokens(&ctx.recent) <= RECENT_TOKENS);
    }

    #[test]
    fn the_recent_notes_run_oldest_to_newest() {
        // M6 final review I5: the note being written is the newest; last in the prompt, saving it
        // leaves the prefix the server keeps before it as it was
        let (_t, conn) = cache();
        let ms = now().timestamp_millis();
        index::index_note(&conn, "notes/sabah.md", "# Sabah\n\nKahvaltı.", ms - 5 * 3_600_000, 10).unwrap();
        index::index_note(&conn, "notes/akşam.md", "# Akşam\n\nToplantı.", ms - 3_600_000, 10).unwrap();
        let ctx = build(&conn, "ne yaptım", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(ctx.recent.find("Kahvaltı").unwrap() < ctx.recent.find("Toplantı").unwrap(), "{}", ctx.recent);
    }

    #[test]
    fn the_recent_notes_are_the_same_for_every_question() {
        // M6 performance: the model server reuses the prompt up to the end of this part; a cut
        // that moved with the question's length or the follow-ups made it read 4 000 tokens again
        let (_t, conn) = cache();
        let ms = now().timestamp_millis();
        let long = "kelime ".repeat(2500);
        index::index_note(&conn, "notes/yeni.md", &format!("# Yeni\n\nYENİ {long}"), ms - 3_600_000, 10).unwrap();
        let short = build(&conn, "ne?", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        let long_q = build(&conn, &"bu hafta neler yaptım ".repeat(30), &[turn("bir", &"a ".repeat(800))], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert_eq!(short.recent, long_q.recent);
        assert_eq!(long_q.turns.len(), 1, "the follow-up still fits");
    }

    #[test]
    fn follow_ups_take_their_room_and_the_oldest_goes_first() {
        let (_t, conn) = cache();
        let big = "a ".repeat(3000);
        let turns = [turn("bir", &big), turn("iki", &big), turn("üç", "kısa")];
        let ctx = build(&conn, "peki ya cuma?", &turns, now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert_eq!(ctx.turns.iter().map(|t| t.question.as_str()).collect::<Vec<_>>(), ["iki", "üç"]);
    }

    fn manual(text: &str) -> recall::Entry {
        recall::Entry {
            id: String::new(),
            text: text.into(),
            kind: recall::Kind::Manual,
            topic: String::new(),
            source: None,
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            person: None,
            notes: Vec::new(),
        }
    }

    /// A memory that fills its 600 tokens.
    fn full_memory() -> Recall {
        let mut r = Recall::default();
        for i in 0..80 {
            r.about.push(manual(&format!("Kullanıcının {i} numaralı tercihi: sabah kahve içer ve erken kalkar")));
        }
        r
    }

    fn lots_of_notes(conn: &Connection) {
        let old = now().timestamp_millis() - 30 * 86_400_000;
        for i in 0..12 {
            let body = format!("# Proje {i}\n\n{}", "bütçe kararı toplantı ".repeat(120));
            index::index_note(conn, &format!("notes/p{i}.md"), &body, old, 10).unwrap();
        }
    }

    #[test]
    fn memory_is_paid_by_the_recent_share_and_people_by_the_found_share() {
        let (_t, conn) = cache();
        lots_of_notes(&conn);
        let q = "Deniz ile bütçe kararı toplantı";
        let plain = build(&conn, q, &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        // the recent share is a constant: the memory block fits beside it in 40% of the room
        assert_eq!(RECENT_TOKENS + recall::MEMORY_TOKENS, (WINDOW - RESERVED) * 2 / 5);
        let mut recall = full_memory();
        let with = build(&conn, q, &[], now(), None, "reports", &recall, "memory").unwrap();
        let used = tokens(&with.memory);
        assert!(used > 500 && used <= recall::MEMORY_TOKENS, "{used}");
        assert_eq!(with.found, plain.found, "memory takes nothing from the found notes");
        assert_eq!(with.tool_budget, plain.tool_budget);
        assert_eq!(with.recent, plain.recent);
        // the people the question names do
        for i in 0..8 {
            let mut e = manual(&format!("Deniz için {i}. bilgi: kahveyi sütsüz ve şekersiz içer, kayak yapmayı sever"));
            e.person = Some("Deniz".into());
            recall.people.push(e);
        }
        let named = build(&conn, q, &[], now(), None, "reports", &recall, "memory").unwrap();
        let people = tokens(&named.people);
        assert!(people > 100 && people <= recall::PEOPLE_TOKENS, "{people}");
        let room = WINDOW - RESERVED - tokens(q);
        let found_budget = room - RECENT_TOKENS - room / 5;
        assert!(tokens(&plain.found) > found_budget - people, "the plain run fills more than the shrunk share: {}", tokens(&plain.found));
        assert!(tokens(&named.found) <= found_budget - people, "{} > {}", tokens(&named.found), found_budget - people);
        assert_eq!(named.recent, plain.recent);
    }

    #[test]
    fn active_memory_files_are_not_found_again_but_the_archive_is() {
        let (_t, conn) = cache();
        let old = now().timestamp_millis() - 30 * 86_400_000;
        index::index_note(&conn, "memory/Durum.md", "# Durum\n\nDeniz ile kayak planı yapıyoruz.", old, 10).unwrap();
        index::index_note(&conn, "memory/Arşiv.md", "# Arşiv\n\nEski kayak planı iptal oldu.", old, 10).unwrap();
        index::index_note(&conn, "notes/kayak.md", "# Kayak\n\nKayak malzemeleri listesi.", old, 10).unwrap();
        let ctx = build(&conn, "kayak planı", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(!ctx.found.contains("Deniz ile kayak"), "{}", ctx.found);
        assert!(ctx.found.contains("Eski kayak planı"), "{}", ctx.found);
        assert!(!ctx.sources.iter().any(|s| s == "memory/Durum.md"), "{:?}", ctx.sources);
        assert!(ctx.sources.iter().any(|s| s == "memory/Arşiv.md"), "{:?}", ctx.sources);
    }

    #[test]
    fn a_memory_file_lends_no_links_and_is_not_brought_in_by_one() {
        let (_t, conn) = cache();
        let old = now().timestamp_millis() - 30 * 86_400_000;
        // the memory file ranks first and links to a note; a note links to the memory file
        index::index_note(&conn, "memory/Durum.md", "# Durum\n\nkayak kayak kayak planı. Bkz [[Otel]].", old, 10).unwrap();
        index::index_note(&conn, "notes/Otel.md", "# Otel\n\nDağdaki otel rezervasyonu.", old, 10).unwrap();
        index::index_note(&conn, "notes/Kayak.md", "# Kayak\n\nKayak planı. Durumu: [[Durum]].", old, 10).unwrap();
        let ctx = build(&conn, "kayak planı", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(!ctx.found.contains("otel rezervasyonu"), "the memory file lent its link: {}", ctx.found);
        assert!(!ctx.sources.iter().any(|s| s.starts_with("memory/")), "{:?}", ctx.sources);
        assert!(ctx.sources.iter().any(|s| s == "notes/Kayak.md"), "{:?}", ctx.sources);
    }

    #[test]
    fn active_memory_files_stay_out_of_the_recent_notes() {
        let (_t, conn) = cache();
        let ms = now().timestamp_millis();
        index::index_note(&conn, "memory/Durum.md", "# Durum\n\nDURUM satırı.", ms - 3_600_000, 10).unwrap();
        index::index_note(&conn, "memory/Arşiv.md", "# Arşiv\n\nARŞİV satırı.", ms - 3_600_000, 10).unwrap();
        index::index_note(&conn, "notes/bugün.md", "# Bugün\n\nBUGÜN notu.", ms - 3_600_000, 10).unwrap();
        let ctx = build(&conn, "ne yaptım", &[], now(), None, "reports", &Recall::default(), "memory").unwrap();
        assert!(!ctx.recent.contains("DURUM satırı"), "{}", ctx.recent);
        assert!(ctx.recent.contains("ARŞİV satırı") && ctx.recent.contains("BUGÜN notu"), "{}", ctx.recent);
        let shared = shared(&conn, now(), &Recall::default(), "memory").unwrap();
        assert_eq!(shared.recent, ctx.recent);
    }
}
