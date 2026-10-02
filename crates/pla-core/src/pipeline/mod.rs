//! The extraction pipeline: note → blocks → model → items in pla.db (SRS FR-EXT).
//! Runs headless; the UI (F4) only enqueues notes and calls `process_queue`.

pub mod blocks;
pub mod items;
pub mod matching;

use chrono::{DateTime, FixedOffset, Local, NaiveDate};
use rusqlite::{params, Connection};

use crate::extraction::{parse_extraction, ValidationSettings};
use crate::llm::LlmError;
use crate::notes::{daily_note_date, is_checked_item, is_excluded, is_generated, list_user_notes, split_blocks};
use crate::vault::Vault;

pub use items::{ItemRef, Outcome};

/// Longer blocks (pasted articles, transcripts) do not fit the model's context; they go to Review.
pub const MAX_BLOCK_CHARS: usize = 2000;

/// The model, as seen by the pipeline. `llm::ModelHost` is the real one; tests script answers.
pub trait Extractor {
    fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError>;
    /// Called about once a second while nothing else happens (lets the host stop an idle model, FR-MDL-013).
    fn tick(&mut self) {}
    fn is_running(&self) -> bool {
        false
    }
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
        "INSERT INTO extraction_queue (note_path, queued_at) VALUES (?1, ?2)
         ON CONFLICT (note_path) DO UPDATE SET generation = generation + 1",
        params![note_path, now.to_rfc3339()],
    )?;
    Ok(())
}

/// Queues every user note (first run, or after the app was closed while notes changed), and every
/// known note that is no longer on disk, so its items get marked (FR-VLT-019).
pub fn enqueue_all(vault: &Vault, conn: &Connection, now: DateTime<FixedOffset>) -> Result<usize, PipelineError> {
    let mut notes = list_user_notes(vault)?;
    let known: Vec<String> = conn
        .prepare("SELECT DISTINCT note_path FROM block WHERE missing = 0")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    notes.extend(known.into_iter().filter(|n| !vault.root.join(n).exists()));
    for note in &notes {
        enqueue(conn, note, now)?;
    }
    Ok(notes.len())
}

pub fn queued_notes(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    conn.prepare("SELECT note_path FROM extraction_queue ORDER BY attempts, queued_at, note_path")?
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
    let mut queue = queued_notes(conn)?;
    // Vanished notes first, so a renamed note can take over their blocks (decision 2026-10-02).
    queue.sort_by_key(|note| vault.root.join(note).exists());
    for note in queue {
        let generation: i64 =
            conn.query_row("SELECT generation FROM extraction_queue WHERE note_path = ?1", [&note], |r| r.get(0))?;
        let path = vault.root.join(&note);
        let mut written = None;
        let text = if is_excluded(&note, &vault.config.folders) {
            None
        } else {
            match std::fs::metadata(&path).and_then(|meta| {
                written = meta.modified().ok().map(|t| DateTime::<Local>::from(t).fixed_offset());
                std::fs::read_to_string(&path)
            }) {
                Ok(text) if !is_generated(&text) => Some(text),
                Ok(_) => None,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let tx = conn.transaction()?;
                    blocks::mark_note_missing(&tx, &note, now)?;
                    tx.commit()?;
                    None
                }
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => None, // FR-VLT-016: not UTF-8, left alone
                Err(e) => {
                    // Locked or unreadable right now: try again next run, keep going with the others.
                    conn.execute(
                        "UPDATE extraction_queue SET attempts = attempts + 1, last_error = ?1 WHERE note_path = ?2",
                        params![e.to_string(), note],
                    )?;
                    continue;
                }
            }
        };

        if let Some(text) = text {
            let tx = conn.transaction()?;
            let synced = blocks::sync_note_blocks(
                &tx,
                &note,
                &split_blocks(&text),
                daily_note_date(&note, &vault.config.folders),
                written,
                now,
                settings.similarity_threshold,
            )?;
            tx.commit()?;

            for block in synced.iter().filter(|b| b.needs_extraction) {
                if is_checked_item(&block.text) {
                    blocks::mark_extracted(conn, &block.block_id)?;
                    continue;
                }
                let answer = if block.text.chars().count() > MAX_BLOCK_CHARS {
                    Err("block too long for the model".to_owned())
                } else {
                    match extractor.extract_raw(block.reference_date, &block.text) {
                        Ok(answer) => Ok(answer),
                        // The server refused or gave up on this block: its problem, not the run's (FR-EXT-022 covers outages).
                        Err(e @ (LlmError::Status(400..=499) | LlmError::Timeout | LlmError::BadResponse)) => Err(e.to_string()),
                        Err(e) => {
                            conn.execute(
                                "UPDATE extraction_queue SET attempts = attempts + 1, last_error = ?1 WHERE note_path = ?2",
                                params![e.to_string(), note],
                            )?;
                            report.model_error = Some(e.to_string());
                            return Ok(report);
                        }
                    }
                };
                let tx = conn.transaction()?;
                let outcomes = match answer {
                    Err(problem) => vec![items::record_block_problem(&tx, &block.block_id, &problem, now)?],
                    Ok(answer) => match parse_extraction(&answer) {
                        Ok(extraction) => items::apply_block_items(
                            &tx,
                            &block.block_id,
                            block.reference_date,
                            &extraction,
                            &settings.validation,
                            block.first_sight.then(|| now.date_naive()),
                            now,
                        )?,
                        Err(e) => vec![items::record_unreadable_answer(&tx, &block.block_id, &answer, &e.to_string(), now)?],
                    },
                };
                blocks::mark_extracted(&tx, &block.block_id)?;
                tx.commit()?;
                report.blocks_extracted += 1;
                report.outcomes.extend(outcomes.into_iter().map(|o| (note.clone(), o)));
            }
        }
        // Only if nobody re-queued the note meanwhile (an edit during processing must not be lost).
        conn.execute("DELETE FROM extraction_queue WHERE note_path = ?1 AND generation = ?2", params![note, generation])?;
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
        rejects: HashMap<String, u16>,
        /// Enqueues this note through a second connection during the first call (an edit arriving mid-run).
        side_enqueue: Option<(Connection, String)>,
    }

    impl Scripted {
        fn new(pairs: &[(&str, &str)]) -> Self {
            Self {
                answers: pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
                calls: Vec::new(),
                down: false,
                rejects: HashMap::new(),
                side_enqueue: None,
            }
        }
    }

    impl Extractor for Scripted {
        fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
            if self.down {
                return Err(LlmError::Http("connection refused".into()));
            }
            if let Some((side, note)) = self.side_enqueue.take() {
                enqueue(&side, &note, at(T2)).unwrap();
            }
            if let Some(code) = self.rejects.get(text) {
                return Err(LlmError::Status(*code));
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
    /// Writes a note "just now": tests simulate `now`, so the real file time is pushed past it
    /// (a file time later than `now` counts as written now).
    fn write(vault: &Vault, rel: &str, body: &[u8]) {
        let p = vault.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        let later = std::time::SystemTime::from(DateTime::parse_from_rfc3339("2030-01-01T00:00:00+03:00").unwrap());
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(later).unwrap();
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

    #[test]
    fn a_block_the_model_rejects_does_not_block_the_vault() {
        // Final review C1: a request the server refuses (e.g. too long) is the block's problem, not the run's
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/a.md", b"Sorunlu blok.");
        write(&vault, "notes/b.md", DENTIST.as_bytes());
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        x.rejects.insert("Sorunlu blok.".into(), 400);

        let report = run(&vault, &mut conn, &mut x, T1);
        assert_eq!(report.model_error, None);
        assert_eq!(tasks(&conn).len(), 1, "the other note is processed");
        assert!(queued_notes(&conn).unwrap().is_empty());
        let reviews: i64 = conn.query_row("SELECT COUNT(*) FROM review_item", [], |r| r.get(0)).unwrap();
        assert_eq!(reviews, 1);
        run(&vault, &mut conn, &mut x, T2);
        assert_eq!(x.calls.len(), 1, "the failed block is not retried forever");
    }

    #[test]
    fn an_oversized_block_goes_to_review_without_a_model_call() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/makale.md", "uzun ".repeat(1000).as_bytes());
        let mut x = Scripted::new(&[]);
        let report = run(&vault, &mut conn, &mut x, T1);
        assert!(x.calls.is_empty());
        assert!(
            matches!(&report.outcomes[..], [(_, Outcome::Review { reason, .. })] if reason.contains("too long")),
            "{:?}",
            report.outcomes
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_unreadable_note_does_not_stop_the_others() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/a.md", b"Kilitli.");
        write(&vault, "notes/b.md", DENTIST.as_bytes());
        let _lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(vault.root.join("notes/a.md")).unwrap();
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        let report = run(&vault, &mut conn, &mut x, T1);
        assert_eq!(tasks(&conn).len(), 1);
        assert_eq!(queued_notes(&conn).unwrap(), vec!["notes/a.md"], "kept for the next run");
        assert_eq!(report.notes_done, 1);
    }

    #[test]
    fn an_edit_arriving_during_processing_stays_queued() {
        // Final review I3
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let data = tmp.path().join(".data");
        let mut conn = open_databases(&data).unwrap().pla;
        write(&vault, "notes/plan.md", DENTIST.as_bytes());
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        x.side_enqueue = Some((Connection::open(data.join("pla.db")).unwrap(), "notes/plan.md".into()));
        run(&vault, &mut conn, &mut x, T1);
        assert_eq!(queued_notes(&conn).unwrap(), vec!["notes/plan.md"]);
    }

    #[test]
    fn a_note_deleted_while_the_app_was_closed_is_noticed() {
        // Final review I4: FR-VLT-019
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/plan.md", DENTIST.as_bytes());
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        run(&vault, &mut conn, &mut x, T1);
        std::fs::remove_file(vault.root.join("notes/plan.md")).unwrap();
        run(&vault, &mut conn, &mut x, T2);
        assert_eq!(tasks(&conn)[0].2, 1);
    }

    #[test]
    fn splitting_a_paragraph_into_a_list_keeps_the_tasks() {
        // Final review I1
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        let para = "Yarın dişçiye git, cuma faturayı öde.";
        let both = r#"{"items": [{"type": "task", "title": "Dişçi", "when": {"day_offset": 1}}, {"type": "task", "title": "Fatura öde", "when": {"weekday": "fri"}}]}"#;
        let mut x = Scripted::new(&[
            (para, both),
            ("- Yarın dişçiye git", r#"{"items": [{"type": "task", "title": "Dişçi", "when": {"day_offset": 1}}]}"#),
            ("- Cuma faturayı öde", r#"{"items": [{"type": "task", "title": "Fatura öde", "when": {"weekday": "fri"}}]}"#),
        ]);
        write(&vault, "notes/plan.md", para.as_bytes());
        run(&vault, &mut conn, &mut x, T1);
        write(&vault, "notes/plan.md", "- Yarın dişçiye git\n- Cuma faturayı öde".as_bytes());
        run(&vault, &mut conn, &mut x, T2);
        assert_eq!(
            tasks(&conn),
            vec![("Dişçi".into(), Some("2026-10-07".into()), 0), ("Fatura öde".into(), Some("2026-10-09".into()), 0)]
        );
    }

    #[test]
    fn an_old_note_is_dated_by_its_file_time() {
        // Final review I5
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/eski.md", DENTIST.as_bytes());
        let written = std::time::SystemTime::from(DateTime::parse_from_rfc3339("2025-03-01T09:00:00+03:00").unwrap());
        std::fs::File::options().write(true).open(vault.root.join("notes/eski.md")).unwrap().set_modified(written).unwrap();
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        run(&vault, &mut conn, &mut x, T1);
        assert_eq!(x.calls[0].0, d("2025-03-01"));
    }
    #[test]
    fn checked_items_never_reach_the_model() {
        // Decision 2
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/liste.md", format!("- [x] {DENTIST}\n- [ ] {DENTIST}").as_bytes());
        let mut x = Scripted::new(&[(&*format!("- [ ] {DENTIST}"), DENTIST_JSON)]);
        run(&vault, &mut conn, &mut x, T1);
        assert_eq!(x.calls.len(), 1, "only the unchecked item is sent");
        assert_eq!(tasks(&conn).len(), 1);
    }

    #[test]
    fn importing_an_old_note_creates_no_past_tasks() {
        // Decision 3
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/eski.md", DENTIST.as_bytes());
        let old = std::time::SystemTime::from(DateTime::parse_from_rfc3339("2025-03-01T09:00:00+03:00").unwrap());
        std::fs::File::options().write(true).open(vault.root.join("notes/eski.md")).unwrap().set_modified(old).unwrap();
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        let report = run(&vault, &mut conn, &mut x, T1);
        assert!(tasks(&conn).is_empty());
        assert!(matches!(&report.outcomes[..], [(_, Outcome::SkippedPast)]));
    }

    #[test]
    fn a_note_renamed_outside_pla_keeps_its_tasks() {
        // Decision 4
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let mut conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        write(&vault, "notes/plan.md", DENTIST.as_bytes());
        let mut x = Scripted::new(&[(DENTIST, DENTIST_JSON)]);
        run(&vault, &mut conn, &mut x, T1);
        std::fs::rename(vault.root.join("notes/plan.md"), vault.root.join("notes/yeni ad.md")).unwrap();
        run(&vault, &mut conn, &mut x, T1);
        assert_eq!(tasks(&conn), vec![("Dişçi".into(), Some("2026-10-07".into()), 0)]);
        assert_eq!(x.calls.len(), 1, "unchanged text is not sent again");
        let note: String = conn.query_row("SELECT note_path FROM block", [], |r| r.get(0)).unwrap();
        assert_eq!(note, "notes/yeni ad.md");
    }
}
