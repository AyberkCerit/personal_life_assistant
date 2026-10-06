/** The editor's frontmatter and images (FR-EDT-006/016/017): dim YAML, previews, paste and drop. */
import { RangeSetBuilder, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { frontmatterLines, imageEmbeds, isImageFile, MAX_IMAGE, type ImageCache } from "./media";

export interface MediaConfig {
  cache: ImageCache;
  /** "Image not found: {name}" in the UI language. */
  missing: (name: string) => string;
  /** Stores a pasted (`name` = null) or dropped image; returns what goes inside `![[…]]`. */
  save: (bytes: Uint8Array, name: string | null) => Promise<string>;
  /** A `<code>|<detail>|<reason>` error, as `save_image` gives them. */
  onError: (err: unknown) => void;
}

class ImagesWidget extends WidgetType {
  constructor(
    readonly targets: string[],
    readonly generation: number,
    readonly config: MediaConfig,
  ) {
    super();
  }

  eq(other: ImagesWidget): boolean {
    // a new generation (an image was just saved) redraws the previews that said "missing"
    return other.generation === this.generation && other.targets.join("\n") === this.targets.join("\n");
  }

  toDOM(view: EditorView): HTMLElement {
    const box = document.createElement("div");
    box.className = "cm-images";
    for (const target of this.targets) {
      const slot = document.createElement("div");
      slot.className = "cm-image";
      box.append(slot);
      void this.config.cache.get(target).then((url) => {
        if (url) {
          const img = document.createElement("img");
          img.alt = target;
          img.title = target;
          img.addEventListener("load", () => view.requestMeasure());
          img.src = url;
          slot.append(img);
        } else {
          slot.className = "cm-image cm-image-missing";
          slot.textContent = this.config.missing(target);
        }
        view.requestMeasure();
      });
    }
    return box;
  }

  ignoreEvent(): boolean {
    return true; // clicks on a preview do not move the cursor
  }
}

const dimLine = Decoration.line({ class: "cm-frontmatter" });
/** An image was saved: the previews are drawn again. */
const mediaChanged = StateEffect.define<null>();

function decorations(state: EditorState, generation: number, config: MediaConfig): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const { doc } = state;
  const head: string[] = [];
  for (let i = 1; i <= doc.lines; i++) {
    head.push(doc.line(i).text);
    if (i === 1 && head[0].replace(/^﻿/, "").trimEnd() !== "---") break;
    if (i > 1 && /^(---|\.\.\.)\s*$/.test(head[i - 1])) break;
  }
  const front = frontmatterLines(head);
  for (let i = 1; i <= doc.lines; i++) {
    const line = doc.line(i);
    if (i <= front) {
      builder.add(line.from, line.from, dimLine);
      continue;
    }
    if (!line.text.includes("![[")) continue;
    const targets = imageEmbeds(line.text);
    if (targets.length) builder.add(line.to, line.to, Decoration.widget({ widget: new ImagesWidget(targets, generation, config), block: true, side: 1 }));
  }
  return builder.finish();
}

export function mediaExtension(config: MediaConfig): Extension {
  let generation = 0;
  const field = StateField.define<DecorationSet>({
    create: (state) => decorations(state, generation, config),
    update: (deco, tr) => (tr.docChanged || tr.effects.some((e) => e.is(mediaChanged)) ? decorations(tr.state, generation, config) : deco),
    provide: (f) => EditorView.decorations.from(f),
  });

  // Where a pending image goes: moved along with what is typed while it is being saved (I6).
  const pending = new Set<{ pos: number }>();

  async function insertImages(view: EditorView, files: File[], pasted: boolean, pos: number) {
    const at = { pos };
    pending.add(at);
    const embeds: string[] = [];
    try {
      for (const file of files) {
        if (file.size > MAX_IMAGE) {
          config.onError("too_big|20|"); // before reading 20 MB into the page (M5)
          continue;
        }
        try {
          const bytes = new Uint8Array(await file.arrayBuffer());
          embeds.push(`![[${await config.save(bytes, pasted ? null : file.name)}]]`);
        } catch (e) {
          config.onError(e);
        }
      }
    } finally {
      pending.delete(at);
    }
    if (!embeds.length || !view.dom.isConnected) return; // the note was closed: the file stays saved
    config.cache.forgetMissing();
    generation++;
    const from = Math.min(at.pos, view.state.doc.length);
    const insert = embeds.join("\n");
    view.dispatch({ changes: { from, insert }, selection: { anchor: from + insert.length }, effects: mediaChanged.of(null), scrollIntoView: true });
    view.focus();
  }

  const images = (list: FileList | undefined | null) => [...(list ?? [])].filter(isImageFile);
  return [
    field,
    EditorView.updateListener.of((u) => {
      if (u.docChanged) for (const at of pending) at.pos = u.changes.mapPos(at.pos, 1);
    }),
    EditorView.domEventHandlers({
      paste: (event, view) => {
        // copied cells (Excel, Word) carry text and a picture of it: the text is what was meant (M8)
        if (event.clipboardData?.getData("text/plain")) return false;
        const files = images(event.clipboardData?.files);
        if (!files.length || view.state.readOnly) return false;
        event.preventDefault();
        void insertImages(view, files, true, view.state.selection.main.head);
        return true;
      },
      drop: (event, view) => {
        const all = [...(event.dataTransfer?.files ?? [])];
        if (!all.length) return false; // text or a tree item: CodeMirror's own handling
        // other files are never read into the note as text (I3)
        event.preventDefault();
        const files = all.filter(isImageFile);
        if (!files.length) {
          config.onError("not_image||");
          return true;
        }
        if (view.state.readOnly) return true;
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY }) ?? view.state.selection.main.head;
        void insertImages(view, files, false, pos);
        return true;
      },
    }),
    EditorView.theme({
      ".cm-frontmatter": { color: "var(--color-text-muted)", fontFamily: "var(--font-mono)", fontSize: "0.85em" },
      // YAML closed by `---` reads as a setext heading to Markdown: its heading style stays out
      ".cm-frontmatter span": { fontSize: "inherit", fontWeight: "inherit", color: "inherit", fontStyle: "inherit" },
      ".cm-images": { display: "flex", flexWrap: "wrap", gap: "var(--space-2)", padding: "var(--space-1) 0 var(--space-2)" },
      ".cm-image img": { display: "block", maxWidth: "100%", maxHeight: "400px", objectFit: "contain", borderRadius: "var(--radius-md)" },
      ".cm-image-missing": {
        fontSize: "var(--text-sm)", color: "var(--color-text-muted)", padding: "var(--space-1) var(--space-2)",
        border: "1px dashed var(--color-border-strong)", borderRadius: "var(--radius-md)",
      },
    }),
  ];
}
