//! PDF import (owner decision 2026-10-30), first step: a PDF becomes a Markdown note without a
//! model. The PDF goes to `attachments/`, its text to `sources/<name>.md`, marked `pla_generated` so
//! task extraction and memory leave it alone while search and the assistant can still read it.
//! PDFium (Chromium's PDF engine) is a library PLA ships next to the app (`src-tauri/pdfium/`).

mod layout;
mod read;

use std::io::Write;
use std::path::{Path, PathBuf};

pub use layout::{to_markdown, Converted, Glyph, OutlineEntry, RawDoc, RawPage};
pub use read::pdfium;

use crate::files::{resolve, FileError};
use crate::media::clean_stem_or;
use crate::vault::Vault;

/// Larger PDFs are books with images; their text alone would not need this much.
pub const MAX_PDF: usize = 100 * 1024 * 1024;
/// Under this many characters a page, the PDF is taken for a scan (images of pages, no text).
const MIN_CHARS_PER_PAGE: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("PDFium is not installed: {}", .0.display())]
    LibraryMissing(PathBuf),
    #[error("PDFium: {0}")]
    Library(String),
    #[error("not a PDF")]
    NotPdf,
    #[error("the PDF is password-protected")]
    Encrypted,
    #[error("the PDF is damaged")]
    Damaged,
    #[error("the PDF is larger than {} MB", MAX_PDF / 1024 / 1024)]
    TooBig,
    #[error("the PDF has no text (a scanned document?)")]
    NoText,
    #[error(transparent)]
    File(#[from] FileError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Where an import put things, as vault-relative paths.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub pdf: String,
    pub note: String,
    pub pages: usize,
    pub chars: usize,
}

/// The bytes are a PDF: its header sits in the first kilobyte.
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.len() >= 5 && bytes[..bytes.len().min(1024)].windows(5).any(|w| w == b"%PDF-")
}

/// Reads and converts a PDF; `source` is the file name the note links back to.
pub fn convert(lib_dir: &Path, bytes: &[u8], source: &str) -> Result<Converted, PdfError> {
    if bytes.len() > MAX_PDF {
        return Err(PdfError::TooBig);
    }
    if !is_pdf(bytes) {
        return Err(PdfError::NotPdf);
    }
    let doc = read::read(pdfium(lib_dir)?, bytes)?;
    let converted = to_markdown(&doc, source);
    if converted.chars < MIN_CHARS_PER_PAGE * converted.pages.max(1) {
        return Err(PdfError::NoText);
    }
    Ok(converted)
}

/// Writes a new file, never over one that is there (or appeared meanwhile).
fn create_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
        drop(f);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(())
}

/// Imports a dropped PDF named `name`: the PDF into `attachments/`, its text into `sources/`, both
/// under the PDF's name (` 1`, ` 2`… when taken). Nothing is written when the PDF cannot be read.
pub fn import(vault: &Vault, lib_dir: &Path, bytes: &[u8], name: &str) -> Result<Imported, PdfError> {
    let stem = clean_stem_or(name, "PDF");
    let folders = &vault.config.folders;
    let (attachments, sources) = (folders.attachments.trim_matches('/'), folders.sources.trim_matches('/'));
    for n in 0..1000 {
        let base = if n == 0 { stem.clone() } else { format!("{stem} {n}") };
        let (pdf_rel, note_rel) = (format!("{attachments}/{base}.pdf"), format!("{sources}/{base}.md"));
        let (pdf_path, note_path) = (resolve(vault, &pdf_rel)?, resolve(vault, &note_rel)?);
        if pdf_path.exists() || note_path.exists() {
            continue;
        }
        let converted = convert(lib_dir, bytes, &format!("{base}.pdf"))?;
        std::fs::create_dir_all(resolve(vault, attachments)?)?;
        std::fs::create_dir_all(resolve(vault, sources)?)?;
        match create_new(&pdf_path, bytes) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            r => r?,
        }
        if let Err(e) = create_new(&note_path, converted.markdown.as_bytes()) {
            let _ = std::fs::remove_file(&pdf_path);
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(e.into());
        }
        return Ok(Imported { pdf: pdf_rel, note: note_rel, pages: converted.pages, chars: converted.chars });
    }
    Err(FileError::Exists(format!("{attachments}/{stem}.pdf")).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdf_is_known_by_its_header() {
        assert!(is_pdf(b"%PDF-1.7\n..."));
        assert!(is_pdf(b"\xef\xbb\xbfjunk before %PDF-1.4"), "some writers put bytes before the header");
        assert!(!is_pdf(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_pdf(b"%PD"));
    }

    #[test]
    fn nothing_is_written_without_pdfium() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = crate::vault::open_vault(tmp.path()).unwrap();
        let missing = tmp.path().join("no-pdfium");
        assert!(matches!(import(&vault, &missing, b"%PDF-1.7", "a.pdf"), Err(PdfError::LibraryMissing(_))));
        assert!(matches!(import(&vault, &missing, b"not a pdf", "a.pdf"), Err(PdfError::NotPdf)));
        assert!(std::fs::read_dir(tmp.path().join("attachments")).unwrap().next().is_none());
        assert!(!tmp.path().join("sources").exists());
    }
}
