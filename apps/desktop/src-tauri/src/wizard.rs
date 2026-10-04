//! The first-run wizard's rules (FR-SET-001…009); the Tauri commands live in commands.rs.

use std::path::{Path, PathBuf};

use pla_core::settings::AppSettings;
use pla_core::vault::{inspect_folder, FolderState};

/// FR-SET-001: no settings yet, or a setup that never chose a vault. Users whose settings already
/// have a vault (from before the wizard existed) go straight to the app.
pub fn show_wizard(first_run: bool, settings: &AppSettings) -> bool {
    first_run || (!settings.setup_complete && settings.vault_path.is_none())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    Network(PathBuf),
    NotFolder(PathBuf),
    NotWritable { path: PathBuf, reason: String },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(p) => write!(f, "network:{}", p.display()),
            Self::NotFolder(p) => write!(f, "not_folder:{}", p.display()),
            Self::NotWritable { path, reason } => write!(f, "not_writable:{}:{reason}", path.display()),
        }
    }
}

/// Makes `path` ready to become the vault: refuses network paths and files (FR-SET-007), creates a
/// missing folder, and checks it can be written. Returns `true` when the folder was missing or
/// empty, i.e. when a welcome note belongs in it (FR-SET-006). A folder with notes is not touched
/// (FR-SET-005); the write probe is removed again.
pub fn prepare_folder(path: &Path) -> Result<bool, PrepareError> {
    let not_writable = |e: std::io::Error| PrepareError::NotWritable { path: path.to_path_buf(), reason: e.to_string() };
    let fresh = match inspect_folder(path).state {
        FolderState::Network => return Err(PrepareError::Network(path.to_path_buf())),
        FolderState::NotFolder => return Err(PrepareError::NotFolder(path.to_path_buf())),
        FolderState::Missing => {
            std::fs::create_dir_all(path).map_err(not_writable)?;
            true
        }
        FolderState::Empty => true,
        FolderState::Notes | FolderState::Other => false,
    };
    let probe = path.join(".pla-write-check");
    std::fs::write(&probe, b"").map_err(not_writable)?;
    std::fs::remove_file(&probe).map_err(not_writable)?;
    Ok(fresh)
}

/// The note a new vault starts with (FR-SET-006).
pub fn welcome_note(lang: &str) -> (&'static str, &'static str) {
    if lang == "tr" {
        (
            "Hoş geldin",
            "# PLA'ya hoş geldin\n\nNotlarını bu klasöre yaz; PLA içlerindeki görevleri, hatırlatıcıları ve ölçümleri kendiliğinden bulur.\n\n- Yarın 9'da dişçi var.\n- Cuma günü faturayı ödemeyi unutma.\n\nBu not silinebilir.\n",
        )
    } else {
        (
            "Welcome",
            "# Welcome to PLA\n\nWrite your notes in this folder; PLA finds the tasks, reminders and measurements in them by itself.\n\n- Dentist tomorrow at 9.\n- Don't forget to pay the bill on Friday.\n\nYou can delete this note.\n",
        )
    }
}

/// Whether `path` lies inside the user's OneDrive (so the vault would sync).
pub fn in_onedrive(path: &Path, env: &dyn Fn(&str) -> Option<PathBuf>) -> bool {
    let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
    let path = lower(path);
    ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"].iter().filter_map(|k| env(k)).any(|root| path.starts_with(lower(&root)))
}

/// The user's Documents folder as Windows knows it (it may have moved into OneDrive).
#[cfg(windows)]
pub fn documents_dir() -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: a valid known-folder id and out pointer; the returned string is freed below.
    let hr = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT as _, std::ptr::null_mut(), &mut raw) };
    if hr != 0 || raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a NUL-terminated UTF-16 string owned by us until CoTaskMemFree.
    let path = unsafe {
        let len = (0..).take_while(|&i| *raw.add(i) != 0).count();
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw as *const _);
        text
    };
    Some(PathBuf::from(path))
}

#[cfg(not(windows))]
pub fn documents_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents"))
}

/// FR-SET-004: `<Documents>\PLA Vault`.
pub fn suggested_vault() -> Option<PathBuf> {
    documents_dir().map(|d| d.join("PLA Vault"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pla_core::settings::AppSettings;

    #[test]
    fn only_a_first_start_or_an_unfinished_setup_without_a_vault_shows_the_wizard() {
        // Review Focus 1: existing users with a vault never see it
        let with_vault = AppSettings { vault_path: Some("C:/Kasa".into()), ..AppSettings::default() };
        assert!(show_wizard(true, &AppSettings::default()));
        assert!(show_wizard(false, &AppSettings::default()), "quit before choosing a vault");
        assert!(!show_wizard(false, &with_vault), "an existing user from before the wizard");
        assert!(!show_wizard(false, &AppSettings { setup_complete: true, ..AppSettings::default() }));
    }

    #[test]
    fn a_missing_folder_is_created_and_greeted() {
        // FR-SET-006
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("PLA Kasası ğüş");
        assert_eq!(prepare_folder(&path), Ok(true));
        assert!(path.is_dir());
        let empty = tmp.path().join("bos");
        std::fs::create_dir_all(empty.join(".obsidian")).unwrap();
        assert_eq!(prepare_folder(&empty), Ok(true));
    }

    #[test]
    fn a_folder_with_notes_is_left_exactly_as_it_is() {
        // FR-SET-005, Review Focus 2
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Not.md"), "benim notum").unwrap();
        assert_eq!(prepare_folder(tmp.path()), Ok(false));
        let names: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("Not.md")], "no probe file left behind");
        assert_eq!(std::fs::read_to_string(tmp.path().join("Not.md")).unwrap(), "benim notum");
    }

    #[test]
    fn network_paths_and_files_are_refused_with_the_folder_named() {
        // FR-SET-007
        let unc = std::path::Path::new(r"\\sunucu\paylasim\Kasa");
        assert_eq!(prepare_folder(unc), Err(PrepareError::Network(unc.to_path_buf())));
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("dosya.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(prepare_folder(&file), Err(PrepareError::NotFolder(file.clone())));
        assert!(PrepareError::NotFolder(file.clone()).to_string().contains("dosya.txt"));
    }

    #[test]
    fn the_welcome_note_speaks_the_chosen_language() {
        let (tr_title, tr_body) = welcome_note("tr");
        let (en_title, en_body) = welcome_note("en");
        assert_eq!((tr_title, en_title), ("Hoş geldin", "Welcome"));
        assert!(tr_body.contains("PLA") && en_body.contains("PLA"));
        assert_eq!(welcome_note("de").0, "Welcome", "unknown languages fall back to English");
    }

    #[test]
    fn knows_when_a_folder_syncs_with_onedrive() {
        let env = |k: &str| match k {
            "OneDrive" => Some(std::path::PathBuf::from(r"C:\Users\ayber\OneDrive")),
            _ => None,
        };
        assert!(in_onedrive(std::path::Path::new(r"C:\Users\ayber\OneDrive\Belgeler\PLA Vault"), &env));
        assert!(in_onedrive(std::path::Path::new(r"c:\users\AYBER\onedrive\Belgeler"), &env), "case-insensitive");
        assert!(!in_onedrive(std::path::Path::new(r"C:\Users\ayber\PLA Vault"), &env));
        assert!(!in_onedrive(std::path::Path::new(r"C:\Users\ayber\OneDriveX\a"), &env), "whole folder names only");
    }
}
