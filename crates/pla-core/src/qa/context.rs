//! FR-MEM-009/010: what the model reads for one question, inside its 8 192-token window: about 40%
//! the last 48 hours of notes (cut from the oldest), 40% notes found for the question, 20% tool
//! results. The follow-up turns (decision A) come out of the recent share.

use chrono::{DateTime, FixedOffset, TimeZone};
use rusqlite::Connection;

use super::Turn;
use crate::index::{self, NoteText};

/// The model's window (`ServerConfig::ctx_size`).
pub const WINDOW: usize = 8192;
/// Kept for the instructions (rules, calendar, tools, examples: about 1 300), the message
/// wrappers and the 900-token answer (final review I5: 2 048 was too thin).
pub const RESERVED: usize = 3000;
pub const ANSWER_TOKENS: usize = 900;

/// A cautious token count: about 3 characters a token for words, one token for each digit (the
/// tokenizer splits numbers, and tool results are full of dates and ids).
pub fn tokens(s: &str) -> usize {
    let digits = s.chars().filter(char::is_ascii_digit).count();
    (s.chars().count() - digits).div_ceil(3) + digits
}

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

/// The paragraphs of `body` that hold one of the question's words (or the first one).
fn paragraphs_for(body: &str, words: &[String]) -> String {
    let parts: Vec<&str> = body.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).collect();
    let hit = |p: &str| {
        let folded = index::key(p);
        words.iter().any(|w| folded.contains(w.as_str()))
    };
    let picked: Vec<&str> = parts.iter().copied().filter(|p| hit(p)).take(4).collect();
    if picked.is_empty() { parts.first().copied().unwrap_or_default().to_owned() } else { picked.join("\n\n") }
}

fn question_words(question: &str) -> Vec<String> {
    question
        .split(|c: char| !c.is_alphanumeric())
        .map(index::key)
        .filter(|w| w.chars().count() >= 4)
        .map(|w| w.chars().take(5).collect()) // a stem: Turkish suffixes vary
        .collect()
}

/// Builds the context for `question` with the earlier `turns` (oldest first).
pub fn build(cache: &Connection, question: &str, turns: &[Turn], now: DateTime<FixedOffset>) -> rusqlite::Result<Context> {
    let room = WINDOW - RESERVED - tokens(question).min(700);
    let (mut recent_budget, mut found_budget, tool_budget) = (room * 2 / 5, room * 2 / 5, room / 5);

    // Follow-ups first (from the recent share), newest kept when they do not all fit.
    let mut kept: Vec<Turn> = Vec::new();
    for t in turns.iter().rev() {
        let cost = tokens(&t.question) + tokens(&t.answer) + 8;
        if cost > recent_budget {
            break;
        }
        recent_budget -= cost;
        kept.insert(0, t.clone());
    }

    // FR-MEM-010: newest first; what does not fit is the oldest, the last one that fits is cut.
    let since = now.timestamp_millis() - 48 * 3600 * 1000;
    let mut recent = String::new();
    let mut in_recent: Vec<String> = Vec::new();
    for n in index::recent(cache, since, 40)? {
        let block = note_block(&n, &n.body);
        let cost = tokens(&block);
        if cost <= recent_budget {
            recent_budget -= cost;
            recent.push_str(&block);
            in_recent.push(n.note_path.clone());
        } else {
            if recent_budget > 60 {
                recent.push_str(&cut(&block, recent_budget));
            }
            break;
        }
    }

    // FR-MEM-005 (keyword half for now): paragraphs of the notes the question's words lead to.
    let words = question_words(question);
    let mut found = String::new();
    let mut sources = Vec::new();
    for n in index::retrieve(cache, question, 8)? {
        if in_recent.contains(&n.note_path) {
            sources.push(n.note_path.clone()); // already in full above
            continue;
        }
        let block = note_block(&n, &cut(&paragraphs_for(&n.body, &words), 400));
        let cost = tokens(&block);
        if cost > found_budget {
            continue;
        }
        found_budget -= cost;
        found.push_str(&block);
        sources.push(n.note_path);
    }
    Ok(Context { recent, found, turns: kept, sources, tool_budget })
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
        let ctx = build(&conn, "PLA projesinde hangi kararları aldık?", &[], now()).unwrap();
        assert!(ctx.recent.contains("Dişçi randevusu"), "the last 48 hours");
        assert!(!ctx.recent.contains("Tauri"), "an old note is not recent");
        assert!(ctx.found.contains("Karar: Tauri") && !ctx.found.contains("Başka konu"), "the paragraph that matters");
        assert_eq!(ctx.sources, ["notes/PLA.md"]);
        assert_eq!(ctx.tool_budget, (WINDOW - RESERVED - tokens("PLA projesinde hangi kararları aldık?")) / 5);
    }

    #[test]
    fn too_much_recent_text_is_cut_from_the_oldest() {
        // FR-MEM-010
        let (_t, conn) = cache();
        let ms = now().timestamp_millis();
        let long = "kelime ".repeat(2500); // ~4 300 tokens, more than the recent share
        index::index_note(&conn, "notes/eski.md", &format!("# Eski\n\nESKİ {long}"), ms - 40 * 3_600_000, 10).unwrap();
        index::index_note(&conn, "notes/yeni.md", &format!("# Yeni\n\nYENİ {long}"), ms - 3_600_000, 10).unwrap();
        let ctx = build(&conn, "ne yaptım", &[], now()).unwrap();
        assert!(ctx.recent.contains("YENİ") && !ctx.recent.contains("ESKİ"), "the newest stays, the oldest goes");
        assert!(tokens(&ctx.recent) <= (WINDOW - RESERVED) * 2 / 5);
    }

    #[test]
    fn follow_ups_take_their_room_and_the_oldest_goes_first() {
        let (_t, conn) = cache();
        let big = "a ".repeat(3000);
        let turns = [turn("bir", &big), turn("iki", &big), turn("üç", "kısa")];
        let ctx = build(&conn, "peki ya cuma?", &turns, now()).unwrap();
        assert_eq!(ctx.turns.iter().map(|t| t.question.as_str()).collect::<Vec<_>>(), ["iki", "üç"]);
    }
}
