/**
 * Line types without Markdown: a handle beside the current (or hovered) line opens a type menu,
 * `/` at the start of a line offers the same types, empty lines hint at `/`, and a heading's `#`
 * marks are hidden while the cursor is on another line.
 */
import { syntaxTree } from "@codemirror/language";
import { StateEffect, StateField, type EditorState, type Extension, type Line } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, showTooltip, type DecorationSet, type Tooltip, type ViewUpdate } from "@codemirror/view";
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { mount, type Component } from "svelte";
import Heading1 from "@lucide/svelte/icons/heading-1";
import Heading2 from "@lucide/svelte/icons/heading-2";
import Heading3 from "@lucide/svelte/icons/heading-3";
import Heading4 from "@lucide/svelte/icons/heading-4";
import Heading5 from "@lucide/svelte/icons/heading-5";
import Heading6 from "@lucide/svelte/icons/heading-6";
import List from "@lucide/svelte/icons/list";
import ListOrdered from "@lucide/svelte/icons/list-ordered";
import ListTodo from "@lucide/svelte/icons/list-todo";
import Quote from "@lucide/svelte/icons/quote";
import Type from "@lucide/svelte/icons/type";
import Icon from "./ui/Icon.svelte";
import { convertLine, MARKDOWN_HINT, MENU_KINDS, matchBlocks, parseLine, slashQuery, type BlockKind, type MenuKind } from "./blocks";
import { frontmatterEnd } from "./editorMedia";

export interface BlocksConfig {
  /** A type's name in the UI language. */
  label: (kind: MenuKind) => string;
  /** Every name a type goes by (both languages), for the slash search. */
  names: Record<MenuKind, string[]>;
  /** The handle's accessible name ("Change line type"). */
  handle: string;
  /** The menu's accessible name. */
  menu: string;
  /** What an empty line says ("Type / for headings, lists and more"). */
  hint: string;
}

const ICONS: Record<BlockKind, Component<any>> = {
  p: Type, h1: Heading1, h2: Heading2, h3: Heading3, h4: Heading4, h5: Heading5, h6: Heading6,
  bullet: List, numbered: ListOrdered, task: ListTodo, quote: Quote,
};

// Each glyph is drawn once through <Icon> and copied from then on.
const glyphs = new Map<BlockKind, Element>();
function glyph(kind: BlockKind): Element {
  let svg = glyphs.get(kind);
  if (!svg) {
    const box = document.createElement("span");
    mount(Icon, { target: box, props: { icon: ICONS[kind], size: "md" } });
    svg = box.querySelector("svg") ?? box;
    glyphs.set(kind, svg);
  }
  return svg.cloneNode(true) as Element;
}

/** Lines whose type can be changed: not frontmatter, not inside a code block. */
function changeable(state: EditorState, line: Line): boolean {
  if (state.readOnly || line.number <= frontmatterEnd(state.doc)) return false;
  for (let n: { name: string; parent: any } | null = syntaxTree(state).resolveInner(line.from, 1); n; n = n.parent) {
    if (n.name === "FencedCode" || n.name === "CodeBlock" || n.name === "HTMLBlock") return false;
  }
  return true;
}

/** Turns the line at `pos` into `kind`; the cursor keeps its place in the text. */
function setBlock(view: EditorView, pos: number, kind: MenuKind) {
  const line = view.state.doc.lineAt(pos);
  const { length, insert } = convertLine(line.text, kind);
  const changes = view.state.changes({ from: line.from, to: line.from + length, insert });
  view.dispatch({ changes, selection: view.state.selection.map(changes, 1), effects: setMenu.of(null), userEvent: "input.block" });
}

// ---- the type menu (opened from the handle) ----

/** The start of the line whose menu is open, or null. */
const setMenu = StateEffect.define<number | null>();
const menuField = StateField.define<number | null>({
  create: () => null,
  update(open, tr) {
    for (const e of tr.effects) if (e.is(setMenu)) return e.value;
    return open !== null && tr.docChanged ? null : open;
  },
});

function menuTooltip(config: BlocksConfig) {
  return showTooltip.from(menuField, (pos): Tooltip | null =>
    pos === null
      ? null
      : {
          pos,
          above: false,
          create: (view) => {
            const current = parseLine(view.state.doc.lineAt(pos).text).kind;
            const dom = document.createElement("div");
            dom.className = "cm-blockMenu";
            dom.setAttribute("role", "menu");
            dom.setAttribute("aria-label", config.menu);
            const items = MENU_KINDS.map((kind) => {
              const item = document.createElement("button");
              item.type = "button";
              item.className = "cm-blockMenu-item";
              item.tabIndex = -1;
              item.setAttribute("role", "menuitemradio");
              item.setAttribute("aria-checked", String(kind === current));
              const name = document.createElement("span");
              name.textContent = config.label(kind);
              const hint = document.createElement("span");
              hint.className = "cm-blockMenu-hint";
              hint.textContent = MARKDOWN_HINT[kind];
              item.append(glyph(kind), name, hint);
              item.addEventListener("click", () => {
                setBlock(view, pos, kind);
                view.focus();
              });
              dom.append(item);
              return item;
            });
            const close = (refocus: boolean) => {
              if (view.state.field(menuField) !== pos) return;
              view.dispatch({ effects: setMenu.of(null) });
              if (refocus) view.focus();
            };
            dom.addEventListener("keydown", (e) => {
              const at = items.indexOf(document.activeElement as HTMLButtonElement);
              const go = (i: number) => items[(i + items.length) % items.length].focus();
              if (e.key === "ArrowDown") go(at + 1);
              else if (e.key === "ArrowUp") go(at - 1);
              else if (e.key === "Home") go(0);
              else if (e.key === "End") go(items.length - 1);
              else if (e.key === "Escape" || e.key === "Tab") close(true);
              else return;
              e.preventDefault();
            });
            // a click elsewhere closes it (after this event, never inside another update)
            dom.addEventListener("focusout", (e) => {
              if (!dom.contains(e.relatedTarget as Node | null)) setTimeout(() => close(false));
            });
            return {
              dom,
              offset: { x: -30, y: 4 },
              mount: () => (items[MENU_KINDS.indexOf(current as MenuKind)] ?? items[0]).focus(),
            };
          },
        },
  );
}

// ---- the handle ----

const HANDLE = 24;

function handlePlugin(config: BlocksConfig) {
  return ViewPlugin.fromClass(
    class {
      button: HTMLButtonElement;
      /** The line the pointer is over, or null. */
      hover: number | null = null;
      /** The line the handle stands beside now. */
      shown: number | null = null;
      kind: BlockKind | null = null;
      onMove = (e: MouseEvent) => {
        const pos = this.view.posAtCoords({ x: e.clientX, y: e.clientY }, false);
        const from = this.view.state.doc.lineAt(pos).from;
        if (from !== this.hover) {
          this.hover = from;
          this.measure();
        }
      };
      onLeave = () => {
        this.hover = null;
        this.measure();
      };
      // typing brings the handle back to the cursor's line until the pointer moves again
      onKey = () => {
        if (this.hover !== null) this.onLeave();
      };

      constructor(readonly view: EditorView) {
        const b = (this.button = document.createElement("button"));
        b.type = "button";
        b.className = "cm-blockHandle";
        b.title = config.handle;
        b.setAttribute("aria-label", config.handle);
        b.setAttribute("aria-haspopup", "menu");
        b.setAttribute("aria-expanded", "false");
        b.style.display = "none";
        b.addEventListener("mousedown", (e) => e.preventDefault()); // the text keeps its cursor
        b.addEventListener("click", () => {
          if (this.shown === null) return;
          const open = view.state.field(menuField) === this.shown;
          view.dispatch({ effects: setMenu.of(open ? null : this.shown) });
          if (open) view.focus();
        });
        view.scrollDOM.append(b);
        view.dom.addEventListener("mousemove", this.onMove);
        view.dom.addEventListener("mouseleave", this.onLeave);
        view.contentDOM.addEventListener("keydown", this.onKey);
        this.measure();
      }

      update(u: ViewUpdate) {
        if (u.docChanged && this.hover !== null) this.hover = u.state.doc.lineAt(u.changes.mapPos(this.hover)).from;
        if (u.docChanged || u.selectionSet || u.viewportChanged || u.geometryChanged || u.focusChanged || u.startState.field(menuField) !== u.state.field(menuField)) {
          this.measure();
        }
      }

      /** The line the handle belongs beside: the open menu's, the hovered one, else the cursor's. */
      target(): number | null {
        const { state } = this.view;
        const open = state.field(menuField);
        if (open !== null) return open;
        if (this.hover !== null) return this.hover;
        return this.view.hasFocus ? state.doc.lineAt(state.selection.main.head).from : null;
      }

      measure() {
        this.view.requestMeasure({
          key: this,
          read: (view) => {
            const at = this.target();
            if (at === null || at > view.state.doc.length) return null;
            const line = view.state.doc.lineAt(at);
            if (!changeable(view.state, line)) return null;
            const c = view.coordsAtPos(line.from, 1);
            if (!c) return null;
            const box = view.scrollDOM.getBoundingClientRect();
            return {
              from: line.from,
              kind: parseLine(line.text).kind,
              top: c.top - box.top + view.scrollDOM.scrollTop + (c.bottom - c.top - HANDLE) / 2,
              left: c.left - box.left + view.scrollDOM.scrollLeft - HANDLE - 6,
            };
          },
          write: (m, view) => {
            const b = this.button;
            this.shown = m?.from ?? null;
            if (!m) {
              b.style.display = "none";
              return;
            }
            b.style.display = "";
            b.style.top = `${m.top}px`;
            b.style.left = `${m.left}px`;
            b.setAttribute("aria-expanded", String(view.state.field(menuField) === m.from));
            if (m.kind !== this.kind) {
              this.kind = m.kind;
              b.replaceChildren(glyph(m.kind));
            }
          },
        });
      }

      destroy() {
        this.button.remove();
        this.view.dom.removeEventListener("mousemove", this.onMove);
        this.view.dom.removeEventListener("mouseleave", this.onLeave);
        this.view.contentDOM.removeEventListener("keydown", this.onKey);
      }
    },
  );
}

// ---- `#` marks hidden away from the cursor ----

const hidden = Decoration.replace({});

function headingMarks(view: EditorView): DecorationSet {
  const { state } = view;
  const active = new Set<number>();
  if (view.hasFocus) {
    for (const r of state.selection.ranges) {
      for (let n = state.doc.lineAt(r.from).number; n <= state.doc.lineAt(r.to).number; n++) active.add(n);
    }
  }
  const front = frontmatterEnd(state.doc);
  const marks: ReturnType<typeof hidden.range>[] = [];
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        if (!node.name.startsWith("ATXHeading")) return;
        const mark = node.node.firstChild;
        if (mark?.name === "HeaderMark") {
          const line = state.doc.lineAt(mark.from);
          if (!active.has(line.number) && line.number > front) {
            const end = state.doc.sliceString(mark.to, mark.to + 1) === " " ? mark.to + 1 : mark.to;
            marks.push(hidden.range(mark.from, end));
          }
        }
        return false;
      },
    });
  }
  return Decoration.set(marks, true);
}

const headingMarksPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = headingMarks(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.selectionSet || u.viewportChanged || u.focusChanged || syntaxTree(u.startState) !== syntaxTree(u.state)) {
        this.decorations = headingMarks(u.view);
      }
    }
  },
  { decorations: (p) => p.decorations },
);

// ---- the hint on an empty line ----

class HintWidget extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  eq(other: HintWidget) {
    return other.text === this.text;
  }
  toDOM() {
    const span = document.createElement("span");
    span.className = "cm-blockHint";
    span.setAttribute("aria-hidden", "true");
    span.textContent = this.text;
    return span;
  }
}

function hintPlugin(config: BlocksConfig) {
  const hint = (view: EditorView): DecorationSet => {
    const { state } = view;
    const sel = state.selection.main;
    if (!view.hasFocus || !sel.empty) return Decoration.none;
    const line = state.doc.lineAt(sel.head);
    if (sel.head !== line.to || !changeable(state, line)) return Decoration.none;
    // an empty line points at `/`; an empty heading or item says what it is
    const parts = parseLine(line.text);
    const text = !line.text ? config.hint : !parts.body && (MENU_KINDS as readonly string[]).includes(parts.kind) && parts.kind !== "p" ? config.label(parts.kind as MenuKind) : null;
    return text ? Decoration.set(Decoration.widget({ widget: new HintWidget(text), side: 1 }).range(line.to)) : Decoration.none;
  };
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = hint(view);
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.selectionSet || u.focusChanged) this.decorations = hint(u.view);
      }
    },
    { decorations: (p) => p.decorations },
  );
}

// ---- the slash command ----

interface BlockCompletion extends Completion {
  block: MenuKind;
}

/** A completion source: `/` at the start of a line (or after its marker) offers the line types. */
export function slashCompletion(config: BlocksConfig) {
  return (ctx: CompletionContext): CompletionResult | null => {
    const line = ctx.state.doc.lineAt(ctx.pos);
    const found = slashQuery(line.text.slice(0, ctx.pos - line.from));
    if (!found || !changeable(ctx.state, line)) return null;
    const kinds = matchBlocks(found.query, config.names);
    if (!kinds.length) return null;
    return {
      from: line.from + found.offset,
      to: ctx.pos,
      filter: false,
      options: kinds.map(
        (kind): BlockCompletion => ({
          label: config.label(kind),
          detail: MARKDOWN_HINT[kind] || undefined,
          block: kind,
          apply: (view, _c, _from, to) => {
            // the slash and its query go; the marker before them becomes the chosen one
            const { length, insert } = convertLine(line.text.slice(0, found.offset), kind);
            view.dispatch({
              changes: { from: line.from, to: Math.max(to, line.from + length), insert },
              selection: { anchor: line.from + insert.length },
              userEvent: "input.complete",
            });
          },
        }),
      ),
    };
  };
}

/** For `autocompletion({ addToOptions })`: a line type's icon in front of its name. */
export const completionIcon = {
  position: 20,
  render: (c: Completion): Node | null => {
    const kind = (c as Partial<BlockCompletion>).block;
    if (!kind) return null;
    const box = document.createElement("span");
    box.className = "cm-blockIcon";
    box.append(glyph(kind));
    return box;
  },
};

export const BLOCKS_THEME = {
  ".cm-blockHandle": {
    position: "absolute", zIndex: "2", width: `${HANDLE}px`, height: `${HANDLE}px`, padding: "0",
    display: "flex", alignItems: "center", justifyContent: "center",
    border: "none", borderRadius: "var(--radius-sm)", backgroundColor: "transparent",
    color: "var(--color-text-muted)", opacity: "0.6", cursor: "pointer",
  },
  ".cm-blockHandle:hover, .cm-blockHandle[aria-expanded=true], .cm-blockHandle:focus-visible": {
    opacity: "1", color: "var(--color-text)", backgroundColor: "var(--color-surface-panel)",
  },
  ".cm-tooltip.cm-blockMenu": {
    display: "flex", flexDirection: "column", minWidth: "13rem", padding: "var(--space-1)",
    backgroundColor: "var(--color-surface-raised)", border: "1px solid var(--color-border)",
    borderRadius: "var(--radius-md)", boxShadow: "var(--shadow-raised)",
    fontFamily: "var(--font-ui)", fontSize: "var(--text-md)",
  },
  ".cm-blockMenu-item": {
    display: "flex", alignItems: "center", gap: "var(--space-2)", width: "100%",
    padding: "var(--space-1) var(--space-2)", border: "none", borderRadius: "var(--radius-sm)",
    backgroundColor: "transparent", color: "var(--color-text)", font: "inherit", textAlign: "left", cursor: "pointer",
  },
  ".cm-blockMenu-item svg": { color: "var(--color-text-muted)", flexShrink: "0" },
  ".cm-blockMenu-item:hover, .cm-blockMenu-item:focus": { backgroundColor: "var(--color-accent-subtle)", outline: "none" },
  ".cm-blockMenu-item[aria-checked=true]": { color: "var(--color-accent)", fontWeight: "600" },
  ".cm-blockMenu-item[aria-checked=true] svg": { color: "var(--color-accent)" },
  ".cm-blockMenu-hint": { marginLeft: "auto", paddingLeft: "var(--space-4)", color: "var(--color-text-muted)", fontFamily: "var(--font-mono)", fontSize: "var(--text-sm)", fontWeight: "400" },
  ".cm-blockIcon": { display: "inline-flex", verticalAlign: "-3px", marginRight: "var(--space-2)", color: "var(--color-text-muted)" },
  ".cm-blockHint": { color: "var(--color-text-muted)", pointerEvents: "none", fontWeight: "400" },
};

/** The handle, its menu, the hidden heading marks and the empty-line hint (the slash source goes in the editor's autocompletion). */
export function blocksExtension(config: BlocksConfig): Extension {
  return [menuField, menuTooltip(config), handlePlugin(config), headingMarksPlugin, hintPlugin(config), EditorView.theme(BLOCKS_THEME)];
}
