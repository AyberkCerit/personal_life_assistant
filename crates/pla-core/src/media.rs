//! Frontmatter for the properties strip (FR-EDT-006/007) and images in notes (FR-EDT-016/017):
//! which image an embed shows, and saving a pasted or dropped image into `attachments/`.

use std::path::Path;

use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use yaml_rust2::{Yaml, YamlLoader};

use crate::files::{resolve, FileError, FORBIDDEN, FORBIDDEN_EXTRA};
use crate::vault::Vault;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FrontState {
    None,
    Ok,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrontMatter {
    pub state: FrontState,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// Lines the frontmatter takes, both `---` fences included (0 when there is none).
    pub lines: usize,
}

/// The frontmatter's lines (without the fences) and how many lines it takes, or `None` when the
/// note does not start with `---` or the block never closes (then there is no frontmatter).
fn split(text: &str) -> Option<(Vec<&str>, usize)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return None;
    }
    let end = lines.iter().enumerate().skip(1).find(|(_, l)| matches!(l.trim_end(), "---" | "..."))?.0;
    Some((lines[1..end].to_vec(), end + 1))
}

/// A YAML value as a list of strings: a list, or one string split at commas (Obsidian accepts both).
fn strings(value: &Yaml) -> Vec<String> {
    let one = |v: &Yaml| match v {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    };
    let items: Vec<String> = match value {
        Yaml::Array(list) => list.iter().filter_map(one).collect(),
        Yaml::String(s) => s.split(',').map(str::to_owned).collect(),
        other => one(other).into_iter().collect(),
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let item = item.trim().to_owned();
        if !item.is_empty() && !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// FR-EDT-006/007: the note's tags and aliases from its frontmatter. Invalid YAML (or YAML that is
/// not a set of keys) is reported and counts as no frontmatter; PLA never rewrites it.
pub fn read_frontmatter(text: &str) -> FrontMatter {
    let none = FrontMatter { state: FrontState::None, tags: Vec::new(), aliases: Vec::new(), lines: 0 };
    let Some((front, lines)) = split(text) else { return none };
    let invalid = FrontMatter { state: FrontState::Invalid, lines, ..none.clone() };
    let docs = match YamlLoader::load_from_str(&front.join("\n")) {
        Ok(docs) => docs,
        Err(_) => return invalid,
    };
    let doc = match docs.into_iter().next() {
        None => return FrontMatter { state: FrontState::Ok, lines, ..none },
        Some(doc) => doc,
    };
    if !matches!(doc, Yaml::Hash(_)) {
        return invalid;
    }
    let field = |keys: &[&str]| keys.iter().map(|k| &doc[*k]).find(|v| !v.is_badvalue() && !v.is_null()).map(strings).unwrap_or_default();
    let tags = field(&["tags", "tag"]).into_iter().map(|t| t.trim_start_matches('#').to_owned()).filter(|t| !t.is_empty()).collect();
    FrontMatter { state: FrontState::Ok, tags, aliases: field(&["aliases", "alias"]), lines }
}

// ---------------------------------------------------------------- images

/// The largest image PLA stores or shows.
pub const MAX_IMAGE: usize = 20 * 1024 * 1024;

pub const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("not a PNG, JPG, GIF or WebP image")]
    NotImage,
    #[error("the image is larger than 20 MB")]
    TooBig,
    #[error(transparent)]
    File(#[from] FileError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The image type by its first bytes (never by the name): its extension, or `None`.
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

pub fn is_image_name(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Every image file in the vault (vault-relative, `/`), hidden places left out.
fn images_in(vault: &Vault) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![(vault.root.clone(), String::new())];
    while let Some((dir, prefix)) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let rel = format!("{prefix}{name}");
            match entry.file_type() {
                Ok(t) if t.is_dir() => dirs.push((entry.path(), format!("{rel}/"))),
                Ok(t) if t.is_file() && is_image_name(&name) => out.push(rel),
                _ => {}
            }
        }
    }
    out
}

/// FR-EDT-016: the image an embed target (`x.png`, `img/x.png`, `x.png|300`) shows, like Obsidian:
/// the exact vault path first, then the file name anywhere, `attachments/` first, then the shortest
/// path. `None` when there is no such image in the vault.
pub fn resolve_image(vault: &Vault, target: &str) -> Option<String> {
    let target = target.split('|').next().unwrap_or_default().split('#').next().unwrap_or_default().trim().replace('\\', "/");
    let target = target.trim_start_matches('/');
    if !is_image_name(target) {
        return None;
    }
    if target.contains('/') {
        let path = resolve(vault, target).ok()?;
        return path.is_file().then(|| target.to_owned());
    }
    let wanted = target.to_lowercase();
    let attachments = format!("{}/", vault.config.folders.attachments.trim_matches('/').to_lowercase());
    images_in(vault)
        .into_iter()
        .filter(|rel| rel.rsplit('/').next().is_some_and(|n| n.to_lowercase() == wanted))
        .min_by_key(|rel| (!rel.to_lowercase().starts_with(&attachments), rel.len(), rel.clone()))
}

/// The image's bytes (for the preview), at most `MAX_IMAGE`.
pub fn read_image(vault: &Vault, rel: &str) -> Result<Vec<u8>, MediaError> {
    let path = resolve(vault, rel)?;
    if std::fs::metadata(&path)?.len() > MAX_IMAGE as u64 {
        return Err(MediaError::TooBig);
    }
    Ok(std::fs::read(path)?)
}

/// A dropped file's name as a safe file stem: forbidden characters become `-`, no leading dot.
fn clean_stem(name: &str) -> String {
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let cleaned: String = stem.chars().map(|c| if FORBIDDEN.contains(&c) || FORBIDDEN_EXTRA.contains(&c) || c.is_control() { '-' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() { "image".to_owned() } else { cleaned }
}

/// FR-EDT-017: stores a pasted (`name` = `None`) or dropped image in `attachments/` and returns its
/// vault path. Pasted images are `Pasted image YYYYMMDDHHmmss`; a taken name gets ` 1`, ` 2`… The
/// type comes from the bytes; an existing file is never overwritten.
pub fn save_attachment(vault: &Vault, bytes: &[u8], name: Option<&str>, now: DateTime<FixedOffset>) -> Result<String, MediaError> {
    if bytes.len() > MAX_IMAGE {
        return Err(MediaError::TooBig);
    }
    let ext = image_kind(bytes).ok_or(MediaError::NotImage)?;
    let stem = match name {
        Some(n) => clean_stem(n),
        None => format!("Pasted image {}", now.format("%Y%m%d%H%M%S")),
    };
    let folder = vault.config.folders.attachments.trim_matches('/').to_owned();
    let dir = resolve(vault, &folder)?;
    std::fs::create_dir_all(&dir)?;
    for n in 0..1000 {
        let file = if n == 0 { format!("{stem}.{ext}") } else { format!("{stem} {n}.{ext}") };
        let rel = format!("{folder}/{file}");
        let path = resolve(vault, &rel)?;
        // create_new: never over a file that appeared in the meantime
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                use std::io::Write;
                if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
                    drop(f);
                    let _ = std::fs::remove_file(&path);
                    return Err(e.into());
                }
                return Ok(rel);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(FileError::Exists(format!("{folder}/{stem}.{ext}")).into())
}

/// What goes inside `![[…]]` for `rel`: the file name when that finds it, else the whole path.
pub fn embed_target(vault: &Vault, rel: &str) -> String {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if resolve_image(vault, name).as_deref() == Some(rel) { name.to_owned() } else { rel.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    fn vault() -> (tempfile::TempDir, Vault) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = crate::vault::open_vault(tmp.path()).unwrap();
        (tmp, vault)
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-10-06T09:30:15+03:00").unwrap()
    }

    #[test]
    fn frontmatter_gives_tags_and_aliases() {
        // FR-EDT-006
        let fm = read_frontmatter("---\ntags: [iş, \"#proje/pla\"]\naliases:\n  - Plan\n  - Yol Haritası\n---\n# Not\n");
        assert_eq!(fm.state, FrontState::Ok);
        assert_eq!(fm.tags, ["iş", "proje/pla"]);
        assert_eq!(fm.aliases, ["Plan", "Yol Haritası"]);
        assert_eq!(fm.lines, 6);
        assert_eq!(read_frontmatter("---\ntag: a, b\nalias: Tek\n---\n").tags, ["a", "b"]);
        assert_eq!(read_frontmatter("---\ntag: a, b\nalias: Tek\n---\n").aliases, ["Tek"]);
        assert_eq!(read_frontmatter("---\n---\nboş").state, FrontState::Ok);
        assert_eq!(read_frontmatter("# Not\n---\n").state, FrontState::None);
        assert_eq!(read_frontmatter("---\nkapanmıyor: evet\n").state, FrontState::None, "an unclosed block is not frontmatter");
    }

    #[test]
    fn invalid_yaml_is_reported_and_counts_as_none() {
        // FR-EDT-007
        let fm = read_frontmatter("---\ntags: [a, b\nbaşlık: \"yarım\n---\nmetin");
        assert_eq!(fm.state, FrontState::Invalid);
        assert!(fm.tags.is_empty() && fm.aliases.is_empty());
        assert_eq!(fm.lines, 4, "the editor still dims it");
        assert_eq!(read_frontmatter("---\n- bir liste\n---\n").state, FrontState::Invalid, "not a set of keys");
        // the index agrees: no tags or aliases from it, and its lines are not the body
        let parsed = crate::index::parse_note("a.md", "---\ntags: [a, b\n---\nmetin #gerçek");
        assert_eq!(parsed.tags, ["gerçek"]);
        assert!(parsed.aliases.is_empty() && parsed.body == "metin #gerçek");
    }

    #[test]
    fn image_types_come_from_the_bytes() {
        assert_eq!(image_kind(PNG), Some("png"));
        assert_eq!(image_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_kind(b"GIF89a.."), Some("gif"));
        assert_eq!(image_kind(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_kind(b"%PDF-1.7"), None);
    }

    #[test]
    fn embeds_find_their_image_like_obsidian() {
        // FR-EDT-016
        let (_t, v) = vault();
        std::fs::create_dir_all(v.root.join("notes/img")).unwrap();
        std::fs::create_dir_all(v.root.join(".obsidian")).unwrap();
        std::fs::write(v.root.join("notes/img/şema.png"), PNG).unwrap();
        std::fs::write(v.root.join("attachments/şema.png"), PNG).unwrap();
        std::fs::write(v.root.join(".obsidian/gizli.png"), PNG).unwrap();
        assert_eq!(resolve_image(&v, "şema.png").as_deref(), Some("attachments/şema.png"), "attachments/ first");
        assert_eq!(resolve_image(&v, "Şema.PNG|300").as_deref(), Some("attachments/şema.png"));
        assert_eq!(resolve_image(&v, "notes/img/şema.png").as_deref(), Some("notes/img/şema.png"));
        assert_eq!(resolve_image(&v, "gizli.png"), None, "hidden places stay hidden");
        assert_eq!(resolve_image(&v, "../dışarı.png"), None);
        assert_eq!(resolve_image(&v, "yok.png"), None);
        assert_eq!(resolve_image(&v, "Not"), None, "not an image");
    }

    #[test]
    fn pasted_and_dropped_images_go_to_attachments_without_overwriting() {
        // FR-EDT-017
        let (_t, v) = vault();
        assert_eq!(save_attachment(&v, PNG, None, now()).unwrap(), "attachments/Pasted image 20261006093015.png");
        assert_eq!(save_attachment(&v, PNG, None, now()).unwrap(), "attachments/Pasted image 20261006093015 1.png");
        // a dropped file keeps its name; the extension follows the bytes
        assert_eq!(save_attachment(&v, PNG, Some("Ekran: görüntüsü.jpeg"), now()).unwrap(), "attachments/Ekran- görüntüsü.png");
        assert_eq!(save_attachment(&v, PNG, Some("../../.gizli.png"), now()).unwrap(), "attachments/gizli.png");
        assert!(matches!(save_attachment(&v, b"%PDF-1.7", None, now()), Err(MediaError::NotImage)));
        let big = [PNG, &vec![0u8; MAX_IMAGE]].concat();
        assert!(matches!(save_attachment(&v, &big, None, now()), Err(MediaError::TooBig)));
        assert_eq!(std::fs::read(v.root.join("attachments/Pasted image 20261006093015.png")).unwrap(), PNG);
    }

    #[test]
    fn embeds_use_the_name_unless_another_image_would_win() {
        let (_t, v) = vault();
        std::fs::write(v.root.join("attachments/a.png"), PNG).unwrap();
        assert_eq!(embed_target(&v, "attachments/a.png"), "a.png");
        std::fs::create_dir_all(v.root.join("x")).unwrap();
        std::fs::write(v.root.join("x/b.png"), PNG).unwrap();
        std::fs::write(v.root.join("attachments/b.png"), PNG).unwrap();
        assert_eq!(embed_target(&v, "x/b.png"), "x/b.png");
    }
}
