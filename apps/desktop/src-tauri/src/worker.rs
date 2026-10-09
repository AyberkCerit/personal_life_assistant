//! The background worker: the only owner of pla.db. It queues notes, runs the extraction pipeline
//! in one model session (FR-EXT-008), keeps work queued when there is no model (FR-EXT-022) and lets
//! the model host stop an idle model (FR-MDL-013). The UI learns everything through `WorkerStatus`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, Local};
use chrono::NaiveDate;
use pla_core::llm::LlmError;
use pla_core::pipeline::items::ItemRef;
use pla_core::pipeline::{enqueue, enqueue_all, process_queue_with, queued_notes, Extractor, Outcome, PipelineSettings};
use pla_core::qa::engine::{self, Progress, QaError};
use pla_core::qa::tools::ToolEnv;
use pla_core::qa::{self, ToolRecord, Turn};
use pla_core::vault::Vault;
use rusqlite::Connection;
use serde::Serialize;

const TICK: Duration = Duration::from_secs(1);
/// Decision 5: an outside change is extracted after this long without further changes to the note.
const EXTERNAL_DELAY: Duration = Duration::from_secs(45);
/// FR-MDL-013: the model process stops after this long without requests.
const MODEL_IDLE_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug)]
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
    /// FR-QA-003: answer a question with the same model; extraction waits (see `WorkerHandle::ask`).
    Ask(AskJob),
    /// FR-MEM-006: summarise the days that need it (maintenance), or `only` this day (the user
    /// asked for it again).
    Summarize { only: Option<chrono::NaiveDate>, cache_path: std::path::PathBuf },
    Shutdown,
}

/// One question for the assistant. `on` hears the progress; `cancel` is the Stop button.
pub struct AskJob {
    pub turn_id: String,
    pub question: String,
    pub new_topic: bool,
    pub cache_path: std::path::PathBuf,
    /// The embedding server (FR-MEM-003/005); `None` inside until its model is there.
    pub memory: crate::memory_cmds::Embeds,
    pub cancel: Arc<AtomicBool>,
    pub on: Box<dyn Fn(QaEvent) + Send>,
}

impl std::fmt::Debug for AskJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskJob").field("turn_id", &self.turn_id).finish_non_exhaustive()
    }
}

/// What the Q&A panel hears (`qa-event`), all for one `turn_id`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QaEvent {
    /// The model is choosing (no tool) or running a tool.
    Working { turn_id: String, tool: Option<String> },
    Tool { turn_id: String, record: Box<ToolRecord> },
    Token { turn_id: String, text: String },
    Done { turn_id: String, turn: Box<Turn>, sources: Vec<String> },
    /// `no_model`, `model` (llama-server failed) or `db`.
    Failed { turn_id: String, code: String, detail: String },
}

/// The running worker. Dropping it (or `shutdown`) stops the current run between two model calls,
/// waits for the thread, and so drops the model host, which ends llama-server (FR-MDL-017).
pub struct WorkerHandle {
    tx: Sender<Command>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    dropping: Arc<AtomicBool>,
    asking: Arc<AtomicUsize>,
    /// Stops a running summary request: a question, a model removal or closing comes first.
    interrupt: Arc<AtomicBool>,
    /// The Stop switch of the question being answered: closing the session flips it (final review C2).
    ask_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
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

    /// FR-MDL-019: stops the current run between two model calls (not after the whole queue), then
    /// drops the model; `done` hears when llama-server is gone (settings final review I1).
    pub fn drop_model(&self, done: Sender<()>) {
        self.dropping.store(true, Ordering::SeqCst);
        self.interrupt.store(true, Ordering::SeqCst);
        self.send(Command::DropModel(done));
    }

    /// FR-QA-003: a question goes before the queue: a running extraction stops between two notes
    /// (the rest stays queued, as with a pause) and goes on after the answer.
    pub fn ask(&self, job: AskJob) {
        self.asking.fetch_add(1, Ordering::SeqCst);
        self.interrupt.store(true, Ordering::SeqCst);
        *self.ask_cancel.lock().expect("ask lock") = Some(Arc::clone(&job.cancel));
        self.send(Command::Ask(job));
    }

    pub fn sender(&self) -> Sender<Command> {
        self.tx.clone()
    }

    pub fn shutdown(self) {}

    fn stop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.interrupt.store(true, Ordering::SeqCst);
        if let Some(c) = self.ask_cancel.lock().expect("ask lock").as_ref() {
            c.store(true, Ordering::SeqCst); // a question must not hold up a vault switch or quitting
        }
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
    dropping: &'a AtomicBool,
    asking: &'a AtomicUsize,
}

impl Extractor for Cancellable<'_> {
    fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(LlmError::Http("PLA is closing".into()));
        }
        if self.pause.load(Ordering::SeqCst) {
            return Err(LlmError::Http("background AI is paused".into()));
        }
        if self.dropping.load(Ordering::SeqCst) {
            return Err(LlmError::Http("the model is being removed".into()));
        }
        if self.asking.load(Ordering::SeqCst) > 0 {
            return Err(LlmError::Http("answering a question first".into()));
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
/// A day summarised (`None`) or why it was not (`no_model`, `busy`, `no_notes`, `model`).
type SummaryListener = Box<dyn Fn(&str, Option<&str>) + Send>;

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
    dropping: Arc<AtomicBool>,
    asking: Arc<AtomicUsize>,
    interrupt: Arc<AtomicBool>,
    on_added: Option<AddedListener>,
    /// Hears each day summarised (the note's box refreshes).
    on_summary: Option<SummaryListener>,
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
            dropping: Arc::new(AtomicBool::new(false)),
            asking: Arc::new(AtomicUsize::new(0)),
            interrupt: Arc::new(AtomicBool::new(false)),
            on_added: None,
            on_summary: None,
            reader,
            status: WorkerStatus { queued: 0, model, busy: false, last_error: None, added: 0, paused: false },
        }
    }

    pub fn with_added_listener(mut self, listener: AddedListener) -> Self {
        self.on_added = Some(listener);
        self
    }

    pub fn with_summary_listener(mut self, listener: SummaryListener) -> Self {
        self.on_summary = Some(listener);
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
        let (cancel, pause, dropping, asking, interrupt) =
            (Arc::clone(&self.cancel), Arc::clone(&self.pause), Arc::clone(&self.dropping), Arc::clone(&self.asking), Arc::clone(&self.interrupt));
        let join = std::thread::Builder::new().name("pla-worker".into()).spawn(move || self.run(rx)).expect("spawn worker");
        WorkerHandle { tx, cancel, pause, dropping, asking, interrupt, ask_cancel: Arc::new(Mutex::new(None)), join: Some(join) }
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
                    self.dropping.store(false, Ordering::SeqCst);
                    self.status.model = self.model_state();
                    self.status.last_error = None; // the interrupted run's "being removed" is no error
                    self.publish();
                    let _ = done.send(());
                }
                Ok(Command::Ask(job)) => {
                    self.ask(job);
                    self.asking.fetch_sub(1, Ordering::SeqCst);
                    self.process(); // what the question interrupted
                }
                Ok(Command::Summarize { only, cache_path }) => {
                    self.summarize(only, &cache_path);
                    self.process(); // what the summaries waited for
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
            let mut guarded =
                Cancellable { inner: extractor.as_mut(), cancel: &self.cancel, pause: &self.pause, dropping: &self.dropping, asking: &self.asking };
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
            if self.asking.load(Ordering::SeqCst) > 0 {
                self.status.last_error = None; // stopped for a question; it goes on afterwards
            }
            self.status.busy = false;
        }
        self.status.model = self.model_state();
        self.status.queued = queued_notes(&self.conn).map_or(0, |q| q.len());
        self.publish();
    }

    /// FR-MEM-006/007: one day at a time, newest first; a question, a pause or closing stops it
    /// between two days (the rest waits for the next maintenance). Only cache.db is written.
    fn summarize(&mut self, only: Option<chrono::NaiveDate>, cache_path: &std::path::Path) {
        // a day the user asked for always hears how it went (final review I2)
        let tell = |listener: &Option<SummaryListener>, why: &str| {
            if let (Some(day), Some(l)) = (only, listener) {
                l(&day.format("%Y-%m-%d").to_string(), Some(why));
            }
        };
        if self.status.paused && only.is_none() {
            return; // background AI paused (FR-SCH-014); a day the user asks for still goes
        }
        self.interrupt.store(false, Ordering::SeqCst);
        let Some(model) = self.extractor.as_mut().and_then(|x| x.chat_model()) else {
            tell(&self.on_summary, "no_model");
            return;
        };
        let Ok(cache) = pla_core::db::connect(cache_path) else {
            tell(&self.on_summary, "model");
            return;
        };
        let folders = self.vault.config.folders.clone();
        let today = (self.clock)().date_naive();
        let days = match only {
            Some(d) => vec![d],
            None => pla_core::summary::plan(&cache, &folders, today).unwrap_or_default(),
        };
        for day in days {
            let interrupted = self.cancel.load(Ordering::SeqCst)
                || self.dropping.load(Ordering::SeqCst)
                || self.asking.load(Ordering::SeqCst) > 0
                || (self.pause.load(Ordering::SeqCst) && only.is_none());
            if interrupted {
                tell(&self.on_summary, "busy");
                break;
            }
            let Ok(sources) = pla_core::summary::sources(&cache, &folders, day) else { continue };
            if sources.notes.is_empty() {
                tell(&self.on_summary, "no_notes");
                continue;
            }
            self.status.busy = true;
            (self.notify)(&self.status);
            // a question, a model removal or closing stops the request itself (final review I3)
            match pla_core::summary::summarize(model, &sources, &self.interrupt) {
                Ok(text) if !text.is_empty() => {
                    if pla_core::summary::save(&cache, &sources, &text, (self.clock)()).is_ok() {
                        if let Some(listener) = &self.on_summary {
                            listener(&day.format("%Y-%m-%d").to_string(), None);
                        }
                    }
                }
                Ok(_) => tell(&self.on_summary, "model"),
                Err(LlmError::Cancelled) => {
                    tell(&self.on_summary, "busy");
                    break;
                }
                Err(e) => {
                    // the model is not answering: the next maintenance tries again, and it shows
                    self.status.last_error = Some(e.to_string());
                    tell(&self.on_summary, "model");
                    break;
                }
            }
        }
        self.status.busy = false;
        self.status.model = self.model_state();
        self.publish();
    }

    /// FR-QA-003…016 on the worker thread, which owns the model and pla.db (tools write there).
    fn ask(&mut self, job: AskJob) {
        let AskJob { turn_id, question, new_topic, cache_path, memory, cancel, on } = job;
        let fail = |code: &str, detail: String| on(QaEvent::Failed { turn_id: turn_id.clone(), code: code.into(), detail });
        let Some(model) = self.extractor.as_mut().and_then(|x| x.chat_model()) else {
            fail("no_model", String::new());
            return;
        };
        let cache = match pla_core::db::connect(&cache_path) {
            Ok(c) => c,
            Err(e) => return fail("db", e.to_string()),
        };
        let turns = qa::follow_up(&self.conn, new_topic).unwrap_or_default();
        // FR-MEM-003: chunks still waiting are embedded first, at most 10 s (the rest later), then
        // the question's own vector joins the keyword search.
        // Busy from here on: the panel shows it and the indexer leaves the CPU to the question.
        self.status.busy = true;
        (self.notify)(&self.status);
        let mut vector: Option<(Vec<f32>, String)> = None;
        if let Some(embedder) = memory.lock().expect("embeds lock").as_mut() {
            let pending = pla_core::memory::embed_pending(&cache, embedder.as_mut(), 8, Instant::now() + Duration::from_secs(10), &cancel);
            // a server that could not start is not asked again for the question (final review I2)
            let server_down = matches!(pending, Err(pla_core::memory::EmbedError::Model(ref e)) if !matches!(e, LlmError::Status(_) | LlmError::Cancelled));
            if !server_down {
                if let Ok(mut v) = embedder.embed(&[pla_core::memory::query_text(&question)], &cancel) {
                    vector = v.pop().map(|v| (v, embedder.model_id().to_owned()));
                }
            }
            // the chat model answers next; both together are the RAM peak, worth a cold start next
            // time only when memory is short (tests always release, to see it happen)
            if cfg!(test) || crate::system::should_release(crate::system::available_ram_mb()) {
                embedder.release();
            }
        }
        let memory_arg = vector.as_ref().map(|(v, m)| (v.as_slice(), m.as_str()));
        let now = (self.clock)();
        let env = ToolEnv { vault: &self.vault, pla: &self.conn, cache: &cache, now, validation: PipelineSettings::default().validation };
        let result = engine::answer(model, &env, &question, &turns, memory_arg, crate::notify::ui_lang(), &cancel, &mut |p| {
            let event = match p {
                Progress::Working(tool) => QaEvent::Working { turn_id: turn_id.clone(), tool },
                Progress::Tool(record) => QaEvent::Tool { turn_id: turn_id.clone(), record },
                Progress::Token(text) => QaEvent::Token { turn_id: turn_id.clone(), text },
            };
            on(event);
        });
        let turn = |answer: String, tools: Vec<ToolRecord>, status: &str| Turn {
            turn_id: turn_id.clone(),
            question: question.clone(),
            answer,
            tools,
            created_at: now.to_rfc3339(),
            status: status.into(),
            new_topic,
        };
        let (saved, sources, failure) = match result {
            Ok(a) => (turn(a.text, a.tools, "done"), a.sources, None),
            Err(QaError::Stopped { partial, tools }) => (turn(partial, tools, "stopped"), Vec::new(), None),
            Err(QaError::Model { error, tools }) => (turn(String::new(), tools, "failed"), Vec::new(), Some(("model", error.to_string()))),
            Err(QaError::Db(e)) => (turn(String::new(), Vec::new(), "failed"), Vec::new(), Some(("db", e.to_string()))),
        };
        let _ = qa::save_turn(&self.conn, &saved); // FR-QA-014, also a stopped or failed one
        match failure {
            Some((code, detail)) => fail(code, detail),
            None => on(QaEvent::Done { turn_id: turn_id.clone(), turn: Box::new(saved), sources }),
        }
        self.status.busy = false;
        self.status.model = self.model_state();
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

    /// Extracts slowly (one second a note) and answers questions from a script.
    struct Chatty {
        extract_delay: Duration,
    }
    impl Extractor for Chatty {
        fn extract_raw(&mut self, _: NaiveDate, _: &str) -> Result<String, LlmError> {
            std::thread::sleep(self.extract_delay);
            Ok(TASK.to_owned())
        }
        fn chat_model(&mut self) -> Option<&mut dyn pla_core::qa::ChatModel> {
            Some(self)
        }
    }
    impl pla_core::qa::ChatModel for Chatty {
        fn complete(&mut self, _: &serde_json::Value, _: &AtomicBool) -> Result<String, LlmError> {
            Ok(r#"{"tool":"add_task","args":{"title":"Doktor randevusu","date":"2026-10-07","time":"10:00","remind":true}}"#.into())
        }
        fn stream(&mut self, _: &serde_json::Value, _: &AtomicBool, on: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
            on("Hatırlatıcı ");
            on("eklendi.");
            Ok("Hatırlatıcı eklendi.".into())
        }
    }

    fn ask(handle: &WorkerHandle, data: &std::path::Path, question: &str) -> std::sync::mpsc::Receiver<QaEvent> {
        ask_with(handle, data, question, Arc::new(Mutex::new(None)))
    }

    fn ask_with(handle: &WorkerHandle, data: &std::path::Path, question: &str, memory: crate::memory_cmds::Embeds) -> std::sync::mpsc::Receiver<QaEvent> {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        handle.ask(AskJob {
            turn_id: "t1".into(),
            question: question.into(),
            new_topic: false,
            cache_path: data.join("cache.db"),
            memory,
            cancel: Arc::new(AtomicBool::new(false)),
            on: Box::new(move |e| {
                let _ = tx.lock().unwrap().send(e);
            }),
        });
        rx
    }

    fn until_end(rx: &std::sync::mpsc::Receiver<QaEvent>) -> Vec<QaEvent> {
        let mut events = Vec::new();
        loop {
            let e = rx.recv_timeout(Duration::from_secs(10)).expect("the answer never ended");
            let end = matches!(e, QaEvent::Done { .. } | QaEvent::Failed { .. });
            events.push(e);
            if end {
                return events;
            }
        }
    }

    /// Counts how often it was told to let its server go.
    struct Releases(Arc<std::sync::atomic::AtomicUsize>);
    impl pla_core::memory::Embedder for Releases {
        fn model_id(&self) -> &str {
            "e"
        }
        fn embed(&mut self, texts: &[String], _: &AtomicBool) -> Result<Vec<Vec<f32>>, LlmError> {
            Ok(texts.iter().map(|_| vec![1.0; 768]).collect())
        }
        fn release(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn the_embedding_server_goes_before_the_model_answers() {
        // nl-quality: the two models are not in memory together
        let (_tmp, handle, statuses, data) = setup(Some(Box::new(Chatty { extract_delay: Duration::ZERO })));
        wait_for(&statuses, |s| !s.busy && s.queued == 0);
        let released = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let memory: crate::memory_cmds::Embeds = Arc::new(Mutex::new(Some(Box::new(Releases(Arc::clone(&released))))));
        let events = until_end(&ask_with(&handle, &data, "Yarın 10'da doktor randevum var", memory));
        assert!(matches!(events.last(), Some(QaEvent::Done { .. })), "{events:?}");
        assert_eq!(released.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_question_is_answered_with_tools_and_kept() {
        // FR-QA-003/008/014
        let (_tmp, handle, statuses, data) = setup(Some(Box::new(Chatty { extract_delay: Duration::ZERO })));
        wait_for(&statuses, |s| !s.busy && s.queued == 0);
        let events = until_end(&ask(&handle, &data, "Yarın 10'da doktor randevum var, hatırlat"));
        assert!(events.iter().any(|e| matches!(e, QaEvent::Tool { record, .. } if record.tool == "add_task" && record.ok)));
        let QaEvent::Done { turn, .. } = events.last().unwrap() else { panic!("{events:?}") };
        assert_eq!(turn.answer, "Hatırlatıcı eklendi.");
        let conn = rusqlite::Connection::open(data.join("pla.db")).unwrap();
        let origin: String = conn.query_row("SELECT origin FROM task WHERE title = 'Doktor randevusu'", [], |r| r.get(0)).unwrap();
        assert_eq!(origin, "assistant");
        let kept: i64 = conn.query_row("SELECT count(*) FROM qa_turn", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 1);
        handle.shutdown();
    }

    /// Thinks until stopped.
    struct Ponders;
    impl Extractor for Ponders {
        fn extract_raw(&mut self, _: NaiveDate, _: &str) -> Result<String, LlmError> {
            Ok(TASK.to_owned())
        }
        fn chat_model(&mut self) -> Option<&mut dyn pla_core::qa::ChatModel> {
            Some(self)
        }
    }
    impl pla_core::qa::ChatModel for Ponders {
        fn complete(&mut self, _: &serde_json::Value, cancel: &AtomicBool) -> Result<String, LlmError> {
            while !cancel.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(LlmError::Cancelled)
        }
        fn stream(&mut self, _: &serde_json::Value, _: &AtomicBool, _: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
            Err(LlmError::Cancelled)
        }
    }

    #[test]
    fn closing_does_not_wait_for_a_question() {
        // final review C2: a vault switch or quitting stops the answer
        let (_tmp, handle, statuses, data) = setup(Some(Box::new(Ponders)));
        wait_for(&statuses, |s| !s.busy && s.queued == 0);
        let rx = ask(&handle, &data, "uzun düşün");
        std::thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        drop(handle);
        assert!(started.elapsed() < Duration::from_secs(2), "closing took {:?}", started.elapsed());
        assert!(rx.try_iter().any(|e| matches!(e, QaEvent::Done { .. })), "the stopped turn still ends");
    }

    #[test]
    fn past_days_are_summarised_into_the_cache_only() {
        // FR-MEM-006/007
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let data = tmp.path().join(".data");
        let dbs = open_databases(&data).unwrap();
        let old = Local::now().timestamp_millis() - 3 * 86_400_000;
        pla_core::index::index_note(&dbs.cache, "notes/a.md", "# A\n\nBütçe toplantısı.", old, 10).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let handle = Worker::new(vault, dbs.pla, Some(Box::new(Chatty { extract_delay: Duration::ZERO })), Box::new(|_| {}))
            .with_summary_listener(Box::new(move |d, _| {
                let _ = tx.lock().unwrap().send(d.to_owned());
            }))
            .spawn();
        handle.send(Command::Summarize { only: None, cache_path: data.join("cache.db") });
        let day = rx.recv_timeout(Duration::from_secs(10)).expect("a day was summarised");
        let reader = rusqlite::Connection::open(data.join("cache.db")).unwrap();
        let saved: i64 = reader.query_row("SELECT count(*) FROM daily_summary WHERE date = ?1", [&day], |r| r.get(0)).unwrap();
        assert_eq!(saved, 1);
        assert_eq!(task_count(&data), 0, "a summary never writes records");
        handle.shutdown();
    }

    #[test]
    fn a_day_asked_for_without_a_model_hears_why() {
        // final review I2: the box must not show "summarising…" forever
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let data = tmp.path().join(".data");
        let dbs = open_databases(&data).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let handle = Worker::new(vault, dbs.pla, None, Box::new(|_| {}))
            .with_summary_listener(Box::new(move |d, why| {
                let _ = tx.lock().unwrap().send((d.to_owned(), why.map(str::to_owned)));
            }))
            .spawn();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        handle.send(Command::Summarize { only: Some(day), cache_path: data.join("cache.db") });
        let (d, why) = rx.recv_timeout(Duration::from_secs(5)).expect("an answer");
        assert_eq!((d.as_str(), why.as_deref()), ("2026-10-05", Some("no_model")));
        handle.shutdown();
    }

    #[test]
    fn without_a_model_the_panel_hears_so() {
        // FR-QA-002
        let (_tmp, handle, _statuses, data) = setup(None);
        let events = until_end(&ask(&handle, &data, "merhaba"));
        assert!(matches!(&events[..], [QaEvent::Failed { code, .. }] if code == "no_model"));
        handle.shutdown();
    }

    #[test]
    fn a_question_goes_before_a_long_extraction_which_then_goes_on() {
        // FR-QA-003: the queue waits, nothing is lost and no error is reported for the pause
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        for i in 0..6 {
            std::fs::write(vault.root.join(format!("notes/n{i}.md")), format!("Not {i}: yarın dişçi.")).unwrap();
        }
        let data = tmp.path().join(".data");
        let conn = open_databases(&data).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&statuses);
        let handle = Worker::new(vault, conn, Some(Box::new(Chatty { extract_delay: Duration::from_millis(400) })), Box::new(move |s| sink.lock().unwrap().push(s.clone())))
            .spawn();
        wait_for(&statuses, |s| s.busy);
        let started = Instant::now();
        let events = until_end(&ask(&handle, &data, "hatırlat"));
        assert!(matches!(events.last(), Some(QaEvent::Done { .. })));
        assert!(started.elapsed() < Duration::from_secs(2), "answered after {:?}, not after the whole queue", started.elapsed());
        let done = wait_for(&statuses, |s| !s.busy && s.queued == 0);
        assert_eq!(done.last_error, None);
        handle.shutdown();
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

    struct Counted(Arc<std::sync::atomic::AtomicUsize>);
    impl Extractor for Counted {
        fn extract_raw(&mut self, _: NaiveDate, _: &str) -> Result<String, LlmError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
            Ok(TASK.to_owned())
        }
    }

    #[test]
    fn removing_the_model_does_not_wait_for_the_whole_queue() {
        // settings final review I1: a long catch-up queue must not hold the removal
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        for i in 0..20 {
            std::fs::write(vault.root.join(format!("notes/n{i}.md")), format!("Yarın {i} dişçi.")).unwrap();
        }
        let conn = open_databases(&tmp.path().join(".data")).unwrap().pla;
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&statuses);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let handle = Worker::new(vault, conn, Some(Box::new(Counted(Arc::clone(&calls)))), Box::new(move |s| sink.lock().unwrap().push(s.clone()))).spawn();
        wait_for(&statuses, |s| s.busy);
        let (ack, done) = std::sync::mpsc::channel();
        let asked = Instant::now();
        handle.drop_model(ack);
        done.recv_timeout(Duration::from_secs(5)).expect("answered between two model calls");
        assert!(asked.elapsed() < Duration::from_secs(2), "took {:?}", asked.elapsed());
        assert!(calls.load(Ordering::SeqCst) < 20, "the rest of the queue was not run first");
        let s = wait_for(&statuses, |s| s.model == ModelState::NotInstalled && !s.busy);
        assert_eq!(s.last_error, None, "removing is not an error");
        handle.shutdown();
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
