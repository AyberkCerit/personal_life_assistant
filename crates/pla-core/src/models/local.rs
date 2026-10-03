//! A model file the user already has (FR-MDL-008): checked, then used where it is.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocalModel {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalModelError {
    #[error("file not found: {0}")]
    NotFound(PathBuf),
    #[error("not a GGUF model file: {0}")]
    NotGguf(PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// GGUF files start with the magic `GGUF` and a little-endian format version (2 or 3 today).
pub fn validate_gguf(path: &Path) -> Result<LocalModel, LocalModelError> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(LocalModelError::NotFound(path.to_path_buf())),
        Err(e) => return Err(e.into()),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(LocalModelError::NotGguf(path.to_path_buf()));
    }
    let mut head = [0u8; 8];
    if file.read_exact(&mut head).is_err() {
        return Err(LocalModelError::NotGguf(path.to_path_buf()));
    }
    let version = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
    if &head[..4] != b"GGUF" || !(2..=3).contains(&version) {
        return Err(LocalModelError::NotGguf(path.to_path_buf()));
    }
    Ok(LocalModel { path: path.to_path_buf(), size: meta.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn accepts_a_gguf_v3_file_and_reports_its_size() {
        let tmp = tempfile::tempdir().unwrap();
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 100]);
        let p = write(tmp.path(), "m.gguf", &bytes);
        assert_eq!(validate_gguf(&p).unwrap(), LocalModel { path: p.clone(), size: 108 });
    }

    #[test]
    fn rejects_files_that_are_not_gguf() {
        let tmp = tempfile::tempdir().unwrap();
        let txt = write(tmp.path(), "notes.gguf", b"just some text, renamed");
        assert!(matches!(validate_gguf(&txt), Err(LocalModelError::NotGguf(_))));
        let short = write(tmp.path(), "short.gguf", b"GGU");
        assert!(matches!(validate_gguf(&short), Err(LocalModelError::NotGguf(_))));
        let mut v9 = b"GGUF".to_vec();
        v9.extend_from_slice(&9u32.to_le_bytes());
        let future = write(tmp.path(), "v9.gguf", &v9);
        assert!(matches!(validate_gguf(&future), Err(LocalModelError::NotGguf(_))));
        assert!(matches!(validate_gguf(&tmp.path().join("missing.gguf")), Err(LocalModelError::NotFound(_))));
        assert!(matches!(validate_gguf(tmp.path()), Err(LocalModelError::NotGguf(_) | LocalModelError::Io(_))), "a folder is not a model");
    }
}
