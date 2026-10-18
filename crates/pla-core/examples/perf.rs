//! The core performance measurements (SRS §5.1, M6 performance plan § 2) on the reference data set
//! made by `make_reference`. p95 of 20 runs unless a line says otherwise.
//!   cargo run --release -p pla-core --example perf -- <reference out dir> [--model]
//! With --model (PLA_LLAMA_SERVER and PLA_MODEL set) the model's numbers are measured too.
//! Prints a table and writes research/perf/core.json.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, NaiveDate};
use pla_core::extraction::MetricKind;
use pla_core::llm::{LlamaServer, ModelHost, ServerConfig};
use pla_core::pipeline::Extractor;
use serde_json::{json, Value};

const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../research/perf");

fn p95(mut ms: Vec<f64>) -> f64 {
    ms.sort_by(f64::total_cmp);
    ms[((ms.len() as f64 * 0.95).ceil() as usize).saturating_sub(1).min(ms.len() - 1)]
}

fn time<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let t = Instant::now();
    let out = f();
    (t.elapsed().as_secs_f64() * 1000.0, out)
}

fn runs(n: usize, mut f: impl FnMut(usize)) -> f64 {
    p95((0..n).map(|i| time(|| f(i)).0).collect())
}

/// One interface query to time, by name.
type Query<'a> = Box<dyn Fn() + 'a>;

struct Row {
    id: &'static str,
    what: String,
    value: f64,
    unit: &'static str,
    limit: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: perf <reference out dir> [--model]")?);
    let with_model = std::env::args().any(|a| a == "--model");
    let mut vault = pla_core::vault::open_vault(&out.join("vault"))?;
    let data = pla_core::vault::resolve_data_dir(&out.join("PLA"), &mut vault)?;
    let dbs = pla_core::db::open_databases(&data)?;
    let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
    let now: DateTime<FixedOffset> = DateTime::parse_from_rfc3339("2026-10-10T20:00:00+03:00").unwrap();
    let mut rows: Vec<Row> = Vec::new();

    // NFR-PERF-011: the first full-text index of the reference vault (one run: minutes, not ms)
    // a new cache.db each run: the first index, as on a first start
    let fresh = tempfile::tempdir()?;
    let mut dbs = dbs;
    dbs.cache = pla_core::db::open_databases(fresh.path())?.cache;
    let (ms, report) = time(|| pla_core::index::sync_vault(&dbs.cache, &vault.root));
    let report = report?;
    println!("indexed {} notes", report.indexed);
    rows.push(Row { id: "NFR-PERF-011", what: format!("first index of {} notes (one run)", report.indexed), value: ms / 1000.0, unit: "s", limit: 300.0 });

    // NFR-PERF-004: full-text search, first results
    let queries = ["proje", "toplantı bütçe", "design", "rapor taslak", "sınav", "kitap okuma", "müşteri sunum", "release", "veri model", "hafta plan"];
    let ms = runs(20, |i| {
        let hits = pla_core::index::search(&dbs.cache, queries[i % queries.len()], None, None, 20).unwrap();
        assert!(!hits.is_empty());
    });
    rows.push(Row { id: "NFR-PERF-004", what: "search, first 20 results".into(), value: ms, unit: "ms", limit: 300.0 });

    // NFR-PERF-005: the queries behind the task panel, the measurements screen and the reports
    let pla = &dbs.pla;
    use pla_core::tasks::{list_tasks, TaskList};
    let queries: Vec<(&str, Query)> = vec![
        ("tasks: today", Box::new(|| drop(list_tasks(pla, TaskList::Today, today).unwrap()))),
        ("tasks: upcoming", Box::new(|| drop(list_tasks(pla, TaskList::Upcoming, today).unwrap()))),
        ("tasks: completed", Box::new(|| drop(list_tasks(pla, TaskList::Completed, today).unwrap()))),
        ("measurements: overview", Box::new(|| drop(pla_core::metrics::overview(pla, today).unwrap()))),
        ("measurements: sleep, 90 days", Box::new(|| drop(pla_core::metrics::summary(pla, MetricKind::Sleep, 90, today).unwrap()))),
        ("measurements: steps, 365 days", Box::new(|| drop(pla_core::metrics::summary(pla, MetricKind::Steps, 365, today).unwrap()))),
        ("weekly report numbers", Box::new(|| drop(pla_core::report::week_stats(pla, pla_core::report::Week::of(today)).unwrap()))),
    ];
    for (name, q) in &queries {
        let ms = runs(20, |_| q());
        rows.push(Row { id: "NFR-PERF-005", what: (*name).into(), value: ms, unit: "ms", limit: 100.0 });
    }

    // NFR-PERF-012: background jobs use at most N−1 logical cores
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let threads = ServerConfig::new("x".into(), "y".into()).threads as usize;
    rows.push(Row { id: "NFR-PERF-012", what: format!("model threads of {cores} logical cores (indexer: 1)"), value: threads as f64, unit: "cores", limit: (cores - 1) as f64 });

    if with_model {
        let bin = PathBuf::from(std::env::var("PLA_LLAMA_SERVER")?);
        let model = PathBuf::from(std::env::var("PLA_MODEL")?);
        // the minimum PC of SRS §2.4 has 4 cores: PLA gives the model 3 there (NFR-PERF-012)
        let config = || {
            let mut cfg = ServerConfig::new(bin.clone(), model.clone());
            cfg.threads = pla_core::llm::model_threads(4);
            cfg.startup_timeout = Duration::from_secs(120);
            cfg
        };
        let threads = config().threads;
        // NFR-PERF-008: from start to healthy (the file in the OS cache after the first run, as on a warm SSD)
        let mut loads = Vec::new();
        for _ in 0..5 {
            let (ms, server) = time(|| LlamaServer::start(&config()));
            drop(server?);
            loads.push(ms);
        }
        rows.push(Row { id: "NFR-PERF-008", what: "model load to healthy (5 runs)".into(), value: p95(loads) / 1000.0, unit: "s", limit: 5.0 });

        let mut host = ModelHost::new(config(), Duration::from_secs(600));
        // NFR-PERF-009: a note of 10 changed blocks, each with a task and a measurement (the worst
        // case: a block with nothing in it is a few tokens), model loaded
        let blocks: Vec<String> = (0..10).map(|i| format!("Not {i}: yarın saat {}'da toplantı var, dün gece 7 saat uyudum.", 9 + i % 8)).collect();
        host.extract_raw(today, "ısınma")?; // load first: the limit excludes loading
        let mut notes = Vec::new();
        for _ in 0..5 {
            let (ms, _) = time(|| {
                for b in &blocks {
                    host.extract_raw(today, b).unwrap();
                }
            });
            notes.push(ms);
        }
        rows.push(Row { id: "NFR-PERF-009", what: format!("extraction of a 10-block note, {threads} threads (5 runs)"), value: p95(notes) / 1000.0, unit: "s", limit: 60.0 });

        // NFR-PERF-010 (SRS 1.3): the first answer token once the panel's read-ahead is done; a
        // question that calls a tool reads its result too, so the two are kept apart
        let env = pla_core::qa::tools::ToolEnv { vault: &vault, pla: &dbs.pla, cache: &dbs.cache, now, validation: Default::default() };
        let first_token = |host: &mut ModelHost, q: &str| -> (f64, bool) {
            let started = Instant::now();
            let mut first: Option<f64> = None;
            let answered = pla_core::qa::engine::answer(host, &env, &pla_core::qa::engine::Question { text: q, turns: &[], memory: None, recall: None, ui_lang: "tr" }, &AtomicBool::new(false), &mut |p| {
                if matches!(p, pla_core::qa::engine::Progress::Token(_)) && first.is_none() {
                    first = Some(started.elapsed().as_secs_f64() * 1000.0);
                }
            });
            (first.unwrap_or(f64::NAN), answered.is_ok_and(|a| !a.tools.is_empty()))
        };
        let questions = ["Bu hafta neler yaptım?", "Proje toplantısında ne karar verdik?", "Kitap notlarımda neler var?", "Sınav ne zaman?", "Bütçe hakkında ne yazdım?"];
        let (mut ahead, mut plain, mut with_tool) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..10 {
            host.stop(); // the panel opens with no model loaded: the read-ahead starts it
            let (ms, warmed) = time(|| pla_core::qa::engine::warm(&mut host, &dbs.cache, now, &pla_core::recall::Recall::default(), &AtomicBool::new(false)));
            warmed?;
            ahead.push(ms);
            let (ms, tool) = first_token(&mut host, questions[i % questions.len()]);
            if tool {
                with_tool.push(ms)
            } else {
                plain.push(ms)
            }
        }
        rows.push(Row { id: "—", what: format!("read-ahead when the panel opens, model not loaded, {threads} threads (10 runs)"), value: p95(ahead) / 1000.0, unit: "s", limit: f64::INFINITY });
        let n = (plain.len(), with_tool.len());
        if !plain.is_empty() {
            rows.push(Row { id: "NFR-PERF-010", what: format!("first answer token after the read-ahead, no tool ({} runs)", n.0), value: p95(plain) / 1000.0, unit: "s", limit: 45.0 });
        }
        if !with_tool.is_empty() {
            rows.push(Row { id: "NFR-PERF-010", what: format!("first answer token after the read-ahead, a tool ({} runs)", n.1), value: p95(with_tool) / 1000.0, unit: "s", limit: 70.0 });
        }
        // without the read-ahead (a question asked the moment the panel opens), for the record
        host.stop();
        let (cold, _) = first_token(&mut host, questions[3]);
        rows.push(Row { id: "—", what: "first answer token with no read-ahead, model not loaded (one run)".into(), value: cold / 1000.0, unit: "s", limit: f64::INFINITY });
    }

    println!("\n| SRS | measured | p95 | limit | |\n| --- | --- | --- | --- | --- |");
    let mut json_rows: Vec<Value> = Vec::new();
    for r in &rows {
        let ok = r.value <= r.limit;
        println!("| {} | {} | {:.2} {} | {} {} | {} |", r.id, r.what, r.value, r.unit, r.limit, r.unit, if ok { "✓" } else { "✗" });
        json_rows.push(json!({ "id": r.id, "what": r.what, "value": r.value, "unit": r.unit, "limit": r.limit, "pass": ok }));
    }
    std::fs::create_dir_all(OUT)?;
    std::fs::write(format!("{OUT}/core.json"), serde_json::to_string_pretty(&json_rows)?)?;
    Ok(())
}
