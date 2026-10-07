//! The assistant panel (FR-QA-001…016): ask, stop, history, clear and undo. The answer itself is
//! made on the worker thread (it owns the model and pla.db) and arrives as `qa-event`s.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Local;
use pla_core::qa::{self, Turn};
use tauri::{AppHandle, Emitter, State};

use crate::commands::AppState;
use crate::worker::{AskJob, QaEvent};

/// The question being answered: its turn and its Stop switch. One at a time.
static CURRENT: Mutex<Option<(String, Arc<AtomicBool>)>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(0);

const MAX_QUESTION: usize = 2000;

/// Ends a turn that never got its last event (the worker was gone, or the job was dropped): the
/// panel hears `Failed` and the next question is not refused as `busy` forever (final review I1).
struct EndGuard {
    turn_id: String,
    ended: Arc<AtomicBool>,
    app: AppHandle,
}

impl Drop for EndGuard {
    fn drop(&mut self) {
        if !self.ended.swap(true, Ordering::SeqCst) {
            finished(&self.turn_id);
            let _ = self.app.emit("qa-event", &QaEvent::Failed { turn_id: self.turn_id.clone(), code: "model".into(), detail: "interrupted".into() });
        }
    }
}

fn finished(turn_id: &str) {
    let mut current = CURRENT.lock().expect("qa lock");
    if current.as_ref().is_some_and(|(id, _)| id == turn_id) {
        *current = None;
    }
}

/// FR-QA-003: starts answering `question`; returns the turn id the events carry. `busy` while
/// another question is still being answered.
#[tauri::command]
pub fn qa_ask(app: AppHandle, state: State<AppState>, question: String, new_topic: bool) -> Result<String, String> {
    let question = question.trim().to_owned();
    if question.is_empty() || question.chars().count() > MAX_QUESTION {
        return Err("bad_question||".into());
    }
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let mut current = CURRENT.lock().expect("qa lock");
    if current.is_some() {
        return Err("busy||".into());
    }
    let turn_id = format!("{}-{}", Local::now().format("%Y%m%d%H%M%S%3f"), NEXT.fetch_add(1, Ordering::SeqCst));
    let cancel = Arc::new(AtomicBool::new(false));
    *current = Some((turn_id.clone(), Arc::clone(&cancel)));
    drop(current);
    let events = app.clone();
    let ended = Arc::new(AtomicBool::new(false));
    let guard = EndGuard { turn_id: turn_id.clone(), ended: Arc::clone(&ended), app: app.clone() };
    session.worker.ask(AskJob {
        turn_id: turn_id.clone(),
        question,
        new_topic,
        cache_path: session.cache_path.clone(),
        memory: Arc::clone(&session.embeds),
        cancel,
        on: Box::new(move |event| {
            let _ = &guard; // lives as long as the job
            match &event {
                // what the assistant wrote shows at once in the task panel, metrics and tree
                QaEvent::Tool { record, .. } if record.ok => match record.undo.as_ref().map(|u| u.kind.as_str()) {
                    Some("note") => {
                        let _ = events.emit("tree-changed", ());
                    }
                    Some(_) => {
                        let _ = events.emit("tasks-changed", ());
                    }
                    None => {}
                },
                QaEvent::Done { turn_id, .. } | QaEvent::Failed { turn_id, .. } => {
                    ended.store(true, Ordering::SeqCst);
                    finished(turn_id);
                }
                _ => {}
            }
            let _ = events.emit("qa-event", &event);
        }),
    });
    Ok(turn_id)
}

/// FR-QA-013: stops the answer being made (the worker gives up within a second).
#[tauri::command]
pub fn qa_stop() {
    if let Some((_, cancel)) = CURRENT.lock().expect("qa lock").as_ref() {
        cancel.store(true, Ordering::SeqCst);
    }
}

/// The panel's history, oldest first, and whether a question is still being answered.
#[tauri::command]
pub fn qa_history(state: State<AppState>) -> Result<Vec<Turn>, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let db = session.db.lock().expect("db lock");
    let turns = qa::history(&db, 100).map_err(|e| format!("io||{e}"));
    turns
}

/// FR-QA-015 (the panel asks first).
#[tauri::command]
pub fn qa_clear(state: State<AppState>) -> Result<(), String> {
    if CURRENT.lock().expect("qa lock").is_some() {
        return Err("busy||".into());
    }
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let db = session.db.lock().expect("db lock");
    let cleared = qa::clear_history(&db).map(|_| ()).map_err(|e| format!("io||{e}"));
    cleared
}

/// FR-QA-008: takes back tool call `index` of turn `turn_id`. A note goes to the Recycle Bin.
#[tauri::command]
pub fn qa_undo(app: AppHandle, state: State<AppState>, turn_id: String, index: usize) -> Result<(), String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault||")?;
    let db = session.db.lock().expect("db lock");
    let undo = qa::take_undo(&db, &turn_id, index).map_err(|e| format!("io||{e}"))?.ok_or("missing||")?;
    let done = if undo.kind == "note" {
        crate::fileops_cmds::deletable(&session.vault, &undo.id).and_then(|abs| crate::fileops_cmds::to_recycle_bin(&abs).map_err(|e| format!("io||{e}")))
    } else {
        qa::undo(&db, &undo, Local::now().fixed_offset()).map_err(|e| format!("io||{e}"))
    };
    if let Err(e) = done {
        let _ = qa::restore_undo(&db, &turn_id, index, undo);
        return Err(e);
    }
    let _ = app.emit(if undo.kind == "note" { "tree-changed" } else { "tasks-changed" }, ());
    Ok(())
}
