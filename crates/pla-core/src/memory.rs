//! Semantic memory (FR-MEM-001…005, 011, 013): notes in 200–400-token chunks, their vectors in
//! cache.db (sqlite-vec), and a hybrid search that joins keyword and vector rankings.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use rusqlite::{params, Connection, OptionalExtension};

use crate::llm::LlmError;
use crate::qa::context::tokens;

/// The vector size of EmbeddingGemma (FR-MEM-004).
pub const DIMENSIONS: usize = 768;
const MIN_TOKENS: usize = 200;
const MAX_TOKENS: usize = 400;

/// Makes `sqlite-vec` part of every connection opened from now on. Called before the first open.
pub fn register_vec() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: the documented way to register sqlite-vec's entry point as an auto extension;
        // the function has the signature SQLite expects for an extension initialiser.
        unsafe {
            #[allow(clippy::missing_transmute_annotations)]
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ())));
        }
    });
}

/// A model that turns texts into vectors (the EmbeddingGemma server, or a stand-in in tests).
pub trait Embedder {
    /// Stored with each vector: another model means everything is embedded again (FR-MEM-013).
    fn model_id(&self) -> &str;
    /// One vector per text, in order. Stops with `LlmError::Cancelled` once `stop` is set.
    fn embed(&mut self, texts: &[String], stop: &AtomicBool) -> Result<Vec<Vec<f32>>, LlmError>;
    /// Called while nothing else happens: lets the host stop an idle server (FR-MDL-013).
    fn tick(&mut self) {}
}

/// EmbeddingGemma's prompts: a question, and a document with its title.
pub fn query_text(question: &str) -> String {
    format!("task: search result | query: {question}")
}

pub fn document_text(title: &str, heading: &str, text: &str) -> String {
    let title = if title.trim().is_empty() { "none" } else { title.trim() };
    if heading.is_empty() { format!("title: {title} | text: {text}") } else { format!("title: {title} | text: {heading}\n{text}") }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub heading: String,
    pub text: String,
    pub tokens: usize,
}

/// Splits `text` into pieces of at most `MAX_TOKENS`, at line ends, then at spaces.
fn split_long(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in text.split_inclusive([' ', '\n']) {
        if !current.is_empty() && tokens(&current) + tokens(word) > MAX_TOKENS {
            out.push(std::mem::take(&mut current).trim().to_owned());
        }
        current.push_str(word);
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

/// FR-MEM-001: a note body in chunks of about 200–400 tokens, cut at headings and paragraphs. A
/// chunk carries the heading its first text sits under.
pub fn chunk(body: &str) -> Vec<Chunk> {
    struct Acc {
        out: Vec<Chunk>,
        parts: Vec<String>,
        size: usize,
        heading: String,
    }
    impl Acc {
        fn flush(&mut self) {
            let text = self.parts.join("\n\n").trim().to_owned();
            if !text.is_empty() {
                self.out.push(Chunk { heading: self.heading.clone(), tokens: tokens(&text), text });
            }
            self.parts.clear();
            self.size = 0;
        }
        fn add(&mut self, piece: String, under: &str) {
            if self.size + tokens(&piece) > MAX_TOKENS {
                self.flush();
            }
            if self.parts.is_empty() {
                self.heading = under.to_owned();
            }
            self.size += tokens(&piece);
            self.parts.push(piece);
        }
    }
    let mut acc = Acc { out: Vec::new(), parts: Vec::new(), size: 0, heading: String::new() };
    let mut under = String::new();
    for block in body.split("\n\n").map(str::trim).filter(|b| !b.is_empty()) {
        let mut text = block.to_owned();
        if let Some(first) = block.lines().next().filter(|l| l.starts_with('#')) {
            // a heading starts a new chunk once the current one is big enough
            if acc.size >= MIN_TOKENS {
                acc.flush();
            }
            under = first.trim_start_matches('#').trim().to_owned();
            text = block.lines().skip(1).collect::<Vec<_>>().join("\n");
        }
        for piece in split_long(text.trim()) {
            acc.add(piece, &under);
        }
    }
    acc.flush();
    acc.out
}

fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// FR-MEM-002: keeps the chunks (and vectors) of `rel` whose text did not change; the rest wait
/// for embedding. Called inside the indexer's transaction for the note.
pub fn sync_chunks(conn: &Connection, rel: &str, body: &str) -> rusqlite::Result<()> {
    let new = chunk(body);
    let old: Vec<(i64, String)> =
        conn.prepare("SELECT chunk_id, text_hash FROM chunk WHERE note_path = ?1")?.query_map([rel], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut keep: Vec<i64> = Vec::new();
    for (ord, c) in new.iter().enumerate() {
        let h = hash(&format!("{}\n{}", c.heading, c.text));
        match old.iter().find(|(id, oh)| *oh == h && !keep.contains(id)) {
            Some((id, _)) => {
                conn.execute("UPDATE chunk SET ord = ?2 WHERE chunk_id = ?1", params![id, ord as i64])?;
                keep.push(*id);
            }
            None => {
                conn.execute(
                    "INSERT INTO chunk (note_path, ord, heading, text, token_count, text_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![rel, ord as i64, c.heading, c.text, c.tokens as i64, h],
                )?;
                let id = conn.last_insert_rowid();
                conn.execute("INSERT INTO chunk_fts (rowid, text) VALUES (?1, ?2)", params![id, format!("{}\n{}", c.heading, c.text)])?;
                keep.push(id);
            }
        }
    }
    for (id, _) in old.iter().filter(|(id, _)| !keep.contains(id)) {
        delete_chunk(conn, *id)?;
    }
    Ok(())
}

fn delete_chunk(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM chunk WHERE chunk_id = ?1", [id])?;
    conn.execute("DELETE FROM chunk_fts WHERE rowid = ?1", [id])?;
    conn.execute("DELETE FROM chunk_vec WHERE chunk_id = ?1", [id])?;
    Ok(())
}

pub fn drop_chunks(conn: &Connection, rel: &str) -> rusqlite::Result<()> {
    let ids: Vec<i64> = conn.prepare("SELECT chunk_id FROM chunk WHERE note_path = ?1")?.query_map([rel], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for id in ids {
        delete_chunk(conn, id)?;
    }
    Ok(())
}

/// Chunks still to embed with `model` (never embedded, changed, or embedded with another model).
pub fn pending(conn: &Connection, model: &str) -> rusqlite::Result<usize> {
    conn.query_row("SELECT count(*) FROM chunk WHERE embedded_model IS NOT ?1", [model], |r| r.get::<_, i64>(0)).map(|n| n as usize)
}

pub fn total(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row("SELECT count(*) FROM chunk", [], |r| r.get::<_, i64>(0)).map(|n| n as usize)
}

fn as_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedReport {
    pub embedded: usize,
    pub left: usize,
}

/// FR-MEM-003: embeds pending chunks in groups of `batch` until none are left, `until` passes
/// or `stop` is set. What is done stays done: a group is written as soon as it is embedded.
pub fn embed_pending(conn: &Connection, embedder: &mut dyn Embedder, batch: usize, until: Instant, stop: &AtomicBool) -> Result<EmbedReport, EmbedError> {
    let model = embedder.model_id().to_owned();
    let mut embedded = 0;
    while Instant::now() < until && !stop.load(Ordering::SeqCst) {
        let group: Vec<(i64, String)> = conn
            .prepare(
                "SELECT c.chunk_id, coalesce(n.title, ''), c.heading, c.text FROM chunk c LEFT JOIN note_index n ON n.note_path = c.note_path
                 WHERE c.embedded_model IS NOT ?1 ORDER BY c.chunk_id LIMIT ?2",
            )?
            .query_map(params![model, batch as i64], |r| Ok((r.get(0)?, document_text(&r.get::<_, String>(1)?, &r.get::<_, String>(2)?, &r.get::<_, String>(3)?))))?
            .collect::<Result<_, _>>()?;
        if group.is_empty() {
            break;
        }
        let texts: Vec<String> = group.iter().map(|(_, t)| t.clone()).collect();
        let vectors = embedder.embed(&texts, stop)?;
        if vectors.len() != group.len() || vectors.iter().any(|v| v.len() != DIMENSIONS) {
            return Err(EmbedError::Model(LlmError::BadResponse));
        }
        let tx = conn.unchecked_transaction()?;
        for ((id, _), v) in group.iter().zip(&vectors) {
            // the chunk may have changed meanwhile: only a row still there gets its vector
            let still: Option<i64> = tx.query_row("SELECT chunk_id FROM chunk WHERE chunk_id = ?1", [id], |r| r.get(0)).optional()?;
            if still.is_none() {
                continue;
            }
            tx.execute("DELETE FROM chunk_vec WHERE chunk_id = ?1", [id])?;
            tx.execute("INSERT INTO chunk_vec (chunk_id, embedding) VALUES (?1, ?2)", params![id, as_blob(v)])?;
            tx.execute("UPDATE chunk SET embedded_model = ?2 WHERE chunk_id = ?1", params![id, model])?;
        }
        tx.commit()?;
        embedded += group.len();
    }
    Ok(EmbedReport { embedded, left: pending(conn, &model)? })
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error(transparent)]
    Model(#[from] LlmError),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

/// One chunk found for a question.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHit {
    pub note_path: String,
    pub title: String,
    pub heading: String,
    pub text: String,
    pub mtime: i64,
    pub score: f64,
}

/// Reciprocal rank fusion's constant: ranks matter, raw scores of the two searches do not.
const RRF_K: f64 = 60.0;
const CANDIDATES: usize = 20;

/// FR-MEM-005/011: chunks for `question`, keyword (FTS5) and vector (`query`, embedded with
/// `model`) rankings joined by reciprocal rank fusion. Chunks under `reports` count half: a
/// generated report never outranks the notes it came from. Without a vector, keywords alone.
pub fn hybrid(conn: &Connection, question: &str, query: Option<(&[f32], &str)>, reports: &str, limit: usize) -> rusqlite::Result<Vec<ChunkHit>> {
    let mut scores: Vec<(i64, f64)> = Vec::new();
    let mut add = |id: i64, rank: usize| match scores.iter_mut().find(|(i, _)| *i == id) {
        Some((_, s)) => *s += 1.0 / (RRF_K + rank as f64),
        None => scores.push((id, 1.0 / (RRF_K + rank as f64))),
    };
    if let Some(q) = crate::index::or_query(question) {
        let ids: Vec<i64> = conn
            .prepare("SELECT rowid FROM chunk_fts WHERE chunk_fts MATCH ?1 ORDER BY rank LIMIT ?2")?
            .query_map(params![q, CANDIDATES as i64], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for (rank, id) in ids.into_iter().enumerate() {
            add(id, rank + 1);
        }
    }
    if let Some((vector, model)) = query {
        // only vectors of this model compare (FR-MEM-013: a changed model is re-embedded)
        let ids: Vec<i64> = conn
            .prepare(
                "SELECT v.chunk_id FROM chunk_vec v JOIN chunk c ON c.chunk_id = v.chunk_id
                 WHERE v.embedding MATCH ?1 AND k = ?2 AND c.embedded_model = ?3 ORDER BY v.distance",
            )?
            .query_map(params![as_blob(vector), CANDIDATES as i64, model], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for (rank, id) in ids.into_iter().enumerate() {
            add(id, rank + 1);
        }
    }
    let reports = format!("{}/", reports.trim_matches('/'));
    let mut hits: Vec<ChunkHit> = Vec::new();
    for (id, score) in scores {
        let hit = conn
            .query_row(
                "SELECT c.note_path, coalesce(n.title, ''), c.heading, c.text, coalesce(n.mtime, 0) FROM chunk c
                 LEFT JOIN note_index n ON n.note_path = c.note_path WHERE c.chunk_id = ?1",
                [id],
                |r| Ok(ChunkHit { note_path: r.get(0)?, title: r.get(1)?, heading: r.get(2)?, text: r.get(3)?, mtime: r.get(4)?, score }),
            )
            .optional()?;
        if let Some(mut h) = hit {
            if h.note_path.starts_with(&reports) {
                h.score /= 2.0;
            }
            hits.push(h);
        }
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.truncate(limit);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Words to fixed directions: texts about the same thing point the same way.
    struct Topics {
        calls: usize,
    }
    impl Embedder for Topics {
        fn model_id(&self) -> &str {
            "test-topics-1"
        }
        fn embed(&mut self, texts: &[String], _: &AtomicBool) -> Result<Vec<Vec<f32>>, LlmError> {
            self.calls += 1;
            Ok(texts
                .iter()
                .map(|t| {
                    let t = t.to_lowercase();
                    let mut v = vec![0.01f32; DIMENSIONS];
                    if t.contains("dişçi") || t.contains("ağız") || t.contains("diş") {
                        v[0] = 1.0;
                    }
                    if t.contains("fatura") || t.contains("ödeme") {
                        v[1] = 1.0;
                    }
                    v
                })
                .collect())
        }
    }

    fn cache() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = crate::db::open_databases(tmp.path()).unwrap().cache;
        (tmp, conn)
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    #[test]
    fn notes_are_cut_at_headings_and_paragraphs() {
        // FR-MEM-001
        let para = "kelime ".repeat(200); // ~470 tokens: more than one chunk
        let body = format!("# Giriş\n\nKısa giriş.\n\n## Ayrıntı\n\n{para}\n\nSon paragraf.");
        let chunks = chunk(&body);
        assert!(chunks.iter().all(|c| c.tokens <= MAX_TOKENS), "{chunks:?}");
        assert_eq!(chunks[0].heading, "Giriş");
        assert!(chunks.iter().any(|c| c.heading == "Ayrıntı"));
        assert!(chunk("").is_empty());
        let small = chunk("Bir.\n\nİki.\n\nÜç.");
        assert_eq!(small.len(), 1, "small paragraphs share a chunk");
    }

    #[test]
    fn only_changed_chunks_are_embedded_again() {
        // FR-MEM-002/003
        let (_t, conn) = cache();
        crate::index::index_note(&conn, "notes/a.md", "# A\n\nYarın dişçi randevusu.\n\n## Para\n\n".to_owned().as_str(), 1, 1).unwrap();
        let mut model = Topics { calls: 0 };
        assert!(pending(&conn, model.model_id()).unwrap() > 0);
        let r = embed_pending(&conn, &mut model, 8, far(), &AtomicBool::new(false)).unwrap();
        assert_eq!(r.left, 0);
        let before = r.embedded;
        crate::index::index_note(&conn, "notes/a.md", "# A\n\nYarın dişçi randevusu.\n\nYeni: fatura öde.", 2, 1).unwrap();
        assert_eq!(pending(&conn, model.model_id()).unwrap(), 1, "only the new text");
        assert!(before >= 1);
        crate::index::remove_note(&conn, "notes/a.md").unwrap();
        assert_eq!(total(&conn).unwrap(), 0);
        let vectors: i64 = conn.query_row("SELECT count(*) FROM chunk_vec", [], |r| r.get(0)).unwrap();
        assert_eq!(vectors, 0, "no vector outlives its chunk");
    }

    #[test]
    fn meaning_finds_what_words_miss() {
        // FR-MEM-005: "ağız sağlığı" shares no word with "dişçi", but the meaning is close
        let (_t, conn) = cache();
        crate::index::index_note(&conn, "notes/dis.md", "# Randevu\n\nCuma günü dişçi var.", 1, 1).unwrap();
        crate::index::index_note(&conn, "notes/para.md", "# Para\n\nKira ve fatura ödemesi.", 1, 1).unwrap();
        crate::index::index_note(&conn, "reports/weekly/w.md", "# Hafta\n\nDişçi randevusu yapıldı.", 1, 1).unwrap();
        let mut model = Topics { calls: 0 };
        embed_pending(&conn, &mut model, 8, far(), &AtomicBool::new(false)).unwrap();
        let q = model.embed(&[query_text("ağız sağlığı kontrolüm ne zaman?")], &AtomicBool::new(false)).unwrap().remove(0);
        let hits = hybrid(&conn, "ağız sağlığı kontrolüm ne zaman?", Some((&q, "test-topics-1")), "reports", 3).unwrap();
        assert_eq!(hits[0].note_path, "notes/dis.md", "{hits:?}");
        assert!(hits.iter().position(|h| h.note_path.starts_with("reports/")) > Some(0), "reports rank below notes (FR-MEM-011)");
        // without the model: keywords only, still useful
        let words = hybrid(&conn, "fatura", None, "reports", 3).unwrap();
        assert_eq!(words[0].note_path, "notes/para.md");
        // vectors of another model are not compared (FR-MEM-013)
        assert!(hybrid(&conn, "xyz", Some((&q, "other-model")), "reports", 3).unwrap().is_empty());
        assert_eq!(pending(&conn, "other-model").unwrap(), total(&conn).unwrap(), "a new model embeds everything again");
    }

    #[test]
    fn embedding_stops_when_asked_and_keeps_what_it_did() {
        let (_t, conn) = cache();
        for i in 0..5 {
            crate::index::index_note(&conn, &format!("notes/{i}.md"), &format!("# N{i}\n\nNot {i} dişçi."), 1, 1).unwrap();
        }
        let mut model = Topics { calls: 0 };
        let r = embed_pending(&conn, &mut model, 2, far(), &AtomicBool::new(true)).unwrap();
        assert_eq!((r.embedded, model.calls), (0, 0), "stopped before starting");
        let r = embed_pending(&conn, &mut model, 2, Instant::now(), &AtomicBool::new(false)).unwrap();
        assert_eq!(r.embedded, 0, "no time left");
        let r = embed_pending(&conn, &mut model, 2, far(), &AtomicBool::new(false)).unwrap();
        assert_eq!((r.embedded, r.left, model.calls), (5, 0, 3));
    }

    #[test]
    fn prompts_follow_the_model_card() {
        assert_eq!(query_text("diş"), "task: search result | query: diş");
        assert_eq!(document_text("", "", "x"), "title: none | text: x");
        assert_eq!(document_text("Plan", "Hedef", "x"), "title: Plan | text: Hedef\nx");
    }
}
