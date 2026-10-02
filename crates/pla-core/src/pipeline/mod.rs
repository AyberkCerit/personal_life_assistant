//! The extraction pipeline: note → blocks → model → items in pla.db (SRS FR-EXT).
//! Runs headless; the UI (F4) only enqueues notes and calls `process_queue`.

pub mod blocks;
pub mod items;
pub mod matching;

use chrono::{DateTime, FixedOffset, NaiveDate};
use rusqlite::{params, Connection};

use crate::extraction::{parse_extraction, ValidationSettings};
use crate::llm::LlmError;
use crate::notes::{daily_note_date, is_excluded, is_generated, list_user_notes, split_blocks};
use crate::vault::Vault;

pub use items::{ItemRef, Outcome};

/// The model, as seen by the pipeline. `llm::ModelHost` is the real one; tests script answers.
pub trait Extractor {
    fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PipelineSettings {
    pub similarity_threshold: f64,
    pub validation: ValidationSettings,
}

impl Default for PipelineSettings {
    fn default() -> Self {
        Self { similarity_threshold: 0.6, validation: ValidationSettings::default() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Default)]
pub struct RunReport {
    pub notes_done: usize,
    pub blocks_extracted: usize,
    pub outcomes: Vec<(String, Outcome)>,
    /// Set when the run stopped because the model failed; the rest stays queued (FR-EXT-022).
    pub model_error: Option<String>,
}

pub fn enqueue(conn: &Connection, note_path: &str, now: DateTime<FixedOffset>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO extraction_queue (note_path, queued_at) VALUES (?1, ?2) ON CONFLICT (note_path) DO NOTHING",
        params![note_path, now.to_rfc3339()],
    )?;
    Ok(())
}

/// Queues every user note (first run, or after the app was closed while notes changed).
pub fn enqueue_all(vault: &Vault, conn: &Connection, now: DateTime<FixedOffset>) -> Result<usize, PipelineError> {
    let notes = list_user_notes(vault)?;
    for note in &notes {
        enqueue(conn, note, now)?;
    }
    Ok(notes.len())
}

pub fn queued_notes(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT note_path FROM extraction_queue ORDER BY queued_at, note_path")?
        .query_map([], |r| r.get(0))?
        .collect()
}

/// Processes queued notes in one model session (FR-EXT-008). Only changed blocks reach the model.
pub fn process_queue(
    vault: &Vault,
    conn: &mut Connection,
    extractor: &mut dyn Extractor,
    settings: &PipelineSettings,
    now: DateTime<FixedOffset>,
) -> Result<RunReport, PipelineError> {
    let mut report = RunReport::default();
    for note in queued_notes(conn)? {
        let path = vault.root.join(&note);
        let text = if is_excluded(&note, &vault.config.folders) {
            None
        } else if !path.exists() {
            let tx = conn.transaction()?;
            blocks::mark_note_missing(&tx, &note)?;
            tx.commit()?;
            None
        } else {
            match std::fs::read_to_string(&path) {
                Ok(text) if !is_generated(&text) => Some(text),
                Ok(_) => None,
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => None, // FR-VLT-016: not UTF-8, left alone
                Err(e) => return Err(e.into()),
            }
        };

        if let Some(text) = text {
            let tx = conn.transaction()?;
            let synced = blocks::sync_note_blocks(
                &tx,
                &note,
                &split_blocks(&text),
                daily_note_date(&note, &vault.config.folders),
                now,
                settings.similarity_threshold,
            )?;
            tx.commit()?;

            for block in synced.iter().filter(|b| b.needs_extraction) {
                let answer = match extractor.extract_raw(block.reference_date, &block.text) {
                    Ok(answer) => answer,
                    Err(e) => {
                        conn.execute(
                            "UPDATE extraction_queue SET attempts = attempts + 1, last_error = ?1 WHERE note_path = ?2",
                            params![e.to_string(), note],
                        )?;
                        report.model_error = Some(e.to_string());
                        return Ok(report);
                    }
                };
                let tx = conn.transaction()?;
                let outcomes = match parse_extraction(&answer) {
                    Ok(extraction) => items::apply_block_items(
                        &tx,
                        &block.block_id,
                        block.reference_date,
                        &extraction,
                        &settings.validation,
                        now,
                    )?,
                    Err(e) => vec![items::record_unreadable_answer(&tx, &block.block_id, &answer, &e.to_string(), now)?],
                };
                blocks::mark_extracted(&tx, &block.block_id)?;
                tx.commit()?;
                report.blocks_extracted += 1;
                report.outcomes.extend(outcomes.into_iter().map(|o| (note.clone(), o)));
            }
        }
        conn.execute("DELETE FROM extraction_queue WHERE note_path = ?1", [&note])?;
        report.notes_done += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;
    use crate::vault::open_vault;
    use std::collections::HashMap;

    struct Scripted {
        answers: HashMap<String, String>,
        calls: Vec<(NaiveDate, String)>,
        down: bool,
    }

    impl Scripted {
        fn new(pairs: &[(&str, &str)]) -> Self {
            Self { answers: pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(), calls: Vec::new(), down: false }
        }
    }

    impl Extractor for Scripted {
        fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
            if self.down {
                return Err(LlmError::Http("connection refused".into()));
            }
            self.calls.push((reference, text.to_owned()));
            Ok(self.answers.get(text).cloned().unwrap_or_else(|| r#"{"items": []}"#.into()))
        }
    }

    const T1: &str = "2026-10-06T10:00:00+03:00";
    const T2: &str = "2026-10-07T08:00:00+03:00";
    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn write(vault: &Vault, rel: &str, body: &[u8]) {
        let p = vault.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    fn tasks(conn: &Connection) -> Vec<(String, Option<String>, i64)> {
        conn.prepare("SELECT title, date, source_missing FROM task ORDER BY title")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    fn run(vault: &Vault, conn: &mut Connection, x: &mut Scripted, now: &str) -> RunReport {
        enqueue_all(vault, conn, at(now)).unwrap();
        process_queue(vault, conn, x, &PipelineSettings::default(), at(now)).unwrap()
    }

    const DENTIST: &str = "Yarın 9'da dişçi.";
    const DENTIST_JSON: &str = r#"{"items": [{"type": "task", "title": "Dişçi", "when": {"day_offset": 1, "time": "09:00"}}]}"#;

    #[test]
    fn a_daily_note_becomes_a_task_once() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "daily/2026/2026-10-05.md", format!("{DENTIST}\n\nBugün çok yorgundum.").as_bytes());
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);

        let report = run(&vault, &mut conn, &mut x, T1);
        assert_eq!(report.notes_done, 1);
        assert_eq!(report.blocks_extracted, 2);
        assert_eq!(x.calls[0].0, d("2026-10-05"), "reference = the daily note's date");
        assert_eq!(tasks(&conn), vec![("Dişçi".into(), Some("2026-10-06".into()), 0)]);
        assert!(queued_notes(&conn).unwrap().is_empty());

        let again = run(&vault, &mut conn, &mut x, T2);
        assert_eq!(again.blocks_extracted, 0, "unchanged blocks are not sent again");
        assert_eq!(x.calls.len(), 2);
    }

    #[test]
    fn editing_the_note_updates_instead_of_duplicating() {
        // Review Focus 1
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        let typo = "Yarın 9da dişci.";
        write(&vault, "notes/plan.md", typo.as_bytes());
        let mut x = Scripted::new(&[
            (typo, DENTIST_JSON),
            (DENTIST, r#"{"items": [{"type": "task", "title": "Dişçi randevusu", "when": {"day_offset": 1, "time": "09:00"}}]}"#),
        ]);
        run(&vault, &mut conn, &mut x, T1);
        write(&vault, "notes/plan.md", format!("Önce market.\n\n{DENTIST}").as_bytes());
        run(&vault, &mut conn, &mut x, T2);
        assert_eq!(tasks(&conn), vec![("Dişçi randevusu".into(), Some("2026-10-07".into()), 0)], "reference stays first-seen (T1)");
    }

    #[test]
    fn removing_the_paragraph_or_the_note_keeps_the_task() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/plan.md", format!("{DENTIST}\n\nBaşka.").as_bytes());
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        run(&vault, &mut conn, &mut x, T1);

        write(&vault, "notes/plan.md", b"Ba\xc5\x9fka.");
        run(&vault, &mut conn, &mut x, T2);
        assert_eq!(tasks(&conn)[0].2, 1, "FR-EXT-020");

        std::fs::remove_file(vault.root.join("notes/plan.md")).unwrap();
        enqueue(&conn, "notes/plan.md", at(T2)).unwrap();
        process_queue(&vault, &mut conn, &mut x, &PipelineSettings::default(), at(T2)).unwrap();
        assert_eq!(tasks(&conn).len(), 1, "FR-EXT-021: never deleted");
        assert!(queued_notes(&conn).unwrap().is_empty());
    }

    #[test]
    fn model_outage_keeps_the_work_queued() {
        // Review Focus 3: FR-EXT-022
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/plan.md", DENTIST.as_bytes());
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        x.down = true;

        let report = run(&vault, &mut conn, &mut x, T1);
        assert!(report.model_error.as_deref().is_some_and(|e| e.contains("connection refused")));
        assert_eq!(queued_notes(&conn).unwrap(), vec!["notes/plan.md"]);
        let attempts: i64 = conn.query_row("SELECT attempts FROM extraction_queue", [], |r| r.get(0)).unwrap();
        assert_eq!(attempts, 1);
        assert!(tasks(&conn).is_empty());

        x.down = false;
        let report = process_queue(&vault, &mut conn, &mut x, &PipelineSettings::default(), at(T2)).unwrap();
        assert_eq!(report.model_error, None);
        assert_eq!(tasks(&conn).len(), 1);
        assert_eq!(x.calls[0].0, d("2026-10-06"), "reference = first seen during the failed run");
    }

    #[test]
    fn unreadable_answers_go_to_review_and_are_not_retried() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/plan.md", DENTIST.as_bytes());
        let mut x = Scripted::new(&[(DENTIST, r#"{"items": [{"type": "ta"#)]);
        let report = run(&vault, &mut conn, &mut x, T1);
        assert!(matches!(&report.outcomes[..], [(_, Outcome::Review { .. })]));
        run(&vault, &mut conn, &mut x, T2);
        assert_eq!(x.calls.len(), 1);
    }

    #[test]
    fn generated_excluded_and_unreadable_notes_are_skipped() {
        // Review Focus 5
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/rapor.md", format!("---\npla_generated: true\n---\n{DENTIST}").as_bytes());
        write(&vault, "templates/gunluk.md", DENTIST.as_bytes());
        write(&vault, "notes/eski.md", b"Yar\xfdn di\xfe\xe7i"); // Windows-1254, not UTF-8
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);

        enqueue(&conn, "templates/gunluk.md", at(T1)).unwrap();
        let report = run(&vault, &mut conn, &mut x, T1);
        assert!(x.calls.is_empty());
        assert!(tasks(&conn).is_empty());
        assert!(queued_notes(&conn).unwrap().is_empty());
        assert_eq!(report.model_error, None);
    }
}
