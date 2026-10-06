//! Commands for connected notes (M3): search, quick open, backlinks, tags and following a wikilink.

use pla_core::files;
use pla_core::index::{self, Backlink, QuickHit, SearchHit, TagCount};
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;
use crate::indexer::IndexCommand;

fn with_cache<T>(state: &State<AppState>, f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault")?;
    let cache = session.cache.lock().expect("cache lock");
    f(&cache).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn search_notes(state: State<AppState>, query: String, folder: Option<String>, tag: Option<String>) -> Result<Vec<SearchHit>, String> {
    with_cache(&state, |c| index::search(c, &query, folder.as_deref(), tag.as_deref(), 50))
}

#[tauri::command]
pub fn quick_open(state: State<AppState>, query: String) -> Result<Vec<QuickHit>, String> {
    with_cache(&state, |c| index::quick_open(c, &query, 20))
}

#[tauri::command]
pub fn backlinks(state: State<AppState>, path: String) -> Result<Vec<Backlink>, String> {
    with_cache(&state, |c| index::backlinks(c, &path))
}

#[tauri::command]
pub fn list_tags(state: State<AppState>) -> Result<Vec<TagCount>, String> {
    with_cache(&state, index::list_tags)
}

/// The title a missing link target gets as a new note in the inbox: what follows the last `/`,
/// without `#heading` or `.md` (FR-EDT-009).
pub fn new_note_title(target: &str) -> String {
    let target = target.split('#').next().unwrap_or_default().trim().replace('\\', "/");
    let name = target.rsplit('/').next().unwrap_or_default().trim();
    index::strip_md(name).trim().to_owned()
}

/// A link to something that is not a note (`[[rapor.pdf]]`, `[[#Başlık]]`) never creates one.
pub fn is_note_target(title: &str) -> bool {
    match title.rsplit_once('.') {
        _ if title.is_empty() => false,
        Some((_, ext)) => !(2..=5).contains(&ext.len()) || !ext.chars().all(|c| c.is_ascii_alphanumeric()),
        None => true,
    }
}

#[derive(Serialize)]
pub struct OpenedLink {
    path: String,
    created: bool,
}

/// FR-EDT-008/009: the note a wikilink points to; a missing one is created in the inbox and opened.
#[tauri::command]
pub fn open_link(state: State<AppState>, target: String) -> Result<OpenedLink, String> {
    let guard = state.session.lock().expect("session lock");
    let session = guard.as_ref().ok_or("no_vault")?;
    if let Some(path) = index::resolve_link(&session.cache.lock().expect("cache lock"), &target).map_err(|e| e.to_string())? {
        return Ok(OpenedLink { path, created: false });
    }
    // Not indexed yet (first open, a long sync, a note made a moment ago): the disk decides (I1).
    if let Some(path) = index::find_on_disk(&session.vault.root, &target) {
        session.indexer.send(IndexCommand::Touch(path.clone()));
        return Ok(OpenedLink { path, created: false });
    }
    let title = new_note_title(&target);
    if !is_note_target(&title) {
        return Err(format!("not_a_note|{target}|"));
    }
    let inbox = &session.vault.config.folders.inbox;
    // Clicked again before the index caught up: the note made a moment ago, not a second one.
    let existing = format!("{inbox}/{title}.md");
    if files::resolve(&session.vault, &existing).is_ok_and(|p| p.is_file()) {
        return Ok(OpenedLink { path: existing, created: false });
    }
    let path = files::create_note(&session.vault, inbox, &title).map_err(|e| e.to_string())?;
    state.self_writes.record_text(&path, "");
    session.indexer.send(IndexCommand::Touch(path.clone()));
    Ok(OpenedLink { path, created: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_link_becomes_a_note_named_after_its_last_part() {
        assert_eq!(new_note_title("Proje Planı"), "Proje Planı");
        assert_eq!(new_note_title("projeler/Yeni Fikir#Başlık"), "Yeni Fikir");
        assert_eq!(new_note_title(" Toplantı.md "), "Toplantı");
        assert_eq!(new_note_title("a\\b\\Kitap"), "Kitap");
        assert_eq!(new_note_title("Not.Md"), "Not", "any case of .md");
    }

    #[test]
    fn only_note_names_become_notes() {
        assert!(is_note_target("Proje Planı"));
        assert!(is_note_target("v1.2 notları"), "a dot inside a name is fine");
        assert!(!is_note_target("rapor.pdf"));
        assert!(!is_note_target("resim.png"));
        assert!(!is_note_target(""), "[[#Başlık]] has no note name");
    }
}
