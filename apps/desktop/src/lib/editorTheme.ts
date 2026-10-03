/** CodeMirror look for notes (spec § 5): Segoe UI, token colours, soft Markdown marks, wikilinks. */
import { HighlightStyle, syntaxHighlighting, type TagStyle } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import type { Extension } from "@codemirror/state";
import { Decoration, EditorView, MatchDecorator, ViewPlugin, dropCursor, highlightActiveLine, keymap, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { minimalSetup } from "codemirror";

export const EDITOR_THEME = {
  "&": { height: "100%", backgroundColor: "var(--color-surface-reading)", color: "var(--color-text)", fontSize: "var(--text-lg)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-ui)", lineHeight: "1.65" },
  ".cm-content": { width: "100%", maxWidth: "72ch", boxSizing: "border-box", margin: "0 auto", padding: "var(--space-6) var(--space-8)", caretColor: "var(--color-accent)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-accent)", borderLeftWidth: "2px" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "var(--color-selection)",
  },
  // See-through, so the selection layer drawn behind the text still shows on this line.
  ".cm-activeLine": { backgroundColor: "var(--color-active-line)" },
  ".cm-selectionMatch": { backgroundColor: "var(--color-accent-subtle)" },
  ".cm-searchMatch": { backgroundColor: "var(--color-warning-subtle)", outline: "1px solid var(--color-warning)" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--color-accent-subtle)" },
  ".cm-panels": { backgroundColor: "var(--color-surface-panel)", color: "var(--color-text)", fontFamily: "var(--font-ui)", fontSize: "var(--text-sm)" },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--color-border)" },
  ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--color-border)" },
  ".cm-textfield": {
    backgroundColor: "var(--color-surface-reading)", color: "var(--color-text)",
    border: "1px solid var(--color-border)", borderRadius: "var(--radius-md)",
  },
  ".cm-button": {
    backgroundImage: "none", backgroundColor: "var(--color-surface-reading)", color: "var(--color-text)",
    border: "1px solid var(--color-border)", borderRadius: "var(--radius-md)",
  },
  ".cm-wikilink": { color: "var(--color-link)", textDecoration: "underline", textDecorationColor: "var(--color-link-underline)", textUnderlineOffset: "3px" },
};

export const HIGHLIGHT: TagStyle[] = [
  { tag: tags.heading1, fontSize: "var(--text-2xl)", fontWeight: "600" },
  { tag: tags.heading2, fontSize: "var(--text-xl)", fontWeight: "600" },
  { tag: [tags.heading3, tags.heading4, tags.heading5, tags.heading6], fontSize: "var(--text-lg)", fontWeight: "600" },
  { tag: tags.processingInstruction, color: "var(--color-text-muted)" }, // #, *, -, > and other Markdown marks
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strong, fontWeight: "600" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: [tags.link, tags.url], color: "var(--color-link)" },
  { tag: tags.monospace, fontFamily: "var(--font-mono)", fontSize: "0.9em", backgroundColor: "var(--color-surface-panel)", borderRadius: "var(--radius-sm)" },
  { tag: tags.quote, color: "var(--color-text-muted)", fontStyle: "italic" },
];

/** `[[Note]]` or `[[Note|alias]]` on one line. */
export const WIKILINK = /\[\[[^\]\n]+\]\]/g;

const wikilinks = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    matcher = new MatchDecorator({ regexp: WIKILINK, decoration: Decoration.mark({ class: "cm-wikilink" }) });
    constructor(view: EditorView) {
      this.decorations = this.matcher.createDeco(view);
    }
    update(u: ViewUpdate) {
      this.decorations = this.matcher.updateDeco(u, this.decorations);
    }
  },
  { decorations: (p) => p.decorations },
);

/** Replaces `basicSetup`: no line numbers or fold gutter, faint active line, search kept. */
export function editorExtensions(): Extension[] {
  return [
    minimalSetup,
    dropCursor(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    keymap.of(searchKeymap),
    EditorView.theme(EDITOR_THEME),
    syntaxHighlighting(HighlightStyle.define(HIGHLIGHT)),
    wikilinks,
  ];
}
