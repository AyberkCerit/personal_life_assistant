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
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import { followMove, treeError, type TreeActions } from "./lib/tree";
  import TaskPanel from "./lib/TaskPanel.svelte";
  import Editor from "./lib/Editor.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import StatusBar from "./lib/StatusBar.svelte";
  import Wizard from "./lib/Wizard.svelte";
  import Settings from "./lib/Settings.svelte";
  import MetricsView from "./lib/metrics/MetricsView.svelte";
  import QuickMetric from "./lib/metrics/QuickMetric.svelte";
  import ChartLine from "@lucide/svelte/icons/chart-line";
  import SearchIcon from "@lucide/svelte/icons/search";
  import Backlinks from "./lib/Backlinks.svelte";
  import QuickOpen from "./lib/QuickOpen.svelte";
  import SearchPanel from "./lib/SearchPanel.svelte";
  import type { MetricKind } from "./lib/metrics";
  import type { MetricRecord } from "./lib/api";
  import type { Section } from "./lib/settings";
  import type { Progress } from "./lib/wizard";
  import { t, tf, lang, setLang, type Lang } from "./lib/i18n";
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
  // The middle pane: the note editor or the metrics view (SRS S-06).
  let center = $state<"editor" | "metrics">("editor");
  let metricKind = $state<MetricKind | null>(null);
  let metricsVersion = $state(0);
  let quickMetric = $state<{ record: MetricRecord | null } | null>(null);
  // M3: the left sidebar shows the folders or the search; the right panel tasks or backlinks.
  let leftView = $state<"tree" | "search">("tree");
  let sideTab = $state<"tasks" | "links">("tasks");
  let quickOpen = $state(false);
  let searchPanel: ReturnType<typeof SearchPanel> | undefined = $state();
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

  function anyWindowOpen() {
    return settingsOpen || quickMetric !== null || quickOpen;
  }

  function openSearch() {
    if (wizard || vaultPath === null || anyWindowOpen()) return; // never behind an open window (I3)
    leftView = "search";
    void tick().then(() => searchPanel?.focus());
  }

  let switcherOpener: HTMLElement | null = null;

  function openQuickOpen() {
    if (wizard || vaultPath === null || anyWindowOpen()) return;
    switcherOpener = document.activeElement as HTMLElement | null;
    quickOpen = true;
  }

  /** FR-EDT-008/009: follow a wikilink; a missing target note is created in the inbox. */
  async function followLink(target: string) {
    try {
      const opened = await api.openLink(target);
      if (opened.created) await refresh();
      await openNote(opened.path);
    } catch (e) {
      const [code, what] = String(e).split("|");
      error = code === "not_a_note" ? tf("links.notANote", { target: what ?? target }) : tf("links.error", { reason: String(e) });
    }
  }

  /** FR-EDT-010: titles (or the alias that matched) for `[[` completion. */
  async function suggestLinks(query: string) {
    const hits = (await api.quickOpen(query).catch(() => [])).slice(0, 8);
    const fold = (s: string) => s.toLocaleLowerCase("tr");
    // A title two notes share is written with its path, so the link reaches the chosen one (M3).
    const shared = (h: (typeof hits)[number]) => hits.filter((o) => fold(o.title) === fold(h.title)).length > 1;
    return hits.map((h) => ({
      label: h.alias ?? h.title,
      detail: h.note_path.split("/").slice(0, -1).join("/") || null,
      insert: h.alias ?? (shared(h) ? h.note_path.replace(/\.md$/i, "") : h.title),
    }));
  }

  function openSettings() {
    if (wizard || anyWindowOpen()) return; // one window at a time: Esc and Tab belong to it
    settingsOpener = document.activeElement as HTMLElement | null;
    settingsOpen = true;
  }

  function closeSettings() {
    settingsOpen = false;
    // A language switch re-renders the app, so the element that opened the window may be gone.
    const back = settingsOpener?.isConnected ? settingsOpener : document.querySelector<HTMLElement>(".status .end button");
    back?.focus();
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
    const open = current;
    current = null;
    try {
      await vaultOpened(await api.setupVault(path, lang));
      tasksVersion++;
    } catch (e) {
      if (String(e).startsWith("vault_lost|")) {
        vaultPath = null; // nothing is open any more: the welcome screen lets the user pick one
      } else {
        current = open; // still the old vault: the note comes back (settings final review M2)
      }
      throw e;
    }
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

  async function showMetrics() {
    try {
      await editor?.close(); // the note is saved before the editor leaves the middle pane
    } catch (e) {
      error = String(e);
      return;
    }
    center = "metrics";
  }

  let quickOpener: HTMLElement | null = null;

  function openQuickMetric(record: MetricRecord | null = null) {
    if (wizard || vaultPath === null || anyWindowOpen()) return;
    quickOpener = document.activeElement as HTMLElement | null;
    quickMetric = { record };
  }

  function closeQuickMetric() {
    quickMetric = null;
    if (quickOpener?.isConnected) quickOpener.focus();
  }

  async function openNote(path: string, text: string | null = null) {
    center = "editor";
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


  function treeFailed(e: unknown) {
    const { key, detail, reason } = treeError(String(e));
    error = tf(key, { detail, reason });
  }

  /**
   * FR-EDT-004: the open note is saved before a tree action can move, rename or rewrite it, and the
   * editor re-opens it afterwards from wherever it is now (a link update may have changed its text).
   */
  async function treeChange(work: () => Promise<{ from: string; to: string | null } | null>) {
    try {
      await editor?.close();
    } catch (e) {
      error = String(e); // never lose what could not be saved
      return;
    }
    const open = current;
    current = null;
    await tick();
    try {
      const moved = await work();
      error = null;
      await refresh();
      if (moved === null) current = open;
      else current = moved.to === null ? (open === moved.from || open?.startsWith(`${moved.from}/`) ? null : open) : followMove(open, moved.from, moved.to);
    } catch (e) {
      current = open;
      treeFailed(e);
    }
  }

  const treeActions: TreeActions = {
    createNote: async (folder, title, template) => {
      try {
        const rel = template === null ? await api.createNote(folder, title) : await api.createNoteIn(folder, title, template);
        error = null;
        await refresh();
        await openNote(rel);
      } catch (e) {
        treeFailed(e);
      }
    },
    createFolder: async (parent, name) => {
      try {
        await api.createFolder(parent, name);
        error = null;
        await refresh();
      } catch (e) {
        treeFailed(e);
      }
    },
    renameNote: (path, name, updateLinks) =>
      treeChange(async () => ({ from: path, to: await api.renameNote(path, name, updateLinks) })),
    renameFolder: (path, name) => treeChange(async () => ({ from: path, to: await api.renameFolder(path, name) })),
    move: (path, folder) => treeChange(async () => ({ from: path, to: await api.moveEntry(path, folder) })),
    remove: (path) =>
      treeChange(async () => {
        await api.deleteEntry(path);
        return { from: path, to: null };
      }),
    linkCount: (path) => api.linkCount(path),
    templates: () => api.listTemplates(),
  };

  /** FR-EDT-015: today's daily note, created from the daily template when it is missing. */
  async function openToday() {
    if (wizard || vaultPath === null || anyWindowOpen()) return;
    try {
      const rel = await api.openToday(lang);
      await refresh();
      await openNote(rel);
    } catch (e) {
      treeFailed(e);
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
    const unQuick = api.onOpenQuickMetric(() => openQuickMetric());
    const unAdded = api.onItemsAdded((items) => {
      if (items.some((i) => i.kind === "metric")) metricsVersion++;
    });
    const shortcut = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === ",") {
        e.preventDefault();
        openSettings();
      } else if (e.ctrlKey && !e.shiftKey && e.key.toLocaleLowerCase("tr") === "o") {
        e.preventDefault(); // FR-EDT-005
        openQuickOpen();
      } else if (e.ctrlKey && e.shiftKey && e.key.toLocaleLowerCase("tr") === "f") {
        e.preventDefault(); // FR-EDT-014
        openSearch();
      } else if (e.ctrlKey && e.shiftKey && e.key.toLocaleLowerCase("tr") === "d") {
        e.preventDefault(); // FR-EDT-015
        void openToday();
      } else if (e.ctrlKey && e.shiftKey && e.key.toLocaleLowerCase("tr") === "m") {
        // by the letter, not the key position: on a Turkish F keyboard M sits elsewhere
        e.preventDefault(); // FR-MET-003
        openQuickMetric();
      }
    };
    window.addEventListener("keydown", shortcut);
    const unClose = inTauri
      ? getCurrentWindow().onCloseRequested(async (event) => {
          // No vault open: nothing runs in the background, so closing quits (deferred F4c minor).
          if (vaultPath === null) return;
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
      void unQuick.then((f) => f());
      void unAdded.then((f) => f());
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
      <nav class="shortcuts" aria-label={t("metrics.title")}>
        <button class:on={center === "metrics"} aria-current={center === "metrics" ? "page" : undefined} onclick={showMetrics}>
          <ChartLine size={16} strokeWidth={1.75} aria-hidden="true" />{t("metrics.title")}
        </button>
        <button class:on={leftView === "search"} title={t("search.open")} onclick={() => (leftView === "search" ? (leftView = "tree") : openSearch())}>
          <SearchIcon size={16} strokeWidth={1.75} aria-hidden="true" />{t("search.title")}
        </button>
        <button title={t("tree.todayHint")} onclick={() => void openToday()}>
          <CalendarDays size={16} strokeWidth={1.75} aria-hidden="true" />{t("tree.today")}
        </button>
      </nav>
      {#if leftView === "search"}
        <SearchPanel bind:this={searchPanel} {entries} onOpen={(p, line) => void openNote(p, line)} onClose={() => (leftView = "tree")} />
      {:else}
        <FileTree {entries} selected={center === "editor" ? current : null} {inbox} onOpen={openNote} actions={treeActions} />
      {/if}
    </aside>
    <section class="main">
      <ModelBanner queued={status.queued} forceOpen={showModel} onOpened={() => (showModel = false)} />
      <ReminderBanner version={tasksVersion + panelEdits} onChanged={() => tasksVersion++} />
      {#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{t("error.generic")}: {error}</Banner>{/if}
      {#if center === "metrics"}
        <MetricsView
          bind:selected={metricKind}
          version={metricsVersion + tasksVersion}
          onQuick={() => openQuickMetric()}
          onEdit={(r) => openQuickMetric(r)}
          onOpenSource={openSource}
        />
      {:else if current}
        {#key current}
          <Editor bind:this={editor} path={current} {reveal} onSavedCopy={() => void refresh()} onOpenLink={(target) => void followLink(target)} suggest={suggestLinks} />
        {/key}
      {:else}
        <EmptyState icon={FileText} title={t("editor.empty")} />
      {/if}
    </section>
    <aside class="side-panel">
      <div class="side-tabs" role="tablist" aria-label={t("links.panel")}>
        <button role="tab" aria-selected={sideTab === "tasks"} class:on={sideTab === "tasks"} onclick={() => (sideTab = "tasks")}>{t("links.tab.tasks")}</button>
        <button role="tab" aria-selected={sideTab === "links"} class:on={sideTab === "links"} onclick={() => (sideTab = "links")}>{t("links.tab.links")}</button>
      </div>
      {#if sideTab === "tasks"}
        <TaskPanel version={tasksVersion} onOpenSource={openSource} onChanged={() => panelEdits++} />
      {:else}
        <Backlinks path={center === "editor" ? current : null} onOpen={(p, line) => void openNote(p, line)} />
      {/if}
    </aside>
    <StatusBar {status} onResume={() => void api.setPaused(false)} download={modelDownload} onShowModel={() => (showModel = true)} onSettings={openSettings} />
    <AddedToast onChanged={() => tasksVersion++} />
  </div>
{/if}
{#if quickOpen && !wizard}
  <QuickOpen onOpen={(p) => void openNote(p)} onClose={() => (quickOpen = false)} onCancel={() => (switcherOpener?.isConnected ? switcherOpener.focus() : undefined)} />
{/if}
{#if quickMetric && !wizard}
  <QuickMetric record={quickMetric.record} onClose={closeQuickMetric} onSaved={() => metricsVersion++} />
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
  .shortcuts { padding: var(--space-2) var(--space-2) 0; display: flex; flex-direction: column; gap: 2px; }
  .side-tabs { display: flex; gap: var(--space-3); padding: var(--space-2) var(--space-3) 0; border-bottom: 1px solid var(--color-border); }
  .side-tabs button {
    border: 0; background: none; padding: var(--space-1) 0 var(--space-2); color: var(--color-text-muted); font: inherit; font-size: var(--text-md);
    border-bottom: 2px solid transparent; margin-bottom: -1px; cursor: pointer;
  }
  .side-tabs button.on { color: var(--color-text); border-bottom-color: var(--color-accent); }
  .side-tabs button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .shortcuts button {
    width: 100%; display: flex; align-items: center; gap: var(--space-2); padding: var(--space-1) var(--space-2);
    border: 0; border-radius: var(--radius-md); background: none; color: var(--color-text); font: inherit; font-size: var(--text-md); cursor: pointer;
  }
  .shortcuts button:hover { background: var(--color-surface-hover); }
  .shortcuts button.on { background: var(--color-accent-subtle); }
  .shortcuts button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  .welcome { margin: auto; max-width: 26rem; padding: var(--space-8) var(--space-4); display: flex; flex-direction: column; align-items: center; gap: var(--space-3); text-align: center; }
  h1 { margin: var(--space-2) 0 0; font-size: var(--text-2xl); font-weight: 600; }
  p { margin: 0; }
  .pitch { font-size: var(--text-lg); color: var(--color-text); }
  .body { color: var(--color-text-muted); }
</style>
