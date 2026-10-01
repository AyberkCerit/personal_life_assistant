//! Manual end-to-end check: note text -> model -> validated items.
//!   cargo run -p pla-core --example extract -- 2026-10-06 "yarın 9da dişçi var, dün 7 saat uyudum"
//! Uses PLA_LLAMA_SERVER and PLA_MODEL like tests/llm_live.rs.

use std::path::PathBuf;
use std::time::Instant;

use chrono::NaiveDate;
use pla_core::extraction::{validate, ValidationSettings};
use pla_core::llm::{LlamaServer, ServerConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let reference = NaiveDate::parse_from_str(&args.next().ok_or("usage: extract <YYYY-MM-DD> <text>")?, "%Y-%m-%d")?;
    let text = args.collect::<Vec<_>>().join(" ");

    let mut cfg = ServerConfig::new(PathBuf::from(std::env::var("PLA_LLAMA_SERVER")?), PathBuf::from(std::env::var("PLA_MODEL")?));
    cfg.ctx_size = 2048;
    cfg.startup_timeout = std::time::Duration::from_secs(120);

    let t = Instant::now();
    let server = LlamaServer::start(&cfg)?;
    println!("model ready in {:.1}s", t.elapsed().as_secs_f64());

    let t = Instant::now();
    let raw = server.extract_raw(reference, &text)?;
    println!("raw ({:.1}s): {raw}", t.elapsed().as_secs_f64());
    for item in pla_core::extraction::parse_extraction(&raw)?.items {
        println!("{:?}", validate(&item, reference, &ValidationSettings::default()));
    }
    Ok(())
}
