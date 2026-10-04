//! Application-level settings in `%APPDATA%/PLA/settings.json` (FR-SET-015, -018).
//! Vault-level settings stay in `<vault>/.pla/config` (vault.rs).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_atomic::write_atomic;

const FILE: &str = "settings.json";
const BROKEN: &str = "settings.broken.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub vault_path: Option<PathBuf>,
    /// `None` = follow the OS locale (FR-SET-003).
    pub language: Option<String>,
    pub theme: Theme,
    pub llama_server: Option<PathBuf>,
    pub model_path: Option<PathBuf>,
    /// The catalogue id of the installed model; `None` for a file the user chose (FR-MDL-008).
    pub model_id: Option<String>,
    /// The "PLA keeps running in the tray" hint was shown once (FR-SCH-001).
    pub tray_hint_shown: bool,
    /// The first-run wizard was finished (FR-SET-001); older files load as `false`.
    pub setup_complete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSettings {
    pub settings: AppSettings,
    pub first_run: bool,
    pub recovered_from_broken: bool,
}

pub fn load_settings(app_root: &Path) -> std::io::Result<LoadedSettings> {
    let path = app_root.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedSettings { settings: AppSettings::default(), first_run: true, recovered_from_broken: false });
        }
        Err(e) => return Err(e),
    };
    match serde_json::from_str(&text) {
        Ok(settings) => Ok(LoadedSettings { settings, first_run: false, recovered_from_broken: false }),
        Err(_) => {
            std::fs::rename(&path, app_root.join(BROKEN))?;
            Ok(LoadedSettings { settings: AppSettings::default(), first_run: false, recovered_from_broken: true })
        }
    }
}

/// The saved language, read without changing anything: a broken file is left for `load_settings`
/// (and `startup`) to move aside and report.
pub fn peek_language(app_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app_root.join(FILE)).ok()?;
    serde_json::from_str::<AppSettings>(&text).ok()?.language
}

pub fn save_settings(app_root: &Path, settings: &AppSettings) -> std::io::Result<()> {
    std::fs::create_dir_all(app_root)?;
    let text = serde_json::to_string_pretty(settings).expect("settings serialize");
    write_atomic(&app_root.join(FILE), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peeking_at_the_language_leaves_a_broken_file_for_startup() {
        // settings final review M6: only `startup` may move a broken file aside and say so
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(peek_language(tmp.path()), None);
        save_settings(tmp.path(), &AppSettings { language: Some("tr".into()), ..AppSettings::default() }).unwrap();
        assert_eq!(peek_language(tmp.path()).as_deref(), Some("tr"));
        std::fs::write(tmp.path().join(FILE), "{ bozuk").unwrap();
        assert_eq!(peek_language(tmp.path()), None);
        assert!(tmp.path().join(FILE).exists(), "not renamed");
        assert!(load_settings(tmp.path()).unwrap().recovered_from_broken);
    }

    #[test]
    fn no_file_means_first_run_with_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let loaded = load_settings(tmp.path()).unwrap();
        assert!(loaded.first_run && !loaded.recovered_from_broken);
        assert_eq!(loaded.settings, AppSettings::default());
        assert_eq!(loaded.settings.theme, Theme::Dark);
    }

    #[test]
    fn settings_round_trip_with_unicode_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("PLA");
        let s = AppSettings { vault_path: Some("C:/Users/x/Belgeler/PLA Kasası".into()), theme: Theme::Light, ..AppSettings::default() };
        save_settings(&app, &s).unwrap();
        let loaded = load_settings(&app).unwrap();
        assert_eq!(loaded.settings, s);
        assert!(!loaded.first_run);
    }

    #[test]
    fn a_broken_file_is_set_aside_and_defaults_are_used() {
        // FR-SET-018
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("settings.json"), "{ bozuk").unwrap();
        let loaded = load_settings(tmp.path()).unwrap();
        assert!(loaded.recovered_from_broken);
        assert_eq!(loaded.settings, AppSettings::default());
        assert!(!tmp.path().join("settings.json").exists());
        assert_eq!(std::fs::read_to_string(tmp.path().join("settings.broken.json")).unwrap(), "{ bozuk");
    }

    #[test]
    fn remembers_which_catalogue_model_is_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let s = AppSettings { model_path: Some("C:/m.gguf".into()), model_id: Some("gemma-4-e2b-it-q3km".into()), ..AppSettings::default() };
        save_settings(tmp.path(), &s).unwrap();
        assert_eq!(load_settings(tmp.path()).unwrap().settings.model_id.as_deref(), Some("gemma-4-e2b-it-q3km"));
        std::fs::write(tmp.path().join("settings.json"), r#"{ "theme": "dark" }"#).unwrap();
        assert_eq!(load_settings(tmp.path()).unwrap().settings.model_id, None, "older files still load");
    }

    #[test]
    fn setup_is_unfinished_until_the_wizard_says_so() {
        // First-run wizard: older settings files load as "not finished"
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("settings.json"), r#"{ "vault_path": "C:/Kasa" }"#).unwrap();
        assert!(!load_settings(tmp.path()).unwrap().settings.setup_complete);
        save_settings(tmp.path(), &AppSettings { setup_complete: true, ..AppSettings::default() }).unwrap();
        assert!(load_settings(tmp.path()).unwrap().settings.setup_complete);
    }
}
