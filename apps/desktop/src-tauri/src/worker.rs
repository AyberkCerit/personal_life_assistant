//! The background worker: the only owner of pla.db. It queues notes, runs the extraction pipeline
//! in one model session (FR-EXT-008), keeps work queued when there is no model (FR-EXT-022) and lets
//! the model host stop an idle model (FR-MDL-013). The UI learns everything through `WorkerStatus`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Local};
use chrono::NaiveDate;
use pla_core::llm::LlmError;
use pla_core::pipeline::items::ItemRef;
use pla_core::pipeline::{enqueue, enqueue_all, process_queue_with, queued_notes, Extractor, Outcome, PipelineSettings};
use pla_core::vault::Vault;
use rusqlite::Connection;
use serde::Serialize;

const TICK: Duration = Duration::from_secs(1);
/// Decision 5: an outside change is extracted after this long without further changes to the note.
const EXTERNAL_DELAY: Duration = Duration::from_secs(45);
/// FR-MDL-013: the model process stops after this long without requests.
const MODEL_IDLE_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub enum Command {
    Enqueue(String),
    EnqueueLater(String),
    /// A folder changed outside PLA: queue every note again (and the ones that vanished).
    Rescan,
    /// FR-SCH-014 / FR-EXT-026: stop processing (notes still queue) until resumed.
    Pause(bool),
    /// A model became available (download or chosen file): use it from now on.
    UseModel(pla_core::llm::ServerConfig),
    /// FR-MDL-019: stop and forget the model (ends llama-server), then answer, so the file can go.
    DropModel(std::sync::mpsc::Sender<()>),
    Shutdown,
}

/// The running worker. Dropping it (or `shutdown`) stops the current run between two model calls,
/// waits for the thread, and so drops the model host, which ends llama-server (FR-MDL-017).
pub struct WorkerHandle {
    tx: Sender<Command>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    /// Pauses at once, also in the middle of a run (between two model calls), then tells the thread.
    pub fn set_paused(&self, paused: bool) {
        self.pause.store(paused, Ordering::SeqCst);
        self.send(Command::Pause(paused));
    }

    pub fn sender(&self) -> Sender<Command> {
        self.tx.clone()
    }

    pub fn shutdown(self) {}

    fn stop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Command::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Lets a closing session interrupt a long run: once cancelled, the next model call fails like an
/// outage, so the pipeline stops and keeps the rest queued (FR-EXT-022).
struct Cancellable<'a> {
    inner: &'a mut (dyn Extractor + Send),
    cancel: &'a AtomicBool,
    pause: &'a AtomicBool,
}

impl Extractor for Cancellable<'_> {
    fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(LlmError::Http("PLA is closing".into()));
        }
        if self.pause.load(Ordering::SeqCst) {
            return Err(LlmError::Http("background AI is paused".into()));
        }
        self.inner.extract_raw(reference, text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    NotInstalled,
    Off,
    Running,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkerStatus {
    pub queued: usize,
    pub model: ModelState,
    pub busy: bool,
    pub last_error: Option<String>,
    /// Items added since the app started (the task panel arrives in F4b).
    pub added: usize,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddedItem {
    pub kind: String,
    pub id: String,
    pub title: String,
}

type AddedListener = Box<dyn Fn(&[AddedItem]) + Send>;

type Notify = Box<dyn Fn(&WorkerStatus) + Send>;
type Clock = Box<dyn Fn() -> DateTime<FixedOffset> + Send>;
type ExtractorFactory = Box<dyn Fn(pla_core::llm::ServerConfig) -> Box<dyn Extractor + Send> + Send>;

pub struct Worker {
    vault: Vault,
    conn: Connection,
    extractor: Option<Box<dyn Extractor + Send>>,
    make_extractor: ExtractorFactory,
    notify: Notify,
    clock: Clock,
    external_delay: Duration,
    later: HashMap<String, Instant>,
    status: WorkerStatus,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    on_added: Option<AddedListener>,
    /// Reads titles of added items while the main connection is busy in the pipeline.
    reader: Option<Connection>,
}

impl Worker {
    pub fn new(vault: Vault, conn: Connection, extractor: Option<Box<dyn Extractor + Send>>, notify: Notify) -> Self {
        let model = if extractor.is_some() { ModelState::Off } else { ModelState::NotInstalled };
        let reader = conn.path().and_then(|p| Connection::open(p).ok());
        Self {
            vault,
            conn,
            extractor,
            make_extractor: Box::new(|cfg| Box::new(pla_core::llm::ModelHost::new(cfg, MODEL_IDLE_AFTER)) as Box<dyn Extractor + Send>),
            notify,
            clock: Box::new(|| Local::now().fixed_offset()),
            external_delay: EXTERNAL_DELAY,
            later: HashMap::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
            on_added: None,
            reader,
            status: WorkerStatus { queued: 0, model, busy: false, last_error: None, added: 0, paused: false },
        }
    }

    pub fn with_added_listener(mut self, listener: AddedListener) -> Self {
        self.on_added = Some(listener);
        self
    }

    pub fn with_extractor_factory(mut self, f: ExtractorFactory) -> Self {
        self.make_extractor = f;
        self
    }

    pub fn with_external_delay(mut self, delay: Duration) -> Self {
        self.external_delay = delay;
        self
    }

    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    pub fn spawn(self) -> WorkerHandle {
        let (tx, rx) = mpsc::channel();
        let (cancel, pause) = (Arc::clone(&self.cancel), Arc::clone(&self.pause));
        let join = std::thread::Builder::new().name("pla-worker".into()).spawn(move || self.run(rx)).expect("spawn worker");
        WorkerHandle { tx, cancel, pause, join: Some(join) }
    }

    fn run(mut self, rx: Receiver<Command>) {
        // Catch-up: notes changed while PLA was closed (FR-VLT-014), unfinished work (FR-EXT-022).
        let now = (self.clock)();
        if let Err(e) = enqueue_all(&self.vault, &self.conn, now) {
            self.status.last_error = Some(e.to_string());
        }
        self.process();
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Command::Enqueue(note)) => {
                    self.later.remove(&note);
                    self.enqueue_now(&note);
                    self.process();
                }
                Ok(Command::EnqueueLater(note)) => {
                    self.later.insert(note, Instant::now());
                }
                Ok(Command::Pause(paused)) => {
                    self.pause.store(paused, Ordering::SeqCst);
                    self.status.paused = paused;
                    if !paused {
                        self.process();
                    } else {
                        self.publish();
                    }
                }
                Ok(Command::UseModel(cfg)) => {
                    self.extractor = Some((self.make_extractor)(cfg));
                    self.status.model = self.model_state();
                    self.status.last_error = None;
                    self.process();
                }
                Ok(Command::DropModel(done)) => {
                    self.extractor = None; // dropping the host ends llama-server
                    self.status.model = self.model_state();
                    self.publish();
                    let _ = done.send(());
                }
                Ok(Command::Rescan) => {
                    if let Err(e) = enqueue_all(&self.vault, &self.conn, (self.clock)()) {
                        self.status.last_error = Some(e.to_string());
                    }
                    self.process();
                }
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => self.tick(),
            }
            if !self.later.is_empty() {
                self.flush_quiet_external_changes();
            }
        }
    }

    fn tick(&mut self) {
        if let Some(x) = self.extractor.as_mut() {
            x.tick();
        }
        let model = self.model_state();
        if model != self.status.model {
            self.status.model = model;
            self.publish();
        }
    }

    fn flush_quiet_external_changes(&mut self) {
        let due: Vec<String> =
            self.later.iter().filter(|(_, at)| at.elapsed() >= self.external_delay).map(|(n, _)| n.clone()).collect();
        if due.is_empty() {
            return;
        }
        for note in &due {
            self.later.remove(note);
            self.enqueue_now(note);
        }
        self.process();
    }

    fn enqueue_now(&mut self, note: &str) {
        if let Err(e) = enqueue(&self.conn, note, (self.clock)()) {
            self.status.last_error = Some(e.to_string());
        }
    }

    fn process(&mut self) {
        if self.status.paused {
            self.status.queued = queued_notes(&self.conn).map_or(0, |q| q.len());
            self.publish();
            return; // FR-EXT-026: keep queuing, do not process
        }
        if let Some(extractor) = self.extractor.as_mut() {
            self.status.busy = true;
            self.status.queued = queued_notes(&self.conn).map_or(0, |q| q.len());
            (self.notify)(&self.status); // field borrows: the extractor stays borrowed
            let now = (self.clock)();
            let mut guarded = Cancellable { inner: extractor.as_mut(), cancel: &self.cancel, pause: &self.pause };
            let (reader, listener, added) = (&self.reader, &self.on_added, &mut self.status.added);
            // Each note's additions are reported as soon as it is done (FR-EXT-015), not at the end of a long run.
            let mut on_note = |_: &str, outcomes: &[Outcome]| {
                let items: Vec<AddedItem> = outcomes
                    .iter()
                    .filter_map(|o| match o {
                        Outcome::Added(item) => reader.as_ref().and_then(|c| describe(c, item)),
                        _ => None,
                    })
                    .collect();
                *added += outcomes.iter().filter(|o| matches!(o, Outcome::Added(_))).count();
                if let (Some(listener), false) = (listener, items.is_empty()) {
                    listener(&items);
                }
            };
            match process_queue_with(&self.vault, &mut self.conn, &mut guarded, &PipelineSettings::default(), now, &mut on_note) {
                Ok(report) => self.status.last_error = report.model_error,
                Err(e) => self.status.last_error = Some(e.to_string()),
            }
            if self.pause.load(Ordering::SeqCst) {
                self.status.paused = true; // stopped by a pause, not by a failure
                self.status.last_error = None;
            }
            self.status.busy = false;
        }
        self.status.model = self.model_state();
        self.status.queued = queued_notes(&self.conn).map_or(0, |q| q.len());
        self.publish();
    }

    fn model_state(&self) -> ModelState {
        match &self.extractor {
            None => ModelState::NotInstalled,
            Some(x) if x.is_running() => ModelState::Running,
            Some(_) => ModelState::Off,
        }
    }

    fn publish(&self) {
        (self.notify)(&self.status);
    }
}

fn describe(conn: &Connection, item: &ItemRef) -> Option<AddedItem> {
    let (kind, id, sql) = match item {
        ItemRef::Task(id) => ("task", id, "SELECT title FROM task WHERE task_id = ?1"),
        ItemRef::Metric(id) => ("metric", id, "SELECT type FROM metric_record WHERE metric_id = ?1"),
    };
    let title: String = conn.query_row(sql, [id], |r| r.get(0)).ok()?;
    Some(AddedItem { kind: kind.to_owned(), id: id.clone(), title })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pla_core::db::open_databases;
    use pla_core::vault::open_vault;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    struct Fixed(&'static str);
    impl Extractor for Fixed {
        fn extract_raw(&mut self, _: NaiveDate, _: &str) -> Result<String, LlmError> {
            Ok(self.0.to_owned())
        }
    }

    const TASK: &str = r#"{"items": [{"type": "task", "title": "Dişçi", "when": {"day_offset": 1}}]}"#;

    fn wait_for(statuses: &Arc<Mutex<Vec<WorkerStatus>>>, pred: impl Fn(&WorkerStatus) -> bool) -> WorkerStatus {
        let start = Instant::now();
        loop {
            if let Some(s) = statuses.lock().unwrap().iter().rev().find(|s| pred(s)).cloned() {
                return s;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "status never reached: {:?}", statuses.lock().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn setup(extractor: Option<Box<dyn Extractor + Send>>) -> (tempfile::TempDir, WorkerHandle, Arc<Mutex<Vec<WorkerStatus>>>, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        std::fs::write(vault.root.join("notes/plan.md"), "Yarın dişçi.").unwrap();
        let data = tmp.path().join(".data");
        let conn = open_databases(&data).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&statuses);
        let worker = Worker::new(vault, conn, extractor, Box::new(move |s| sink.lock().unwrap().push(s.clone())))
            .with_external_delay(Duration::from_millis(300));
        let handle = worker.spawn();
        (tmp, handle, statuses, data)
    }

    fn task_count(data: &std::path::Path) -> i64 {
        rusqlite::Connection::open(data.join("pla.db")).unwrap().query_row("SELECT COUNT(*) FROM task", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn catches_up_on_start_and_reports_what_it_added() {
        let (_tmp, tx, statuses, data) = setup(Some(Box::new(Fixed(TASK))));
        let done = wait_for(&statuses, |s| !s.busy && s.added == 1);
        assert_eq!(done.queued, 0);
        assert_eq!(task_count(&data), 1);
        tx.shutdown();
    }

    #[test]
    fn without_a_model_notes_wait_in_the_queue() {
        // Review Focus 4
        let (_tmp, tx, statuses, data) = setup(None);
        let s = wait_for(&statuses, |s| s.queued == 1);
        assert_eq!(s.model, ModelState::NotInstalled);
        assert_eq!(task_count(&data), 0);
        tx.shutdown();
    }

    #[test]
    fn external_changes_wait_for_quiet_before_processing() {
        // Decision 5
        let (tmp, tx, statuses, data) = setup(Some(Box::new(Fixed(TASK))));
        wait_for(&statuses, |s| !s.busy && s.added == 1);
        let before = statuses.lock().unwrap().len();
        std::fs::write(tmp.path().join("notes/plan.md"), "Yarın dişçi var.").unwrap();
        tx.send(Command::EnqueueLater("notes/plan.md".into()));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(statuses.lock().unwrap().len(), before, "nothing happens before the quiet period");
        let start = Instant::now();
        loop {
            {
                let s = statuses.lock().unwrap();
                if s.len() > before && !s.last().unwrap().busy {
                    assert_eq!(s.last().unwrap().queued, 0);
                    assert_eq!(s.last().unwrap().last_error, None);
                    break;
                }
            }
            assert!(start.elapsed() < Duration::from_secs(10), "the external change was never processed");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(task_count(&data), 1, "updated, not duplicated");
        tx.shutdown();
    }

    #[test]
    fn rescan_picks_up_notes_added_outside() {
        // Final review I5
        let (tmp, tx, statuses, data) = setup(Some(Box::new(Fixed(TASK))));
        wait_for(&statuses, |s| !s.busy && s.added == 1);
        std::fs::write(tmp.path().join("notes/ikinci.md"), "Cuma fatura.").unwrap();
        tx.send(Command::Rescan);
        wait_for(&statuses, |s| !s.busy && s.added == 2);
        assert_eq!(task_count(&data), 2);
        tx.shutdown();
    }

    struct Slow;
    impl Extractor for Slow {
        fn extract_raw(&mut self, _: NaiveDate, _: &str) -> Result<String, LlmError> {
            std::thread::sleep(Duration::from_millis(300));
            Ok(r#"{"items": []}"#.to_owned())
        }
    }

    #[test]
    fn shutdown_interrupts_a_long_run_and_keeps_the_rest_queued() {
        // Final review I6: replacing or closing a session must not wait for a whole catch-up
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let body: Vec<String> = (0..30).map(|i| format!("Paragraf {i}.")).collect();
        std::fs::write(vault.root.join("notes/uzun.md"), body.join("\n\n")).unwrap();
        let data = tmp.path().join(".data");
        let conn = open_databases(&data).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&statuses);
        let handle = Worker::new(vault, conn, Some(Box::new(Slow)), Box::new(move |s| sink.lock().unwrap().push(s.clone()))).spawn();
        wait_for(&statuses, |s| s.busy);
        let started = Instant::now();
        handle.shutdown();
        assert!(started.elapsed() < Duration::from_secs(3), "shutdown took {:?}", started.elapsed());
        let queued: i64 = rusqlite::Connection::open(data.join("pla.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM extraction_queue", [], |r| r.get(0))
            .unwrap();
        assert_eq!(queued, 1, "the unfinished note waits for the next start");
    }
    #[test]
    fn reports_what_it_added_with_titles() {
        // FR-EXT-015: the UI shows each automatic addition with an Undo action
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        std::fs::write(vault.root.join("notes/plan.md"), "Yarın dişçi.").unwrap();
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        let added = Arc::new(Mutex::new(Vec::<AddedItem>::new()));
        let sink = Arc::clone(&added);
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let status_sink = Arc::clone(&statuses);
        let handle = Worker::new(vault, conn, Some(Box::new(Fixed(TASK))), Box::new(move |s| status_sink.lock().unwrap().push(s.clone())))
            .with_added_listener(Box::new(move |items| sink.lock().unwrap().extend_from_slice(items)))
            .spawn();
        wait_for(&statuses, |s| !s.busy && s.added == 1);
        let got = added.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].kind.as_str(), got[0].title.as_str()), ("task", "Dişçi"));
        handle.shutdown();
    }

    struct SlowTask;
    impl Extractor for SlowTask {
        fn extract_raw(&mut self, _: NaiveDate, _: &str) -> Result<String, LlmError> {
            std::thread::sleep(Duration::from_millis(400));
            Ok(TASK.to_owned())
        }
    }

    #[test]
    fn additions_are_reported_while_a_long_run_continues() {
        // Final review I5: the first note's addition must not wait for the whole catch-up
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        for name in ["a", "b", "c"] {
            std::fs::write(vault.root.join(format!("notes/{name}.md")), format!("Yarın {name}.")).unwrap();
        }
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::<WorkerStatus>::new()));
        let status_sink = Arc::clone(&statuses);
        let busy_when_reported = Arc::new(Mutex::new(Vec::<bool>::new()));
        let (seen, probe) = (Arc::clone(&busy_when_reported), Arc::clone(&statuses));
        let handle = Worker::new(vault, conn, Some(Box::new(SlowTask)), Box::new(move |s| status_sink.lock().unwrap().push(s.clone())))
            .with_added_listener(Box::new(move |_| {
                let busy = probe.lock().unwrap().last().is_some_and(|s| s.busy);
                seen.lock().unwrap().push(busy);
            }))
            .spawn();
        wait_for(&statuses, |s| !s.busy && s.added == 3);
        let reports = busy_when_reported.lock().unwrap().clone();
        assert_eq!(reports.len(), 3, "one report per note: {reports:?}");
        assert!(reports[0], "the first report came while the run was still going");
        handle.shutdown();
    }
    #[test]
    fn pausing_stops_a_running_batch_between_notes() {
        // Final review I3: the tray says "paused", so the CPU must stop too
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        for name in ["a", "b", "c", "d"] {
            std::fs::write(vault.root.join(format!("notes/{name}.md")), format!("Yarın {name}.")).unwrap();
        }
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::<WorkerStatus>::new()));
        let sink = Arc::clone(&statuses);
        let handle = Worker::new(vault, conn, Some(Box::new(SlowTask)), Box::new(move |s| sink.lock().unwrap().push(s.clone()))).spawn();
        wait_for(&statuses, |s| s.busy);
        handle.set_paused(true);
        let s = wait_for(&statuses, |s| s.paused && !s.busy);
        assert!(s.added < 4, "stopped early: {s:?}");
        assert!(s.queued >= 1, "the rest waits: {s:?}");
        assert_eq!(s.last_error, None, "a pause is not an error");
        handle.set_paused(false);
        wait_for(&statuses, |s| !s.paused && !s.busy && s.added == 4);
        handle.shutdown();
    }

    #[test]
    fn paused_work_waits_in_the_queue() {
        // FR-SCH-014 / FR-EXT-026
        let (tmp, tx, statuses, data) = setup(Some(Box::new(Fixed(TASK))));
        wait_for(&statuses, |s| !s.busy && s.added == 1);
        tx.send(Command::Pause(true));
        std::fs::write(tmp.path().join("notes/ikinci.md"), "Cuma fatura.").unwrap();
        tx.send(Command::Rescan);
        let s = wait_for(&statuses, |s| s.paused && s.queued >= 1);
        assert_eq!(s.added, 1);
        tx.send(Command::Pause(false));
        wait_for(&statuses, |s| !s.paused && !s.busy && s.added == 2);
        assert_eq!(task_count(&data), 2);
        tx.shutdown();
    }

    #[test]
    fn a_model_arriving_later_processes_the_waiting_notes() {
        // Model-manager spec § 5, Review Focus 4
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        std::fs::write(vault.root.join("notes/plan.md"), "Yarın dişçi.").unwrap();
        let data = tmp.path().join(".data");
        let conn = open_databases(&data).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&statuses);
        let handle = Worker::new(vault, conn, None, Box::new(move |s| sink.lock().unwrap().push(s.clone())))
            .with_extractor_factory(Box::new(|_| Box::new(Fixed(TASK))))
            .spawn();
        wait_for(&statuses, |s| s.queued == 1 && s.model == ModelState::NotInstalled);
        handle.send(Command::UseModel(pla_core::llm::ServerConfig::new("llama-server.exe".into(), "m.gguf".into())));
        wait_for(&statuses, |s| !s.busy && s.added == 1 && s.queued == 0);
        assert_eq!(task_count(&data), 1);
        handle.shutdown();
    }

    /// Ends like the model host: dropping it is what stops llama-server.
    struct Hosted(Arc<AtomicBool>);
    impl Extractor for Hosted {
        fn extract_raw(&mut self, _: NaiveDate, _: &str) -> Result<String, LlmError> {
            Ok(TASK.to_owned())
        }
    }
    impl Drop for Hosted {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn removing_the_model_stops_it_before_saying_so() {
        // FR-MDL-019, settings Review Focus 2: the file can only be deleted once llama-server is gone
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let data = tmp.path().join(".data");
        let conn = open_databases(&data).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&statuses);
        let dropped = Arc::new(AtomicBool::new(false));
        let handle = Worker::new(vault, conn, Some(Box::new(Hosted(Arc::clone(&dropped)))), Box::new(move |s| sink.lock().unwrap().push(s.clone()))).spawn();
        wait_for(&statuses, |s| s.model == ModelState::Off);
        let (ack, done) = std::sync::mpsc::channel();
        handle.send(Command::DropModel(ack));
        done.recv_timeout(Duration::from_secs(5)).expect("the worker answers");
        assert!(dropped.load(Ordering::SeqCst), "the host was dropped before the answer");
        wait_for(&statuses, |s| s.model == ModelState::NotInstalled);
        std::fs::write(tmp.path().join("notes/sonra.md"), "Yarın dişçi.").unwrap();
        handle.send(Command::Enqueue("notes/sonra.md".into()));
        let s = wait_for(&statuses, |s| s.queued >= 1);
        assert_eq!(s.added, 0, "no model, the note waits");
        handle.shutdown();
    }
}
