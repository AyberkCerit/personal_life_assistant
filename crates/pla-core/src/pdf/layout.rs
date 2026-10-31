//! From characters to Markdown, without a model: lines from the characters' boxes, page furniture
//! (running headers and footers, page numbers) left out, headings by font size or the PDF's outline,
//! lists, code blocks from monospaced lines, and paragraphs joined across line and page breaks.
//! A `<!-- page N -->` comment opens each page's content so a summary can cite its page.

use std::collections::{HashMap, HashSet};

/// One character as PDFium reports it. PDF space: x grows right, y grows up, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub ch: char,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub size: f32,
    pub bold: bool,
    pub mono: bool,
    /// Inserted by PDFium (a space or line break the PDF only implies by position).
    pub generated: bool,
}

/// A small drawn shape (a box in PDF space): a list bullet when it sits just left of a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawPage {
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<Glyph>,
    pub marks: Vec<Mark>,
}

/// A bookmark of the PDF's outline: its title, 0-based page and nesting depth (0 = top level).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
    pub title: String,
    pub page: usize,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawDoc {
    pub pages: Vec<RawPage>,
    pub outline: Vec<OutlineEntry>,
}

/// A converted PDF: the note's Markdown and how much text it carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Converted {
    pub markdown: String,
    pub pages: usize,
    /// Characters of text kept (no spaces, no page furniture): near zero for a scanned PDF.
    pub chars: usize,
}

/// Marks a soft hyphen: PDFs write it as U+00AD or as the control character U+0002.
const SOFT_HYPHEN: char = '\u{ad}';
const BULLETS: &[char] = &['•', '◦', '▪', '▫', '‣', '●', '○', '■', '□', '➢', '➤', '►', '▶', '✓', '✔', '·', '⁃', '∙', '◆', '◇', '❖'];
const DASHES: &[char] = &['-', '–', '—', '*', '+'];
const HEADING_RATIO: f32 = 1.15;
const MAX_LEVEL: u8 = 4;

/// A stretch of a line in one style.
#[derive(Debug, Clone, PartialEq)]
struct Run {
    text: String,
    bold: bool,
    mono: bool,
}

#[derive(Debug, Clone)]
struct Line {
    page: usize,
    runs: Vec<Run>,
    x0: f32,
    x1: f32,
    top: f32,
    bottom: f32,
    size: f32,
    /// Median character width (a monospaced line's indentation is measured in it).
    char_w: f32,
    bold: bool,
    mono: bool,
    chars: usize,
    /// A drawn bullet sits just left of the line.
    bulleted: bool,
    /// Where lines of this page's text column end: a line that stops well before it was broken on
    /// purpose (end of a paragraph or list item), not wrapped.
    right: f32,
}

impl Line {
    /// The line ended by choice: its next line's first word would have fitted after it.
    fn broke_before(&self, next: &Line) -> bool {
        let word = next.text().split_whitespace().next().map_or(0, |w| w.chars().count()) as f32;
        self.x1 + 0.3 * next.size + word * next.char_w < self.right
    }

    fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect::<String>().trim().to_owned()
    }
}

fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

fn line_of(page: usize, glyphs: &[Glyph]) -> Option<Line> {
    let ink: Vec<&Glyph> = glyphs.iter().filter(|g| g.ch != ' ').collect();
    if ink.is_empty() {
        return None;
    }
    let letters: Vec<&&Glyph> = ink.iter().filter(|g| g.ch.is_alphanumeric()).collect();
    let bold = !letters.is_empty() && letters.iter().filter(|g| g.bold).count() * 10 >= letters.len() * 6;
    let mono = ink.iter().filter(|g| g.mono).count() * 10 >= ink.len() * 8;
    let mut runs: Vec<Run> = Vec::new();
    for g in glyphs {
        let ch = if g.ch == '\u{2}' { SOFT_HYPHEN } else { g.ch };
        match runs.last_mut() {
            // a space takes the style of the run it follows, so `**a b**` stays one run
            Some(r) if ch == ' ' || (r.bold == g.bold && r.mono == g.mono) => r.text.push(ch),
            _ => runs.push(Run { text: ch.to_string(), bold: g.bold, mono: g.mono }),
        }
    }
    Some(Line {
        page,
        runs,
        x0: ink.iter().map(|g| g.x0).fold(f32::INFINITY, f32::min),
        x1: ink.iter().map(|g| g.x1).fold(f32::NEG_INFINITY, f32::max),
        top: ink.iter().map(|g| g.y1).fold(f32::NEG_INFINITY, f32::max),
        bottom: ink.iter().map(|g| g.y0).fold(f32::INFINITY, f32::min),
        size: median(ink.iter().map(|g| g.size).collect()),
        char_w: median(ink.iter().map(|g| g.x1 - g.x0).collect()),
        bold,
        mono,
        chars: ink.len(),
        bulleted: false,
        right: 0.0,
    })
}

/// A page's lines in drawing order: a new line where the baseline moves or the text jumps back left.
fn lines_of(page: usize, raw: &RawPage) -> Vec<Line> {
    let mut out = Vec::new();
    let mut cur: Vec<Glyph> = Vec::new();
    let mut space = false;
    let flush = |cur: &mut Vec<Glyph>, out: &mut Vec<Line>| {
        out.extend(line_of(page, cur));
        cur.clear();
    };
    for g in &raw.glyphs {
        if g.ch == '\n' || g.ch == '\r' {
            flush(&mut cur, &mut out);
            space = false;
            continue;
        }
        if g.ch.is_whitespace() {
            space = !cur.is_empty();
            continue;
        }
        if g.ch.is_control() && g.ch != '\u{2}' {
            continue;
        }
        if let Some(last) = cur.iter().rev().find(|c| c.ch != ' ') {
            let h = last.size.max(g.size).max(1.0);
            let mid = |c: &Glyph| (c.y0 + c.y1) / 2.0;
            if (mid(g) - mid(last)).abs() > 0.5 * h || g.x1 < last.x0 - 0.5 * h {
                flush(&mut cur, &mut out);
            } else if space || g.x0 - last.x1 > 0.3 * h {
                cur.push(Glyph { ch: ' ', ..*last });
            }
        }
        space = false;
        cur.push(*g);
    }
    flush(&mut cur, &mut out);
    out
}

/// A line's text with its digits blurred, so "Sayfa 3" and "Sayfa 4" read the same.
fn furniture_key(text: &str) -> String {
    text.to_lowercase().chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).filter(|c| !c.is_whitespace()).collect()
}

/// "12", "- 12 -", "Sayfa 12 / 40", "Page 12 of 40".
fn is_page_number(text: &str) -> bool {
    let lower = text.to_lowercase();
    let rest: String = ["sayfa", "page", "of", "/", "-", "–", "|", "(", ")"].iter().fold(lower, |s, w| s.replace(w, " "));
    let rest: String = rest.split_whitespace().collect();
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// Running headers, footers and page numbers: lines in the top or bottom band of a page that are a
/// page number, or that come back (digits aside) on at least half the pages of a 3+ page PDF.
fn furniture(pages: &[RawPage], lines: &[Vec<Line>]) -> HashSet<(usize, usize)> {
    let in_band = |p: &RawPage, l: &Line| p.height > 0.0 && (l.bottom > p.height * 0.88 || l.top < p.height * 0.12);
    let mut seen: HashMap<String, HashSet<usize>> = HashMap::new();
    for (p, page_lines) in lines.iter().enumerate() {
        for l in page_lines.iter().filter(|l| in_band(&pages[p], l)) {
            seen.entry(furniture_key(&l.text())).or_default().insert(p);
        }
    }
    let n = pages.len();
    let mut out = HashSet::new();
    for (p, page_lines) in lines.iter().enumerate() {
        for (i, l) in page_lines.iter().enumerate() {
            if !in_band(&pages[p], l) {
                continue;
            }
            let repeated = n >= 3 && seen.get(&furniture_key(&l.text())).is_some_and(|s| s.len() * 2 >= n);
            if repeated || is_page_number(&l.text()) {
                out.insert((p, i));
            }
        }
    }
    out
}

fn round_half(x: f32) -> i32 {
    (x * 2.0).round() as i32
}

/// The body text's size: the one most characters are set in (code left out).
fn body_size(lines: &[&Line]) -> f32 {
    let mut weight: HashMap<i32, usize> = HashMap::new();
    for l in lines.iter().filter(|l| !l.mono) {
        *weight.entry(round_half(l.size)).or_default() += l.chars;
    }
    weight.into_iter().max_by_key(|&(size, w)| (w, -size)).map_or(10.0, |(size, _)| size as f32 / 2.0)
}

fn normalized(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A list item's Markdown marker and the text after the PDF's own marker.
fn list_marker(text: &str) -> Option<(String, String)> {
    let mut chars = text.chars();
    let first = chars.next()?;
    let rest: String = chars.collect();
    let private_use = ('\u{e000}'..='\u{f8ff}').contains(&first);
    if BULLETS.contains(&first) || private_use {
        let rest = rest.trim();
        return (!rest.is_empty()).then(|| ("-".to_owned(), rest.to_owned()));
    }
    if DASHES.contains(&first) && rest.starts_with(' ') {
        let rest = rest.trim();
        return (!rest.is_empty()).then(|| ("-".to_owned(), rest.to_owned()));
    }
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    if (1..=2).contains(&digits.len()) {
        let after = &text[digits.len()..];
        for punct in ['.', ')'] {
            if let Some(rest) = after.strip_prefix(punct).filter(|r| r.starts_with(' ')) {
                let rest = rest.trim();
                return (!rest.is_empty()).then(|| (format!("{digits}{punct}"), rest.to_owned()));
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Heading(u8),
    Item,
    Code,
    Body,
}

/// Heading levels: the sizes clearly above the body text, largest first, then whole-line bold.
/// A line that matches an outline entry on its page takes that entry's depth instead.
fn classify(lines: &[&Line], body: f32, outline: &[OutlineEntry]) -> Vec<Kind> {
    let large = |l: &Line| !l.mono && l.size >= body * HEADING_RATIO && l.chars <= 150 && l.text().chars().any(char::is_alphabetic);
    // large text that runs on for more than three lines or 150 characters is a lead paragraph
    let mut long_run = vec![false; lines.len()];
    let mut start = 0;
    while start < lines.len() {
        let mut end = start + 1;
        while end < lines.len()
            && large(lines[start])
            && large(lines[end])
            && lines[end].page == lines[start].page
            && round_half(lines[end].size) == round_half(lines[start].size)
        {
            end += 1;
        }
        let chars: usize = lines[start..end].iter().map(|l| l.chars).sum();
        if end - start > 3 || chars > 150 {
            long_run[start..end].fill(true);
        }
        start = end;
    }
    let is_size_heading = |i: usize| large(lines[i]) && !long_run[i];
    let mut sizes: Vec<i32> = (0..lines.len()).filter(|&i| is_size_heading(i)).map(|i| round_half(lines[i].size)).collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    let level_of = |size: f32| sizes.iter().position(|&s| s == round_half(size)).map_or(MAX_LEVEL, |i| (i as u8 + 1).min(MAX_LEVEL));
    let bold_level = (sizes.len() as u8 + 1).min(MAX_LEVEL);
    let titles: HashMap<(usize, String), u8> = outline.iter().map(|e| ((e.page, normalized(&e.title)), (e.depth as u8 + 1).min(MAX_LEVEL))).collect();
    let mut kinds: Vec<Kind> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let text = l.text();
            if l.mono {
                return Kind::Code;
            }
            if let Some(&level) = titles.get(&(l.page, normalized(&text))) {
                return Kind::Heading(level);
            }
            if is_size_heading(i) {
                return Kind::Heading(level_of(l.size));
            }
            if l.bulleted || list_marker(&text).is_some() {
                return Kind::Item;
            }
            let next_bold = lines.get(i + 1).is_some_and(|n| n.page == l.page && n.bold && !n.mono);
            let ends = text.ends_with(['.', ',', ';']);
            if l.bold && l.size >= body * 0.95 && l.chars <= 80 && !ends && !next_bold && text.chars().any(char::is_alphabetic) {
                return Kind::Heading(bold_level);
            }
            Kind::Body
        })
        .collect();
    // an outline title set over two lines
    for i in 0..lines.len().saturating_sub(1) {
        let (a, b) = (lines[i], lines[i + 1]);
        if a.page == b.page {
            if let Some(&level) = titles.get(&(a.page, normalized(&format!("{} {}", a.text(), b.text())))) {
                kinds[i] = Kind::Heading(level);
                kinds[i + 1] = Kind::Heading(level);
            }
        }
    }
    kinds
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = ' ';
    for c in text.chars() {
        match c {
            // `#` would make a tag, `<` an HTML tag, `[[` a link, `*` and `` ` `` emphasis or code
            '#' | '<' | '*' | '`' => out.push('\\'),
            '[' if prev == '[' => out.push('\\'),
            _ => {}
        }
        out.push(c);
        prev = c;
    }
    out
}

fn code_span(text: &str) -> String {
    let fence = if text.contains('`') { "``" } else { "`" };
    let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// A line's text as Markdown: bold and monospaced stretches marked unless the whole line is so.
fn inline(line: &Line) -> String {
    let mut out = String::new();
    for r in &line.runs {
        let core = r.text.trim();
        if core.is_empty() {
            out.push_str(&r.text);
            continue;
        }
        let lead = &r.text[..r.text.len() - r.text.trim_start().len()];
        let trail = &r.text[r.text.trim_end().len()..];
        out.push_str(lead);
        if r.mono && !line.mono {
            out.push_str(&code_span(core));
        } else if r.bold && !line.bold && core.chars().any(char::is_alphanumeric) {
            out.push_str(&format!("**{}**", escape(core)));
        } else {
            out.push_str(&escape(core));
        }
        out.push_str(trail);
    }
    out.trim().replace(SOFT_HYPHEN, "")
}

/// A line's text, with the soft hyphen that ended it (if any) still in place for `join`.
fn inline_keeping_hyphen(line: &Line) -> String {
    let text = inline(line);
    if line.runs.last().is_some_and(|r| r.text.trim_end().ends_with(SOFT_HYPHEN)) {
        format!("{text}{SOFT_HYPHEN}")
    } else {
        text
    }
}

/// Joins a wrapped line to the text before it: a word hyphenated at the line end is mended.
fn join(text: &mut String, next: &str) {
    let starts_lower = next.chars().next().is_some_and(char::is_lowercase);
    let hyphenated = starts_lower && text.ends_with('-') && text.chars().rev().nth(1).is_some_and(char::is_alphabetic);
    if text.ends_with(SOFT_HYPHEN) || hyphenated {
        text.pop();
    } else {
        text.push(' ');
    }
    text.push_str(next);
}

#[derive(Debug, Clone)]
enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Para {
        text: String,
        x0: f32,
    },
    /// `hang`: a wrapped line of the item starts right of this x.
    Item {
        depth: usize,
        marker: String,
        text: String,
        hang: f32,
    },
    /// Each line: x of its first character, character width, blank lines before it, text.
    Code {
        lines: Vec<(f32, f32, usize, String)>,
    },
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end_matches(['*', '`', ')', '"', '\'', '”', '’']).ends_with(['.', '!', '?', ':'])
}

/// Lines to blocks, in order. `prev` is the line before `l` (none at the start).
fn blocks(lines: &[&Line], kinds: &[Kind]) -> Vec<(usize, Block)> {
    let mut out: Vec<(usize, Block)> = Vec::new();
    let mut indents: Vec<f32> = Vec::new();
    let mut prev: Option<(&Line, Kind)> = None;
    for (l, &kind) in lines.iter().copied().zip(kinds) {
        let same_page = prev.is_some_and(|(p, _)| p.page == l.page);
        let gap = prev.map_or(f32::INFINITY, |(p, _)| if same_page { p.bottom - l.top } else { f32::INFINITY });
        // just below the line before (a jump up is the top of the next column)
        let close = gap > -0.5 * l.size && gap < 0.7 * l.size;
        if !matches!(kind, Kind::Item) && !(matches!(kind, Kind::Body) && matches!(out.last(), Some((_, Block::Item { .. })))) {
            indents.clear();
        }
        let last = out.last_mut().map(|(_, b)| b);
        match (kind, last) {
            (Kind::Heading(level), Some(Block::Heading { level: l0, text })) if *l0 == level && close => {
                text.push(' ');
                text.push_str(&escape(&l.text()));
            }
            (Kind::Heading(level), _) => out.push((l.page, Block::Heading { level, text: escape(&l.text()) })),
            (Kind::Code, Some(Block::Code { lines })) if gap < 3.0 * l.size || (!same_page && prev.is_some_and(|(_, k)| k == Kind::Code)) => {
                let blank = if same_page { ((gap / (l.size * 1.2)).round() as usize).min(2) } else { 0 };
                lines.push((l.x0, l.char_w, blank, l.text()));
            }
            (Kind::Code, _) => out.push((l.page, Block::Code { lines: vec![(l.x0, l.char_w, 0, l.text())] })),
            (Kind::Item, _) => {
                let (marker, text) = match list_marker(&l.text()) {
                    // the item's own text, styled: the PDF's marker dropped
                    Some((marker, _)) => {
                        let full = inline_keeping_hyphen(l);
                        let text = if l.text().chars().nth(1).is_some_and(char::is_whitespace) || marker != "-" {
                            full.split_once(' ').map_or(String::new(), |(_, t)| t.trim().to_owned())
                        } else {
                            full.trim_start_matches('\\').chars().skip(1).collect::<String>().trim().to_owned()
                        };
                        (marker, text)
                    }
                    None => ("-".to_owned(), inline_keeping_hyphen(l)),
                };
                while indents.last().is_some_and(|&x| l.x0 < x - 0.5 * l.size) {
                    indents.pop();
                }
                match indents.last() {
                    // a list continued in the next column is not nested deeper
                    Some(&x) if l.x0 > x + 4.0 * l.size => indents = vec![l.x0],
                    Some(&x) if l.x0 <= x + 0.5 * l.size => {}
                    _ => indents.push(l.x0),
                }
                // a written marker starts the line, its wrapped lines start further right; a drawn
                // bullet stands left of the line, which its wrapped lines line up with
                let hang = if l.bulleted { l.x0 - 0.3 * l.size } else { l.x0 + 0.3 * l.size };
                out.push((l.page, Block::Item { depth: indents.len() - 1, marker, text, hang }));
            }
            (Kind::Body, Some(Block::Item { text, hang, .. })) if close && l.x0 > *hang && !prev.is_some_and(|(p, _)| p.broke_before(l)) => {
                join(text, &inline_keeping_hyphen(l))
            }
            (Kind::Body, Some(Block::Para { text, x0 })) => {
                let continues = if same_page {
                    let indented = l.x0 > *x0 + l.size;
                    let broke = prev.is_some_and(|(p, _)| p.broke_before(l));
                    let size_change = prev.is_some_and(|(p, _)| (p.size - l.size).abs() > 0.1 * l.size);
                    close && !indented && !broke && !size_change
                } else {
                    !ends_sentence(text) && l.text().chars().next().is_some_and(char::is_lowercase)
                };
                if continues {
                    join(text, &inline_keeping_hyphen(l));
                    *x0 = x0.min(l.x0);
                } else {
                    out.push((l.page, Block::Para { text: inline_keeping_hyphen(l), x0: l.x0 }));
                }
            }
            (Kind::Body, _) => out.push((l.page, Block::Para { text: inline_keeping_hyphen(l), x0: l.x0 })),
        }
        prev = Some((l, kind));
    }
    out
}

fn render_code(lines: &[(f32, f32, usize, String)]) -> String {
    let left = lines.iter().map(|l| l.0).fold(f32::INFINITY, f32::min);
    let body: Vec<String> = lines
        .iter()
        .flat_map(|(x0, w, blank, text)| {
            let indent = if *w > 0.0 { ((x0 - left) / w).round() as usize } else { 0 };
            std::iter::repeat_n(String::new(), *blank).chain(std::iter::once(format!("{}{}", " ".repeat(indent), text)))
        })
        .collect();
    let body = body.join("\n");
    let fence = if body.contains("```") { "~~~" } else { "```" };
    format!("{fence}\n{body}\n{fence}")
}

/// A paragraph that would read as Markdown syntax at its start (`- `, `1. `, `> `) is escaped.
fn guard_start(text: &str) -> String {
    let risky = text.starts_with(['-', '+', '>', '=']) || list_marker(text).is_some_and(|(m, _)| m != "-");
    if risky {
        format!("\\{text}")
    } else {
        text.to_owned()
    }
}

/// Marks the lines with a drawn bullet, and gives each line its text column's right edge: where the
/// nearby lines that start where it starts end, be it a page's column or a box's. Furniture left out.
fn place(pages: &[RawPage], lines: &mut [Vec<Line>], skip: &HashSet<(usize, usize)>) {
    for (p, page_lines) in lines.iter_mut().enumerate() {
        let edges: Vec<f32> = page_lines
            .iter()
            .map(|l| {
                let near = |(i, o): &(usize, &Line)| {
                    !skip.contains(&(p, *i))
                        && !o.mono
                        && (o.size - l.size).abs() < 0.1 * l.size
                        && (o.x0 - l.x0).abs() < l.size
                        && (o.top - l.top).abs() < 12.0 * l.size
                };
                page_lines.iter().enumerate().filter(near).map(|(_, o)| o.x1).fold(l.x1, f32::max)
            })
            .collect();
        for (l, edge) in page_lines.iter_mut().zip(edges) {
            l.right = edge;
            l.bulleted = pages[p].marks.iter().any(|m| {
                let mid = (m.y0 + m.y1) / 2.0;
                m.x1 <= l.x0 + 1.0 && m.x1 >= l.x0 - 2.5 * l.size && mid > l.bottom && mid < l.top
            });
        }
    }
}

/// The PDF as a Markdown note. `source` is the PDF's file name, linked from the front matter.
pub fn to_markdown(doc: &RawDoc, source: &str) -> Converted {
    let mut per_page: Vec<Vec<Line>> = doc.pages.iter().enumerate().map(|(i, p)| lines_of(i, p)).collect();
    let skip = furniture(&doc.pages, &per_page);
    place(&doc.pages, &mut per_page, &skip);
    let skip = &skip;
    let lines: Vec<&Line> =
        per_page.iter().enumerate().flat_map(|(p, ls)| ls.iter().enumerate().filter(move |(i, _)| !skip.contains(&(p, *i))).map(|(_, l)| l)).collect();
    let body = body_size(&lines);
    let kinds = classify(&lines, body, &doc.outline);
    let source = source.replace(['"', '\n', '\r'], "");
    let mut md = format!("---\npla_generated: true\nsource: \"[[{source}]]\"\npages: {}\n---\n", doc.pages.len());
    let mut page = None;
    // the list the last block belonged to (bulleted or numbered), none after other blocks
    let mut prev_list: Option<bool> = None;
    for (start, block) in blocks(&lines, &kinds) {
        let new_page = page != Some(start);
        if new_page {
            md.push_str(&format!("\n<!-- page {} -->\n", start + 1));
            page = Some(start);
        }
        // items of one list stay together; top-level bullets and numbers are two lists
        let list = match &block {
            Block::Item { depth: 0, marker, .. } => Some(marker == "-"),
            Block::Item { .. } => prev_list,
            _ => None,
        };
        if new_page || list.is_none() || list != prev_list {
            md.push('\n');
        }
        prev_list = list;
        match block {
            Block::Heading { level, text } => md.push_str(&format!("{} {}\n", "#".repeat(level as usize), text)),
            Block::Para { text, .. } => md.push_str(&format!("{}\n", guard_start(&text.replace(SOFT_HYPHEN, "")))),
            Block::Item { depth, marker, text, .. } => md.push_str(&format!("{}{} {}\n", "    ".repeat(depth), marker, text.replace(SOFT_HYPHEN, ""))),
            Block::Code { lines } => md.push_str(&format!("{}\n", render_code(&lines))),
        }
    }
    Converted { markdown: md, pages: doc.pages.len(), chars: lines.iter().map(|l| l.chars).sum() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 595.0;
    const H: f32 = 842.0;

    #[derive(Clone, Copy, Default)]
    struct Style {
        bold: bool,
        mono: bool,
    }

    /// One line of glyphs, each half its size wide, followed by PDFium's generated line break.
    fn line(out: &mut Vec<Glyph>, text: &str, x: f32, y: f32, size: f32, style: Style) {
        let w = size * 0.5;
        for (i, ch) in text.chars().enumerate() {
            let x0 = x + i as f32 * w;
            out.push(Glyph { ch, x0, x1: x0 + w, y0: y, y1: y + size * 1.2, size, bold: style.bold, mono: style.mono, generated: false });
        }
        out.push(Glyph { ch: '\n', x0: 0.0, x1: 0.0, y0: y, y1: y, size, bold: false, mono: false, generated: true });
    }

    /// Lines of one page, top to bottom: (text, x, size, style), each `size * 1.4` below the last
    /// unless the text is empty, which leaves a paragraph gap.
    fn page(rows: &[(&str, f32, f32, Style)]) -> RawPage {
        let mut glyphs = Vec::new();
        let mut y = 780.0;
        for &(text, x, size, style) in rows {
            if text.is_empty() {
                y -= size * 1.2;
                continue;
            }
            line(&mut glyphs, text, x, y, size, style);
            y -= size * 1.4;
        }
        RawPage { width: W, height: H, glyphs, marks: Vec::new() }
    }

    fn body(text: &str) -> (&str, f32, f32, Style) {
        (text, 50.0, 10.0, Style::default())
    }

    fn md(pages: Vec<RawPage>) -> String {
        to_markdown(&RawDoc { pages, outline: Vec::new() }, "ders.pdf").markdown
    }

    /// The Markdown without its front matter and page comments.
    fn content(md: &str) -> String {
        let body = md.splitn(3, "---\n").nth(2).unwrap_or(md);
        body.lines().filter(|l| !l.starts_with("<!--")).collect::<Vec<_>>().join("\n").trim().to_owned()
    }

    #[test]
    fn front_matter_marks_the_note_as_generated_and_links_the_pdf() {
        let out = md(vec![page(&[body("Merhaba.")])]);
        assert!(out.starts_with("---\npla_generated: true\nsource: \"[[ders.pdf]]\"\npages: 1\n---\n"), "{out}");
        assert!(crate::notes::is_generated(&out));
        assert!(out.contains("<!-- page 1 -->"));
    }

    #[test]
    fn headings_by_size_and_wrapped_lines_joined_into_paragraphs() {
        let p = page(&[
            ("Nesneler", 50.0, 22.0, Style::default()),
            ("Kapsülleme", 50.0, 15.0, Style::default()),
            body("Kapsülleme, bir nesnenin iç durumunu gizleyip yalnız metot-"),
            body("larla erişime izin vermektir ve tutarlılığı korur. Nesnenin"),
            body("alanları dışarıdan değişmez."),
            ("", 0.0, 10.0, Style::default()),
            body("İkinci paragraf burada başlar."),
        ]);
        assert_eq!(
            content(&md(vec![p])),
            "# Nesneler\n\n## Kapsülleme\n\nKapsülleme, bir nesnenin iç durumunu gizleyip yalnız metotlarla erişime izin vermektir ve \
             tutarlılığı korur. Nesnenin alanları dışarıdan değişmez.\n\nİkinci paragraf burada başlar."
        );
    }

    #[test]
    fn a_line_that_stops_short_ends_its_paragraph() {
        // the next line's first word would have fitted: the PDF broke the line on purpose
        let p = page(&[body("Birinci satır kısa."), body("İkinci satır ayrı bir paragraf.")]);
        assert_eq!(content(&md(vec![p])), "Birinci satır kısa.\n\nİkinci satır ayrı bir paragraf.");
    }

    #[test]
    fn running_headers_footers_and_page_numbers_are_left_out() {
        let pages: Vec<RawPage> = (1..=3)
            .map(|n| {
                let mut glyphs = Vec::new();
                line(&mut glyphs, "OOP Ders Notları", 50.0, 815.0, 8.0, Style::default());
                line(&mut glyphs, &format!("Sayfa {n} sayfanın metni."), 50.0, 600.0, 10.0, Style::default());
                line(&mut glyphs, &format!("Sayfa {n} / 3"), 270.0, 20.0, 8.0, Style::default());
                RawPage { width: W, height: H, glyphs, marks: Vec::new() }
            })
            .collect();
        let out = md(pages);
        assert!(!out.contains("Ders Notları") && !out.contains("/ 3"), "{out}");
        for n in 1..=3 {
            assert!(out.contains(&format!("<!-- page {n} -->\n\nSayfa {n} sayfanın metni.")), "{out}");
        }
    }

    #[test]
    fn page_numbers_are_recognised() {
        for t in ["12", "- 12 -", "Sayfa 3 / 40", "Page 3 of 9", "(7)"] {
            assert!(is_page_number(t), "{t}");
        }
        for t in ["2026 yılı", "Sayfa", "3.2 Kalıtım", ""] {
            assert!(!is_page_number(t), "{t}");
        }
    }

    #[test]
    fn written_bullets_and_numbers_make_nested_lists() {
        let p = page(&[
            body("• Veri gizleme: alanlar dışarıdan görünmez ve yalnız sınıfın"),
            ("kendi metotları onlara dokunabilir.", 60.0, 10.0, Style::default()),
            ("◦ getter okumayı denetler", 70.0, 10.0, Style::default()),
            ("◦ setter yazmayı denetler", 70.0, 10.0, Style::default()),
            body("• Değişime dayanıklılık"),
            ("", 0.0, 10.0, Style::default()),
            body("1. Alt sınıf üst sınıfın üyelerini kullanır."),
            body("2) super üst sınıfı çağırır."),
        ]);
        assert_eq!(
            content(&md(vec![p])),
            "- Veri gizleme: alanlar dışarıdan görünmez ve yalnız sınıfın kendi metotları onlara dokunabilir.\n    \
             - getter okumayı denetler\n    - setter yazmayı denetler\n- Değişime dayanıklılık\n\n\
             1. Alt sınıf üst sınıfın üyelerini kullanır.\n2) super üst sınıfı çağırır."
        );
    }

    #[test]
    fn drawn_bullets_make_list_items() {
        let mut p = page(&[body("Aynı çağrı, farklı davranış"), body("Dinamik bağlama")]);
        // a 4pt dot left of each line, centred on it
        p.marks = vec![Mark { x0: 38.0, x1: 42.0, y0: 784.0, y1: 788.0 }, Mark { x0: 38.0, x1: 42.0, y0: 770.0, y1: 774.0 }];
        assert_eq!(content(&md(vec![p])), "- Aynı çağrı, farklı davranış\n- Dinamik bağlama");
    }

    #[test]
    fn list_markers_are_told_from_text() {
        assert_eq!(list_marker("• madde"), Some(("-".into(), "madde".into())));
        assert_eq!(list_marker("\u{f0b7} madde"), Some(("-".into(), "madde".into())), "a Symbol-font bullet");
        assert_eq!(list_marker("- madde"), Some(("-".into(), "madde".into())));
        assert_eq!(list_marker("12. madde"), Some(("12.".into(), "madde".into())));
        assert_eq!(list_marker("a) madde"), None);
        assert_eq!(list_marker("-5 derece"), None);
        assert_eq!(list_marker("2026. yılda"), None);
        assert_eq!(list_marker("3.2 Kalıtım"), None);
    }

    #[test]
    fn code_keeps_its_indentation_and_inline_styles_are_marked() {
        let mono = Style { mono: true, bold: false };
        let p = page(&[
            ("class Kedi extends Hayvan {", 50.0, 10.0, mono),
            ("void ses() {", 70.0, 10.0, mono),
            ("}", 50.0, 10.0, mono),
            ("", 0.0, 10.0, Style::default()),
        ]);
        let mut glyphs = p.glyphs;
        // "Örnek: new Kedi() bir nesnedir." with the code in a monospaced font and "nesnedir" bold
        let mut x = 50.0;
        for (text, style) in [
            ("Örnek: ", Style::default()),
            ("new Kedi()", mono),
            (" bir ", Style::default()),
            ("nesnedir", Style { bold: true, mono: false }),
            (".", Style::default()),
        ] {
            for ch in text.chars() {
                glyphs.push(Glyph { ch, x0: x, x1: x + 5.0, y0: 700.0, y1: 712.0, size: 10.0, bold: style.bold, mono: style.mono, generated: false });
                x += 5.0;
            }
        }
        let out = content(&md(vec![RawPage { width: W, height: H, glyphs, marks: Vec::new() }]));
        assert_eq!(out, "```\nclass Kedi extends Hayvan {\n    void ses() {\n}\n```\n\nÖrnek: `new Kedi()` bir **nesnedir**.");
    }

    #[test]
    fn the_outline_names_headings_set_in_body_text() {
        let doc = RawDoc {
            pages: vec![page(&[body("Giriş"), body("Bu bölüm konuya girer.")])],
            outline: vec![OutlineEntry { title: "Giriş".into(), page: 0, depth: 1 }],
        };
        assert_eq!(content(&to_markdown(&doc, "x.pdf").markdown), "## Giriş\n\nBu bölüm konuya girer.");
    }

    #[test]
    fn a_long_run_of_large_text_is_a_paragraph_not_a_heading() {
        let lead = |t| (t, 50.0, 13.0, Style::default());
        let p = page(&[
            lead("Bu belge projenin amacını, yöntemini ve iş planını"),
            lead("ayrıntılı olarak anlatır; başvuru metni ayrı bir belgede"),
            lead("yer alır ve ekibin tüm üyeleri için hazırlanmıştır."),
            ("", 0.0, 10.0, Style::default()),
            body("Gövde metni burada başlar ve olağan boyuttadır."),
        ]);
        let out = content(&md(vec![p]));
        assert!(!out.contains('#'), "{out}");
    }

    #[test]
    fn markdown_syntax_in_the_text_is_escaped() {
        let p = page(&[body("#include <stdio.h> ve [[bağlantı]] ile *yıldız* kalır.")]);
        assert_eq!(content(&md(vec![p])), "\\#include \\<stdio.h> ve [\\[bağlantı]] ile \\*yıldız\\* kalır.");
        assert_eq!(guard_start("> 5 puan"), "\\> 5 puan");
        assert_eq!(guard_start("3. sırada"), "\\3. sırada");
    }

    #[test]
    fn a_paragraph_runs_on_across_a_page_break() {
        let p1 = page(&[body("Kalıtım bir sınıfın başka bir sınıfın üyelerini alması ve")]);
        let p2 = page(&[body("onları genişletmesidir."), ("", 0.0, 10.0, Style::default()), body("Yeni paragraf.")]);
        let out = md(vec![p1, p2]);
        assert!(out.contains("üyelerini alması ve onları genişletmesidir."), "{out}");
        assert!(out.contains("<!-- page 2 -->\n\nYeni paragraf."), "{out}");
    }

    #[test]
    fn a_page_without_text_gives_no_characters() {
        let c = to_markdown(&RawDoc { pages: vec![RawPage { width: W, height: H, ..RawPage::default() }], outline: Vec::new() }, "tarama.pdf");
        assert_eq!((c.pages, c.chars), (1, 0));
    }
}
