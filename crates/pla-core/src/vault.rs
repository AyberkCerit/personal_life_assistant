//! Opening a vault: system folders, `.pla/config` and the per-vault data folder (FR-VLT-006, -008, E-D1, E-D17).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_atomic::write_atomic;

const CONFIG_PATH: &str = ".pla/config";
const CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub vault_id: String,
    pub schema_version: u32,
    pub folders: Folders,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self { vault_id: String::new(), schema_version: CONFIG_SCHEMA_VERSION, folders: Folders::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folders {
    pub daily: String,
    pub inbox: String,
    pub notes: String,
    pub reports: String,
    pub attachments: String,
    pub templates: String,
}

impl Default for Folders {
    fn default() -> Self {
        Self {
            daily: "daily".into(),
            inbox: "inbox".into(),
            notes: "notes".into(),
            reports: "reports".into(),
            attachments: "attachments".into(),
            templates: "templates".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vault {
    pub root: PathBuf,
    pub config: VaultConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("not a folder: {0}")]
    NotADirectory(PathBuf),
    #[error("network folders are not supported: {0}")]
    NetworkPathUnsupported(PathBuf),
    #[error("cannot read .pla/config: {0}")]
    CorruptConfig(String),
    #[error("invalid vault_id: {0:?}")]
    InvalidVaultId(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn open_vault(root: &Path) -> Result<Vault, VaultError> {
    if is_network_path(root) {
        return Err(VaultError::NetworkPathUnsupported(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(VaultError::NotADirectory(root.to_path_buf()));
    }
    std::fs::create_dir_all(root.join(".pla/backup"))?;

    let config_path = root.join(CONFIG_PATH);
    let mut config = if config_path.exists() {
        let text = std::fs::read_to_string(&config_path)?;
        toml::from_str::<VaultConfig>(&text).map_err(|e| VaultError::CorruptConfig(e.to_string()))?
    } else {
        VaultConfig::default()
    };

    if config.vault_id.is_empty() {
        config.vault_id = uuid::Uuid::new_v4().simple().to_string();
        let text = toml::to_string(&config).map_err(|e| VaultError::CorruptConfig(e.to_string()))?;
        write_atomic(&config_path, text.as_bytes())?;
    } else if !is_valid_vault_id(&config.vault_id) {
        return Err(VaultError::InvalidVaultId(config.vault_id));
    }

    let f = &config.folders;
    for dir in [&f.inbox, &f.daily, &f.notes, &f.attachments, &f.templates] {
        std::fs::create_dir_all(root.join(dir))?;
    }
    std::fs::create_dir_all(root.join(&f.reports).join("weekly"))?;

    Ok(Vault { root: root.to_path_buf(), config })
}

/// `<app_root>/vaults/<vault_id>/` — where this vault's pla.db and cache.db live.
pub fn data_dir(app_root: &Path, vault_id: &str) -> Result<PathBuf, VaultError> {
    if !is_valid_vault_id(vault_id) {
        return Err(VaultError::InvalidVaultId(vault_id.to_owned()));
    }
    Ok(app_root.join("vaults").join(vault_id))
}

/// `%APPDATA%/PLA` on Windows.
pub fn default_app_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("PLA"))
}

fn is_valid_vault_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_network_path(path: &Path) -> bool {
    let s = path.as_os_str().to_string_lossy();
    s.starts_with(r"\\?\UNC\") || (s.starts_with(r"\\") && !s.starts_with(r"\\?\") && !s.starts_with(r"\\.\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM_DIRS: [&str; 8] = ["inbox", "daily", "notes", "reports/weekly", "attachments", "templates", ".pla", ".pla/backup"];

    #[test]
    fn new_vault_gets_folders_and_a_persistent_id() {
        let tmp = tempfile::tempdir().unwrap();
        // Review Focus 2: Turkish characters, spaces and emoji in the path
        let root = tmp.path().join("🏰 Kişisel Kasa ğüşıöç");
        std::fs::create_dir(&root).unwrap();

        let v = open_vault(&root).unwrap();
        for d in SYSTEM_DIRS {
            assert!(root.join(d).is_dir(), "{d} missing");
        }
        assert_eq!(v.config.vault_id.len(), 32);
        assert!(!root.join(".obsidian").exists(), "must never create .obsidian");

        let again = open_vault(&root).unwrap();
        assert_eq!(again.config.vault_id, v.config.vault_id);
    }

    #[test]
    fn custom_folder_paths_are_respected_and_kept() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".pla")).unwrap();
        std::fs::write(tmp.path().join(".pla/config"), "[folders]\ndaily = \"Günlük\"\n").unwrap();

        let v = open_vault(tmp.path()).unwrap();
        assert!(tmp.path().join("Günlük").is_dir());
        assert!(!tmp.path().join("daily").exists());
        let written = std::fs::read_to_string(tmp.path().join(".pla/config")).unwrap();
        assert!(written.contains("Günlük"));
        assert!(written.contains(&v.config.vault_id));
    }

    #[test]
    fn corrupt_config_is_reported_and_left_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".pla")).unwrap();
        std::fs::write(tmp.path().join(".pla/config"), "vault_id = [broken").unwrap();
        assert!(matches!(open_vault(tmp.path()), Err(VaultError::CorruptConfig(_))));
        assert_eq!(std::fs::read_to_string(tmp.path().join(".pla/config")).unwrap(), "vault_id = [broken");
    }

    #[test]
    fn vault_id_cannot_escape_the_vaults_folder() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".pla")).unwrap();
        std::fs::write(tmp.path().join(".pla/config"), "vault_id = \"../../evil\"\n").unwrap();
        assert!(matches!(open_vault(tmp.path()), Err(VaultError::InvalidVaultId(_))));
        assert!(matches!(data_dir(Path::new("C:/app"), "../x"), Err(VaultError::InvalidVaultId(_))));
    }

    #[test]
    fn data_dir_is_per_vault() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(data_dir(Path::new("C:/app/PLA"), id).unwrap(), Path::new("C:/app/PLA").join("vaults").join(id));
    }

    #[test]
    fn rejects_missing_folder_and_network_paths() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(open_vault(&tmp.path().join("yok")), Err(VaultError::NotADirectory(_))));
        assert!(matches!(open_vault(Path::new(r"\\server\share\vault")), Err(VaultError::NetworkPathUnsupported(_))));
        assert!(matches!(open_vault(Path::new(r"\\?\UNC\server\share\vault")), Err(VaultError::NetworkPathUnsupported(_))));
    }
}
