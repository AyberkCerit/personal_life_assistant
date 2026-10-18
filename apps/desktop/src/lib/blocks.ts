/** Line types the editor offers without Markdown: what a line is, and turning it into another. */

export type BlockKind = "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "bullet" | "numbered" | "task" | "quote";

/** The types the menus offer, in their order (`h4`–`h6` are recognised, not offered). */
export const MENU_KINDS = ["p", "h1", "h2", "h3", "bullet", "numbered", "task", "quote"] as const;
export type MenuKind = (typeof MENU_KINDS)[number];

/** The Markdown each type is written with, shown beside it so the shortcut is learnt. */
export const MARKDOWN_HINT: Record<MenuKind, string> = {
  p: "",
  h1: "#",
  h2: "##",
  h3: "###",
  bullet: "-",
  numbered: "1.",
  task: "[ ]",
  quote: ">",
};

/** Extra words a slash query finds a type by, besides its label in both languages. */
const KEYWORDS: Record<MenuKind, string[]> = {
  p: ["metin", "paragraf", "text", "paragraph", "normal"],
  h1: ["h1", "başlık 1", "heading 1", "title"],
  h2: ["h2", "başlık 2", "heading 2"],
  h3: ["h3", "başlık 3", "heading 3"],
  bullet: ["liste", "madde", "list", "bullet", "ul"],
  numbered: ["numaralı", "sıralı", "liste", "numbered", "ordered", "ol"],
  task: ["yapılacak", "görev", "todo", "to-do", "task", "checkbox"],
  quote: ["alıntı", "quote", "blockquote"],
};

export interface LineParts {
  kind: BlockKind;
  /** Leading spaces kept for list items (nesting). */
  indent: string;
  /** The marker with its trailing space: `## `, `- [x] `, `1. `, `> `; empty for plain text. */
  marker: string;
  /** Text after the marker. */
  body: string;
}

const HEADING = /^( {0,3})(#{1,6})(?:[ \t]+|$)/;
const TASK = /^(\s*)([-*+])[ \t]+\[([ xX])\](?:[ \t]+|$)/;
const BULLET = /^(\s*)([-*+])(?:[ \t]+|$)/;
const NUMBERED = /^(\s*)(\d{1,9})([.)])(?:[ \t]+|$)/;
const QUOTE = /^( {0,3})>[ \t]?/;

export function parseLine(text: string): LineParts {
  let m: RegExpMatchArray | null;
  const parts = (kind: BlockKind, indent: string, len: number): LineParts => ({ kind, indent, marker: text.slice(indent.length, len), body: text.slice(len) });
  if ((m = text.match(HEADING))) return parts(`h${m[2].length}` as BlockKind, m[1], m[0].length);
  if ((m = text.match(TASK))) return parts("task", m[1], m[0].length);
  // `---` and `***` are rules, not empty items
  if ((m = text.match(BULLET)) && !/^\s*([-*_])(\s*\1){2,}\s*$/.test(text)) return parts("bullet", m[1], m[0].length);
  if ((m = text.match(NUMBERED))) return parts("numbered", m[1], m[0].length);
  if ((m = text.match(QUOTE))) return parts("quote", m[1], m[0].length);
  return { kind: "p", indent: "", marker: "", body: text };
}

/** The new marker (with indent) for `kind`, keeping what the old one had: bullet char, number, tick. */
export function prefixFor(old: LineParts, kind: MenuKind): string {
  const bullet = old.kind === "bullet" || old.kind === "task" ? old.marker.trimStart()[0] : "-";
  const listIndent = old.kind === "bullet" || old.kind === "task" || old.kind === "numbered" ? old.indent : "";
  switch (kind) {
    case "p":
      return "";
    case "h1":
    case "h2":
    case "h3":
      return `${"#".repeat(Number(kind[1]))} `;
    case "bullet":
      return `${listIndent}${bullet} `;
    case "numbered":
      return `${listIndent}${old.kind === "numbered" ? old.marker.trim() : "1."} `;
    case "task": {
      const ticked = old.kind === "task" && /\[[xX]\]/.test(old.marker);
      return `${listIndent}${bullet} [${ticked ? "x" : " "}] `;
    }
    case "quote":
      return "> ";
  }
}

/** The edit that turns a line into `kind`: replace `[0, length)` of the line with `insert`. */
export function convertLine(text: string, kind: MenuKind): { length: number; insert: string } {
  const old = parseLine(text);
  return { length: old.indent.length + old.marker.length, insert: prefixFor(old, kind) };
}

/** A slash command being typed: `/` right after the line's marker, up to the cursor. */
export function slashQuery(before: string): { offset: number; query: string } | null {
  const parts = parseLine(before);
  const start = parts.indent.length + parts.marker.length;
  const m = before.slice(start).match(/^\/([^/\n]{0,24})$/);
  if (!m) return null;
  // a plain line may be indented (the slash still comes first), list items keep theirs
  if (parts.kind === "p" && before.slice(0, start).trim()) return null;
  return { offset: start, query: m[1] };
}

/** Lowercase, without Turkish letters or spaces, so `baslik1` finds "Başlık 1". */
export function fold(s: string): string {
  return s
    .toLocaleLowerCase("tr")
    .replace(/[çğıöşü]/g, (c) => ({ ç: "c", ğ: "g", ı: "i", ö: "o", ş: "s", ü: "u" })[c] as string)
    .replace(/\s+/g, "");
}

/** The menu types a slash query matches, in menu order; an empty query matches all. */
export function matchBlocks(query: string, labels: Record<MenuKind, string[]>): MenuKind[] {
  const q = fold(query);
  if (!q) return [...MENU_KINDS];
  return MENU_KINDS.filter((k) => [...labels[k], ...KEYWORDS[k]].some((w) => fold(w).includes(q)));
}
