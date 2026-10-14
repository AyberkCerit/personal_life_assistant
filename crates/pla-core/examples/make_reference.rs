//! The reference data set (SRS §5, TBD-08; M6 performance plan § 1): 10 000 notes / about 1 GB of
//! vault (five years of daily notes, project and course notes, one 1 MB note, attachments), 20 000
//! tasks and 50 000 measurements in pla.db. Deterministic: the same seed gives the same data.
//!   cargo run --release -p pla-core --example make_reference -- <out dir> [notes] [attachment MB]
//! Writes <out>/vault and <out>/PLA (PLA's data folder for it: run PLA with APPDATA=<out>).

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDate};
use rusqlite::params;

/// xorshift64*: enough randomness for test data, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, list: &[&'a str]) -> &'a str {
        list[self.below(list.len())]
    }
}

const WORDS_TR: [&str; 40] = [
    "proje", "toplantı", "rapor", "bütçe", "tasarım", "ders", "sınav", "kitap", "fikir", "plan", "müşteri", "sunum", "taslak", "karar", "hafta", "sistem",
    "veri", "model", "arayüz", "test", "hata", "sürüm", "ekip", "görüşme", "notlar", "özet", "konu", "bölüm", "örnek", "liste", "hedef", "süreç", "kaynak",
    "sonuç", "yöntem", "deneme", "çalışma", "okuma", "tekrar", "akşam",
];
const WORDS_EN: [&str; 24] = [
    "project", "meeting", "report", "budget", "design", "course", "exam", "book", "idea", "plan", "client", "draft", "decision", "system", "data", "model",
    "interface", "release", "team", "summary", "chapter", "example", "goal", "result",
];
const FOLDERS: [&str; 6] = ["notes/projeler", "notes/dersler", "notes/kitaplar", "notes/iş", "notes/kişisel", "inbox"];
const TASKS: [&str; 10] = [
    "Yarın 10'da dişçi randevusu var.",
    "Cuma günü raporu teslim etmem lazım.",
    "Faturayı ödemeyi unutma.",
    "Annemi aramam gerek.",
    "Pazartesi sunumu hazırla.",
    "Kargoyu teslim al.",
    "Call the plumber on Thursday.",
    "Send the draft to Ali tomorrow.",
    "Kütüphaneye kitabı iade et.",
    "Haftaya proje toplantısı var.",
];
const METRICS: [&str; 6] = ["Dün gece 7 saat uyudum.", "Bugün 2 litre su içtim.", "10 bin adım attım.", "Sabah 78 kiloydum.", "Squat 4x5 100 kg yaptım.", "Slept 6 hours."];

fn sentence(rng: &mut Rng, base: usize, spread: usize) -> String {
    let words = base + rng.below(spread);
    let mut s: Vec<&str> = Vec::with_capacity(words);
    for _ in 0..words {
        s.push(if rng.below(5) == 0 { rng.pick(&WORDS_EN) } else { rng.pick(&WORDS_TR) });
    }
    let mut out = s.join(" ");
    if let Some(first) = out.get(..1) {
        out = first.to_uppercase() + &out[1..];
    }
    out + "."
}

/// One note's body: paragraphs, a list, a task or a measurement now and then, links and tags.
fn body(rng: &mut Rng, title: &str, titles: &[String], paragraphs: usize) -> String {
    let mut s = format!("# {title}\n\n");
    for p in 0..paragraphs {
        let n = 2 + rng.below(4);
        let para: Vec<String> = (0..n).map(|_| sentence(rng, 6, 10)).collect();
        s.push_str(&para.join(" "));
        if !titles.is_empty() && rng.below(3) == 0 {
            s.push_str(&format!(" Bkz. [[{}]].", titles[rng.below(titles.len())]));
        }
        s.push_str("\n\n");
        if p == 0 && rng.below(2) == 0 {
            for _ in 0..(2 + rng.below(4)) {
                s.push_str(&format!("- {}\n", sentence(rng, 4, 5)));
            }
            s.push('\n');
        }
        if rng.below(4) == 0 {
            s.push_str(rng.pick(&TASKS));
            s.push_str("\n\n");
        }
        if rng.below(6) == 0 {
            s.push_str(rng.pick(&METRICS));
            s.push_str("\n\n");
        }
    }
    if rng.below(3) == 0 {
        s.push_str(&format!("#{} #{}\n", rng.pick(&WORDS_TR), rng.pick(&WORDS_EN)));
    }
    s
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: make_reference <out dir> [notes] [attachment MB]")?);
    let notes: usize = std::env::args().nth(2).map(|n| n.parse()).transpose()?.unwrap_or(10_000);
    let attachment_mb: usize = std::env::args().nth(3).map(|n| n.parse()).transpose()?.unwrap_or(900);
    let vault_dir = out.join("vault");
    let app_root = out.join("PLA");
    std::fs::create_dir_all(&vault_dir)?;
    let mut rng = Rng(0x5EED_1234_ABCD_0042);
    let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
    let started = std::time::Instant::now();

    // five years of daily notes, then the rest in folders
    let days = 1825.min(notes);
    let others = notes.saturating_sub(days + 1);
    let titles: Vec<String> = (0..others).map(|i| format!("{} {} {i}", rng.pick(&WORDS_TR), rng.pick(&WORDS_EN))).collect();
    let mut bytes = 0usize;
    for d in 0..days {
        let date = today - Duration::days(d as i64);
        let paragraphs = 1 + rng.below(4);
        let text = body(&mut rng, &date.to_string(), &titles, paragraphs);
        bytes += text.len();
        write(&vault_dir.join(format!("daily/{}/{date}.md", date.format("%Y"))), &text);
    }
    for (i, title) in titles.iter().enumerate() {
        let folder = FOLDERS[i % FOLDERS.len()];
        let paragraphs = 2 + rng.below(8);
        let text = body(&mut rng, title, &titles, paragraphs);
        bytes += text.len();
        write(&vault_dir.join(format!("{folder}/{title}.md")), &text);
    }
    // NFR-PERF-002: one note of 1 MB
    let mut big = String::from("# Büyük not\n\n");
    while big.len() < 1024 * 1024 {
        big.push_str(&sentence(&mut rng, 14, 1));
        big.push_str(if rng.below(6) == 0 { "\n\n" } else { " " });
    }
    bytes += big.len();
    write(&vault_dir.join("notes/Büyük not.md"), &big);
    // the rest of the gigabyte: attachments
    let chunk: Vec<u8> = (0..1024 * 1024).map(|_| rng.next() as u8).collect();
    for i in 0..attachment_mb {
        let mut f = std::fs::File::create({
            let p = vault_dir.join(format!("attachments/{:03}/ek-{i}.png", i / 100));
            std::fs::create_dir_all(p.parent().unwrap())?;
            p
        })?;
        f.write_all(&chunk)?;
    }
    println!("{notes} notes ({:.0} MB of text) and {attachment_mb} MB of attachments in {:.1}s", bytes as f64 / 1e6, started.elapsed().as_secs_f64());

    // pla.db: 20 000 tasks and 50 000 measurements over five years
    let mut vault = pla_core::vault::open_vault(&vault_dir)?;
    let data = pla_core::vault::resolve_data_dir(&app_root, &mut vault)?;
    let dbs = pla_core::db::open_databases(&data)?;
    let conn = dbs.pla;
    conn.execute_batch("BEGIN")?;
    for i in 0..20_000 {
        let date = today - Duration::days(rng.below(1825) as i64) + Duration::days(30);
        let created = (date - Duration::days(rng.below(10) as i64)).and_hms_opt(9, 0, 0).unwrap().format("%Y-%m-%dT%H:%M:%S+03:00").to_string();
        let done = date < today && rng.below(10) < 8;
        let completed = done.then(|| date.and_hms_opt(18, 0, 0).unwrap().format("%Y-%m-%dT%H:%M:%S+03:00").to_string());
        conn.execute(
            "INSERT INTO task (task_id, title, date, time, status, origin, completed_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6, ?7, ?7)",
            params![
                format!("ref-task-{i}"),
                format!("{} {}", rng.pick(&WORDS_TR), rng.pick(&WORDS_TR)),
                date.to_string(),
                (rng.below(3) == 0).then(|| format!("{:02}:00", 8 + rng.below(12))),
                if done { "done" } else { "open" },
                completed,
                created
            ],
        )?;
    }
    let kinds = [("sleep", "h", 5.0, 9.0), ("water", "ml", 500.0, 3000.0), ("steps", "count", 2000.0, 15000.0), ("weight", "kg", 70.0, 85.0), ("workout", "kg", 20.0, 120.0)];
    for i in 0..50_000 {
        let (kind, unit, lo, hi) = kinds[i % kinds.len()];
        let date = today - Duration::days(rng.below(1825) as i64);
        let value = lo + (hi - lo) * (rng.below(1000) as f64 / 1000.0);
        let json = if kind == "workout" { format!(r#"{{"value":{value:.1},"exercise":"squat","sets":4,"reps":5}}"#) } else { format!(r#"{{"value":{value:.1}}}"#) };
        conn.execute(
            "INSERT INTO metric_record (metric_id, type, value_json, unit, date, origin, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'manual', ?6)",
            params![format!("ref-metric-{i}"), kind, json, unit, date.to_string(), format!("{date}T21:00:00+03:00")],
        )?;
    }
    conn.execute_batch("COMMIT")?;
    let settings = pla_core::settings::AppSettings { vault_path: Some(vault_dir.clone()), setup_complete: true, tray_hint_shown: true, ..Default::default() };
    pla_core::settings::save_settings(&app_root, &settings)?;
    println!("pla.db: 20000 tasks, 50000 measurements; data {} ; total {:.1}s", data.display(), started.elapsed().as_secs_f64());
    Ok(())
}
