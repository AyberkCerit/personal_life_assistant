//! The first-run wizard's rules (FR-SET-001…009); the Tauri commands live in commands.rs.

use std::path::{Path, PathBuf};

use pla_core::settings::AppSettings;
use pla_core::vault::{folder_state, FolderState};

/// FR-SET-001: no settings yet, or a setup that never chose a vault. Users whose settings already
/// have a vault (from before the wizard existed) go straight to the app.
pub fn show_wizard(first_run: bool, settings: &AppSettings) -> bool {
    first_run || (!settings.setup_complete && settings.vault_path.is_none())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    Network(PathBuf),
    NotFolder(PathBuf),
    Relative(PathBuf),
    AppData(PathBuf),
    NotWritable { path: PathBuf, reason: String },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(p) => write!(f, "network|{}", p.display()),
            Self::NotFolder(p) => write!(f, "not_folder|{}", p.display()),
            Self::Relative(p) => write!(f, "relative|{}", p.display()),
            Self::AppData(p) => write!(f, "app_data|{}", p.display()),
            Self::NotWritable { path, reason } => write!(f, "not_writable|{}|{reason}", path.display()),
        }
    }
}

/// Makes `path` ready to become the vault: refuses network and relative paths and files
/// (FR-SET-007), creates a missing folder, and checks it can be written. Returns `true` when the
/// folder was missing or empty, i.e. when a welcome note belongs in it (FR-SET-006). A folder with
/// notes is not touched (FR-SET-005): the write check uses a new file of its own and removes it.
pub fn prepare_folder(path: &Path) -> Result<bool, PrepareError> {
    let not_writable = |e: std::io::Error| PrepareError::NotWritable { path: path.to_path_buf(), reason: write_failure(&e) };
    let fresh = match folder_state(path) {
        FolderState::Network => return Err(PrepareError::Network(path.to_path_buf())),
        FolderState::NotFolder => return Err(PrepareError::NotFolder(path.to_path_buf())),
        FolderState::Relative => return Err(PrepareError::Relative(path.to_path_buf())),
        FolderState::AppData => return Err(PrepareError::AppData(path.to_path_buf())),
        FolderState::Missing => {
            std::fs::create_dir_all(path).map_err(not_writable)?;
            true
        }
        FolderState::Empty => true,
        FolderState::Notes | FolderState::Other => false,
    };
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let probe = path.join(format!(".pla-write-check-{}-{stamp}", std::process::id()));
    std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(not_writable)?;
    std::fs::remove_file(&probe).map_err(not_writable)?;
    Ok(fresh)
}

/// Why a folder could not be written: a code the UI translates, or the OS text as a last resort.
pub fn write_failure(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "access_denied".into(),
        _ => e.to_string(),
    }
}

/// A folder path as typed or pasted: surrounding spaces and one pair of quotes removed.
pub fn clean_path(input: &str) -> PathBuf {
    let trimmed = input.trim();
    let unquoted = trimmed.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(trimmed);
    PathBuf::from(unquoted.trim())
}

/// The note a new vault starts with (FR-SET-006).
pub fn welcome_note(lang: &str) -> (&'static str, &'static str) {
    if lang == "tr" {
        (
            "Hoş geldin",
            "---\npla_generated: true\n---\n# PLA'ya hoş geldin\n\nNotlarını bu klasöre yaz; PLA içlerindeki görevleri, hatırlatıcıları ve ölçümleri kendiliğinden bulur. Örneğin bir nota şunları yazabilirsin:\n\n- Yarın 9'da dişçi var.\n- Cuma günü faturayı ödemeyi unutma.\n\nBu notu PLA yazdığı için içindeki örnekler göreve dönüşmez; denemek için yeni bir not aç. Bu not silinebilir.\n",
        )
    } else {
        (
            "Welcome",
            "---\npla_generated: true\n---\n# Welcome to PLA\n\nWrite your notes in this folder; PLA finds the tasks, reminders and measurements in them by itself. For example, a note could say:\n\n- Dentist tomorrow at 9.\n- Don't forget to pay the bill on Friday.\n\nPLA wrote this note, so its examples do not become tasks; open a new note to try it. You can delete this note.\n",
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
        // the UI's setupError() reads this `<code>|<path>|<reason>` shape
        assert_eq!(PrepareError::Network(unc.to_path_buf()).to_string(), r"network|\\sunucu\paylasim\Kasa");
        let denied = PrepareError::NotWritable { path: "C:/Kasa".into(), reason: "erişim engellendi".into() };
        assert_eq!(denied.to_string(), "not_writable|C:/Kasa|erişim engellendi");
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("dosya.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(prepare_folder(&file), Err(PrepareError::NotFolder(file.clone())));
        assert!(PrepareError::NotFolder(file.clone()).to_string().contains("dosya.txt"));
    }

    #[test]
    fn relative_and_network_paths_in_any_spelling_are_refused() {
        // final review I1/I2
        let before = std::env::current_dir().unwrap();
        for p in ["Notlar", r"C:Notlar", ""] {
            assert_eq!(prepare_folder(std::path::Path::new(p)), Err(PrepareError::Relative(p.into())), "{p:?}");
        }
        assert_eq!(PrepareError::Relative("Notlar".into()).to_string(), "relative|Notlar");
        let unc = std::path::Path::new("//sunucu/paylasim");
        assert_eq!(prepare_folder(unc), Err(PrepareError::Network(unc.to_path_buf())));
        assert_eq!(std::env::current_dir().unwrap(), before);
        assert!(!before.join("Notlar").exists(), "nothing created next to the process");
    }

    #[test]
    fn plas_own_data_folders_are_refused() {
        // NFR-SEC-010: the uninstaller may delete them
        let Some(root) = pla_core::vault::app_data_roots().into_iter().next() else { return };
        let inside = root.join("Notlar");
        assert_eq!(prepare_folder(&inside), Err(PrepareError::AppData(inside.clone())));
        assert!(!inside.exists(), "nothing created");
        assert!(PrepareError::AppData(inside).to_string().starts_with("app_data|"));
    }

    #[test]
    fn typed_paths_lose_quotes_and_spaces() {
        // Explorer's "Copy as path" adds quotes (final review I2)
        assert_eq!(clean_path(r#"  "C:\Kasa ğüş\"  "#), std::path::PathBuf::from(r"C:\Kasa ğüş\"));
        assert_eq!(clean_path(" C:\\Kasa "), std::path::PathBuf::from(r"C:\Kasa"));
        assert_eq!(clean_path(r#""C:\a"b""#), std::path::PathBuf::from(r#"C:\a"b"#), "only a surrounding pair");
    }

    #[test]
    fn access_denied_is_a_code_the_ui_can_translate() {
        // final review M3: the OS text would come in the OS language
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(write_failure(&denied), "access_denied");
        let other = std::io::Error::other("disk on fire");
        assert_eq!(write_failure(&other), "disk on fire");
    }

    #[test]
    fn the_write_check_never_touches_an_existing_file() {
        // final review M4
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".pla-write-check"), "kullanıcının").unwrap();
        std::fs::write(tmp.path().join("Not.md"), "x").unwrap();
        assert_eq!(prepare_folder(tmp.path()), Ok(false));
        assert_eq!(std::fs::read_to_string(tmp.path().join(".pla-write-check")).unwrap(), "kullanıcının");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_folder_with_only_system_files_still_gets_a_welcome() {
        // final review M7: desktop.ini does not make a folder "used"
        let tmp = tempfile::tempdir().unwrap();
        let ini = tmp.path().join("desktop.ini");
        std::fs::write(&ini, "[.ShellClassInfo]").unwrap();
        let status = std::process::Command::new("attrib").args(["+h", "+s"]).arg(&ini).status().unwrap();
        assert!(status.success());
        assert_eq!(prepare_folder(tmp.path()), Ok(true));
    }

    #[test]
    fn the_welcome_note_speaks_the_chosen_language() {
        let (tr_title, tr_body) = welcome_note("tr");
        let (en_title, en_body) = welcome_note("en");
        assert_eq!((tr_title, en_title), ("Hoş geldin", "Welcome"));
        assert!(tr_body.contains("PLA") && en_body.contains("PLA"));
        // FR-EXT-001: PLA wrote it, so its examples never become the user's tasks
        assert!(pla_core::notes::is_generated(tr_body) && pla_core::notes::is_generated(en_body));
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
