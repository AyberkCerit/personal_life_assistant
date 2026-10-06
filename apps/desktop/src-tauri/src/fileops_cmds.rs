//! Tree actions (FR-EDT-004/013, FR-VLT-009/010/015/018): create, rename, move, delete to the
//! Recycle Bin, today's note and notes from templates. Errors are `<code>|<detail>|`.

use std::path::Path;

use chrono::Local;
use pla_core::fileops::{self, check_name};
use pla_core::files::{self, FileError};
use pla_core::fs_atomic::write_atomic;
use pla_core::index;
use pla_core::vault::Vault;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{AppState, Session};
use crate::indexer::IndexCommand;

/// A file error as a code the UI turns into a sentence in its language.
pub fn code(e: FileError) -> String {
    match e {
        FileError::BadPath(p) => format!("bad_path|{p}|"),
        FileError::BadName(c) => format!("bad_name|{c}|"),
        FileError::Exists(p) => format!("exists|{p}|"),
        FileError::Missing => "missing||".into(),
        FileError::ReadOnly(p) => format!("read_only|{p}|"),
        FileError::Conflict { .. } => "conflict||".into(),
        FileError::Io(e) => format!("io||{e}"),
        FileError::Db(e) => format!("io||{e}"),
    }
}

fn parent_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(p, _)| p)
}

fn join(folder: &str, name: &str) -> String {
    let folder = folder.trim_matches('/');
    if folder.is_empty() { name.to_owned() } else { format!("{folder}/{name}") }
}

fn with_session<T>(state: &State<AppState>, f: impl FnOnce(&Session) -> Result<T, String>) -> Result<T, String> {
    let guard = state.session.lock().expect("session lock");
    f(guard.as_ref().ok_or("no_vault||")?)
}

/// Notes (other than `rel`) with a link to it, by the index.
fn linking_notes(session: &Session, rel: &str) -> Result<Vec<String>, String> {
    let links = index::backlinks(&session.cache.lock().expect("cache lock"), rel).map_err(|e| format!("io||{e}"))?;
    let mut sources: Vec<String> = links.into_iter().map(|b| b.source_path).collect();
    sources.dedup();
    Ok(sources)
}

/// FR-EDT-013: how many notes link here, asked before a rename.
#[tauri::command]
pub fn link_count(state: State<AppState>, path: String) -> Result<usize, String> {
    with_session(&state, |s| Ok(linking_notes(s, &path)?.len()))
}

/// Points the links in `sources` from `old` to `new`, as PLA's own writes. Returns how many changed.
fn relink(state: &State<AppState>, vault: &Vault, sources: &[String], old: &str, new: &str) -> usize {
    let mut changed = 0;
    for source in sources {
        let Ok(path) = files::resolve(vault, source) else { continue };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if let Some(updated) = fileops::rewrite_links(&text, old, new) {
            state.self_writes.record_text(source, &updated);
            if write_atomic(&path, updated.as_bytes()).is_ok() {
                changed += 1;
            }
        }
    }
    changed
}

/// Moves a note and keeps everything around it: records, index, and (when asked) the links to it.
fn move_note_with_links(state: &State<AppState>, session: &Session, from: &str, to: &str, update_links: bool) -> Result<(), String> {
    let sources = if update_links { linking_notes(session, from)? } else { Vec::new() };
    if let Ok(text) = std::fs::read_to_string(files::resolve(&session.vault, from).map_err(code)?) {
        state.self_writes.record_text(to, &text); // the note itself is not an outside change
    }
    fileops::move_note(&session.vault, &session.db.lock().expect("db lock"), from, to).map_err(code)?;
    relink(state, &session.vault, &sources, from, to);
    for rel in [from, to].iter().copied().chain(sources.iter().map(String::as_str)) {
        session.indexer.send(IndexCommand::Touch(rel.to_owned()));
    }
    Ok(())
}

#[tauri::command]
pub fn rename_note(app: AppHandle, state: State<AppState>, path: String, name: String, update_links: bool) -> Result<String, String> {
    let name = name.trim().to_owned();
    check_name(&name).map_err(code)?;
    let to = join(parent_of(&path), &format!("{name}.md"));
    with_session(&state, |s| move_note_with_links(&state, s, &path, &to, update_links))?;
    let _ = app.emit("tree-changed", ());
    Ok(to)
}

/// Drag and drop or "Move…": a note or a folder into `folder` (`""` = the vault root).
#[tauri::command]
pub fn move_entry(app: AppHandle, state: State<AppState>, path: String, folder: String) -> Result<String, String> {
    let name = path.rsplit('/').next().unwrap_or_default().to_owned();
    let to = join(&folder, &name);
    if to == path {
        return Ok(to);
    }
    with_session(&state, |s| {
        if files::resolve(&s.vault, &path).map_err(code)?.is_dir() {
            fileops::move_folder(&s.vault, &s.db.lock().expect("db lock"), &path, &to).map_err(code)?;
            s.indexer.send(IndexCommand::Rescan);
            Ok(())
        } else {
            // a move keeps the name: only links written with the path change
            move_note_with_links(&state, s, &path, &to, true)
        }
    })?;
    let _ = app.emit("tree-changed", ());
    Ok(to)
}

#[tauri::command]
pub fn rename_folder(app: AppHandle, state: State<AppState>, path: String, name: String) -> Result<String, String> {
    let name = name.trim().to_owned();
    check_name(&name).map_err(code)?;
    let to = join(parent_of(&path), &name);
    with_session(&state, |s| {
        fileops::move_folder(&s.vault, &s.db.lock().expect("db lock"), &path, &to).map_err(code)?;
        s.indexer.send(IndexCommand::Rescan);
        Ok(())
    })?;
    let _ = app.emit("tree-changed", ());
    Ok(to)
}

/// What may be deleted: a note or a folder inside the vault, never a system folder or the root.
pub fn deletable(vault: &Vault, rel: &str) -> Result<std::path::PathBuf, String> {
    let folders = &vault.config.folders;
    let system = [&folders.daily, &folders.inbox, &folders.notes, &folders.reports, &folders.attachments, &folders.templates];
    if system.iter().any(|f| f.trim_matches('/').eq_ignore_ascii_case(rel.trim_matches('/'))) {
        return Err(format!("bad_path|{rel}|"));
    }
    let path = files::resolve(vault, rel).map_err(code)?;
    if !path.exists() {
        return Err("missing||".into());
    }
    Ok(path)
}

/// FR-VLT-015: to the Windows Recycle Bin, never deleted for good.
#[tauri::command]
pub fn delete_entry(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    with_session(&state, |s| {
        let abs = deletable(&s.vault, &path)?;
        to_recycle_bin(&abs).map_err(|e| format!("io||{e}"))?;
        s.indexer.send(IndexCommand::Rescan);
        Ok(())
    })?;
    let _ = app.emit("tree-changed", ());
    Ok(())
}

#[cfg(windows)]
fn to_recycle_bin(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    // a list of paths, each NUL-terminated, ended by one more NUL
    let from: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: a valid, double-NUL-terminated path list that outlives the call.
    let result = unsafe { SHFileOperationW(&mut op) };
    if result != 0 || op.fAnyOperationsAborted != 0 {
        return Err(format!("the Recycle Bin refused it (code {result})"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn to_recycle_bin(path: &Path) -> Result<(), String> {
    Err(format!("no Recycle Bin here for {}", path.display()))
}

#[tauri::command]
pub fn create_folder(app: AppHandle, state: State<AppState>, parent: String, name: String) -> Result<String, String> {
    let name = name.trim().to_owned();
    check_name(&name).map_err(code)?;
    let rel = join(&parent, &name);
    with_session(&state, |s| {
        let path = files::resolve(&s.vault, &rel).map_err(code)?;
        if path.exists() {
            return Err(format!("exists|{rel}|"));
        }
        std::fs::create_dir_all(&path).map_err(|e| format!("io||{e}"))
    })?;
    let _ = app.emit("tree-changed", ());
    Ok(rel)
}

/// The note templates in `templates/`, by name.
#[tauri::command]
pub fn list_templates(state: State<AppState>) -> Result<Vec<String>, String> {
    with_session(&state, |s| {
        let dir = files::resolve(&s.vault, &s.vault.config.folders.templates).map_err(code)?;
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.to_lowercase().ends_with(".md") && !n.starts_with('.'))
                    .map(|n| index::strip_md(&n).to_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        Ok(names)
    })
}

/// Writes a new note at `rel` (it must not exist), as PLA's own write.
fn write_new(state: &State<AppState>, vault: &Vault, rel: &str, text: &str) -> Result<(), String> {
    let path = files::resolve(vault, rel).map_err(code)?;
    if path.exists() {
        return Err(format!("exists|{rel}|"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("io||{e}"))?;
    }
    state.self_writes.record_text(rel, text);
    write_atomic(&path, text.as_bytes()).map_err(|e| format!("io||{e}"))
}

/// FR-VLT-009/FR-EDT-018: a note in `folder`, empty or from a template.
#[tauri::command]
pub fn create_note_in(app: AppHandle, state: State<AppState>, folder: String, title: String, template: Option<String>) -> Result<String, String> {
    let rel = with_session(&state, |s| {
        let Some(template) = template else {
            let rel = files::create_note(&s.vault, &folder, &title).map_err(code)?;
            state.self_writes.record_text(&rel, "");
            return Ok(rel);
        };
        let title = title.trim();
        check_name(title).map_err(code)?;
        let source = files::resolve(&s.vault, &join(&s.vault.config.folders.templates, &format!("{template}.md"))).map_err(code)?;
        let body = std::fs::read_to_string(source).map_err(|_| format!("missing|{template}|"))?;
        let rel = join(&folder, &format!("{title}.md"));
        write_new(&state, &s.vault, &rel, &fileops::fill_template(&body, title, Local::now().fixed_offset()))?;
        Ok(rel)
    })?;
    let _ = app.emit("tree-changed", ());
    Ok(rel)
}

/// The daily template PLA writes once if there is none (the user edits it as they like).
pub fn default_daily_template(lang: &str) -> (&'static str, &'static str) {
    if lang == "tr" {
        ("Günlük", "# {{date}}\n\n## Yapılacaklar\n\n\n## Notlar\n\n")
    } else {
        ("Daily", "# {{date}}\n\n## To do\n\n\n## Notes\n\n")
    }
}

/// FR-VLT-010: today's daily note, created from the daily template when it is not there yet.
#[tauri::command]
pub fn open_today(app: AppHandle, state: State<AppState>, lang: String) -> Result<String, String> {
    let now = Local::now().fixed_offset();
    let (rel, created) = with_session(&state, |s| {
        let rel = fileops::daily_note_rel(&s.vault.config.folders, now.date_naive());
        if files::resolve(&s.vault, &rel).map_err(code)?.exists() {
            return Ok((rel, false));
        }
        let (name, default) = default_daily_template(&lang);
        let template_rel = join(&s.vault.config.folders.templates, &format!("{name}.md"));
        let template_path = files::resolve(&s.vault, &template_rel).map_err(code)?;
        let body = match std::fs::read_to_string(&template_path) {
            Ok(text) => text,
            Err(_) => {
                write_new(&state, &s.vault, &template_rel, default)?;
                default.to_owned()
            }
        };
        let title = now.format("%Y-%m-%d").to_string();
        write_new(&state, &s.vault, &rel, &fileops::fill_template(&body, &title, now))?;
        Ok((rel, true))
    })?;
    if created {
        let _ = app.emit("tree-changed", ());
    }
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_errors_become_codes_for_the_ui() {
        assert_eq!(code(FileError::BadName('?')), "bad_name|?|");
        assert_eq!(code(FileError::Exists("inbox/a.md".into())), "exists|inbox/a.md|");
        assert_eq!(code(FileError::Missing), "missing||");
    }

    #[test]
    fn system_folders_and_hidden_places_cannot_be_deleted() {
        // FR-VLT-004/015, Review Focus 1/4
        let tmp = tempfile::tempdir().unwrap();
        let vault = pla_core::vault::open_vault(tmp.path()).unwrap();
        std::fs::write(vault.root.join("notes/a.md"), "a").unwrap();
        assert!(deletable(&vault, "notes/a.md").is_ok());
        assert_eq!(deletable(&vault, "inbox").unwrap_err(), "bad_path|inbox|");
        assert!(deletable(&vault, ".obsidian").is_err());
        assert!(deletable(&vault, "../dışarı.md").is_err());
        assert_eq!(deletable(&vault, "notes/yok.md").unwrap_err(), "missing||");
    }

    #[test]
    fn the_daily_template_speaks_the_ui_language() {
        assert_eq!(default_daily_template("tr").0, "Günlük");
        assert!(default_daily_template("en").1.contains("{{date}}"));
    }
}
