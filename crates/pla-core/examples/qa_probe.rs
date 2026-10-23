//! Where the time of an answer goes (M6 performance, NFR-PERF-009/010 diagnosis): every request the
//! assistant and the extraction make, with llama-server's own prompt and generation timings.
//!   cargo run --release -p pla-core --example qa_probe -- <reference out dir>
//! PLA_LLAMA_SERVER and PLA_MODEL set; uses the reference's cache.db (index it first).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, NaiveDate};
use pla_core::llm::{LlmError, ServerConfig};
use pla_core::qa::ChatModel;
use serde_json::Value;

const PORT: u16 = 18_533;

struct Probe {
    agent: ureq::Agent,
    log: Vec<String>,
}

impl Probe {
    fn post(&mut self, kind: &str, body: &Value) -> Result<String, LlmError> {
        let mut body = body.clone();
        body["stream"] = Value::Bool(false);
        let t = Instant::now();
        let mut r = self.agent.post(format!("http://127.0.0.1:{PORT}/v1/chat/completions")).send_json(&body).map_err(|e| LlmError::Http(e.to_string()))?;
        let v: Value = r.body_mut().read_json().map_err(|e| LlmError::Http(e.to_string()))?;
        let tm = &v["timings"];
        let line = format!(
            "{kind:8} wall {:6.0} ms | prompt {:5} new tok (cached {:5}) {:7.0} ms {:6.1} tok/s | gen {:4} tok {:6.0} ms",
            t.elapsed().as_secs_f64() * 1000.0,
            tm["prompt_n"],
            tm["cache_n"],
            tm["prompt_ms"].as_f64().unwrap_or(0.0),
            tm["prompt_per_second"].as_f64().unwrap_or(0.0),
            tm["predicted_n"],
            tm["predicted_ms"].as_f64().unwrap_or(0.0),
        );
        println!("{line}");
        self.log.push(line);
        Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_owned())
    }
}

impl ChatModel for Probe {
    fn complete(&mut self, body: &Value, _: &AtomicBool) -> Result<String, LlmError> {
        self.post(if body["max_tokens"] == 1 { "warm" } else { "decide" }, body)
    }
    fn stream(&mut self, body: &Value, _: &AtomicBool, on_token: &mut dyn FnMut(&str)) -> Result<String, LlmError> {
        let text = self.post("answer", body)?;
        on_token(&text);
        Ok(text)
    }
}

/// The llama-server this probe started: ended with the probe, also when a step fails (`?`).
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start() -> Result<Child, Box<dyn std::error::Error>> {
    let bin = PathBuf::from(std::env::var("PLA_LLAMA_SERVER")?);
    let model = PathBuf::from(std::env::var("PLA_MODEL")?);
    let args = ServerConfig::new(bin.clone(), model).args(PORT);
    let child = Command::new(bin).args(args).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
    for _ in 0..240 {
        if agent.get(format!("http://127.0.0.1:{PORT}/health")).call().is_ok() {
            return Ok(child);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("llama-server did not start".into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: qa_probe <reference out dir>")?);
    let mut vault = pla_core::vault::open_vault(&out.join("vault"))?;
    let data = pla_core::vault::resolve_data_dir(&out.join("PLA"), &mut vault)?;
    let dbs = pla_core::db::open_databases(&data)?;
    let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
    let now: DateTime<FixedOffset> = DateTime::parse_from_rfc3339("2026-10-10T20:00:00+03:00").unwrap();

    let _server = Server(start()?);
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(600))).build().into();
    let mut probe = Probe { agent, log: Vec::new() };

    if std::env::args().any(|a| a == "--extract") {
        println!("-- extraction, 3 blocks");
        for i in 0..3 {
            let block = format!("Not {i}: yarın saat {}'da toplantı var, dün gece 7 saat uyudum.", 9 + i);
            probe.post("extract", &pla_core::extraction::request_body(today, &block))?;
        }
    }

    println!("-- read-ahead (the panel opens)");
    pla_core::qa::engine::warm(&mut probe, &dbs.cache, &Default::default(), now, &pla_core::recall::Recall::default(), "memory", &AtomicBool::new(false))?;

    let env = pla_core::qa::tools::ToolEnv { vault: &vault, pla: &dbs.pla, cache: &dbs.cache, now, validation: Default::default() };
    for q in ["Bu hafta neler yaptım?", "Sınav ne zaman?"] {
        println!("-- question: {q}");
        let t = Instant::now();
        let r = pla_core::qa::engine::answer(&mut probe, &env, &pla_core::qa::engine::Question { text: q, turns: &[], memory: None, recall: None, ui_lang: "tr", persona: &Default::default() }, &AtomicBool::new(false), &mut |_| {});
        let tools: Vec<String> = r.as_ref().map(|a| a.tools.iter().map(|c| c.tool.clone()).collect()).unwrap_or_default();
        println!("   total {:.1} s, tools {tools:?}", t.elapsed().as_secs_f64());
    }
    Ok(())
}
