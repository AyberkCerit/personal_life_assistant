<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Lock from "@lucide/svelte/icons/lock";
  import { editorExtensions } from "./editorTheme";
  import Banner from "./ui/Banner.svelte";
  import Button from "./ui/Button.svelte";
  import { EditorState, Prec } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { markdown } from "@codemirror/lang-markdown";
  import { api } from "./api";
  import { NoteDoc } from "./saving";
  import { NoteSession } from "./triggers";
  import { revealRange } from "./tasks";
  import { completionQuery, linkAt, linkRest } from "./links";
  import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
  import { keymap } from "@codemirror/view";
  import { t, tf } from "./i18n";
  import { mediaExtension } from "./editorMedia";
  import { ImageCache, mediaError } from "./media";
  import Properties from "./Properties.svelte";
  import DaySummary from "./DaySummary.svelte";
  import type { FrontMatter } from "./api";

  let {
    path,
    reveal = null,
    onSavedCopy,
    onOpenLink = () => {},
    suggest = async () => [],
    onTag = () => {},
    onOpenNote = () => {},
    onOpenCited = () => {},
  }: {
    path: string;
    reveal?: string | null;
    onSavedCopy: (rel: string) => void;
    /** FR-EDT-008: Ctrl+click or Ctrl+Enter on a wikilink. */
    onOpenLink?: (target: string) => void;
    /** FR-EDT-010: note names for `[[` completion. */
    suggest?: (query: string) => Promise<{ label: string; detail: string | null; insert: string }[]>;
    /** FR-EDT-006: a tag chip in the properties strip was clicked. */
    onTag?: (tag: string) => void;
    /** FR-MEM-006: a source note or a note the day's summary names. */
    onOpenNote?: (path: string) => void;
    onOpenCited?: (target: string) => void;
  } = $props();

  // FR-EDT-006/007: the strip follows the text as it is typed (a short pause, not every key).
  let meta = $state<FrontMatter>({ state: "none", tags: [], aliases: [], lines: 0 });
  let metaTimer: ReturnType<typeof setTimeout> | undefined;
  function readMeta(text: string, wait: number) {
    clearTimeout(metaTimer);
    metaTimer = setTimeout(() => {
      void api.noteMeta(text).then((m) => !destroyed && (meta = m), () => {});
    }, wait);
  }

  // FR-EDT-016/017: previews come from one cache per open note; its object URLs go with it.
  const images = new ImageCache((target) => api.imageBytes(target));
  const media = mediaExtension({
    cache: images,
    missing: (name) => tf("image.missing", { name }),
    save: async (bytes, name) => (await api.saveImage(bytes, name)).embed,
    onError: (e) => {
      const { key, reason } = mediaError(String(e));
      imageError = tf(key, { reason }); // its own state: a save's sync() would clear `error`
    },
  });

  /** The wikilink at a document position, if any. */
  function linkAtPos(v: EditorView, pos: number): string | null {
    const line = v.state.doc.lineAt(pos);
    return linkAt(line.text, pos - line.from);
  }

  const linkCompletion = autocompletion({
    activateOnTyping: true,
    override: [
      async (ctx: CompletionContext) => {
        const line = ctx.state.doc.lineAt(ctx.pos);
        const found = completionQuery(line.text.slice(0, ctx.pos - line.from));
        if (!found) return null;
        // inside an existing link, the rest of its name is replaced too (links final review M3)
        const rest = linkRest(line.text.slice(ctx.pos - line.from));
        const names = await suggest(found.query);
        return {
          from: line.from + found.offset,
          to: ctx.pos + rest.length,
          filter: false,
          options: names.map((n) => ({ label: n.label, detail: n.detail ?? undefined, apply: rest.closed ? n.insert : `${n.insert}]]` })),
        };
      },
    ],
  });

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | undefined;
  let session: NoteSession | undefined;
  let doc: NoteDoc | undefined;
  let destroyed = false;
  let readOnly = $state(false);
  let problem = $state<"conflict" | "missing" | null>(null);
  let error = $state<string | null>(null);
  let imageError = $state<string | null>(null);

  function sync() {
    problem = doc?.problem ?? null;
    error = doc?.error ?? null;
  }

  async function load(): Promise<void> {
    const note = await api.readNote(path);
    if (destroyed) return;
    readOnly = note.read_only;
    view?.destroy();
    view = new EditorView({
      parent: host!,
      state: EditorState.create({
        doc: note.text,
        extensions: [
          ...editorExtensions(),
          markdown(),
          EditorView.lineWrapping,
          EditorState.readOnly.of(note.read_only),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              session?.edited();
              readMeta(u.state.doc.toString(), 400);
            }
          }),
          EditorView.domEventHandlers({
            blur: () => {
              void session?.blurred();
            },
            mousedown: (event, v) => {
              if (!(event.ctrlKey || event.metaKey)) return false;
              const pos = v.posAtCoords({ x: event.clientX, y: event.clientY });
              const target = pos === null ? null : linkAtPos(v, pos);
              if (!target) return false;
              event.preventDefault();
              onOpenLink(target);
              return true;
            },
          }),
          // above the default keymap, whose Mod-Enter inserts a blank line
          Prec.high(keymap.of([
            {
              key: "Mod-Enter",
              run: (v) => {
                const target = linkAtPos(v, v.state.selection.main.head);
                if (target) onOpenLink(target);
                return target !== null;
              },
            },
          ])),
          linkCompletion,
          media,
        ],
      }),
    });
    const current = view;
    const saveApi = { save: api.saveNote, saveCopy: api.saveCopy };
    doc = new NoteDoc(path, note.text, note.hash, () => current.state.doc.toString(), saveApi, sync);
    session = new NoteSession(path, () => doc!.save(), (p) => void api.queueNote(p));
    sync();
    readMeta(note.text, 0);
    if (reveal) {
      const range = revealRange(note.text, reveal);
      if (range) {
        current.dispatch({ selection: range, scrollIntoView: true });
        current.focus(); // show the selected source line (FR-TSK-017)
      }
    }
  }

  /** Leaving the note: save, queue it if edited, and never drop text (a copy is written if needed). */
  export async function close(): Promise<void> {
    if (readOnly) return;
    await session?.close();
    const copy = await doc?.leave();
    if (copy) onSavedCopy(copy);
  }

  async function takeExternal(): Promise<void> {
    await load();
  }

  async function keepMineAsCopy(): Promise<void> {
    try {
      const copy = await doc?.leave();
      if (copy) onSavedCopy(copy);
      if (problem === "conflict") await load();
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    load().catch((e) => (error = String(e)));
    const unChanged = api.onNoteChanged((changed) => {
      if (changed !== path || !doc) return;
      if (doc.externalChange() === "reload") void load(); // clean: show the outside version at once
    });
    return () => {
      destroyed = true;
      clearTimeout(metaTimer);
      view?.destroy();
      images.dispose();
      void unChanged.then((f) => f());
    };
  });
</script>

{#if readOnly}<Banner kind="warning" icon={Lock}>{t("editor.readOnly")}</Banner>{/if}
{#if problem === "conflict"}
  <Banner kind="danger" icon={CircleAlert} role="alert">
    {t("conflict.message")}
    {#snippet actions()}
      <Button onclick={takeExternal}>{t("conflict.takeExternal")}</Button>
      <Button variant="primary" onclick={keepMineAsCopy}>{t("conflict.keepMine")}</Button>
    {/snippet}
  </Banner>
{:else if problem === "missing"}
  <Banner kind="danger" icon={CircleAlert} role="alert">
    {t("missing.message")}
    {#snippet actions()}
      <Button variant="primary" onclick={keepMineAsCopy}>{t("conflict.keepMine")}</Button>
    {/snippet}
  </Banner>
{/if}
{#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{t("error.generic")}: {error}</Banner>{/if}
{#if imageError}
  <Banner kind="warning" icon={CircleAlert} role="alert">
    {imageError}
    {#snippet actions()}<Button onclick={() => (imageError = null)}>{t("image.dismiss")}</Button>{/snippet}
  </Banner>
{/if}
<DaySummary {path} {onOpenNote} {onOpenCited} />
<Properties {meta} {onTag} />
<div class="editor" bind:this={host}></div>

<style>
  .editor { flex: 1; overflow: auto; }
  .editor :global(.cm-editor) { height: 100%; }
</style>
