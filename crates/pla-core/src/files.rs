//! Note files as the UI sees them: vault-relative paths only, atomic saves that never overwrite an
//! outside change (FR-VLT-001, -005, -009, -011…013, -016, -017).

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::fs_atomic::write_atomic;
use crate::pipeline::blocks::content_hash;
use crate::vault::Vault;

const FORBIDDEN: [char; 11] = ['[', ']', '#', '^', '|', '\\', '/', ':', '*', '"', '?'];
const FORBIDDEN_EXTRA: [char; 2] = ['<', '>'];

/// Name part of the copy PLA writes when the user keeps their text after a conflict.
pub const CONFLICT_SUFFIX: &str = " (çakışma)";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteFile {
    pub text: String,
    pub hash: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("path is outside the vault or hidden: {0:?}")]
    BadPath(String),
    #[error("a note name cannot contain {0:?}")]
    BadName(char),
    #[error("this note is not UTF-8 text and is open read-only: {0}")]
    ReadOnly(String),
    #[error("the note was changed outside PLA")]
    Conflict { current_hash: String },
    #[error("the note was moved or deleted outside PLA")]
    Missing,
    #[error("a note with this name already exists: {0}")]
    Exists(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A vault-relative path from the UI → an absolute path that is guaranteed to be inside the vault
/// and outside hidden folders (`.obsidian`, `.pla`).
pub fn resolve(vault: &Vault, rel: &str) -> Result<PathBuf, FileError> {
    let path = Path::new(rel);
    let ok = !rel.is_empty()
        && path.components().all(|c| {
            matches!(c, Component::Normal(p) if { let p = p.to_string_lossy(); !p.starts_with('.') && !p.contains(':') })
        });
    if !ok {
        return Err(FileError::BadPath(rel.to_owned()));
    }
    Ok(vault.root.join(path))
}

pub fn read_note(vault: &Vault, rel: &str) -> Result<NoteFile, FileError> {
    let bytes = std::fs::read(resolve(vault, rel)?)?;
    let hash = content_hash(&bytes);
    Ok(match String::from_utf8(bytes) {
        Ok(text) => NoteFile { text, hash, read_only: false },
        Err(e) => NoteFile { text: String::from_utf8_lossy(e.as_bytes()).into_owned(), hash, read_only: true },
    })
}

pub fn save_note(vault: &Vault, rel: &str, text: &str, expected_hash: Option<&str>) -> Result<String, FileError> {
    let path = resolve(vault, rel)?;
    if !Path::new(rel).extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
        return Err(FileError::BadPath(rel.to_owned()));
    }
    match std::fs::read(&path) {
        Ok(_) if expected_hash.is_none() => return Err(FileError::Exists(rel.to_owned())),
        Ok(current) => {
            if std::str::from_utf8(&current).is_err() {
                return Err(FileError::ReadOnly(rel.to_owned()));
            }
            let current_hash = content_hash(&current);
            if expected_hash.is_some_and(|h| h != current_hash) {
                return Err(FileError::Conflict { current_hash });
            }
        }
        // Saving an opened note whose file is gone would bring back a deleted or moved note (FR-VLT-013).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && expected_hash.is_some() => return Err(FileError::Missing),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(&path, text.as_bytes())?;
    Ok(content_hash(text.as_bytes()))
}

pub fn create_note(vault: &Vault, folder: &str, title: &str) -> Result<String, FileError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(FileError::BadName(' '));
    }
    if let Some(c) = title.chars().find(|c| FORBIDDEN.contains(c) || FORBIDDEN_EXTRA.contains(c)) {
        return Err(FileError::BadName(c));
    }
    let rel = unique_rel(vault, folder, title)?;
    save_note(vault, &rel, "", None)?;
    Ok(rel)
}

pub fn save_copy(vault: &Vault, rel: &str, text: &str) -> Result<String, FileError> {
    let path = Path::new(rel);
    let folder = path.parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let copy = unique_rel(vault, &folder, &format!("{stem}{CONFLICT_SUFFIX}"))?;
    save_note(vault, &copy, text, None)?;
    Ok(copy)
}

fn unique_rel(vault: &Vault, folder: &str, title: &str) -> Result<String, FileError> {
    let join = |name: String| if folder.is_empty() { name } else { format!("{folder}/{name}") };
    for n in 1.. {
        let name = if n == 1 { format!("{title}.md") } else { format!("{title} {n}.md") };
        let rel = join(name);
        if !resolve(vault, &rel)?.exists() {
            return Ok(rel);
        }
    }
    unreachable!("some numbered name is free")
}

pub fn list_tree(vault: &Vault) -> std::io::Result<Vec<TreeEntry>> {
    let mut out = Vec::new();
    walk(&vault.root, "", 0, &mut out)?;
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<TreeEntry>) -> std::io::Result<()> {
    let mut dirs = Vec::new();
    let mut notes = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type()?;
        if name.starts_with('.') || kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            dirs.push(name);
        } else if Path::new(&name).extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            notes.push(name);
        }
    }
    dirs.sort_by_key(|n| n.to_lowercase());
    notes.sort_by_key(|n| n.to_lowercase());
    let rel = |name: &str| if prefix.is_empty() { name.to_owned() } else { format!("{prefix}/{name}") };
    for name in dirs {
        let path = rel(&name);
        out.push(TreeEntry { path: path.clone(), name: name.clone(), is_dir: true, depth });
        walk(&dir.join(&name), &path, depth + 1, out)?;
    }
    for name in notes {
        out.push(TreeEntry { path: rel(&name), name, is_dir: false, depth });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::open_vault;

    fn vault() -> (tempfile::TempDir, Vault) {
        let tmp = tempfile::tempdir().unwrap();
        let v = open_vault(tmp.path()).unwrap();
        (tmp, v)
    }

    #[test]
    fn paths_stay_inside_the_vault_and_out_of_hidden_folders() {
        // Review Focus 5
        let (_t, v) = vault();
        for bad in ["../dışarı.md", "C:/Windows/x.md", "/x.md", ".obsidian/app.json", "notes/.gizli/x.md", ".pla/config", ""] {
            assert!(matches!(resolve(&v, bad), Err(FileError::BadPath(_))), "accepted {bad:?}");
        }
        assert_eq!(resolve(&v, "notes/Fikir ğüş.md").unwrap(), v.root.join("notes").join("Fikir ğüş.md"));
    }

    #[test]
    fn save_detects_outside_changes() {
        // Review Focus 1: FR-VLT-012/013
        let (_t, v) = vault();
        let rel = create_note(&v, "inbox", "Plan").unwrap();
        let opened = read_note(&v, &rel).unwrap();
        let h1 = save_note(&v, &rel, "ilk", Some(&opened.hash)).unwrap();
        std::fs::write(v.root.join(&rel), "Obsidian'dan").unwrap();
        let err = save_note(&v, &rel, "benim", Some(&h1)).unwrap_err();
        assert!(matches!(err, FileError::Conflict { .. }));
        assert_eq!(read_note(&v, &rel).unwrap().text, "Obsidian'dan", "nothing overwritten");
        let copy = save_copy(&v, &rel, "benim").unwrap();
        assert_eq!(copy, "inbox/Plan (çakışma).md");
        assert_eq!(read_note(&v, &copy).unwrap().text, "benim");
    }

    #[test]
    fn creating_notes_gives_unique_valid_names() {
        let (_t, v) = vault();
        assert_eq!(create_note(&v, "inbox", "Toplantı").unwrap(), "inbox/Toplantı.md");
        assert_eq!(create_note(&v, "inbox", "Toplantı").unwrap(), "inbox/Toplantı 2.md");
        assert!(matches!(create_note(&v, "inbox", "a/b"), Err(FileError::BadName('/'))));
        assert!(matches!(create_note(&v, "inbox", "ne?"), Err(FileError::BadName('?'))));
        assert!(matches!(create_note(&v, "inbox", "   "), Err(FileError::BadName(' '))));
        assert!(matches!(create_note(&v, ".obsidian", "x"), Err(FileError::BadPath(_))));
    }

    #[test]
    fn non_utf8_notes_open_read_only_and_cannot_be_saved() {
        let (_t, v) = vault();
        std::fs::write(v.root.join("notes/eski.md"), b"Yar\xfdn").unwrap();
        let n = read_note(&v, "notes/eski.md").unwrap();
        assert!(n.read_only);
        assert!(matches!(save_note(&v, "notes/eski.md", "x", Some(&n.hash)), Err(FileError::ReadOnly(_))));
    }

    #[test]
    fn tree_lists_folders_then_notes_without_hidden_items() {
        let (_t, v) = vault();
        create_note(&v, "notes", "b").unwrap();
        create_note(&v, "notes", "A").unwrap();
        std::fs::write(v.root.join("notes/resim.png"), b"x").unwrap();
        let tree = list_tree(&v).unwrap();
        let paths: Vec<&str> = tree.iter().map(|e| e.path.as_str()).collect();
        assert!(!paths.iter().any(|p| p.starts_with(".pla") || p.ends_with(".png")));
        let notes_at = paths.iter().position(|p| *p == "notes").unwrap();
        assert_eq!(&paths[notes_at + 1..notes_at + 3], &["notes/A.md", "notes/b.md"]);
        assert_eq!(tree[notes_at + 1].depth, 1);
        assert!(paths.iter().position(|p| *p == "daily").unwrap() < notes_at);
    }

    #[test]
    fn a_note_removed_outside_is_not_recreated_by_a_save() {
        // Final review I2: FR-VLT-013 also covers deletes and moves made outside PLA
        let (_t, v) = vault();
        let rel = create_note(&v, "inbox", "Plan").unwrap();
        let h = read_note(&v, &rel).unwrap().hash;
        std::fs::remove_file(v.root.join(&rel)).unwrap();
        assert!(matches!(save_note(&v, &rel, "benim", Some(&h)), Err(FileError::Missing)));
        assert!(!v.root.join(&rel).exists());
    }
    #[test]
    fn saves_are_limited_to_new_or_known_markdown_notes() {
        // F4a review M1
        let (_t, v) = vault();
        let rel = create_note(&v, "inbox", "Var").unwrap();
        assert!(matches!(save_note(&v, &rel, "x", None), Err(FileError::Exists(_))));
        assert!(matches!(save_note(&v, "notes/betik.ps1", "x", None), Err(FileError::BadPath(_))));
        assert!(matches!(save_note(&v, "notes/a:gizli.md", "x", None), Err(FileError::BadPath(_))));
        assert!(matches!(resolve(&v, "notes/a.md:akış"), Err(FileError::BadPath(_))));
    }
}
