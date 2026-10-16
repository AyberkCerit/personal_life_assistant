//! What a command tells the page when it fails (FR-I18N-002, M6 translations): `code|detail|reason`.
//! `code` names a message the page has in every language (`error.<code>` in i18n.ts), `detail` goes
//! into it (a path, a name), `reason` is the technical text the user can only copy. No sentence is
//! made here: the page says it in the language chosen now (FR-SET-012).

use std::fmt::Display;

use pla_core::db::DbError;
use pla_core::files::FileError;
use pla_core::llm::LlmError;
use pla_core::vault::VaultError;

/// Every code a command can send; each has `error.<code>` in Turkish and English (tested).
pub const CODES: &[&str] = &[
    "missing",
    "permission",
    "disk_full",
    "read_only",
    "io",
    "db",
    "newer_schema",
    "bad_path",
    "bad_name",
    "reserved",
    "exists",
    "conflict",
    "not_folder",
    "network",
    "bad_config",
    "no_vault",
    "no_app_data",
    "recycle_bin",
    "backup_damaged",
    "model_server_missing",
    "model_file_missing",
    "model_start",
    "model_timeout",
    "model",
    "unexpected",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserError {
    pub code: &'static str,
    pub detail: String,
    pub reason: String,
}

impl UserError {
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        debug_assert!(CODES.contains(&code), "unknown error code {code}");
        Self { code, detail: detail.into(), reason: String::new() }
    }

    pub fn because(mut self, reason: impl Display) -> Self {
        self.reason = reason.to_string();
        self
    }

    /// Nothing the user could act on: a plain message and the text to copy.
    pub fn unexpected(reason: impl Display) -> Self {
        Self::new("unexpected", "").because(reason)
    }
}

impl From<UserError> for String {
    fn from(e: UserError) -> String {
        format!("{}|{}|{}", e.code, e.detail, e.reason)
    }
}

/// For `map_err`: any error the commands meet, as the page reads it.
pub fn ue(e: impl Into<UserError>) -> String {
    e.into().into()
}

/// `err` (a `code|detail|reason`) naming `what` when it names nothing yet: "the vault could not
/// open" says which folder.
pub fn naming(err: &str, what: &str) -> String {
    match err.split_once('|') {
        Some((code, rest)) if rest.starts_with('|') => format!("{code}|{what}{rest}"),
        _ => err.to_owned(),
    }
}

/// For `map_err` on a failure already turned into text (pla-core's assistant undo, for one): a plain
/// message, the text to copy.
pub fn unexpected(e: impl Display) -> String {
    UserError::unexpected(e).into()
}

impl From<std::io::Error> for UserError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let code = match e.kind() {
            ErrorKind::NotFound => "missing",
            ErrorKind::PermissionDenied => "permission",
            ErrorKind::StorageFull => "disk_full",
            ErrorKind::ReadOnlyFilesystem => "read_only",
            _ => "io",
        };
        Self::new(code, "").because(e)
    }
}

impl From<rusqlite::Error> for UserError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("db", "").because(e)
    }
}

impl From<DbError> for UserError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::Io(e) => e.into(),
            DbError::Sqlite(e) => e.into(),
            DbError::NewerSchema { found, supported } => Self::new("newer_schema", "").because(format!("schema {found}, this PLA reads {supported}")),
        }
    }
}

impl From<pla_core::pipeline::PipelineError> for UserError {
    fn from(e: pla_core::pipeline::PipelineError) -> Self {
        match e {
            pla_core::pipeline::PipelineError::Db(e) => e.into(),
            pla_core::pipeline::PipelineError::Io(e) => e.into(),
        }
    }
}

impl From<pla_core::jobs::JobError> for UserError {
    fn from(e: pla_core::jobs::JobError) -> Self {
        match e {
            pla_core::jobs::JobError::Db(e) => e.into(),
            pla_core::jobs::JobError::Io(e) => e.into(),
            pla_core::jobs::JobError::Held => Self::unexpected("a backup waits to be restored"),
        }
    }
}

impl From<pla_core::report::ReportError> for UserError {
    fn from(e: pla_core::report::ReportError) -> Self {
        match e {
            pla_core::report::ReportError::Io(e) => e.into(),
            pla_core::report::ReportError::Path(p) => Self::new("bad_path", p),
        }
    }
}

impl From<FileError> for UserError {
    fn from(e: FileError) -> Self {
        match e {
            FileError::BadPath(p) => Self::new("bad_path", p),
            FileError::BadName(c) => Self::new("bad_name", c.to_string()),
            FileError::Exists(p) => Self::new("exists", p),
            FileError::Reserved(n) => Self::new("reserved", n),
            FileError::Missing => Self::new("missing", ""),
            FileError::ReadOnly(p) => Self::new("read_only", p),
            FileError::Conflict { .. } => Self::new("conflict", ""),
            FileError::Io(e) => e.into(),
            FileError::Db(e) => e.into(),
        }
    }
}

impl From<VaultError> for UserError {
    fn from(e: VaultError) -> Self {
        match e {
            // pla-core says so for a folder that is gone, too: the user deleted or renamed the vault
            VaultError::NotADirectory(p) if !p.exists() => Self::new("missing", p.display().to_string()),
            VaultError::NotADirectory(p) => Self::new("not_folder", p.display().to_string()),
            VaultError::NetworkPathUnsupported(p) => Self::new("network", p.display().to_string()),
            VaultError::Io(e) => e.into(),
            other => Self::new("bad_config", "").because(other),
        }
    }
}

impl From<LlmError> for UserError {
    fn from(e: LlmError) -> Self {
        match e {
            LlmError::MissingBinary(p) => Self::new("model_server_missing", p.display().to_string()),
            LlmError::MissingModel(p) => Self::new("model_file_missing", p.display().to_string()),
            LlmError::Exited(_) | LlmError::StartupTimeout(_) => Self::new("model_start", "").because(e),
            LlmError::Timeout => Self::new("model_timeout", ""),
            other => Self::new("model", "").because(other),
        }
    }
}

/// The window or a plugin failed: nothing the user did, nothing they can fix.
impl From<tauri::Error> for UserError {
    fn from(e: tauri::Error) -> Self {
        Self::unexpected(e)
    }
}

impl From<tauri_plugin_autostart::Error> for UserError {
    fn from(e: tauri_plugin_autostart::Error) -> Self {
        Self::unexpected(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn errors_travel_as_codes_with_their_reason() {
        assert_eq!(ue(Error::new(ErrorKind::NotFound, "gone")), "missing||gone");
        assert_eq!(ue(Error::new(ErrorKind::PermissionDenied, "denied")), "permission||denied");
        assert_eq!(ue(Error::new(ErrorKind::StorageFull, "full")), "disk_full||full");
        assert_eq!(ue(Error::other("odd")), "io||odd");
        assert_eq!(ue(FileError::ReadOnly("notes/a.md".into())), "read_only|notes/a.md|");
        assert_eq!(ue(LlmError::MissingModel("C:/m.gguf".into())), "model_file_missing|C:/m.gguf|");
        assert_eq!(ue(LlmError::Timeout), "model_timeout||");
        assert_eq!(String::from(UserError::unexpected("boom")), "unexpected||boom");
        let gone = std::env::temp_dir().join("pla-no-such-vault-here");
        assert!(ue(VaultError::NotADirectory(gone.clone())).starts_with("missing|"), "a deleted vault folder is missing");
        let file = std::env::current_exe().unwrap();
        assert!(ue(VaultError::NotADirectory(file)).starts_with("not_folder|"));
    }

    #[test]
    fn a_failure_names_the_folder_it_is_about() {
        assert_eq!(naming("missing||os error 2", "C:/Notlar"), "missing|C:/Notlar|os error 2");
        assert_eq!(naming("not_folder|C:/x|", "C:/Notlar"), "not_folder|C:/x|", "it named one already");
        assert_eq!(naming("plain text", "C:/Notlar"), "plain text");
    }

    #[test]
    fn every_code_has_a_message_in_both_languages() {
        // FR-I18N-001/002: the page says every code in Turkish and in English
        let i18n = include_str!("../../src/lib/i18n.ts");
        let en_at = i18n.find("\n  en: {").expect("the English table");
        let (tr, en) = i18n.split_at(en_at);
        for code in CODES {
            let key = format!("\"error.{code}\":");
            assert!(tr.contains(&key), "no Turkish message for {code}");
            assert!(en.contains(&key), "no English message for {code}");
        }
    }

    #[test]
    fn commands_send_no_english_sentences() {
        // FR-I18N-002: a raw Rust message reached the page as it was (M6 translations)
        let sources = [
            ("commands.rs", include_str!("commands.rs")),
            ("fileops_cmds.rs", include_str!("fileops_cmds.rs")),
            ("links_cmds.rs", include_str!("links_cmds.rs")),
            ("media_cmds.rs", include_str!("media_cmds.rs")),
            ("memory_cmds.rs", include_str!("memory_cmds.rs")),
            ("metrics_cmds.rs", include_str!("metrics_cmds.rs")),
            ("model_cmds.rs", include_str!("model_cmds.rs")),
            ("qa_cmds.rs", include_str!("qa_cmds.rs")),
            ("settings_screen.rs", include_str!("settings_screen.rs")),
            ("summary_cmds.rs", include_str!("summary_cmds.rs")),
            ("task_cmds.rs", include_str!("task_cmds.rs")),
            ("tray.rs", include_str!("tray.rs")),
            ("wizard.rs", include_str!("wizard.rs")),
            ("worker.rs", include_str!("worker.rs")),
            ("scheduler.rs", include_str!("scheduler.rs")),
            ("notify.rs", include_str!("notify.rs")),
        ];
        for (name, src) in sources {
            let code = src.split("#[cfg(test)]").next().unwrap();
            for bad in ["map_err(|e| e.to_string())", "format!(\"io||"] {
                assert!(!code.contains(bad), "{name} still sends `{bad}`");
            }
            // a sentence where an error is made: `Err("A…`, `ok_or("A…`, `ok_or_else(|| "A…`
            for opener in ["Err(\"", "Err(format!(\"", "ok_or(\"", "ok_or_else(|| \""] {
                for (at, _) in code.match_indices(opener) {
                    let first = code[at + opener.len()..].chars().next().unwrap_or(' ');
                    assert!(!first.is_uppercase(), "{name} sends a sentence: {}", &code[at..(at + 70).min(code.len())]);
                }
            }
        }
    }
}
