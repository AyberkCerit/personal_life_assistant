<script lang="ts">
  import { onMount } from "svelte";
  import { basicSetup } from "codemirror";
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { markdown } from "@codemirror/lang-markdown";
  import { api } from "./api";
  import { NoteSession } from "./triggers";
  import { t } from "./i18n";

  let { path, onSavedCopy }: { path: string; onSavedCopy: (rel: string) => void } = $props();

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | undefined;
  let session: NoteSession | undefined;
  let hash: string | null = null;
  let readOnly = $state(false);
  let conflict = $state(false);
  let error = $state<string | null>(null);

  async function save(p: string): Promise<void> {
    if (!view || readOnly || conflict) return;
    try {
      const result = await api.saveNote(p, view.state.doc.toString(), hash);
      if (result.kind === "saved") {
        hash = result.hash;
        error = null;
      } else {
        conflict = true; // Review Focus 1: never overwrite; the user decides
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function load(): Promise<void> {
    const note = await api.readNote(path);
    hash = note.hash;
    readOnly = note.read_only;
    conflict = false;
    view?.destroy();
    view = new EditorView({
      parent: host!,
      state: EditorState.create({
        doc: note.text,
        extensions: [
          basicSetup,
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
          }),
        ],
      }),
    });
    session = new NoteSession(path, save, (p) => void api.queueNote(p));
  }

  export async function close(): Promise<void> {
    await session?.close();
  }

  async function takeExternal(): Promise<void> {
    await load();
  }

  async function keepMineAsCopy(): Promise<void> {
    if (!view) return;
    const copy = await api.saveCopy(path, view.state.doc.toString());
    onSavedCopy(copy);
    await load();
  }

  onMount(() => {
    load().catch((e) => (error = String(e)));
    return () => view?.destroy();
  });
</script>

{#if readOnly}<div class="banner">{t("editor.readOnly")}</div>{/if}
{#if conflict}
  <div class="banner danger" role="alert">
    <span>{t("conflict.message")}</span>
    <button onclick={takeExternal}>{t("conflict.takeExternal")}</button>
    <button onclick={keepMineAsCopy}>{t("conflict.keepMine")}</button>
  </div>
{/if}
{#if error}<div class="banner danger" role="alert">{t("error.generic")}: {error}</div>{/if}
<div class="editor" bind:this={host}></div>

<style>
  .editor { flex: 1; overflow: auto; }
  .editor :global(.cm-editor) { height: 100%; background: var(--bg); color: var(--text); }
  .editor :global(.cm-content) { padding: 16px 24px; max-width: 760px; }
  .editor :global(.cm-gutters) { background: var(--bg); border: 0; color: var(--muted); }
  .banner button { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 2px 8px; cursor: pointer; }
</style>
