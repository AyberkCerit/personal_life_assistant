//! Language model files (FR-MDL-001…008): the compiled-in catalogue, where models live, the
//! download allow-list, downloads and local files.

pub mod catalog;
pub mod local;
pub mod policy;

use std::path::PathBuf;

/// `%LOCALAPPDATA%\PLA\models`: stays on this PC, never roams and never syncs with the vault.
pub fn models_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("PLA").join("models"))
}
