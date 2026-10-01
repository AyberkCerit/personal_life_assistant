//! FR-VLT-011: a crash leaves either the old or the new file, never half of one.

use std::io::{self, Write};
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent folder"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_content_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        write_atomic(&path, "eski".as_bytes()).unwrap();
        write_atomic(&path, "yeni içerik".as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "yeni içerik");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
