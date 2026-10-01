//! The llama.cpp `llama-server` sidecar (FR-MDL-011, -012, -015, -017, -020, -021; NFR-SEC-002).
//! Only the Rust core talks to it; it listens on 127.0.0.1 behind a per-run API key.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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

pub struct LlamaServer {
    child: Child,
    base_url: String,
    api_key: String,
    agent: ureq::Agent,
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
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let child = cmd.spawn()?;

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        let mut server = Self { child, base_url: format!("http://127.0.0.1:{port}"), api_key, agent };
        server.wait_healthy(cfg.startup_timeout)?; // on error, Drop kills the child
        Ok(server)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn wait_healthy(&mut self, timeout: Duration) -> Result<(), LlmError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Err(LlmError::Exited(status.to_string()));
            }
            if self.agent.get(format!("{}/health", self.base_url)).call().is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(LlmError::StartupTimeout(timeout));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// The model's raw JSON answer for one block of text.
    pub fn extract_raw(&self, reference: NaiveDate, text: &str) -> Result<String, LlmError> {
        let mut response = self
            .agent
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(request_body(reference, text))
            .map_err(|e| LlmError::Http(e.to_string()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_is_256_bit_hex_and_fresh() {
        let a = new_api_key().unwrap();
        let b = new_api_key().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
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
