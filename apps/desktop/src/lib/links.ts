// Wikilink and search helpers for the editor and the panels (M3); the index itself is in Rust.

/** The target of the `[[…]]` covering `column` in `line` (before `|`), or null. Embeds are skipped. */
export function linkAt(line: string, column: number): string | null {
  const re = /\[\[([^\]\n]*)\]\]/g;
  for (let m = re.exec(line); m; m = re.exec(line)) {
    const start = m.index;
    const end = start + m[0].length;
    if (column < start || column > end) continue;
    if (start > 0 && line[start - 1] === "!") return null;
    const target = m[1].split("|")[0].trim();
    return target === "" ? null : target;
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

/** A search snippet as parts: the matched words sit between U+0002 and U+0003 (never HTML). */
export function snippetParts(snippet: string): { text: string; hit: boolean }[] {
  const parts: { text: string; hit: boolean }[] = [];
  for (const [i, piece] of snippet.split(/[\u0002\u0003]/).entries()) {
    if (piece !== "") parts.push({ text: piece, hit: i % 2 === 1 });
  }
  return parts;
}
