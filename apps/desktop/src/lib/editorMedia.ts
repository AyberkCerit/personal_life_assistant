/** The editor's frontmatter and images (FR-EDT-006/016/017): dim YAML, previews, paste and drop. */
import { RangeSetBuilder, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { frontmatterLines, imageEmbeds, isImageFile, type ImageCache } from "./media";

export interface MediaConfig {
  cache: ImageCache;
  /** "Image not found: {name}" in the UI language. */
  missing: (name: string) => string;
  /** Stores a pasted (`name` = null) or dropped image; returns what goes inside `![[…]]`. */
  save: (bytes: Uint8Array, name: string | null) => Promise<string>;
  onError: (err: unknown) => void;
}

class ImagesWidget extends WidgetType {
  constructor(
    readonly targets: string[],
    readonly config: MediaConfig,
  ) {
    super();
  }

  eq(other: ImagesWidget): boolean {
    return other.targets.join("\n") === this.targets.join("\n");
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

function decorations(state: EditorState, config: MediaConfig): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const { doc } = state;
  const head: string[] = [];
  for (let i = 1; i <= doc.lines && i <= 200; i++) {
    head.push(doc.line(i).text);
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
    if (targets.length) builder.add(line.to, line.to, Decoration.widget({ widget: new ImagesWidget(targets, config), block: true, side: 1 }));
  }
  return builder.finish();
}

async function insertImages(view: EditorView, files: File[], pasted: boolean, pos: number, config: MediaConfig) {
  const embeds: string[] = [];
  for (const file of files) {
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      embeds.push(`![[${await config.save(bytes, pasted ? null : file.name)}]]`);
    } catch (e) {
      config.onError(e);
    }
  }
  if (!embeds.length) return;
  config.cache.forgetMissing();
  const at = Math.min(pos, view.state.doc.length);
  const insert = embeds.join("\n");
  view.dispatch({ changes: { from: at, insert }, selection: { anchor: at + insert.length }, scrollIntoView: true });
  view.focus();
}

export function mediaExtension(config: MediaConfig): Extension {
  const field = StateField.define<DecorationSet>({
    create: (state) => decorations(state, config),
    update: (deco, tr) => (tr.docChanged ? decorations(tr.state, config) : deco),
    provide: (f) => EditorView.decorations.from(f),
  });
  const images = (list: FileList | undefined | null) => [...(list ?? [])].filter(isImageFile);
  return [
    field,
    EditorView.domEventHandlers({
      paste: (event, view) => {
        const files = images(event.clipboardData?.files);
        if (!files.length || view.state.readOnly) return false;
        event.preventDefault();
        void insertImages(view, files, true, view.state.selection.main.head, config);
        return true;
      },
      drop: (event, view) => {
        const files = images(event.dataTransfer?.files);
        if (!files.length || view.state.readOnly) return false;
        event.preventDefault();
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY }) ?? view.state.selection.main.head;
        void insertImages(view, files, false, pos, config);
        return true;
      },
    }),
    EditorView.theme({
      ".cm-frontmatter": { color: "var(--color-text-muted)", fontFamily: "var(--font-mono)", fontSize: "0.85em" },
      ".cm-images": { display: "flex", flexWrap: "wrap", gap: "var(--space-2)", padding: "var(--space-1) 0 var(--space-2)" },
      ".cm-image img": { display: "block", maxWidth: "100%", maxHeight: "400px", objectFit: "contain", borderRadius: "var(--radius-md)" },
      ".cm-image-missing": {
        fontSize: "var(--text-sm)", color: "var(--color-text-muted)", padding: "var(--space-1) var(--space-2)",
        border: "1px dashed var(--color-border-strong)", borderRadius: "var(--radius-md)",
      },
    }),
  ];
}
