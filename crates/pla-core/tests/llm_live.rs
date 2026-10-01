//! Needs a real llama-server and model. Run:
//!   set PLA_LLAMA_SERVER=C:\dev\PLA\research\f1-model-eval\bin\llama-server.exe
//!   set PLA_MODEL=C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf
//!   cargo test -p pla-core --test llm_live -- --ignored --nocapture

use std::path::PathBuf;

use chrono::{NaiveDate, NaiveTime};
use pla_core::extraction::{validate, ValidItem, ValidationSettings};
use pla_core::llm::{LlamaServer, ServerConfig};

fn config() -> ServerConfig {
    let bin = std::env::var("PLA_LLAMA_SERVER").expect("set PLA_LLAMA_SERVER");
    let model = std::env::var("PLA_MODEL").expect("set PLA_MODEL");
    let mut cfg = ServerConfig::new(PathBuf::from(bin), PathBuf::from(model));
    cfg.ctx_size = 2048; // F1 low-memory setting; enough for extraction
    cfg.startup_timeout = std::time::Duration::from_secs(120);
    cfg
}

#[test]
#[ignore]
fn extracts_a_dated_task_end_to_end() {
    let server = LlamaServer::start(&config()).unwrap();
    let reference = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
    let ex = server.extract(reference, "yarın 9da dişçi var").unwrap();
    assert_eq!(ex.items.len(), 1, "{ex:?}");
    match validate(&ex.items[0], reference, &ValidationSettings::default()).unwrap() {
        ValidItem::Action { date, time, .. } => {
            assert_eq!(date, NaiveDate::from_ymd_opt(2026, 10, 7));
            assert_eq!(time, NaiveTime::from_hms_opt(9, 0, 0));
        }
        other => panic!("expected a task, got {other:?}"),
    }
}

#[test]
#[ignore]
fn rejects_requests_without_the_api_key_and_dies_with_the_handle() {
    let server = LlamaServer::start(&config()).unwrap();
    let base = server.base_url().to_owned();
    assert!(ureq::get(&format!("{base}/v1/models")).call().is_err(), "unauthenticated request must fail");
    assert!(ureq::get(&format!("{base}/health")).call().is_ok(), "health stays public");
    drop(server);
    assert!(ureq::get(&format!("{base}/health")).call().is_err(), "server must be gone after drop");
}
