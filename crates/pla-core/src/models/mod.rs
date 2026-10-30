//! Language model files (FR-MDL-001…008): the compiled-in catalogue, where models live, the
//! download allow-list, downloads, local files and the models other tools left on this PC.

pub mod catalog;
pub mod download;
pub mod local;
pub mod policy;
pub mod scan;

use std::path::PathBuf;

/// `%LOCALAPPDATA%\PLA\models`: stays on this PC, never roams and never syncs with the vault.
pub fn models_dir() -> Option<PathBuf> {
    crate::vault::local_app_root().map(|p| p.join("models"))
}
