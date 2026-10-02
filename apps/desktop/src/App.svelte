<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, inTauri, type TreeEntry, type WorkerStatus } from "./lib/api";
  import FileTree from "./lib/FileTree.svelte";
  import Editor from "./lib/Editor.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import StatusBar from "./lib/StatusBar.svelte";
  import { t, lang } from "./lib/i18n";

  let vaultPath = $state<string | null>(null);
  let entries = $state<TreeEntry[]>([]);
  let current = $state<string | null>(null);
  let editor: ReturnType<typeof Editor> | undefined = $state();
  let error = $state<string | null>(null);
  let status = $state<WorkerStatus>({ queued: 0, model: "off", busy: false, last_error: null, added: 0 });

  async function refresh() {
    entries = await api.listTree();
  }

  async function chooseVault() {
    const picked = inTauri ? await open({ directory: true }) : "C:/Deneme Kasası";
    if (typeof picked !== "string") return;
    try {
      vaultPath = await api.openVault(picked);
      error = null;
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function openNote(path: string) {
    if (path === current) return;
    try {
      await editor?.close(); // save, queue if edited (FR-EXT-002); text is never dropped
    } catch (e) {
      error = String(e); // stay on the note rather than lose what could not be saved
      return;
    }
    current = path;
  }

  async function createNote(title: string) {
    try {
      const rel = await api.createNote("inbox", title);
      await refresh();
      await openNote(rel);
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    document.documentElement.lang = lang;
    void (async () => {
      const info = await api.startup();
      document.documentElement.dataset.theme = info.theme;
      error = info.error;
      vaultPath = info.vault_path;
      if (vaultPath) await refresh();
    })();
    const unStatus = api.onStatus((s) => (status = s));
    const unTree = api.onTreeChanged(() => void refresh());
    const unClose = inTauri
      ? getCurrentWindow().onCloseRequested(async () => {
          await editor?.close();
        })
      : Promise.resolve(() => {});
    return () => {
      void unStatus.then((f) => f());
      void unTree.then((f) => f());
      void unClose.then((f) => f());
    };
  });
</script>

{#if vaultPath === null}
  <main class="welcome">
    <h1>{t("welcome.title")}</h1>
    <p>{t("welcome.body")}</p>
    {#if error}<p class="banner danger" role="alert">{error}</p>{/if}
    <button class="primary" onclick={chooseVault}>{t("welcome.open")}</button>
  </main>
{:else}
  <div class="layout">
    <aside class="sidebar">
      <FileTree {entries} selected={current} onOpen={openNote} onCreate={createNote} />
    </aside>
    <section class="main">
      {#if error}<div class="banner danger" role="alert">{t("error.generic")}: {error}</div>{/if}
      {#if current}
        {#key current}
          <Editor bind:this={editor} path={current} onSavedCopy={() => void refresh()} />
        {/key}
      {:else}
        <p class="empty">{t("editor.empty")}</p>
      {/if}
    </section>
    <aside class="side-panel">{t("side.placeholder")}</aside>
    <StatusBar {status} />
  </div>
{/if}
