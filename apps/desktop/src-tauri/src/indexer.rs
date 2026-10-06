//! Keeps the note index in `cache.db` current (M3): a vault sync at open, then one note per watcher
//! event. Its own thread, so a large vault never holds up the window or the model.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use pla_core::db::connect;
use pla_core::index;

pub enum IndexCommand {
    /// A note changed, appeared or vanished.
    Touch(String),
    /// A folder moved or vanished: look at everything again.
    Rescan,
    Shutdown,
}

pub struct IndexerHandle {
    tx: Sender<IndexCommand>,
    join: Option<JoinHandle<()>>,
}

impl IndexerHandle {
    pub fn sender(&self) -> Sender<IndexCommand> {
        self.tx.clone()
    }

    pub fn send(&self, command: IndexCommand) {
        let _ = self.tx.send(command);
    }
}

impl Drop for IndexerHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(IndexCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Which watcher paths concern the index: `.md` files outside hidden folders (also PLA's own saves
/// and excluded folders, unlike extraction), and whether a folder changed so everything is rescanned.
pub fn index_events(root: &Path, paths: &[PathBuf]) -> (Vec<String>, bool) {
    let mut notes = Vec::new();
    let mut rescan = false;
    for path in paths {
        let Ok(rel_path) = path.strip_prefix(root) else { continue };
        let rel = rel_path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        if rel.is_empty() || rel.split('/').any(|p| p.starts_with('.')) {
            continue;
        }
        if rel.to_ascii_lowercase().ends_with(".md") {
            if !notes.contains(&rel) {
                notes.push(rel);
            }
        } else if path.is_dir() || (path.extension().is_none() && !path.exists()) {
            rescan = true;
        }
    }
    (notes, rescan)
}

/// Starts the indexer for the vault at `root` writing to `cache`; `changed` is called after each
/// batch that changed the index (the UI refreshes backlinks and tags).
pub fn spawn(root: PathBuf, cache: PathBuf, changed: Box<dyn Fn() + Send>) -> IndexerHandle {
    let (tx, rx) = mpsc::channel();
    let join = std::thread::Builder::new().name("pla-indexer".into()).spawn(move || run(&root, &cache, rx, changed)).expect("spawn indexer");
    IndexerHandle { tx, join: Some(join) }
}

fn run(root: &Path, cache: &Path, rx: Receiver<IndexCommand>, changed: Box<dyn Fn() + Send>) {
    let Ok(conn) = connect(cache) else { return };
    if let Err(e) = index::sync_vault(&conn, root) {
        eprintln!("PLA: index sync failed: {e}");
    }
    changed();
    loop {
        let first = match rx.recv() {
            Ok(IndexCommand::Shutdown) | Err(_) => return,
            Ok(c) => c,
        };
        // Take what arrived meanwhile too, then tell the UI once.
        let mut batch = vec![first];
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(IndexCommand::Shutdown) => return,
                Ok(c) => batch.push(c),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let mut any = false;
        for command in batch {
            let result = match command {
                IndexCommand::Touch(rel) => index::touch(&conn, root, &rel),
                IndexCommand::Rescan => index::sync_vault(&conn, root).map(|r| r.indexed + r.removed > 0),
                IndexCommand::Shutdown => return,
            };
            match result {
                Ok(true) => any = true,
                Ok(false) => {}
                Err(e) => eprintln!("PLA: indexing failed: {e}"),
            }
        }
        if any {
            changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    #[test]
    fn notes_anywhere_but_hidden_folders_reach_the_index() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("templates")).unwrap();
        let paths = [root.join("templates/Günlük.md"), root.join(".obsidian/x.md"), root.join("notes/a.MD"), root.join("notes/a.MD"), root.join("resim.png")];
        assert_eq!(index_events(root, &paths), (vec!["templates/Günlük.md".to_owned(), "notes/a.MD".to_owned()], false));
        assert!(index_events(root, &[root.join("templates")]).1, "a folder event rescans");
    }

    #[test]
    fn the_indexer_syncs_at_start_and_follows_touches() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("kasa");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/a.md"), "elma [[b]]").unwrap();
        let cache = tmp.path().join("data/cache.db");
        pla_core::db::open_databases(cache.parent().unwrap()).unwrap();
        let calls = Arc::new(Mutex::new(0));
        let seen = Arc::clone(&calls);
        let handle = spawn(root.clone(), cache.clone(), Box::new(move || *seen.lock().unwrap() += 1));
        let reader = connect(&cache).unwrap();
        let wait = |pred: &dyn Fn() -> bool| {
            let start = Instant::now();
            while !pred() {
                assert!(start.elapsed() < Duration::from_secs(5), "never happened");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        wait(&|| index::search(&reader, "elma", None, None, 5).unwrap().len() == 1);
        std::fs::write(root.join("notes/b.md"), "armut").unwrap();
        handle.send(IndexCommand::Touch("notes/b.md".into()));
        wait(&|| index::backlinks(&reader, "notes/b.md").unwrap().len() == 1);
        assert!(*calls.lock().unwrap() >= 2, "the UI hears about the start and the change");
        drop(handle); // stops and joins
    }
}
