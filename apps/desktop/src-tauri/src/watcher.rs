//! Changes made outside PLA (Obsidian, OneDrive, Explorer): they are queued for extraction after a
//! quiet period (decision 5) and refresh the note tree. PLA's own saves are recognised by content
//! hash and ignored (FR-EXT-003).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use pla_core::notes::is_excluded;
use pla_core::pipeline::blocks::content_hash;
use pla_core::vault::Vault;
use tauri::{AppHandle, Emitter};

use crate::worker::Command;

#[derive(Default)]
pub struct SelfWrites(Mutex<HashMap<String, String>>);

impl SelfWrites {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, rel: &str, hash: &str) {
        self.0.lock().expect("self-writes lock").insert(rel.to_owned(), hash.to_owned());
    }

    /// Records a note PLA itself wrote (created, saved, copied) so the watcher stays quiet about it.
    pub fn record_text(&self, rel: &str, text: &str) {
        self.record(rel, &content_hash(text.as_bytes()));
    }

    pub fn is_own(&self, rel: &str, hash: &str) -> bool {
        self.0.lock().expect("self-writes lock").get(rel).is_some_and(|h| h == hash)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    /// Notes changed outside PLA: extracted after the quiet period (decision 5).
    pub notes: Vec<String>,
    /// Notes that disappeared (deleted, renamed, moved): handled at once so a rename can be followed.
    pub vanished: Vec<String>,
    /// A folder appeared, moved or disappeared: every note has to be looked at again.
    pub rescan: bool,
    pub tree_changed: bool,
}

pub fn classify(vault: &Vault, paths: &[PathBuf], own: &SelfWrites) -> Changes {
    let mut out = Changes::default();
    for path in paths {
        let Ok(rel_path) = path.strip_prefix(&vault.root) else { continue };
        let rel = rel_path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        if rel.is_empty() || rel_path.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
            continue;
        }
        out.tree_changed = true;
        if is_excluded(&rel, &vault.config.folders) {
            continue;
        }
        if !rel.to_lowercase().ends_with(".md") {
            if path.is_dir() || (path.extension().is_none() && !path.exists()) {
                out.rescan = true;
            }
            continue;
        }
        match std::fs::read(path) {
            Ok(bytes) if own.is_own(&rel, &content_hash(&bytes)) => {}
            Ok(_) if !out.notes.contains(&rel) => out.notes.push(rel),
            Ok(_) => {}
            Err(_) if !out.vanished.contains(&rel) => out.vanished.push(rel),
            Err(_) => {}
        }
    }
    out
}

pub struct WatchHandle {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

pub fn watch(
    vault: Vault,
    own: Arc<SelfWrites>,
    worker: Sender<Command>,
    indexer: Sender<crate::indexer::IndexCommand>,
    app: AppHandle,
) -> notify_debouncer_full::notify::Result<WatchHandle> {
    let root = vault.root.clone();
    let index_root = vault.root.clone();
    let mut debouncer = new_debouncer(Duration::from_secs(2), None, move |result: DebounceEventResult| {
        let Ok(events) = result else { return };
        let paths: Vec<PathBuf> = events.iter().flat_map(|e| e.paths.iter().cloned()).collect();
        // The index follows every note, PLA's own saves included (M3).
        let (notes, rescan) = crate::indexer::index_events(&index_root, &paths);
        for note in notes {
            let _ = indexer.send(crate::indexer::IndexCommand::Touch(note));
        }
        if rescan {
            let _ = indexer.send(crate::indexer::IndexCommand::Rescan);
        }
        let changes = classify(&vault, &paths, &own);
        for note in changes.vanished {
            let _ = worker.send(Command::Enqueue(note));
        }
        if changes.rescan {
            let _ = worker.send(Command::Rescan);
        }
        for note in changes.notes {
            let _ = app.emit("note-changed", &note); // the editor reloads or flags a conflict at once
            let _ = worker.send(Command::EnqueueLater(note));
        }
        if changes.tree_changed {
            let _ = app.emit("tree-changed", ());
        }
    })?;
    debouncer.watch(&root, RecursiveMode::Recursive)?;
    Ok(WatchHandle { _debouncer: debouncer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pla_core::vault::open_vault;

    #[test]
    fn own_saves_are_not_external_changes() {
        // Review Focus 3
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let own = SelfWrites::new();
        let file = vault.root.join("notes/plan.md");
        std::fs::write(&file, "benim").unwrap();
        own.record("notes/plan.md", &content_hash(b"benim"));
        let c = classify(&vault, &[file.clone(), file.clone()], &own);
        assert!(c.notes.is_empty(), "duplicate events for our save stay quiet");

        std::fs::write(&file, "Obsidian'dan").unwrap();
        assert_eq!(classify(&vault, &[file], &own).notes, vec!["notes/plan.md"]);
    }

    #[test]
    fn hidden_excluded_and_non_markdown_paths_are_ignored_but_refresh_the_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let own = SelfWrites::new();
        let paths = [
            vault.root.join(".obsidian/workspace.json"),
            vault.root.join("templates/gunluk.md"),
            vault.root.join("attachments/resim.png"),
        ];
        let c = classify(&vault, &paths, &own);
        assert!(c.notes.is_empty());
        assert!(c.tree_changed, "a new picture still changes the tree");
        let hidden_only = classify(&vault, &paths[..1], &own);
        assert!(!hidden_only.tree_changed);
    }

    #[test]
    fn deleted_and_renamed_notes_are_both_queued() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        std::fs::write(vault.root.join("notes/yeni.md"), "x").unwrap();
        let c = classify(&vault, &[vault.root.join("notes/eski.md"), vault.root.join("notes/yeni.md")], &SelfWrites::new());
        assert_eq!(c.notes, vec!["notes/yeni.md"]);
        assert_eq!(c.vanished, vec!["notes/eski.md"], "Final review I5: handled at once, no quiet period");
    }

    #[test]
    fn notes_pla_creates_are_not_external_changes() {
        // Final review I4
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        let own = SelfWrites::new();
        std::fs::write(vault.root.join("inbox/Yeni.md"), "").unwrap();
        own.record_text("inbox/Yeni.md", "");
        assert!(classify(&vault, &[vault.root.join("inbox/Yeni.md")], &own).notes.is_empty());
    }

    #[test]
    fn a_renamed_folder_asks_for_a_rescan() {
        // Final review I5
        let tmp = tempfile::tempdir().unwrap();
        let vault = open_vault(tmp.path()).unwrap();
        std::fs::create_dir(vault.root.join("notes/yeni klasör")).unwrap();
        let c = classify(&vault, &[vault.root.join("notes/eski klasör"), vault.root.join("notes/yeni klasör")], &SelfWrites::new());
        assert!(c.rescan);
        let file_only = classify(&vault, &[vault.root.join("notes/eski.md")], &SelfWrites::new());
        assert!(!file_only.rescan);
    }
}
