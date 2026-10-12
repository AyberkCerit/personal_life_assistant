//! PLA's log file (architecture review A7). A release build has no console, so a problem the user
//! is not shown (indexing, embedding, a notification Windows refused) goes to
//! `%APPDATA%\PLA\logs\pla.log`; one older file is kept, about a megabyte each. Local only.

use std::io::Write;
use std::path::Path;

use chrono::{DateTime, FixedOffset, Local};

const FILE: &str = "pla.log";
const OLDER: &str = "pla.1.log";
const MAX_BYTES: u64 = 1024 * 1024;

/// Writes `message` to the log (and to stderr, for `tauri dev`). Never fails: a log that cannot be
/// written must not break what it reports on.
pub fn error(message: &str) {
    eprintln!("PLA: {message}");
    if cfg!(test) {
        return; // tests that fail on purpose stay out of the user's real log
    }
    if let Some(root) = pla_core::vault::default_app_root() {
        let _ = append(&root.join("logs"), message, Local::now().fixed_offset(), MAX_BYTES);
    }
}

/// Appends one line to `dir/pla.log`; a file of `max_bytes` or more becomes `pla.1.log` first.
pub fn append(dir: &Path, message: &str, now: DateTime<FixedOffset>, max_bytes: u64) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= max_bytes) {
        std::fs::rename(&path, dir.join(OLDER))?;
    }
    let line = format!("{} {}\n", now.format("%Y-%m-%d %H:%M:%S"), message.replace(['\r', '\n'], " "));
    // one write per line: threads that log at once do not interleave inside a line
    std::fs::OpenOptions::new().create(true).append(true).open(&path)?.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(s).unwrap()
    }

    #[test]
    fn a_line_is_appended_with_its_time() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        append(&dir, "indexing failed: disk", at("2026-10-12T09:30:00+03:00"), 1024).unwrap();
        append(&dir, "embedding failed", at("2026-10-12T09:31:05+03:00"), 1024).unwrap();
        let text = std::fs::read_to_string(dir.join("pla.log")).unwrap();
        assert_eq!(text, "2026-10-12 09:30:00 indexing failed: disk\n2026-10-12 09:31:05 embedding failed\n");
    }

    #[test]
    fn a_full_file_becomes_the_older_one() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        append(dir, "first", at("2026-10-12T09:30:00+03:00"), 20).unwrap();
        append(dir, "second", at("2026-10-12T09:30:01+03:00"), 20).unwrap(); // the file is full now
        append(dir, "third", at("2026-10-12T09:30:02+03:00"), 20).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("pla.log")).unwrap(), "2026-10-12 09:30:02 third\n");
        assert!(std::fs::read_to_string(dir.join("pla.1.log")).unwrap().ends_with("second\n"));
    }
}
