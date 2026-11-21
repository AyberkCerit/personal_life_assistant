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

/// How many bookmarks of one PDF are looked at. PDFium leaves a bookmark that names itself (or an
/// earlier one) as its next or its child to the caller, and such an outline has no end.
const OUTLINE_NODES: usize = 20_000;
/// How many of them become outline entries, and how deep they nest: real outlines are a few levels.
const OUTLINE_ENTRIES: usize = 5000;
const OUTLINE_DEPTH: usize = 16;

/// A bookmark as the outline walk sees it.
trait Bookmark: Sized {
    /// Its title and 0-based page, when it has both.
    fn entry(&self) -> Option<(String, usize)>;
    fn child(&self) -> Option<Self>;
    fn sibling(&self) -> Option<Self>;
}

impl Bookmark for PdfBookmark<'_> {
    fn entry(&self) -> Option<(String, usize)> {
        let page = self.destination().and_then(|d| d.page_index().ok())?;
        Some((self.title()?, page as usize))
    }

    fn child(&self) -> Option<Self> {
        self.first_child()
    }

    fn sibling(&self) -> Option<Self> {
        self.next_sibling()
    }
}

/// `first`, its siblings and everything under them, in document order; `left` is how many more
/// bookmarks may be looked at.
fn walk<B: Bookmark>(first: B, depth: usize, left: &mut usize, out: &mut Vec<OutlineEntry>) {
    if depth > OUTLINE_DEPTH {
        return;
    }
    let mut next = Some(first);
    while let Some(b) = next {
        if *left == 0 || out.len() >= OUTLINE_ENTRIES {
            return;
        }
        *left -= 1;
        if let Some((title, page)) = b.entry() {
            out.push(OutlineEntry { title, page, depth });
        }
        if let Some(child) = b.child() {
            walk(child, depth + 1, left, out);
        }
        next = b.sibling();
    }
}

fn outline(doc: &PdfDocument<'_>) -> Vec<OutlineEntry> {
    let mut out = Vec::new();
    if let Some(root) = doc.bookmarks().root() {
        let mut left = OUTLINE_NODES;
        walk(root, 0, &mut left, &mut out);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A bookmark of a made-up outline: `(title, child, sibling)` by index.
    #[derive(Clone, Copy)]
    struct Fake<'a>(&'a [(Option<&'a str>, Option<usize>, Option<usize>)], usize);

    impl Bookmark for Fake<'_> {
        fn entry(&self) -> Option<(String, usize)> {
            Some((self.0[self.1].0?.to_string(), self.1))
        }

        fn child(&self) -> Option<Self> {
            self.0[self.1].1.map(|i| Fake(self.0, i))
        }

        fn sibling(&self) -> Option<Self> {
            self.0[self.1].2.map(|i| Fake(self.0, i))
        }
    }

    fn titles(nodes: &[(Option<&str>, Option<usize>, Option<usize>)]) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        let mut left = OUTLINE_NODES;
        walk(Fake(nodes, 0), 0, &mut left, &mut out);
        out.into_iter().map(|e| (e.title, e.depth)).collect()
    }

    #[test]
    fn an_outline_is_read_in_document_order_with_its_depths() {
        let nodes = [(Some("1"), Some(1), Some(3)), (Some("1.1"), None, Some(2)), (None, None, None), (Some("2"), None, None)];
        assert_eq!(titles(&nodes), [("1".to_string(), 0), ("1.1".to_string(), 1), ("2".to_string(), 0)]);
    }

    #[test]
    fn an_outline_that_loops_ends() {
        // code review 2026-11-14: only the depth was limited, and a bookmark whose next is itself never ended
        assert_eq!(titles(&[(Some("a"), None, Some(0))]).len(), OUTLINE_ENTRIES);
        // without a title nothing is collected, so the count of entries cannot end it either
        assert!(titles(&[(None, None, Some(0))]).is_empty());
        assert!(titles(&[(None, Some(1), None), (None, None, Some(0))]).is_empty());
        // a bookmark that is its own child: the depth ends each branch, the budget the whole
        assert!(titles(&[(Some("a"), Some(0), Some(0))]).len() <= OUTLINE_ENTRIES);
    }
}
