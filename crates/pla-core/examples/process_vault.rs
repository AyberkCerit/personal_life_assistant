//! Real end-to-end run: vault → queue → Gemma → pla.db.
//!   $env:PLA_LLAMA_SERVER = "C:\dev\PLA\research\f1-model-eval\bin\llama-server.exe"
//!   $env:PLA_MODEL = "C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
//!   $env:PLA_APP_ROOT = "$env:TEMP\pla-dev"      # keeps test data out of %APPDATA%\PLA
//!   cargo run -p pla-core --example process_vault -- <vault folder>

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::Local;
use pla_core::db::open_databases;
use pla_core::llm::{ModelHost, ServerConfig};
use pla_core::pipeline::{enqueue_all, process_queue, PipelineSettings};
use pla_core::vault::{default_app_root, open_vault, resolve_data_dir};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("usage: process_vault <vault folder>")?);
    let app_root = std::env::var_os("PLA_APP_ROOT").map(PathBuf::from).or_else(default_app_root).ok_or("no app data folder")?;
    let mut vault = open_vault(&root)?;
    let dir = resolve_data_dir(&app_root, &mut vault)?;
    let mut dbs = open_databases(&dir)?;
    let now = Local::now().fixed_offset();
    println!("vault {} → data {}", vault.root.display(), dir.display());
    println!("queued {} notes", enqueue_all(&vault, &dbs.pla, now)?);

    let mut cfg = ServerConfig::new(PathBuf::from(std::env::var("PLA_LLAMA_SERVER")?), PathBuf::from(std::env::var("PLA_MODEL")?));
    cfg.ctx_size = 2048; // F1 low-memory setting; enough for extraction (TBD-10 decides the Q&A context)
    cfg.startup_timeout = Duration::from_secs(120);
    let mut host = ModelHost::new(cfg, Duration::from_secs(60));

    let started = Instant::now();
    let report = process_queue(&vault, &mut dbs.pla, &mut host, &PipelineSettings::default(), now)?;
    println!(
        "{} notes, {} blocks in {:.1}s, model error: {:?}",
        report.notes_done,
        report.blocks_extracted,
        started.elapsed().as_secs_f64(),
        report.model_error
    );
    for (note, outcome) in &report.outcomes {
        println!("  {note}: {outcome:?}");
    }

    let mut tasks = dbs.pla.prepare("SELECT title, date, time, notify_at, source_missing FROM task ORDER BY date, time")?;
    let rows = tasks.query_map([], |r| {
        Ok(format!(
            "{} | {} {} | notify {} | source missing {}",
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            r.get::<_, bool>(4)?
        ))
    })?;
    println!("tasks:");
    for row in rows {
        println!("  {}", row?);
    }
    let mut metrics = dbs.pla.prepare("SELECT type, value_json, unit, date FROM metric_record ORDER BY date")?;
    let rows = metrics.query_map([], |r| {
        Ok(format!("{} {} {} on {}", r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default(), r.get::<_, String>(3)?))
    })?;
    println!("metrics:");
    for row in rows {
        println!("  {}", row?);
    }
    Ok(())
}
