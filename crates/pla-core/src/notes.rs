//! Reading notes: blocks (FR-EXT-004), the `pla_generated` flag (FR-EXT-001), which notes are the
//! user's (FR-VLT-017) and the date of a daily note (FR-EXT-009). Read-only: nothing is written to notes.

use std::path::Path;

use chrono::NaiveDate;

use crate::markdown::{frontmatter_end, is_heading, list_item_body, Fences};
use crate::vault::{is_note_name, is_under, walk_vault, Folders, Vault};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub position: usize,
    pub text: String,
}

/// Splits a note at paragraph, heading and list-item boundaries; frontmatter and fenced code are skipped.
pub fn split_blocks(text: &str) -> Vec<Block> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut fences = Fences::default();

    for line in &lines[frontmatter_end(&lines).unwrap_or(0)..] {
        let trimmed = line.trim_start();
        if fences.is_code(line) {
            flush(&mut current, &mut blocks); // code ends the block before it and is no block itself
        } else if trimmed.is_empty() {
            flush(&mut current, &mut blocks);
        } else if is_heading(trimmed) {
            flush(&mut current, &mut blocks);
            current.push(line);
            flush(&mut current, &mut blocks);
        } else if list_item_body(trimmed).is_some() {
            flush(&mut current, &mut blocks);
            current.push(line);
        } else {
            current.push(line);
        }
    }
    flush(&mut current, &mut blocks);
    blocks
}

fn flush(current: &mut Vec<&str>, blocks: &mut Vec<Block>) {
    let text = current.join("\n").trim().to_owned();
    if !text.is_empty() {
        blocks.push(Block { position: blocks.len(), text });
    }
    current.clear();
}

/// Decision 2026-10-02 (FR-EDT-019): a checked list item is done and never produces items.
pub fn is_checked_item(text: &str) -> bool {
    list_item_body(text.trim_start()).is_some_and(|r| r.starts_with("[x]") || r.starts_with("[X]"))
}

/// FR-EXT-001: notes written by PLA itself carry `pla_generated: true` and are never extracted.
pub fn is_generated(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    frontmatter_end(&lines).is_some_and(|end| {
        lines[1..end - 1].iter().any(|line| {
            line.split_once(':').is_some_and(|(key, value)| {
                key.trim().eq_ignore_ascii_case("pla_generated") && value.trim().trim_matches(|c| c == '"' || c == '\'').eq_ignore_ascii_case("true")
            })
        })
    })
}

/// Hidden paths and the reports/templates/memory folders are never extracted (FR-EXT-001, FR-VLT-017).
pub fn is_excluded(rel: &str, folders: &Folders) -> bool {
    let path = Path::new(rel);
    path.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        || is_under(rel, &folders.reports)
        || is_under(rel, &folders.templates)
        || is_under(rel, &folders.memory)
        || path.file_stem().is_some_and(|s| {
            let s = s.to_string_lossy();
            s.contains(crate::files::CONFLICT_SUFFIX) || s.contains(crate::files::OLD_CONFLICT_SUFFIX)
        })
}

/// `<daily>/…/YYYY-MM-DD.md` → that date (the reference date of a daily note, FR-EXT-009).
pub fn daily_note_date(rel: &str, folders: &Folders) -> Option<NaiveDate> {
    if !is_under(rel, &folders.daily) {
        return None;
    }
    let path = Path::new(rel);
    NaiveDate::parse_from_str(path.file_stem()?.to_str()?, "%Y-%m-%d").ok()
}

/// All user notes of the vault, as sorted vault-relative paths with `/` separators.
pub fn list_user_notes(vault: &Vault) -> std::io::Result<Vec<String>> {
    std::fs::read_dir(&vault.root)?; // a vault that cannot be read is an error, not an empty one
    let folders = &vault.config.folders;
    let notes = walk_vault(&vault.root).filter(|e| !e.is_dir && is_note_name(&e.rel) && !is_excluded(&e.rel, folders));
    let mut out: Vec<String> = notes.map(|e| e.rel).collect();
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{open_vault, Folders};

    #[test]
    fn checked_list_items_are_recognised() {
        assert!(is_checked_item("- [x] Fatura öde"));
        assert!(is_checked_item("* [X] Fatura öde"));
        assert!(is_checked_item("  1. [x] birinci"));
        assert!(!is_checked_item("- [ ] Fatura öde"));
        assert!(!is_checked_item("[x] liste değil"));
        assert!(!is_checked_item("- x işaret değil"));
    }

    fn texts(text: &str) -> Vec<String> {
        split_blocks(text).into_iter().map(|b| b.text).collect()
    }

    #[test]
    fn paragraphs_and_list_items_are_blocks() {
        let note = "Yarın 9'da dişçi.\nSonra market.\n\n- süt al\n- ekmek al\n  (tam buğday)\n1. birinci\n\nSon paragraf.";
        assert_eq!(texts(note), vec!["Yarın 9'da dişçi.\nSonra market.", "- süt al", "- ekmek al\n  (tam buğday)", "1. birinci", "Son paragraf.",]);
        let positions: Vec<usize> = split_blocks(note).iter().map(|b| b.position).collect();
        assert_eq!(positions, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn headings_are_their_own_blocks() {
        assert_eq!(texts("# Pazartesi\nToplantı 10'da"), vec!["# Pazartesi", "Toplantı 10'da"]);
        assert_eq!(texts("#etiket değil başlık"), vec!["#etiket değil başlık"]);
    }

    #[test]
    fn frontmatter_and_code_are_skipped() {
        // Review Focus 5
        let note = "\u{feff}---\ntags: [günlük]\n---\nYarın spor.\n\n```python\nprint('yarın 9da toplantı')\n```\n~~~\nkod\n~~~\nBitti.";
        assert_eq!(texts(note), vec!["Yarın spor.", "Bitti."]);
    }

    #[test]
    fn unclosed_frontmatter_is_ordinary_text() {
        assert_eq!(texts("---\nbaşlık yok\n\nYarın spor."), vec!["---\nbaşlık yok", "Yarın spor."]);
    }

    #[test]
    fn crlf_line_endings_give_the_same_blocks() {
        assert_eq!(texts("Bir.\r\n\r\n- iki\r\n"), vec!["Bir.", "- iki"]);
    }

    #[test]
    fn generated_flag_is_read_from_frontmatter_only() {
        assert!(is_generated("---\npla_generated: true\n---\nRapor"));
        assert!(is_generated("---\nPLA_GENERATED: \"True\"\n---\n"));
        assert!(!is_generated("---\npla_generated: false\n---\n"));
        assert!(!is_generated("pla_generated: true"));
    }

    #[test]
    fn memory_notes_are_never_extracted() {
        let f = Folders::default();
        assert!(is_excluded("memory/Durum.md", &f));
        assert!(is_excluded("Memory/Durum.md", &f));
        assert!(!is_excluded("notes/memory.md", &f));
    }

    #[test]
    fn exclusions_and_daily_dates() {
        let f = Folders::default();
        assert!(is_excluded("reports/weekly/2026-W40.md", &f));
        assert!(is_excluded("templates/daily.md", &f));
        assert!(is_excluded(".obsidian/x.md", &f));
        assert!(is_excluded("notes/.gizli/x.md", &f));
        assert!(!is_excluded("notes/fikirler.md", &f));
        assert_eq!(daily_note_date("daily/2026/2026-10-06.md", &f), NaiveDate::from_ymd_opt(2026, 10, 6));
        assert_eq!(daily_note_date("daily/2026-10-06.md", &f), NaiveDate::from_ymd_opt(2026, 10, 6));
        assert_eq!(daily_note_date("notes/2026-10-06.md", &f), None);
        assert_eq!(daily_note_date("daily/2026/plan.md", &f), None);
    }

    #[test]
    fn system_folders_match_whatever_their_letter_case() {
        // architecture review M1: an Obsidian vault's `Templates/` is the configured `templates`,
        // as Windows sees it
        let f = Folders::default();
        assert!(is_excluded("Templates/Günlük.md", &f));
        assert!(is_excluded("REPORTS/weekly/2026-W40.md", &f));
        assert!(!is_excluded("templatesx/a.md", &f));
        assert_eq!(daily_note_date("Daily/2026/2026-10-06.md", &f), NaiveDate::from_ymd_opt(2026, 10, 6));
    }

    #[test]
    fn lists_only_user_markdown_notes() {
        let tmp = tempfile::tempdir().unwrap();
        let v = open_vault(tmp.path()).unwrap();
        for (path, body) in [
            ("daily/2026/2026-10-06.md", "x"),
            ("notes/Fikir ğüş.MD", "x"),
            ("inbox/resim.png", "x"),
            ("templates/daily.md", "x"),
            ("reports/weekly/2026-W40.md", "x"),
            (".obsidian/workspace.md", "x"),
        ] {
            let p = tmp.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        assert_eq!(list_user_notes(&v).unwrap(), vec!["daily/2026/2026-10-06.md", "notes/Fikir ğüş.MD"]);
    }
    #[test]
    fn conflict_copies_are_not_extracted() {
        // F4a review M5
        let f = Folders::default();
        assert!(is_excluded("inbox/Plan (conflict).md", &f));
        assert!(is_excluded("inbox/Plan (conflict) 2.md", &f));
        assert!(is_excluded("inbox/Plan (çakışma).md", &f), "copies made before the rename");
        assert!(!is_excluded("inbox/Plan.md", &f));
    }
}
