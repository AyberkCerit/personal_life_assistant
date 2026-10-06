//! Moving, renaming and naming notes and folders (FR-EDT-004/013/018, FR-VLT-005/009/010/018):
//! the file moves first, then PLA's records follow it in one transaction.

use chrono::{DateTime, FixedOffset, NaiveDate};
use rusqlite::Connection;

use crate::files::{resolve, FileError, FORBIDDEN, FORBIDDEN_EXTRA};
use crate::vault::{Folders, Vault};

/// FR-VLT-005: a note or folder name: not empty, no forbidden character, not hidden.
pub fn check_name(name: &str) -> Result<(), FileError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(FileError::BadName(' '));
    }
    if name.starts_with('.') {
        return Err(FileError::BadName('.'));
    }
    match name.chars().find(|c| FORBIDDEN.contains(c) || FORBIDDEN_EXTRA.contains(c)) {
        Some(c) => Err(FileError::BadName(c)),
        None => Ok(()),
    }
}

fn same_ignoring_case(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The system folders PLA keeps its own things in: they stay where `.pla/config` says.
fn is_system_folder(folders: &Folders, rel: &str) -> bool {
    [&folders.daily, &folders.inbox, &folders.notes, &folders.reports, &folders.attachments, &folders.templates]
        .iter()
        .any(|f| same_ignoring_case(f.trim_matches('/'), rel.trim_matches('/')))
}

/// Puts `from` at `to` on disk: refuses an existing target (unless only the letter case differs,
/// which Windows treats as the same file) and creates the target's folders.
fn rename_on_disk(vault: &Vault, from: &str, to: &str) -> Result<(std::path::PathBuf, std::path::PathBuf), FileError> {
    let (from_abs, to_abs) = (resolve(vault, from)?, resolve(vault, to)?);
    if !from_abs.exists() {
        return Err(FileError::Missing);
    }
    if to_abs.exists() && !same_ignoring_case(from, to) {
        return Err(FileError::Exists(to.to_owned()));
    }
    if let Some(parent) = to_abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&from_abs, &to_abs)?;
    Ok((from_abs, to_abs))
}

/// Moves (or renames) the note `from` to `to`, both vault-relative `.md` paths. The target must not
/// exist. Blocks (and through them tasks and metrics) and the extraction queue follow (FR-VLT-018).
pub fn move_note(vault: &Vault, conn: &Connection, from: &str, to: &str) -> Result<(), FileError> {
    if !to.to_lowercase().ends_with(".md") {
        return Err(FileError::BadPath(to.to_owned()));
    }
    let name = to.rsplit('/').next().unwrap_or_default();
    check_name(crate::index::strip_md(name))?;
    let (from_abs, to_abs) = rename_on_disk(vault, from, to)?;
    let records = (|| {
        let tx = conn.unchecked_transaction()?;
        tx.execute("UPDATE block SET note_path = ?2 WHERE note_path = ?1", [from, to])?;
        tx.execute("UPDATE OR REPLACE extraction_queue SET note_path = ?2 WHERE note_path = ?1", [from, to])?;
        tx.commit()
    })();
    if let Err(e) = records {
        let _ = std::fs::rename(&to_abs, &from_abs); // the file goes back where its records are
        return Err(e.into());
    }
    Ok(())
}

/// Moves (or renames) the folder `from` to `to`; every note under it follows like `move_note`.
pub fn move_folder(vault: &Vault, conn: &Connection, from: &str, to: &str) -> Result<(), FileError> {
    let (from, to) = (from.trim_matches('/'), to.trim_matches('/'));
    let inside = to.to_lowercase().starts_with(&format!("{}/", from.to_lowercase()));
    if is_system_folder(&vault.config.folders, from) || inside || from == to {
        return Err(FileError::BadPath(from.to_owned()));
    }
    check_name(to.rsplit('/').next().unwrap_or_default())?;
    if !resolve(vault, from)?.is_dir() {
        return Err(FileError::Missing);
    }
    let (from_abs, to_abs) = rename_on_disk(vault, from, to)?;
    let records = (|| {
        let tx = conn.unchecked_transaction()?;
        let moved = "?2 || substr(note_path, length(?1) + 1) WHERE substr(note_path, 1, length(?1) + 1) = ?1 || '/'";
        tx.execute(&format!("UPDATE block SET note_path = {moved}"), [from, to])?;
        tx.execute(&format!("UPDATE OR REPLACE extraction_queue SET note_path = {moved}"), [from, to])?;
        tx.commit()
    })();
    if let Err(e) = records {
        let _ = std::fs::rename(&to_abs, &from_abs);
        return Err(e.into());
    }
    Ok(())
}

/// FR-EDT-013: `text` with every wikilink to `old_rel` pointed at `new_rel` (alias and heading
/// kept), or `None` when nothing changed. Links by name get the new name, links by path the new path.
pub fn rewrite_links(text: &str, old_rel: &str, new_rel: &str) -> Option<String> {
    use crate::index::{key, normalize_name, strip_md};
    let stem = |rel: &str| strip_md(rel.rsplit('/').next().unwrap_or_default()).to_owned();
    let (old_path, old_title) = (normalize_name(old_rel), key(&stem(old_rel)));
    let (new_path, new_title) = (strip_md(new_rel).to_owned(), stem(new_rel));
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (None, Some(m)) => fence = Some(m),
            (Some(open), Some(m)) if open == m => fence = None,
            _ => {}
        }
        if fence.is_some() || marker.is_some() {
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find("[[") {
            let in_code = in_inline_code(line, rest, open);
            let Some(close) = rest[open + 2..].find("]]") else { break };
            let inner = &rest[open + 2..open + 2 + close];
            out.push_str(&rest[..open + 2]);
            let replaced = if in_code { None } else { relink(inner, &old_path, &old_title, &new_path, &new_title) };
            match replaced {
                Some(r) => {
                    changed = true;
                    out.push_str(&r);
                }
                None => out.push_str(inner),
            }
            out.push_str("]]");
            rest = &rest[open + 2 + close + 2..];
        }
        out.push_str(rest);
    }
    changed.then_some(out)
}

/// Whether position `open` of `rest` (a tail of `line`) sits inside an inline code span of `line`.
fn in_inline_code(line: &str, rest: &str, open: usize) -> bool {
    let before = line.len() - rest.len() + open;
    line[..before].matches('`').count() % 2 == 1
}

/// One link's inside (`target#heading|alias`) pointed at the new note, or `None` when it is not
/// a link to the old one.
fn relink(inner: &str, old_path: &str, old_title: &str, new_path: &str, new_title: &str) -> Option<String> {
    use crate::index::{key, normalize_name};
    // the alias part keeps its separator as written (`|`, or `\|` inside a table)
    let split = inner.find("\\|").or_else(|| inner.find('|'));
    let (target_part, alias) = match split {
        Some(i) => (&inner[..i], Some(&inner[i..])),
        None => (inner, None),
    };
    let (target, heading) = match target_part.find('#') {
        Some(i) => (&target_part[..i], Some(&target_part[i..])),
        None => (target_part, None),
    };
    let target_trim = target.trim();
    let new_target = if normalize_name(target_trim) == old_path && target_trim.contains('/') {
        new_path
    } else if !target_trim.contains('/') && key(crate::index::strip_md(target_trim)) == old_title {
        new_title
    } else {
        return None;
    };
    if new_target == target_trim {
        return None;
    }
    Some(format!("{new_target}{}{}", heading.unwrap_or_default(), alias.unwrap_or_default()))
}

/// FR-EDT-018: a template with `{{date}}`, `{{time}}` and `{{title}}` filled in.
pub fn fill_template(template: &str, title: &str, now: DateTime<FixedOffset>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else { break };
        out.push_str(&rest[..open]);
        let name = rest[open + 2..open + 2 + close].trim();
        match name {
            "date" => out.push_str(&now.format("%Y-%m-%d").to_string()),
            "time" => out.push_str(&now.format("%H:%M").to_string()),
            "title" => out.push_str(title),
            _ => out.push_str(&rest[open..open + 2 + close + 2]),
        }
        rest = &rest[open + 2 + close + 2..];
    }
    out.push_str(rest);
    out
}

/// FR-VLT-010: `daily/YYYY/YYYY-MM-DD.md`.
pub fn daily_note_rel(folders: &Folders, date: NaiveDate) -> String {
    format!("{}/{}/{}.md", folders.daily.trim_matches('/'), date.format("%Y"), date.format("%Y-%m-%d"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_databases;
    use crate::vault::open_vault;

    fn setup() -> (tempfile::TempDir, Vault, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("kasa")).unwrap();
        let vault = open_vault(&tmp.path().join("kasa")).unwrap();
        let conn = open_databases(&tmp.path().join("data")).unwrap().pla;
        (tmp, vault, conn)
    }

    fn block(conn: &Connection, id: &str, note: &str) {
        conn.execute(
            "INSERT INTO block (block_id, note_path, position, text, text_hash, first_seen_at, last_seen_at) VALUES (?1, ?2, 0, 'x', 'h', 't', 't')",
            [id, note],
        )
        .unwrap();
    }

    fn note_of(conn: &Connection, id: &str) -> String {
        conn.query_row("SELECT note_path FROM block WHERE block_id = ?1", [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn names_with_forbidden_characters_are_refused() {
        // FR-VLT-005
        assert!(check_name("Proje Planı 2026").is_ok());
        assert!(matches!(check_name("a/b"), Err(FileError::BadName('/'))));
        assert!(matches!(check_name("soru?"), Err(FileError::BadName('?'))));
        assert!(matches!(check_name(".gizli"), Err(FileError::BadName('.'))));
        assert!(matches!(check_name("   "), Err(FileError::BadName(' '))));
    }

    #[test]
    fn a_moved_note_takes_its_records_along() {
        // FR-VLT-018, Review Focus 2
        let (_t, vault, conn) = setup();
        std::fs::write(vault.root.join("inbox/Dişçi.md"), "Yarın dişçi.").unwrap();
        block(&conn, "b1", "inbox/Dişçi.md");
        conn.execute("INSERT INTO extraction_queue (note_path, queued_at) VALUES ('inbox/Dişçi.md', 't')", []).unwrap();
        move_note(&vault, &conn, "inbox/Dişçi.md", "notes/sağlık/Dişçi Randevusu.md").unwrap();
        assert!(!vault.root.join("inbox/Dişçi.md").exists());
        assert_eq!(std::fs::read_to_string(vault.root.join("notes/sağlık/Dişçi Randevusu.md")).unwrap(), "Yarın dişçi.");
        assert_eq!(note_of(&conn, "b1"), "notes/sağlık/Dişçi Randevusu.md");
        let queued: String = conn.query_row("SELECT note_path FROM extraction_queue", [], |r| r.get(0)).unwrap();
        assert_eq!(queued, "notes/sağlık/Dişçi Randevusu.md");
    }

    #[test]
    fn a_move_never_overwrites_or_touches_hidden_places() {
        // Review Focus 1/4
        let (_t, vault, conn) = setup();
        std::fs::write(vault.root.join("inbox/a.md"), "a").unwrap();
        std::fs::write(vault.root.join("inbox/b.md"), "b").unwrap();
        std::fs::create_dir_all(vault.root.join(".obsidian")).unwrap();
        assert!(matches!(move_note(&vault, &conn, "inbox/a.md", "inbox/b.md"), Err(FileError::Exists(_))));
        assert_eq!(std::fs::read_to_string(vault.root.join("inbox/b.md")).unwrap(), "b");
        assert!(matches!(move_note(&vault, &conn, "inbox/a.md", ".obsidian/a.md"), Err(FileError::BadPath(_))));
        assert!(matches!(move_note(&vault, &conn, "inbox/a.md", "inbox/a.txt"), Err(FileError::BadPath(_))), "notes stay .md");
        assert!(matches!(move_note(&vault, &conn, "inbox/yok.md", "inbox/c.md"), Err(FileError::Missing)));
        // a different case of the same name is a rename on Windows, not a clash
        move_note(&vault, &conn, "inbox/a.md", "inbox/A.md").unwrap();
        assert!(std::fs::read_dir(vault.root.join("inbox")).unwrap().any(|e| e.unwrap().file_name() == "A.md"));
    }

    #[test]
    fn a_moved_folder_takes_every_note_inside_along() {
        let (_t, vault, conn) = setup();
        std::fs::create_dir_all(vault.root.join("notes/proje/alt")).unwrap();
        std::fs::write(vault.root.join("notes/proje/a.md"), "a").unwrap();
        std::fs::write(vault.root.join("notes/proje/alt/b.md"), "b").unwrap();
        block(&conn, "b1", "notes/proje/a.md");
        block(&conn, "b2", "notes/proje/alt/b.md");
        block(&conn, "b3", "notes/projeler/c.md"); // a name prefix, not inside
        move_folder(&vault, &conn, "notes/proje", "arşiv/2025 proje").unwrap();
        assert!(vault.root.join("arşiv/2025 proje/alt/b.md").exists());
        assert_eq!(note_of(&conn, "b1"), "arşiv/2025 proje/a.md");
        assert_eq!(note_of(&conn, "b2"), "arşiv/2025 proje/alt/b.md");
        assert_eq!(note_of(&conn, "b3"), "notes/projeler/c.md");
        assert!(matches!(move_folder(&vault, &conn, "arşiv", "arşiv/iç"), Err(FileError::BadPath(_))), "not into itself");
        assert!(matches!(move_folder(&vault, &conn, "inbox", "gelen"), Err(FileError::BadPath(_))), "system folders stay where the config says");
    }

    #[test]
    fn rewriting_links_keeps_aliases_and_headings() {
        // FR-EDT-013, Review Focus 3
        let text = "Bkz [[Proje Planı]], [[proje planı|plan]], [[Proje Planı#Hedefler]] ve [[notes/Proje Planı]].\n[[Proje]] başka not. `[[Proje Planı]]` kod.";
        let out = rewrite_links(text, "notes/Proje Planı.md", "notes/Yol Haritası.md").unwrap();
        assert_eq!(
            out,
            "Bkz [[Yol Haritası]], [[Yol Haritası|plan]], [[Yol Haritası#Hedefler]] ve [[notes/Yol Haritası]].\n[[Proje]] başka not. `[[Proje Planı]]` kod."
        );
        // a move keeps the name: only links by path change
        let moved = rewrite_links("[[Proje Planı]] ve [[notes/Proje Planı|p]]", "notes/Proje Planı.md", "arşiv/Proje Planı.md").unwrap();
        assert_eq!(moved, "[[Proje Planı]] ve [[arşiv/Proje Planı|p]]");
        assert_eq!(rewrite_links("[[Başka]]", "notes/Proje Planı.md", "notes/Yeni.md"), None);
        assert_eq!(rewrite_links("![[Proje Planı]] gömülü", "notes/Proje Planı.md", "notes/Yeni.md").as_deref(), Some("![[Yeni]] gömülü"), "embeds point at the note too");
    }

    #[test]
    fn templates_get_the_date_time_and_title() {
        // FR-EDT-018
        let now = DateTime::parse_from_rfc3339("2026-10-06T09:05:00+03:00").unwrap();
        assert_eq!(fill_template("# {{title}}\n{{date}} {{time}} {{ date }} {{other}}", "Toplantı", now), "# Toplantı\n2026-10-06 09:05 2026-10-06 {{other}}");
        assert_eq!(daily_note_rel(&Folders::default(), NaiveDate::from_ymd_opt(2026, 10, 6).unwrap()), "daily/2026/2026-10-06.md");
    }
}
