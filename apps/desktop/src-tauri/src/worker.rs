//! The background worker: the only owner of pla.db. It queues notes, runs the extraction pipeline
//! in one model session (FR-EXT-008), keeps work queued when there is no model (FR-EXT-022) and lets
//! the model host stop an idle model (FR-MDL-013). The UI learns everything through `WorkerStatus`.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Local};
use pla_core::pipeline::{enqueue, enqueue_all, process_queue, queued_notes, Extractor, Outcome, PipelineSettings};
use pla_core::vault::Vault;
use rusqlite::Connection;
use serde::Serialize;

const TICK: Duration = Duration::from_secs(1);
/// Decision 5: an outside change is extracted after this long without further changes to the note.
const EXTERNAL_DELAY: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Enqueue(String),
    EnqueueLater(String),
    Shutdown,
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
}

type Notify = Box<dyn Fn(&WorkerStatus) + Send>;
type Clock = Box<dyn Fn() -> DateTime<FixedOffset> + Send>;

pub struct Worker {
    vault: Vault,
    conn: Connection,
    extractor: Option<Box<dyn Extractor + Send>>,
    notify: Notify,
    clock: Clock,
    external_delay: Duration,
    later: HashMap<String, Instant>,
    status: WorkerStatus,
}

impl Worker {
    pub fn new(vault: Vault, conn: Connection, extractor: Option<Box<dyn Extractor + Send>>, notify: Notify) -> Self {
        let model = if extractor.is_some() { ModelState::Off } else { ModelState::NotInstalled };
        Self {
            vault,
            conn,
            extractor,
            notify,
            clock: Box::new(|| Local::now().fixed_offset()),
            external_delay: EXTERNAL_DELAY,
            later: HashMap::new(),
            status: WorkerStatus { queued: 0, model, busy: false, last_error: None, added: 0 },
        }
    }

    pub fn with_external_delay(mut self, delay: Duration) -> Self {
        self.external_delay = delay;
        self
    }

    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    pub fn spawn(self) -> (Sender<Command>, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::Builder::new().name("pla-worker".into()).spawn(move || self.run(rx)).expect("spawn worker");
        (tx, handle)
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
        if let Some(extractor) = self.extractor.as_mut() {
            self.status.busy = true;
            self.status.queued = queued_notes(&self.conn).map_or(0, |q| q.len());
            (self.notify)(&self.status); // field borrows: the extractor stays borrowed
            let now = (self.clock)();
            match process_queue(&self.vault, &mut self.conn, extractor.as_mut(), &PipelineSettings::default(), now) {
                Ok(report) => {
                    self.status.added += report.outcomes.iter().filter(|(_, o)| matches!(o, Outcome::Added(_))).count();
                    self.status.last_error = report.model_error;
                }
                Err(e) => self.status.last_error = Some(e.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use pla_core::db::open_databases;
    use pla_core::llm::LlmError;
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

    fn setup(extractor: Option<Box<dyn Extractor + Send>>) -> (tempfile::TempDir, Sender<Command>, Arc<Mutex<Vec<WorkerStatus>>>, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        std::fs::write(vault.root.join("notes/plan.md"), "Yarın dişçi.").unwrap();
        let data = tmp.path().join(".data");
        let conn = open_databases(&data).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&statuses);
        let worker = Worker::new(vault, conn, extractor, Box::new(move |s| sink.lock().unwrap().push(s.clone())))
            .with_external_delay(Duration::from_millis(300));
        let (tx, _join) = worker.spawn();
        (tmp, tx, statuses, data)
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
        tx.send(Command::Shutdown).unwrap();
    }

    #[test]
    fn without_a_model_notes_wait_in_the_queue() {
        // Review Focus 4
        let (_tmp, tx, statuses, data) = setup(None);
        let s = wait_for(&statuses, |s| s.queued == 1);
        assert_eq!(s.model, ModelState::NotInstalled);
        assert_eq!(task_count(&data), 0);
        tx.send(Command::Shutdown).unwrap();
    }

    #[test]
    fn external_changes_wait_for_quiet_before_processing() {
        // Decision 5
        let (tmp, tx, statuses, data) = setup(Some(Box::new(Fixed(TASK))));
        wait_for(&statuses, |s| !s.busy && s.added == 1);
        let before = statuses.lock().unwrap().len();
        std::fs::write(tmp.path().join("notes/plan.md"), "Yarın dişçi var.").unwrap();
        tx.send(Command::EnqueueLater("notes/plan.md".into())).unwrap();
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
        tx.send(Command::Shutdown).unwrap();
    }
}
