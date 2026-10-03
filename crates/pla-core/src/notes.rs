//! Reading notes: blocks (FR-EXT-004), the `pla_generated` flag (FR-EXT-001), which notes are the
//! user's (FR-VLT-017) and the date of a daily note (FR-EXT-009). Read-only: nothing is written to notes.

use std::path::Path;

use chrono::NaiveDate;

use crate::vault::{Folders, Vault};

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
    let mut fence: Option<&str> = None;

    for line in &lines[frontmatter_end(&lines)..] {
        let trimmed = line.trim_start();
        if let Some(marker) = fence {
            if trimmed.starts_with(marker) {
                fence = None;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut current, &mut blocks);
            fence = Some(&trimmed[..3]);
        } else if trimmed.is_empty() {
            flush(&mut current, &mut blocks);
        } else if is_heading(trimmed) {
            flush(&mut current, &mut blocks);
            current.push(line);
            flush(&mut current, &mut blocks);
        } else if is_list_item(trimmed) {
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

fn is_heading(trimmed: &str) -> bool {
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ')
}

fn is_list_item(trimmed: &str) -> bool {
    if ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m)) {
        return true;
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (trimmed[digits..].starts_with(". ") || trimmed[digits..].starts_with(") "))
}

/// Decision 2026-10-02 (FR-EDT-019): a checked list item is done and never produces items.
pub fn is_checked_item(text: &str) -> bool {
    let t = text.trim_start();
    let after_marker = ["- ", "* ", "+ "].iter().find_map(|m| t.strip_prefix(m)).or_else(|| {
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        (digits > 0).then(|| &t[digits..]).and_then(|r| r.strip_prefix(". ").or_else(|| r.strip_prefix(") ")))
    });
    after_marker.is_some_and(|r| r.starts_with("[x]") || r.starts_with("[X]"))
}

/// Index of the first body line: after a `---` … `---`/`...` frontmatter, or 0 when there is none.
fn frontmatter_end(lines: &[&str]) -> usize {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return 0;
    }
    lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| matches!(l.trim_end(), "---" | "..."))
        .map_or(0, |(i, _)| i + 1)
}

/// FR-EXT-001: notes written by PLA itself carry `pla_generated: true` and are never extracted.
pub fn is_generated(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let end = frontmatter_end(&lines);
    end > 0
        && lines[1..end - 1].iter().any(|line| {
            line.split_once(':').is_some_and(|(key, value)| {
                key.trim().eq_ignore_ascii_case("pla_generated")
                    && value.trim().trim_matches(|c| c == '"' || c == '\'').eq_ignore_ascii_case("true")
            })
        })
}

/// Hidden paths and the reports/templates folders are never extracted (FR-EXT-001, FR-VLT-017).
pub fn is_excluded(rel: &str, folders: &Folders) -> bool {
    let path = Path::new(rel);
    path.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        || path.starts_with(&folders.reports)
        || path.starts_with(&folders.templates)
        || path.file_stem().is_some_and(|s| {
            let s = s.to_string_lossy();
            s.contains(crate::files::CONFLICT_SUFFIX) || s.contains(crate::files::OLD_CONFLICT_SUFFIX)
        })
}

/// `<daily>/…/YYYY-MM-DD.md` → that date (the reference date of a daily note, FR-EXT-009).
pub fn daily_note_date(rel: &str, folders: &Folders) -> Option<NaiveDate> {
    let path = Path::new(rel);
    if !path.starts_with(&folders.daily) {
        return None;
    }
    NaiveDate::parse_from_str(path.file_stem()?.to_str()?, "%Y-%m-%d").ok()
}

/// All user notes of the vault, as sorted vault-relative paths with `/` separators.
pub fn list_user_notes(vault: &Vault) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    walk(&vault.root, &vault.root, &vault.config.folders, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, folders: &Folders, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .expect("walked path is under the vault root")
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if is_excluded(&rel, folders) || kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk(root, &path, folders, out)?;
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            out.push(rel);
        }
    }
    Ok(())
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
        assert_eq!(texts(note), vec![
            "Yarın 9'da dişçi.\nSonra market.",
            "- süt al",
            "- ekmek al\n  (tam buğday)",
            "1. birinci",
            "Son paragraf.",
        ]);
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
