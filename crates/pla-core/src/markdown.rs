//! How PLA reads Markdown, in one place (architecture review 5.1): frontmatter, fenced code, headings
//! and list items. Blocks, the index, the properties strip and link rewriting all use these, so a
//! note is never read two ways.

/// The index of the first body line after a `---` … `---`/`...` frontmatter, or `None` when the
/// note does not start with one or it never closes (then there is none, FR-EDT-007). The
/// frontmatter's own lines are `lines[1..end - 1]`.
pub fn frontmatter_end(lines: &[&str]) -> Option<usize> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return None;
    }
    lines.iter().enumerate().skip(1).find(|(_, l)| matches!(l.trim_end(), "---" | "...")).map(|(i, _)| i + 1)
}

/// Fenced code blocks, line by line: a block opens with ```` ``` ```` or `~~~` and closes with the
/// same marker.
#[derive(Debug, Default)]
pub struct Fences {
    open: Option<&'static str>,
}

impl Fences {
    /// Whether `line` is code: a fence line or a line inside a fenced block. Call it for every line,
    /// in order.
    pub fn is_code(&mut self, line: &str) -> bool {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (self.open, marker) {
            (None, Some(m)) => self.open = Some(m),
            (Some(open), Some(m)) if open == m => {
                self.open = None;
                return true;
            }
            _ => {}
        }
        self.open.is_some()
    }
}

/// A Markdown heading: one to six `#` and a space (`#etiket` is a tag, not a heading).
pub fn is_heading(trimmed: &str) -> bool {
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ')
}

/// The text after a list marker (`- `, `* `, `+ `, `1. `, `1) `), or `None` when `trimmed` is not a
/// list item.
pub fn list_item_body(trimmed: &str) -> Option<&str> {
    ["- ", "* ", "+ "].iter().find_map(|m| trimmed.strip_prefix(m)).or_else(|| {
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        (digits > 0).then(|| &trimmed[digits..]).and_then(|r| r.strip_prefix(". ").or_else(|| r.strip_prefix(") ")))
    })
}

/// One `[[…]]` in a text: the byte range from its `[[` to past its `]]`, and what is between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WikiLink<'a> {
    pub start: usize,
    pub end: usize,
    pub inner: &'a str,
}

/// The `[[…]]` of `text` in order, as written (no alias, heading or embed handling: the callers
/// read those their own way). An `[[` that never closes ends the scan.
pub fn wikilinks(text: &str) -> impl Iterator<Item = WikiLink<'_>> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let open = at + text[at..].find("[[")?;
        let close = open + 2 + text[open + 2..].find("]]")?;
        at = close + 2;
        Some(WikiLink { start: open, end: close + 2, inner: &text[open + 2..close] })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_ends_at_its_closing_fence() {
        let lines: Vec<&str> = "---\ntags: [a]\n---\nMetin".lines().collect();
        assert_eq!(frontmatter_end(&lines), Some(3));
        assert_eq!(frontmatter_end(&["---", "a: 1", "...", "x"]), Some(3), "YAML's own end marker");
        assert_eq!(frontmatter_end(&["---  ", "---"]), Some(2), "trailing spaces");
        assert_eq!(frontmatter_end(&["---", "başlık yok"]), None, "never closes: no frontmatter (FR-EDT-007)");
        assert_eq!(frontmatter_end(&["Metin", "---"]), None);
        assert_eq!(frontmatter_end(&[]), None);
    }

    #[test]
    fn fences_cover_their_lines_and_close_on_their_own_marker() {
        let mut f = Fences::default();
        let code: Vec<bool> = ["a", "```rust", "x", "~~~", "```", "b", "~~~~", "y", "~~~", "c"].iter().map(|l| f.is_code(l)).collect();
        assert_eq!(code, [false, true, true, true, true, false, true, true, true, false]);
        let mut indented = Fences::default();
        assert!(indented.is_code("   ```"), "indented fences count too");
    }

    #[test]
    fn headings_and_list_items() {
        assert!(is_heading("# Başlık") && is_heading("###### x"));
        assert!(!is_heading("#etiket") && !is_heading("####### x") && !is_heading("#"));
        assert_eq!(list_item_body("- süt"), Some("süt"));
        assert_eq!(list_item_body("12) on iki"), Some("on iki"));
        assert_eq!(list_item_body("3. üç"), Some("üç"));
        assert_eq!(list_item_body("3.14 pi"), None);
        assert_eq!(list_item_body("-süt"), None);
    }

    #[test]
    fn wikilinks_in_order_with_their_ranges() {
        let text = "a [[Not|takma]] b ![[resim.png]] [[açık";
        let found: Vec<(usize, usize, &str)> = wikilinks(text).map(|l| (l.start, l.end, l.inner)).collect();
        assert_eq!(found, [(2, 15, "Not|takma"), (19, 32, "resim.png")]);
        assert_eq!(&text[19..32], "[[resim.png]]");
        assert_eq!(wikilinks("[[]] [[x]]").map(|l| l.inner).collect::<Vec<_>>(), ["", "x"]);
        assert_eq!(wikilinks("düz metin").count(), 0);
    }
}
