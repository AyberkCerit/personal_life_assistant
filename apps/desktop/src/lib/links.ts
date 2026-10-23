// Wikilink and search helpers for the editor and the panels (M3); the index itself is in Rust.

export interface WikiLink {
  /** Where its `[[` starts. */
  start: number;
  /** Just past its `]]`. */
  end: number;
  /** What is between, as written (alias, heading and embeds are the caller's to read). */
  inner: string;
}

/** The `[[…]]` of `text` in order, each on one line (like pla-core's `markdown::wikilinks`). */
export function wikilinks(text: string): WikiLink[] {
  const out: WikiLink[] = [];
  const re = /\[\[([^\]\n]*)\]\]/g;
  for (let m = re.exec(text); m; m = re.exec(text)) out.push({ start: m.index, end: m.index + m[0].length, inner: m[1] });
  return out;
}

/** The target of the `[[…]]` covering `column` in `line` (before `|`), or null. Embeds are skipped. */
export function linkAt(line: string, column: number): string | null {
  for (const { start, end, inner } of wikilinks(line)) {
    if (column < start || column > end) continue;
    if (start > 0 && line[start - 1] === "!") return null;
    const target = inner.split("|")[0].trim();
    return target === "" || target.startsWith("#") ? null : target; // `[[#Başlık]]`: this note's heading
  }
  return null;
}

/** While typing `[[name`, the name so far and where it starts in the text before the cursor. */
export function completionQuery(before: string): { query: string; offset: number } | null {
  const open = before.lastIndexOf("[[");
  if (open < 0) return null;
  const query = before.slice(open + 2);
  if (/[\]\n]/.test(query)) return null;
  return { query, offset: open + 2 };
}

/**
 * Inside an existing link, the text after the cursor that a picked name replaces (up to `|` or
 * `]]`), and whether the link is already closed.
 */
export function linkRest(after: string): { length: number; closed: boolean } {
  const m = /^([^[\]|\n]*)(\||\]\])/.exec(after);
  return m ? { length: m[1].length, closed: true } : { length: 0, closed: false };
}

/** A note's name from its vault path or file name: the last part, without `.md`. */
export function noteName(path: string): string {
  return (path.split("/").pop() ?? path).replace(/\.md$/i, "");
}

/** A search snippet as parts: the matched words sit between U+0002 and U+0003 (never HTML). */
export function snippetParts(snippet: string): { text: string; hit: boolean }[] {
  const parts: { text: string; hit: boolean }[] = [];
  for (const [i, piece] of snippet.split(/[\u0002\u0003]/).entries()) {
    if (piece !== "") parts.push({ text: piece, hit: i % 2 === 1 });
  }
  return parts;
}
