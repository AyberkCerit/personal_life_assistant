//! The llama.cpp `llama-server` sidecar (FR-MDL-011, -012, -015, -017, -020, -021; NFR-SEC-002).
//! Only the Rust core talks to it; it listens on 127.0.0.1 behind a per-run API key.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use serde_json::Value;

use crate::extraction::{parse_extraction, request_body, RawExtraction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub server_bin: PathBuf,
    pub model: PathBuf,
    pub ctx_size: u32,
    pub threads: u32,
    pub startup_timeout: Duration,
    /// Run as the embedding server (FR-MEM-004) instead of the chat model.
    pub embedding: bool,
}

impl ServerConfig {
    pub fn new(server_bin: PathBuf, model: PathBuf) -> Self {
        Self { server_bin, model, ctx_size: 8192, threads: 4, startup_timeout: Duration::from_secs(30), embedding: false }
    }

    /// EmbeddingGemma: a 2 048-token window, one batch as large as a whole input.
    pub fn embedding(server_bin: PathBuf, model: PathBuf) -> Self {
        Self { server_bin, model, ctx_size: 2048, threads: 4, startup_timeout: Duration::from_secs(30), embedding: true }
    }

    pub fn args(&self, port: u16) -> Vec<String> {
        let mut args: Vec<String> = vec!["-m".into(), self.model.to_string_lossy().into_owned()];
        if self.embedding {
            for (flag, value) in [
                ("-t", self.threads.to_string()),
                ("-c", self.ctx_size.to_string()),
                ("-ngl", "0".into()),
                ("-np", "1".into()),
                ("--host", "127.0.0.1".into()),
                ("--port", port.to_string()),
                ("-ub", self.ctx_size.to_string()),
                ("-b", self.ctx_size.to_string()),
            ] {
                args.push(flag.into());
                args.push(value);
            }
            args.push("--embeddings".into());
            args.push("--no-webui".into());
            return args;
        }
        for (flag, value) in [
            ("-t", self.threads.to_string()),
            ("-c", self.ctx_size.to_string()),
            ("-ngl", "0".into()),
            ("-np", "1".into()),
            ("--host", "127.0.0.1".into()),
            ("--port", port.to_string()),
            ("--reasoning", "off".into()),
            ("-fa", "on".into()),
            ("-ctk", "q8_0".into()),
            ("-ctv", "q8_0".into()),
            ("-ub", "256".into()),
            ("-b", "512".into()),
            // nl-quality: the prompt cache defaults to 8 GiB and grew by a gigabyte over a hundred
            // questions; 512 MiB keeps the speed (a repeated prompt in 0.4 s, not 25 s) and a bound
            ("--cache-ram", "512".into()),
        ] {
            args.push(flag.into());
            args.push(value);
        }
        args.push("--no-webui".into());
        args
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("llama-server not found: {0}")]
    MissingBinary(PathBuf),
    #[error("model file not found: {0}")]
    MissingModel(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("random generator failed: {0}")]
    Random(String),
    #[error("llama-server exited during startup: {0}")]
    Exited(String),
    #[error("llama-server was not healthy within {0:?}")]
    StartupTimeout(Duration),
    #[error("request to llama-server failed: {0}")]
    Http(String),
    #[error("llama-server refused the request with status {0}")]
    Status(u16),
    #[error("llama-server did not answer in time")]
    Timeout,
    #[error("unexpected response from llama-server")]
    BadResponse,
    #[error("stopped by the user")]
    Cancelled,
    #[error("model answer is not valid extraction JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

/// 256-bit key from the OS CSPRNG, hex encoded. Lives only in memory and the child's environment.
pub fn new_api_key() -> Result<String, LlmError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| LlmError::Random(e.to_string()))?;
    Ok(hex::encode(bytes))
}

pub fn free_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

/// Plain HTTP to 127.0.0.1 only: environment proxies are ignored (NFR-SEC-001).
fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// A question can take minutes on a slow CPU (an 8k-token prompt, then the answer); Stop ends it.
fn chat_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into()
}

/// Eight chunks take a few seconds on a CPU; more means the server is stuck (FR-MEM-003).
fn embed_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// One health probe must not outlive the startup limit (FR-MDL-014).
fn health_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(1)))
        .build()
        .into()
}

const STDERR_TAIL_LINES: usize = 20;

pub(crate) fn wait_until_healthy(child: &mut Child, health: &ureq::Agent, base_url: &str, timeout: Duration) -> Result<(), LlmError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(LlmError::Exited(status.to_string()));
        }
        if health.get(format!("{base_url}/health")).call().is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(LlmError::StartupTimeout(timeout));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Ties a child process to PLA's lifetime: when PLA exits for any reason (closed, crashed, killed
/// in Task Manager) Windows ends the child too, so a multi-GB model never lingers (FR-MDL-017).
#[cfg(windows)]
pub(crate) fn kill_with_parent(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    struct Job(HANDLE);
    // SAFETY: a job handle is a kernel handle usable from any thread; it is never closed (lives as long as PLA).
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    static JOB: OnceLock<Option<Job>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls with valid arguments; the handle is checked before use.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            (ok != 0).then_some(Job(handle))
        }
    });
    if let Some(Job(handle)) = job {
        // SAFETY: both handles are valid for the duration of the call.
        unsafe {
            AssignProcessToJobObject(*handle, child.as_raw_handle() as HANDLE);
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn kill_with_parent(_child: &Child) {}

pub struct LlamaServer {
    child: Child,
    base_url: String,
    api_key: String,
    agent: ureq::Agent,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stderr_reader: Option<JoinHandle<()>>,
    /// A stopped chat request still running in llama-server (`-np 1`): the next request waits for
    /// it, so a resumed extraction does not time out behind it (final review I4).
    in_flight: Mutex<Option<JoinHandle<()>>>,
}

impl LlamaServer {
    pub fn start(cfg: &ServerConfig) -> Result<Self, LlmError> {
        if !cfg.server_bin.is_file() {
            return Err(LlmError::MissingBinary(cfg.server_bin.clone()));
        }
        if !cfg.model.is_file() {
            return Err(LlmError::MissingModel(cfg.model.clone()));
        }
        let port = free_port()?;
        let api_key = new_api_key()?;

        let mut cmd = Command::new(&cfg.server_bin);
        cmd.args(cfg.args(port))
            .env("LLAMA_API_KEY", &api_key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn()?;
        kill_with_parent(&child);

        // Keep the last stderr lines for error messages; the pipe must be drained or the server blocks.
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));
        let stderr_reader = child.stderr.take().map(|stderr| {
            let tail = Arc::clone(&stderr_tail);
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = Vec::new();
                while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                    let mut tail = tail.lock().expect("stderr tail lock");
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(String::from_utf8_lossy(&line).trim_end().to_owned());
                    line.clear();
                }
            })
        });

        let mut server = Self {
            child,
            base_url: format!("http://127.0.0.1:{port}"),
            api_key,
            agent: http_agent(),
            stderr_tail,
            stderr_reader,
            in_flight: Mutex::new(None),
        };
        match wait_until_healthy(&mut server.child, &health_agent(), &server.base_url, cfg.startup_timeout) {
            Ok(()) => Ok(server),
            Err(LlmError::Exited(status)) => {
                if let Some(reader) = server.stderr_reader.take() {
                    let _ = reader.join(); // the pipe closed with the process
                }
                let tail = server.stderr_tail.lock().expect("stderr tail lock").iter().cloned().collect::<Vec<_>>().join(" | ");
                Err(LlmError::Exited(if tail.is_empty() { status } else { format!("{status}: {tail}") }))
            }
            Err(e) => Err(e), // Drop kills the child
        }
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The model's raw JSON answer for one block of text.
    pub fn extract_raw(&self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
        self.settle();
        let mut response = self
            .agent
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(request_body(reference, text))
            .map_err(|e| match e {
                ureq::Error::StatusCode(code) => LlmError::Status(code),
                ureq::Error::Timeout(_) => LlmError::Timeout,
                other => LlmError::Http(other.to_string()),
            })?;
        let body: Value = response.body_mut().read_json().map_err(|e| LlmError::Http(e.to_string()))?;
        body["choices"][0]["message"]["content"].as_str().map(str::to_owned).ok_or(LlmError::BadResponse)
    }

    /// FR-MEM-004: one vector per text (L2-normalised by the server).
    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, LlmError> {
        self.settle();
        let mut response = embed_agent()
            .post(format!("{}/v1/embeddings", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(serde_json::json!({ "input": texts }))
            .map_err(http_error)?;
        let body: Value = response.body_mut().read_json().map_err(|e| LlmError::Http(e.to_string()))?;
        let data = body["data"].as_array().ok_or(LlmError::BadResponse)?;
        let mut out = vec![Vec::new(); texts.len()];
        for item in data {
            let i = item["index"].as_u64().ok_or(LlmError::BadResponse)? as usize;
            let v: Vec<f32> = item["embedding"].as_array().ok_or(LlmError::BadResponse)?.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
            *out.get_mut(i).ok_or(LlmError::BadResponse)? = v;
        }
        Ok(out)
    }

    pub fn extract(&self, reference: NaiveDate, text: &str) -> Result<RawExtraction, LlmError> {
        Ok(parse_extraction(&self.extract_raw(reference, text)?)?)
    }
}

/// What a chat request sends back to the waiting thread.
enum Piece {
    Token(String),
    Done(String),
    Failed(LlmError),
}

fn http_error(e: ureq::Error) -> LlmError {
    match e {
        ureq::Error::StatusCode(code) => LlmError::Status(code),
        ureq::Error::Timeout(_) => LlmError::Timeout,
        other => LlmError::Http(other.to_string()),
    }
}

impl LlamaServer {
    /// Waits for a stopped request to leave the server (a stream ends at its next token).
    fn settle(&self) {
        if let Some(handle) = self.in_flight.lock().expect("in-flight lock").take() {
            let _ = handle.join();
        }
    }

    /// FR-QA-003/013: runs one chat request on its own thread and waits for it, looking at `cancel`
    /// every 100 ms, so Stop answers within a second even while the model still reads the prompt.
    /// When stopped, the request thread finds no one listening and drops the connection, which
    /// ends the generation in llama-server. With `on_token`, the answer is streamed.
    fn chat(&self, body: &Value, cancel: &AtomicBool, mut on_token: Option<&mut dyn FnMut(&str)>) -> Result<String, LlmError> {
        let (tx, rx) = std::sync::mpsc::channel::<Piece>();
        let stream = on_token.is_some();
        let mut body = body.clone();
        body["stream"] = Value::Bool(stream);
        self.settle();
        let (agent, url, key) = (chat_agent(), format!("{}/v1/chat/completions", self.base_url), self.api_key.clone());
        let handle = std::thread::spawn(move || {
            let response = agent.post(url).header("Authorization", &format!("Bearer {key}")).send_json(&body);
            let mut response = match response {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Piece::Failed(http_error(e)));
                    return;
                }
            };
            if !stream {
                let piece = match response.body_mut().read_json::<Value>() {
                    Ok(v) => v["choices"][0]["message"]["content"].as_str().map_or(Piece::Failed(LlmError::BadResponse), |s| Piece::Done(s.to_owned())),
                    Err(e) => Piece::Failed(LlmError::Http(e.to_string())),
                };
                let _ = tx.send(piece);
                return;
            }
            let reader = BufReader::new(response.body_mut().as_reader());
            let mut whole = String::new();
            let mut finished = false;
            for line in reader.lines() {
                let Ok(line) = line else {
                    let _ = tx.send(Piece::Failed(LlmError::Http("the answer stream broke off".into())));
                    return;
                };
                let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
                if data == "[DONE]" {
                    finished = true;
                    break;
                }
                let Ok(chunk) = serde_json::from_str::<Value>(data) else { continue };
                if let Some(token) = chunk["choices"][0]["delta"]["content"].as_str().filter(|t| !t.is_empty()) {
                    whole.push_str(token);
                    if tx.send(Piece::Token(token.to_owned())).is_err() {
                        return; // stopped: dropping the response closes the connection
                    }
                }
            }
            // a stream that ends without [DONE] was cut off (the server died): not a whole answer
            let _ = tx.send(if finished { Piece::Done(whole) } else { Piece::Failed(LlmError::Http("the answer stream broke off".into())) });
        });
        loop {
            if cancel.load(Ordering::SeqCst) {
                *self.in_flight.lock().expect("in-flight lock") = Some(handle);
                return Err(LlmError::Cancelled);
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Piece::Token(t)) => {
                    if let Some(f) = on_token.as_mut() {
                        f(&t);
                    }
                }
                Ok(Piece::Done(text)) => return Ok(text),
                Ok(Piece::Failed(e)) => return Err(e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(LlmError::BadResponse),
            }
        }
    }
}

impl crate::qa::ChatModel for LlamaServer {
    fn complete(&mut self, body: &Value, cancel: &AtomicBool) -> Result<String, LlmError> {
        self.chat(body, cancel, None)
    }

    fn stream(&mut self, body: &Value, cancel: &AtomicBool, on_token: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
        self.chat(body, cancel, Some(on_token))
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl crate::pipeline::Extractor for LlamaServer {
    fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
        LlamaServer::extract_raw(self, reference, text)
    }
}

/// Owns the sidecar: starts it on first use, retries a failed start once (FR-MDL-014),
/// restarts it after a crash and stops it after `idle_after` without requests (FR-MDL-013).
pub struct ModelHost {
    cfg: ServerConfig,
    idle_after: Duration,
    server: Option<LlamaServer>,
    last_used: Instant,
    start_attempts: u32,
    /// The model file's name: stored with each vector it makes (FR-MEM-013).
    model_id: String,
    /// When the embedding server last failed to start: it is not tried again for a while, so a
    /// question is not held up and the indexer does not spawn it every 2 s (final review I2).
    failed_at: Option<Instant>,
}

/// How long a failed embedding server is left alone.
const EMBED_RETRY_AFTER: Duration = Duration::from_secs(300);

impl ModelHost {
    pub fn new(cfg: ServerConfig, idle_after: Duration) -> Self {
        let model_id = cfg.model.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { cfg, idle_after, server: None, last_used: Instant::now(), start_attempts: 0, model_id, failed_at: None }
    }

    pub fn is_running(&self) -> bool {
        self.server.is_some()
    }

    pub fn start_attempts(&self) -> u32 {
        self.start_attempts
    }

    pub fn stop_if_idle(&mut self) -> bool {
        if self.server.is_some() && self.last_used.elapsed() >= self.idle_after {
            self.server = None;
            return true;
        }
        false
    }

    pub fn stop(&mut self) {
        self.server = None;
    }

    fn server(&mut self) -> Result<&LlamaServer, LlmError> {
        if self.server.as_mut().is_some_and(|s| !s.is_alive()) {
            self.server = None;
        }
        if self.server.is_none() {
            if self.cfg.embedding && self.failed_at.is_some_and(|t| t.elapsed() < EMBED_RETRY_AFTER) {
                return Err(LlmError::Exited("the embedding server failed to start a moment ago".into()));
            }
            self.start_attempts += 1;
            let started = match LlamaServer::start(&self.cfg) {
                Err(LlmError::MissingBinary(p)) => Err(LlmError::MissingBinary(p)),
                Err(LlmError::MissingModel(p)) => Err(LlmError::MissingModel(p)),
                Err(_) => {
                    self.start_attempts += 1;
                    LlamaServer::start(&self.cfg)
                }
                ok => ok,
            };
            let started = match started {
                Ok(s) => s,
                Err(e) => {
                    self.failed_at = Some(Instant::now());
                    return Err(e);
                }
            };
            self.failed_at = None;
            self.server = Some(started);
        }
        Ok(self.server.as_ref().expect("server was just started"))
    }
}

impl crate::memory::Embedder for ModelHost {
    fn model_id(&self) -> &str {
        self.model_id.as_str()
    }

    fn embed(&mut self, texts: &[String], stop: &AtomicBool) -> Result<Vec<Vec<f32>>, LlmError> {
        if stop.load(Ordering::SeqCst) {
            return Err(LlmError::Cancelled);
        }
        let vectors = self.server()?.embed(texts);
        self.last_used = Instant::now();
        vectors
    }

    fn tick(&mut self) {
        self.stop_if_idle();
    }

    fn release(&mut self) {
        self.stop();
    }
}

impl crate::qa::ChatModel for ModelHost {
    fn complete(&mut self, body: &Value, cancel: &AtomicBool) -> Result<String, LlmError> {
        let answer = self.server()?.chat(body, cancel, None);
        self.last_used = Instant::now();
        answer
    }

    fn stream(&mut self, body: &Value, cancel: &AtomicBool, on_token: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
        let answer = self.server()?.chat(body, cancel, Some(on_token));
        self.last_used = Instant::now();
        answer
    }
}

impl crate::pipeline::Extractor for ModelHost {
    fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
        let answer = self.server()?.extract_raw(reference, text);
        self.last_used = Instant::now();
        answer
    }

    fn tick(&mut self) {
        self.stop_if_idle();
    }

    fn is_running(&self) -> bool {
        ModelHost::is_running(self)
    }

    fn chat_model(&mut self) -> Option<&mut dyn crate::qa::ChatModel> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::Extractor;

    fn dying_config() -> (tempfile::TempDir, ServerConfig) {
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("m.gguf");
        std::fs::write(&model, b"not a model").unwrap();
        let mut cfg = ServerConfig::new(std::env::current_exe().unwrap(), model);
        cfg.startup_timeout = Duration::from_secs(20);
        (tmp, cfg)
    }

    #[test]
    fn startup_failure_reports_what_the_server_printed() {
        // The test binary rejects llama-server's flags and prints why on stderr.
        let (_tmp, cfg) = dying_config();
        let Err(LlmError::Exited(message)) = LlamaServer::start(&cfg) else { panic!("expected Exited") };
        assert!(message.to_lowercase().contains("option"), "stderr tail missing: {message}");
    }

    #[test]
    fn model_host_retries_a_failed_start_once() {
        let (_tmp, cfg) = dying_config();
        let mut host = ModelHost::new(cfg, Duration::from_secs(60));
        let reference = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert!(matches!(host.extract_raw(reference, "x"), Err(LlmError::Exited(_))));
        assert_eq!(host.start_attempts(), 2);
        assert!(!host.is_running());
    }

    #[test]
    fn model_host_does_not_retry_a_missing_model() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = ServerConfig::new(std::env::current_exe().unwrap(), tmp.path().join("yok.gguf"));
        let mut host = ModelHost::new(cfg, Duration::from_secs(60));
        let reference = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert!(matches!(host.extract_raw(reference, "x"), Err(LlmError::MissingModel(_))));
        assert_eq!(host.start_attempts(), 1);
        assert!(!host.stop_if_idle(), "nothing to stop");
    }

    #[cfg(windows)]
    #[test]
    fn a_silent_listener_cannot_stall_startup_past_the_limit() {
        // A port that accepts connections but never answers must not block each probe for minutes.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for conn in listener.incoming() {
                held.push(conn);
            }
        });
        let mut child = Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(Stdio::null()).spawn().unwrap();
        let started = Instant::now();
        let result = wait_until_healthy(&mut child, &health_agent(), &format!("http://127.0.0.1:{port}"), Duration::from_secs(3));
        let _ = child.kill();
        assert!(matches!(result, Err(LlmError::StartupTimeout(_))));
        assert!(started.elapsed() < Duration::from_secs(6), "took {:?}", started.elapsed());
    }

    #[test]
    fn api_key_is_256_bit_hex_and_fresh() {
        let a = new_api_key().unwrap();
        let b = new_api_key().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn http_agent_ignores_system_proxies() {
        // NFR-SEC-001: HTTP_PROXY/ALL_PROXY must never route sidecar traffic off the machine.
        // The proxy is read from the environment when the agent is built, so set one for that moment.
        // (Port 9 is "discard": should another test build an agent meanwhile, its requests fail fast.)
        std::env::set_var("ALL_PROXY", "http://127.0.0.1:9");
        let agent = http_agent();
        std::env::remove_var("ALL_PROXY");
        assert!(agent.config().proxy().is_none(), "proxy picked up: {:?}", agent.config().proxy());
    }

    #[test]
    fn free_port_is_nonzero() {
        assert_ne!(free_port().unwrap(), 0);
    }

    #[test]
    fn args_bind_localhost_with_low_memory_flags_and_no_key() {
        let cfg = ServerConfig::new("llama-server.exe".into(), "m.gguf".into());
        let args = cfg.args(5555).join(" ");
        for expected in ["--host 127.0.0.1", "--port 5555", "-c 8192", "-t 4", "-ngl 0", "-fa on", "-ctk q8_0", "-ctv q8_0", "-ub 256", "-b 512", "--cache-ram 512", "--no-webui"] {
            assert!(args.contains(expected), "missing `{expected}` in `{args}`");
        }
        assert!(!args.contains("api-key"), "API key must not appear on the command line (NFR-SEC-002)");
    }

    #[test]
    fn the_embedding_server_has_its_own_flags() {
        // FR-MEM-004: same local-only rules, embedding mode, one batch per input
        let cfg = ServerConfig::embedding("llama-server.exe".into(), "embeddinggemma-300M-Q8_0.gguf".into());
        let args = cfg.args(5556).join(" ");
        for expected in ["--host 127.0.0.1", "--port 5556", "-c 2048", "-ub 2048", "-b 2048", "--embeddings", "-ngl 0"] {
            assert!(args.contains(expected), "missing `{expected}` in `{args}`");
        }
        assert!(!args.contains("--reasoning") && !args.contains("api-key"));
        let host = ModelHost::new(cfg, Duration::from_secs(60));
        assert_eq!(crate::memory::Embedder::model_id(&host), "embeddinggemma-300M-Q8_0.gguf");
    }

    #[test]
    fn a_failed_embedding_server_is_left_alone_for_a_while() {
        // final review I2: no respawn every 2 s, no long wait for every question
        let (_tmp, mut cfg) = dying_config();
        cfg.embedding = true;
        let mut host = ModelHost::new(cfg, Duration::from_secs(60));
        let stop = std::sync::atomic::AtomicBool::new(false);
        assert!(crate::memory::Embedder::embed(&mut host, &["x".into()], &stop).is_err());
        let tried = host.start_attempts();
        let started = Instant::now();
        assert!(crate::memory::Embedder::embed(&mut host, &["x".into()], &stop).is_err());
        assert_eq!(host.start_attempts(), tried, "not started again");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn missing_files_are_reported_before_spawning() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = ServerConfig::new(tmp.path().join("nope.exe"), tmp.path().join("nope.gguf"));
        assert!(matches!(LlamaServer::start(&cfg), Err(LlmError::MissingBinary(_))));
    }

    #[test]
    fn a_server_that_dies_at_startup_fails_fast() {
        // Review Focus 4: the test binary itself rejects llama-server's flags and exits at once.
        let tmp = tempfile::tempdir().unwrap();
        let model = tmp.path().join("m.gguf");
        std::fs::write(&model, b"not a model").unwrap();
        let mut cfg = ServerConfig::new(std::env::current_exe().unwrap(), model);
        cfg.startup_timeout = Duration::from_secs(20);
        let started = Instant::now();
        assert!(matches!(LlamaServer::start(&cfg), Err(LlmError::Exited(_))));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(windows)]
    #[test]
    fn child_processes_die_with_their_parent() {
        // Final review C2: a closed, crashed or killed PLA must not leave llama-server behind
        if std::env::var_os("PLA_JOB_PARENT").is_some() {
            let child = Command::new("ping")
                .args(["-n", "60", "127.0.0.1"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            kill_with_parent(&child);
            println!("CHILD_PID={}", child.id());
            std::process::exit(0); // leaves without dropping or killing the child
        }
        // Read the pid line and wait for the parent to exit; do not wait for the pipe to close, because a
        // child that inherited it would keep it open (that is exactly what this test must not depend on).
        let mut parent = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "llm::tests::child_processes_die_with_their_parent", "--nocapture", "--test-threads=1"])
            .env("PLA_JOB_PARENT", "1")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(parent.stdout.take().unwrap());
        let mut pid = None;
        let mut line = String::new();
        while pid.is_none() && reader.read_line(&mut line).unwrap() > 0 {
            pid = line.split("CHILD_PID=").nth(1).and_then(|s| s.trim().parse::<u32>().ok());
            line.clear();
        }
        let pid = pid.expect("parent printed the child pid");
        parent.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let list = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output().unwrap();
            if !String::from_utf8_lossy(&list.stdout).contains(&pid.to_string()) {
                break;
            }
            assert!(Instant::now() < deadline, "child {pid} outlived its parent");
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}
