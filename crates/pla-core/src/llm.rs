//! The llama.cpp `llama-server` sidecar (FR-MDL-011, -012, -015, -017, -020, -021; NFR-SEC-002).
//! Only the Rust core talks to it; it listens on 127.0.0.1 behind a per-run API key.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use serde_json::Value;

use crate::extraction::{parse_extraction, request_body, RawExtraction};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub server_bin: PathBuf,
    pub model: PathBuf,
    pub ctx_size: u32,
    pub threads: u32,
    pub startup_timeout: Duration,
}

impl ServerConfig {
    pub fn new(server_bin: PathBuf, model: PathBuf) -> Self {
        Self { server_bin, model, ctx_size: 8192, threads: 4, startup_timeout: Duration::from_secs(30) }
    }

    pub fn args(&self, port: u16) -> Vec<String> {
        let mut args: Vec<String> = vec!["-m".into(), self.model.to_string_lossy().into_owned()];
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

pub struct LlamaServer {
    child: Child,
    base_url: String,
    api_key: String,
    agent: ureq::Agent,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stderr_reader: Option<JoinHandle<()>>,
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

    pub fn extract(&self, reference: NaiveDate, text: &str) -> Result<RawExtraction, LlmError> {
        Ok(parse_extraction(&self.extract_raw(reference, text)?)?)
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
}

impl ModelHost {
    pub fn new(cfg: ServerConfig, idle_after: Duration) -> Self {
        Self { cfg, idle_after, server: None, last_used: Instant::now(), start_attempts: 0 }
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
            self.start_attempts += 1;
            let started = match LlamaServer::start(&self.cfg) {
                Err(LlmError::MissingBinary(p)) => Err(LlmError::MissingBinary(p)),
                Err(LlmError::MissingModel(p)) => Err(LlmError::MissingModel(p)),
                Err(_) => {
                    self.start_attempts += 1;
                    LlamaServer::start(&self.cfg)
                }
                ok => ok,
            }?;
            self.server = Some(started);
        }
        Ok(self.server.as_ref().expect("server was just started"))
    }
}

impl crate::pipeline::Extractor for ModelHost {
    fn extract_raw(&mut self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
        let answer = self.server()?.extract_raw(reference, text);
        self.last_used = Instant::now();
        answer
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
        for expected in ["--host 127.0.0.1", "--port 5555", "-c 8192", "-t 4", "-ngl 0", "-fa on", "-ctk q8_0", "-ctv q8_0", "-ub 256", "-b 512", "--no-webui"] {
            assert!(args.contains(expected), "missing `{expected}` in `{args}`");
        }
        assert!(!args.contains("api-key"), "API key must not appear on the command line (NFR-SEC-002)");
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
}
