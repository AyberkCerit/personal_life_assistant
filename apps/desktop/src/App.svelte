<script lang="ts">
  import { onMount, tick } from "svelte";
  import AddedToast from "./lib/AddedToast.svelte";
  import ReminderBanner from "./lib/ReminderBanner.svelte";
  import ModelBanner from "./lib/ModelBanner.svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import FileText from "@lucide/svelte/icons/file-text";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Logo from "./lib/ui/Logo.svelte";
  import Button from "./lib/ui/Button.svelte";
  import Banner from "./lib/ui/Banner.svelte";
  import EmptyState from "./lib/ui/EmptyState.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, inTauri, type DownloadState, type TreeEntry, type WorkerStatus } from "./lib/api";
  import FileTree from "./lib/FileTree.svelte";
  import TaskPanel from "./lib/TaskPanel.svelte";
  import Editor from "./lib/Editor.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import StatusBar from "./lib/StatusBar.svelte";
  import Wizard from "./lib/Wizard.svelte";
  import Settings from "./lib/Settings.svelte";
  import type { Section } from "./lib/settings";
  import type { Progress } from "./lib/wizard";
  import { t, lang, setLang, type Lang } from "./lib/i18n";
  import type { VaultInfo } from "./lib/api";

  let loaded = $state(false);
  let wizard = $state(false);
  let wizardProgress = $state<Progress>({ step: "language", path: "", opened: null });
  let uiLang = $state<Lang>(lang); // the app is keyed on it: switching re-renders every text
  let vaultPath = $state<string | null>(null);
  let entries = $state<TreeEntry[]>([]);
  let current = $state<string | null>(null);
  let inbox = $state("inbox");
  let tasksVersion = $state(0);
  let panelEdits = $state(0); // changes made in the task panel itself
  let modelDownload = $state<DownloadState | null>(null);
  let showModel = $state(false);
  let reveal = $state<string | null>(null);
  let editor: ReturnType<typeof Editor> | undefined = $state();
  let error = $state<string | null>(null);
  let settingsOpen = $state(false);
  let settingsSection = $state<Section>("general"); // kept here: a language switch re-mounts the window
  let settingsRefocus = $state<string | null>(null);
  let settingsOpener: HTMLElement | null = null;
  let status = $state<WorkerStatus>({ queued: 0, model: "off", busy: false, last_error: null, added: 0, paused: false });

  async function refresh() {
    entries = await api.listTree();
  }

  function switchLanguage(l: Lang) {
    setLang(l);
    uiLang = l;
  }

  function openSettings() {
    if (wizard || settingsOpen) return;
    settingsOpener = document.activeElement as HTMLElement | null;
    settingsOpen = true;
  }

  function closeSettings() {
    settingsOpen = false;
    settingsOpener?.focus();
  }

  // FR-SET-012: the open note is saved before the app re-renders in the new language; if it cannot
  // be saved, the language stays and the section shows why.
  async function settingsLanguage(l: Lang) {
    await editor?.close();
    await api.settingsSet("language", l);
    settingsRefocus = "language";
    switchLanguage(l);
  }

  async function settingsTheme(theme: "dark" | "light") {
    await api.settingsSet("theme", theme);
    document.documentElement.dataset.theme = theme;
  }

  // FR-SET-014: the note is saved into the old vault before it closes.
  async function settingsSwitchVault(path: string) {
    await editor?.close();
    current = null;
    await vaultOpened(await api.setupVault(path, lang));
    tasksVersion++;
  }

  async function wizardFinished(info: VaultInfo) {
    wizard = false;
    await vaultOpened(info);
  }

  async function vaultOpened(info: VaultInfo) {
    vaultPath = info.path;
    inbox = info.inbox;
    status = await api.workerStatus(); // the first status may have been sent before we listened
    error = null;
    await refresh();
  }

  async function chooseVault() {
    const picked = inTauri ? await open({ directory: true }) : "C:/Deneme Kasası";
    if (typeof picked !== "string") return;
    try {
      await vaultOpened(await api.openVault(picked));
    } catch (e) {
      error = String(e);
    }
  }

  async function openNote(path: string, text: string | null = null) {
    if (path === current && text === null) return; // already open: keep cursor, scroll and undo history
    try {
      await editor?.close(); // save, queue if edited (FR-EXT-002); text is never dropped
    } catch (e) {
      error = String(e); // stay on the note rather than lose what could not be saved
      return;
    }
    reveal = text;
    if (path === current) {
      current = null; // re-create the editor so the selection is applied
      await tick();
    }
    current = path;
  }


  function openSource(path: string, blockText: string | null) {
    void openNote(path, blockText);
  }


  async function createNote(title: string) {
    try {
      const rel = await api.createNote(inbox, title);
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
      if (info.language) switchLanguage(info.language);
      wizard = info.show_wizard;
      document.documentElement.dataset.theme = info.theme;
      error = info.error;
      vaultPath = info.vault_path;
      inbox = info.inbox ?? "inbox";
      if (vaultPath) {
        await refresh();
        status = await api.workerStatus(); // F4a M2: do not miss the first status
      }
      loaded = true;
    })();
    const unStatus = api.onStatus((s) => {
      const finished = status.busy && !s.busy;
      status = s;
      if (finished) tasksVersion++;
    });
    const unTree = api.onTreeChanged(() => void refresh());
    const unTasks = api.onTasksChanged(() => tasksVersion++);
    const unNew = api.onNewNote(() => document.querySelector<HTMLInputElement>(".tree input")?.focus());
    const unPaused = api.onPausedChanged((p) => (status = { ...status, paused: p }));
    const unModel = api.onModelDownload((s) => (modelDownload = s));
    const unSettings = api.onOpenSettings(openSettings);
    const shortcut = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === ",") {
        e.preventDefault();
        openSettings();
      }
    };
    window.addEventListener("keydown", shortcut);
    const unClose = inTauri
      ? getCurrentWindow().onCloseRequested(async (event) => {
          event.preventDefault();
          await editor?.close();
          await api.hideToTray();
        })
      : Promise.resolve(() => {});
    return () => {
      void unStatus.then((f) => f());
      void unTree.then((f) => f());
      void unClose.then((f) => f());
      void unTasks.then((f) => f());
      void unNew.then((f) => f());
      void unPaused.then((f) => f());
      void unModel.then((f) => f());
      void unSettings.then((f) => f());
      window.removeEventListener("keydown", shortcut);
    };
  });
</script>

{#key uiLang}
{#if !loaded}
  <!-- startup: nothing to flash before we know whether the wizard is due -->
{:else if wizard}
  <Wizard bind:progress={wizardProgress} onLang={switchLanguage} onFinish={(v) => void wizardFinished(v)} />
{:else if vaultPath === null}
  <main class="welcome">
    <Logo size={64} />
    <h1>{t("welcome.title")}</h1>
    <p class="pitch">{t("welcome.pitch")}</p>
    <p class="body">{t("welcome.body")}</p>
    {#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}
    <Button variant="primary" icon={FolderOpen} onclick={chooseVault}>{t("welcome.open")}</Button>
  </main>
{:else}
  <div class="layout">
    <aside class="sidebar">
      <FileTree {entries} selected={current} onOpen={openNote} onCreate={createNote} />
    </aside>
    <section class="main">
      <ModelBanner queued={status.queued} forceOpen={showModel} onOpened={() => (showModel = false)} />
      <ReminderBanner version={tasksVersion + panelEdits} onChanged={() => tasksVersion++} />
      {#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{t("error.generic")}: {error}</Banner>{/if}
      {#if current}
        {#key current}
          <Editor bind:this={editor} path={current} {reveal} onSavedCopy={() => void refresh()} />
        {/key}
      {:else}
        <EmptyState icon={FileText} title={t("editor.empty")} />
      {/if}
    </section>
    <aside class="side-panel">
      <TaskPanel version={tasksVersion} onOpenSource={openSource} onChanged={() => panelEdits++} />
    </aside>
    <StatusBar {status} onResume={() => void api.setPaused(false)} download={modelDownload} onShowModel={() => (showModel = true)} onSettings={openSettings} />
    <AddedToast onChanged={() => tasksVersion++} />
  </div>
{/if}
{#if settingsOpen && !wizard}
  <Settings
    bind:section={settingsSection}
    bind:refocus={settingsRefocus}
    onClose={closeSettings}
    onLanguage={settingsLanguage}
    onTheme={settingsTheme}
    onSwitchVault={settingsSwitchVault}
  />
{/if}
{/key}

<style>
  .welcome { margin: auto; max-width: 26rem; padding: var(--space-8) var(--space-4); display: flex; flex-direction: column; align-items: center; gap: var(--space-3); text-align: center; }
  h1 { margin: var(--space-2) 0 0; font-size: var(--text-2xl); font-weight: 600; }
  p { margin: 0; }
  .pitch { font-size: var(--text-lg); color: var(--color-text); }
  .body { color: var(--color-text-muted); }
</style>
