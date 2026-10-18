//! What the assistant gets to read for a question (FR-MEM-005/009), with and without the notes
//! the best hits link to: does every fact a case needs reach the context?
//!   cargo run --release -p pla-core --example retrieval_eval -- <label>
//! Vault and cases: `tests/retrieval/vault.json`. With PLA_LLAMA_SERVER and PLA_EMBED_MODEL set
//! (EmbeddingGemma) it measures the hybrid search too; without them, keywords alone.
//! Results: `research/retrieval-eval/results/<label>.jsonl`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset};
use pla_core::index::key;
use pla_core::llm::{ModelHost, ServerConfig};
use pla_core::memory::{self, Embedder};
use pla_core::qa::context::{build_with, LINKED};
use pla_core::util::tokens;
use serde_json::{json, Value};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-10-18T12:00:00+03:00").unwrap()
}

fn embedder() -> Option<ModelHost> {
    let bin = PathBuf::from(std::env::var("PLA_LLAMA_SERVER").ok()?);
    let model = PathBuf::from(std::env::var("PLA_EMBED_MODEL").ok()?);
    Some(ModelHost::new(ServerConfig::embedding(bin, model), Duration::from_secs(600)))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let label = std::env::args().nth(1).unwrap_or_else(|| "run".into());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(root.join("tests/retrieval/vault.json"))?)?;
    let pad: Vec<&str> = fixture["pad"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();

    let tmp = tempfile::tempdir()?;
    let dbs = pla_core::db::open_databases(tmp.path())?;
    let ms = now().timestamp_millis();
    for (i, n) in fixture["notes"].as_array().unwrap().iter().enumerate() {
        let mut text = n["text"].as_str().unwrap().to_owned();
        // filler paragraphs so notes are as long as real ones and the found share has to choose
        for k in 0..n["pad"].as_u64().unwrap_or(0) as usize {
            text.push_str("\n\n");
            text.push_str(pad[(i * 3 + k) % pad.len()]);
        }
        let mtime = ms - (n["age_days"].as_f64().unwrap() * 86_400_000.0) as i64;
        pla_core::index::index_note(&dbs.cache, n["path"].as_str().unwrap(), &text, mtime, text.len() as i64)?;
    }

    // generated dailies, older than the hand-written ones; the same every run (a fixed LCG)
    let daily = &fixture["daily"];
    let lines: Vec<&str> = daily["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
    let mut seed: u64 = 7;
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    let days = daily["days"].as_u64().unwrap() as i64;
    for d in 0..days {
        let age = 20 + d;
        let day = (now() - chrono::Duration::days(age)).format("%Y-%m-%d").to_string();
        let count = 2 + next(3);
        let body: Vec<&str> = (0..count).map(|_| lines[next(lines.len())]).collect();
        let text = format!("# {day}\n\n{}", body.join("\n\n"));
        pla_core::index::index_note(&dbs.cache, &format!("daily/gen/{day}.md"), &text, ms - age * 86_400_000, text.len() as i64)?;
    }

    let stop = AtomicBool::new(false);
    let mut host = embedder();
    if let Some(h) = host.as_mut() {
        let t = Instant::now();
        let r = memory::embed_pending(&dbs.cache, h, 16, Instant::now() + Duration::from_secs(600), &stop)?;
        println!("embedded {} chunks in {:.1} s ({} left)", r.embedded, t.elapsed().as_secs_f64(), r.left);
    } else {
        println!("PLA_LLAMA_SERVER / PLA_EMBED_MODEL not set: keywords only");
    }
    let model_id = host.as_ref().map(|h| h.model_id().to_owned()).unwrap_or_default();

    let mut modes: Vec<(&str, bool, usize)> = vec![("kelime", false, 0), ("kelime+bag", false, LINKED)];
    if host.is_some() {
        modes.extend([("hibrit", true, 0), ("hibrit+bag", true, LINKED)]);
    }
    let out_dir = root.join("../../research/retrieval-eval/results");
    std::fs::create_dir_all(&out_dir)?;
    let mut out = std::fs::File::create(out_dir.join(format!("{label}.jsonl")))?;
    // per mode: (passed linked, all linked, passed direct, all direct, found tokens, time)
    let mut totals = vec![(0, 0, 0, 0, 0usize, Duration::ZERO); modes.len()];
    for case in fixture["cases"].as_array().unwrap() {
        let q = case["q"].as_str().unwrap();
        let linked_case = case["cat"] == "linked";
        let vector = match host.as_mut() {
            Some(h) => Some(h.embed(&[memory::query_text(q)], &stop)?.remove(0)),
            None => None,
        };
        for (m, (name, hybrid, linked)) in modes.iter().enumerate() {
            let memory = if *hybrid { vector.as_deref().map(|v| (v, model_id.as_str())) } else { None };
            let t = Instant::now();
            let ctx = build_with(&dbs.cache, q, &[], now(), memory, "reports", *linked, &pla_core::recall::Recall::default(), "memory")?;
            let took = t.elapsed();
            let read = key(&format!("{}\n{}", ctx.recent, ctx.found));
            let missing: Vec<&str> = case["facts"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).filter(|f| !read.contains(&key(f))).collect();
            let pass = missing.is_empty();
            let tot = &mut totals[m];
            if linked_case {
                tot.1 += 1;
                tot.0 += usize::from(pass);
            } else {
                tot.3 += 1;
                tot.2 += usize::from(pass);
            }
            tot.4 += tokens(&ctx.found);
            tot.5 += took;
            writeln!(out, "{}", json!({"id": case["id"], "mode": name, "pass": pass, "missing": missing, "sources": ctx.sources, "found_tokens": tokens(&ctx.found)}))?;
            if !pass {
                println!("  {name:11} FAIL {:22} missing {missing:?} | sources {:?}", case["id"].as_str().unwrap(), ctx.sources);
            }
        }
    }
    let cases = fixture["cases"].as_array().unwrap().len();
    println!("\n| Mod | Bağlı | Doğrudan | Toplam | Ort. bulunan token | Ort. süre |\n| --- | --- | --- | --- | --- | --- |");
    for ((name, ..), (pl, l, pd, d, tok, took)) in modes.iter().zip(&totals) {
        println!(
            "| {name} | {pl}/{l} | {pd}/{d} | {}/{} | {} | {:.1} ms |",
            pl + pd,
            l + d,
            tok / cases,
            took.as_secs_f64() * 1000.0 / cases as f64
        );
    }
    Ok(())
}
