//! What PDFium sees on each page: every character with its box, size and font traits, in the
//! order the PDF draws them, and the PDF's outline (bookmarks) with their pages and depths.

use std::path::Path;
use std::sync::OnceLock;

use pdfium_render::prelude::*;

use super::layout::{Glyph, Mark, OutlineEntry, RawDoc, RawPage};
use super::PdfError;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

/// PDFium from `lib_dir`, bound once per process (PDFium keeps its bindings global).
pub fn pdfium(lib_dir: &Path) -> Result<&'static Pdfium, PdfError> {
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }
    let lib = Pdfium::pdfium_platform_library_name_at_path(lib_dir);
    if !lib.is_file() {
        return Err(PdfError::LibraryMissing(lib));
    }
    let bindings = Pdfium::bind_to_library(&lib).map_err(|e| PdfError::Library(e.to_string()))?;
    Ok(PDFIUM.get_or_init(|| Pdfium::new(bindings)))
}

fn open_error(e: PdfiumError) -> PdfError {
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => PdfError::Encrypted,
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError | PdfiumInternalError::FileError) => PdfError::Damaged,
        e => PdfError::Library(e.to_string()),
    }
}

fn is_mono(name: &str) -> bool {
    let n = name.to_lowercase();
    ["mono", "courier", "consolas", "menlo", "inconsolata", "fixed", "typewriter", "lucidaconsole", "cmtt", "sourcecodepro"].iter().any(|m| n.contains(m))
}

fn is_bold(c: &PdfPageTextChar<'_>) -> bool {
    let heavy = matches!(c.font_weight(), Some(PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold | PdfFontWeight::Weight800 | PdfFontWeight::Weight900));
    heavy || c.font_is_bold_reenforced() || {
        let n = c.font_name().to_lowercase();
        n.contains("bold") || n.contains("heavy") || n.contains("black") || n.contains("semibold")
    }
}

fn page(p: &PdfPage<'_>) -> Result<RawPage, PdfError> {
    let text = p.text().map_err(open_error)?;
    let mut glyphs = Vec::new();
    for c in text.chars().iter() {
        let Some(ch) = c.unicode_char() else { continue };
        let generated = c.is_generated().unwrap_or(false);
        let Ok(b) = c.loose_bounds() else { continue };
        let font = c.font_name();
        glyphs.push(Glyph {
            ch,
            x0: b.left().value,
            x1: b.right().value,
            y0: b.bottom().value,
            y1: b.top().value,
            size: c.scaled_font_size().value,
            bold: is_bold(&c),
            mono: c.font_is_fixed_pitch() || is_mono(&font),
            generated,
        });
    }
    Ok(RawPage { width: p.width().value, height: p.height().value, glyphs, marks: marks(p) })
}

/// Small square-ish drawn shapes: the bullets of lists whose markers are drawn, not written.
fn marks(p: &PdfPage<'_>) -> Vec<Mark> {
    let mut out = Vec::new();
    for o in p.objects().iter() {
        if o.object_type() != PdfPageObjectType::Path {
            continue;
        }
        let Ok(r) = o.bounds().map(|q| q.to_rect()) else { continue };
        let (w, h) = (r.width().value, r.height().value);
        if (0.5..12.0).contains(&w) && (0.5..12.0).contains(&h) && (w / h - 1.0).abs() < 0.5 {
            out.push(Mark { x0: r.left().value, x1: r.right().value, y0: r.bottom().value, y1: r.top().value });
        }
    }
    out
}

fn outline(doc: &PdfDocument<'_>) -> Vec<OutlineEntry> {
    fn walk(b: PdfBookmark<'_>, depth: usize, out: &mut Vec<OutlineEntry>) {
        // a hostile PDF can nest without end; real outlines are a few levels deep
        if depth > 16 || out.len() > 5000 {
            return;
        }
        let mut next = Some(b);
        while let Some(b) = next {
            let page = b.destination().and_then(|d| d.page_index().ok());
            if let (Some(title), Some(page)) = (b.title(), page) {
                out.push(OutlineEntry { title, page: page as usize, depth });
            }
            if let Some(child) = b.first_child() {
                walk(child, depth + 1, out);
            }
            next = b.next_sibling();
        }
    }
    let mut out = Vec::new();
    if let Some(root) = doc.bookmarks().root() {
        walk(root, 0, &mut out);
    }
    out
}

/// Reads every page of the PDF in `bytes`.
pub fn read(pdfium: &Pdfium, bytes: &[u8]) -> Result<RawDoc, PdfError> {
    let doc = pdfium.load_pdf_from_byte_slice(bytes, None).map_err(open_error)?;
    let mut pages = Vec::new();
    for p in doc.pages().iter() {
        pages.push(page(&p)?);
    }
    Ok(RawDoc { pages, outline: outline(&doc) })
}
