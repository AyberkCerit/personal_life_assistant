//! FR-VLT-011: a crash leaves either the old or the new file, never half of one.

use std::io::{self, Write};
use std::path::Path;

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_until(path, bytes, usize::MAX, true)
}

/// The save, stoppable where a crash or a power cut could stop it (NFR-REL-001, tested a thousand
/// times): the copy beside the note gets `upto` bytes and takes the note's place only when `rename`.
/// A stop leaves the copy behind as a dying process would; it is hidden (`.tmp…`), and the note is
/// the old one.
fn write_until(path: &Path, bytes: &[u8], upto: usize, rename: bool) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent folder"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&bytes[..upto.min(bytes.len())])?;
    if upto < bytes.len() || !rename {
        tmp.keep().map_err(|e| e.error)?;
        return Ok(());
    }
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

    #[test]
    fn a_thousand_interrupted_saves_leave_the_old_note_or_the_new_one() {
        // NFR-REL-001 (M6 acceptance): stopped while the copy is half written, after it is written
        // but before it takes the note's place, or not at all
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let text = |n: u64, len: u64| -> Vec<u8> { format!("# Not {n}\n\n{}", "ğüşiöç kelime ".repeat(len as usize)).into_bytes() };
        let mut current = text(0, 3);
        write_atomic(&path, &current).unwrap();
        for i in 1..=1000u64 {
            let new = text(i, 1 + next(400));
            let upto = match next(3) {
                0 => next(new.len() as u64) as usize, // half written
                1 => new.len(),                       // written, not yet in place
                _ => usize::MAX,                      // finished
            };
            let renamed = upto == usize::MAX;
            write_until(&path, &new, upto, renamed).unwrap();
            let on_disk = std::fs::read(&path).unwrap();
            if renamed {
                assert_eq!(on_disk, new, "save {i} finished");
                current = new;
            } else {
                assert_eq!(on_disk, current, "save {i} was stopped: the note is as it was");
            }
        }
        // what the stops left behind is hidden (`.tmp…`): the index and the tree skip it
        for e in std::fs::read_dir(dir.path()).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            assert!(name == "note.md" || name.starts_with('.'), "{name}");
        }
    }
}
