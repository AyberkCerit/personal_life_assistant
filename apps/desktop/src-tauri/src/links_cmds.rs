//! Commands for connected notes (M3): search, quick open, backlinks, tags and following a wikilink.

use pla_core::files;
use pla_core::index::{self, Backlink, Graph, QuickHit, SearchHit, TagCount};
use pla_core::lang::Lang;
use pla_core::recall::{self, Role};
use pla_core::vault::Vault;
use serde::Serialize;
use tauri::State;

use crate::commands::{with_cache, with_session, AppState};
use crate::errors::{own_code, ue};
use crate::indexer::IndexCommand;

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

#[tauri::command(async)]
pub fn note_graph(state: State<AppState>) -> Result<Graph, String> {
    with_cache(&state, index::graph)
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
    with_session(&state, |session| {
        if let Some(path) = index::resolve_link(&session.cache.lock().expect("cache lock"), &target).map_err(ue)? {
            return Ok(OpenedLink { path, created: false });
        }
        // Not indexed yet (first open, a long sync, a note made a moment ago): the disk decides (I1).
        if let Some(path) = index::find_on_disk(&session.vault.root, &target) {
            session.indexer.send(IndexCommand::Touch(path.clone()));
            return Ok(OpenedLink { path, created: false });
        }
        let title = new_note_title(&target);
        if !is_note_target(&title) {
            return Err(own_code("not_a_note", &target, ""));
        }
        let inbox = &session.vault.config.folders.inbox;
        // Clicked again before the index caught up: the note made a moment ago, not a second one.
        let existing = format!("{inbox}/{title}.md");
        if files::resolve(&session.vault, &existing).is_ok_and(|p| p.is_file()) {
            return Ok(OpenedLink { path: existing, created: false });
        }
        let path = files::create_note(&session.vault, inbox, &title).map_err(ue)?;
        state.self_writes.record_text(&path, "");
        session.indexer.send(IndexCommand::Touch(path.clone()));
        Ok(OpenedLink { path, created: true })
    })
}

/// The memory's `Durum` file: an existing one in either language, else a new empty skeleton named in
/// `lang`. Returns its path and, when it was just made, the text written.
pub fn state_file(vault: &Vault, lang: Lang) -> Result<(String, Option<String>), files::FileError> {
    let folder = &vault.config.folders.memory;
    let existing = std::fs::read_dir(vault.root.join(folder)).into_iter().flatten().flatten().find_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_state = name.to_lowercase().ends_with(".md") && Role::from_file_name(&name) == Some(Role::State);
        (is_state && e.path().is_file()).then_some(name)
    });
    if let Some(name) = existing {
        return Ok((format!("{folder}/{name}"), None));
    }
    let rel = format!("{folder}/{}", Role::State.file_name(lang));
    let text = recall::render(Role::State, &[], lang);
    files::save_note(vault, &rel, &text, None)?;
    Ok((rel, Some(text)))
}

/// The sidebar's "Memory" shortcut: the `Durum` file to open in the editor, created when missing.
#[tauri::command]
pub fn open_memory(state: State<AppState>) -> Result<String, String> {
    with_session(&state, |session| {
        let (rel, created) = state_file(&session.vault, crate::notify::ui_language()).map_err(ue)?;
        if let Some(text) = created {
            state.self_writes.record_text(&rel, &text);
            session.indexer.send(IndexCommand::Touch(rel.clone()));
        }
        Ok(rel)
    })
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

    #[test]
    fn open_memory_creates_the_state_file_once() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = pla_core::vault::open_vault(tmp.path()).unwrap();
        let (rel, created) = state_file(&vault, Lang::Tr).unwrap();
        assert_eq!(rel, "memory/Durum.md");
        let text = created.expect("a new file");
        assert_eq!(std::fs::read_to_string(tmp.path().join(&rel)).unwrap(), text);
        assert!(text.contains("# Durum"));
        assert!(recall::parse(Role::State, &text).is_empty(), "an empty skeleton");
        assert!(pla_core::notes::is_generated(&text));

        std::fs::write(tmp.path().join(&rel), "- Kahveyi sütsüz içer.\n").unwrap();
        assert_eq!(state_file(&vault, Lang::Tr).unwrap(), (rel.clone(), None), "the second click opens it");
        assert_eq!(std::fs::read_to_string(tmp.path().join(&rel)).unwrap(), "- Kahveyi sütsüz içer.\n", "untouched");

        // A vault set up in the other language keeps its own file name.
        assert_eq!(state_file(&vault, Lang::En).unwrap(), (rel, None));
        let other = tempfile::tempdir().unwrap();
        let en = pla_core::vault::open_vault(other.path()).unwrap();
        assert_eq!(state_file(&en, Lang::En).unwrap().0, "memory/Current.md");
    }
}
