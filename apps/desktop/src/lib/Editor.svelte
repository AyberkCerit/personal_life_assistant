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
  import { completionQuery, linkAt } from "./links";
  import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
  import { keymap } from "@codemirror/view";
  import { t } from "./i18n";

  let {
    path,
    reveal = null,
    onSavedCopy,
    onOpenLink = () => {},
    suggest = async () => [],
  }: {
    path: string;
    reveal?: string | null;
    onSavedCopy: (rel: string) => void;
    /** FR-EDT-008: Ctrl+click or Ctrl+Enter on a wikilink. */
    onOpenLink?: (target: string) => void;
    /** FR-EDT-010: note names for `[[` completion. */
    suggest?: (query: string) => Promise<{ label: string; detail: string | null }[]>;
  } = $props();

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
        const closed = line.text.slice(ctx.pos - line.from).startsWith("]]");
        const names = await suggest(found.query);
        return {
          from: line.from + found.offset,
          filter: false,
          options: names.map((n) => ({ label: n.label, detail: n.detail ?? undefined, apply: closed ? n.label : `${n.label}]]` })),
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
            if (u.docChanged) session?.edited();
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
        ],
      }),
    });
    const current = view;
    const saveApi = { save: api.saveNote, saveCopy: api.saveCopy };
    doc = new NoteDoc(path, note.text, note.hash, () => current.state.doc.toString(), saveApi, sync);
    session = new NoteSession(path, () => doc!.save(), (p) => void api.queueNote(p));
    sync();
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
      view?.destroy();
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
<div class="editor" bind:this={host}></div>

<style>
  .editor { flex: 1; overflow: auto; }
  .editor :global(.cm-editor) { height: 100%; }
</style>
