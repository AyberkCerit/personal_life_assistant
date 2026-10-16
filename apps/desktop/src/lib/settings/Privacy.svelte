<script lang="ts">
  import type { Failure } from "../i18n";
  import ErrorBanner from "../ui/ErrorBanner.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Download from "@lucide/svelte/icons/download";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import { api, inTauri } from "../api";
  import { onMount } from "svelte";
  import ArchiveRestore from "@lucide/svelte/icons/archive-restore";
  import { lang, t, tf } from "../i18n";
  import { backupDay, describeError } from "../settings";
  import Button from "../ui/Button.svelte";
  import Row from "./Row.svelte";

  let { onBeforeRestore }: { onBeforeRestore: () => Promise<void> } = $props();

  let saved = $state<string[]>([]);
  let busy = $state(false);
  let error = $state<string | Failure | null>(null);

  // NFR-SEC-008: tasks and measurements as CSV files the user keeps.
  async function exportData() {
    error = null;
    const dir = inTauri ? await open({ directory: true }) : "C:/Belgeler";
    if (typeof dir !== "string") return;
    busy = true;
    try {
      saved = await api.exportData(dir, lang);
    } catch (e) {
      error = describeError(e);
    } finally {
      busy = false;
    }
  }

  // FR-BKP-005: one of the vault's last 7 backups, after a confirmation
  let backups = $state<string[]>([]);
  let chosen = $state<string | null>(null);
  let confirming = $state(false);
  let restoring = $state(false);
  onMount(() => {
    if (inTauri) void api.backupList().then((b) => (backups = b)).catch(() => {});
  });
  const dayOf = (name: string) => {
    const [y, m, d] = (backupDay(name) ?? "").split("-").map(Number);
    return y ? new Intl.DateTimeFormat(lang, { day: "numeric", month: "long", year: "numeric" }).format(new Date(y, m - 1, d)) : name;
  };
  async function restore() {
    if (!chosen) return;
    restoring = true;
    error = null;
    try {
      await onBeforeRestore(); // what is being typed is saved before the page reloads
      await api.restoreBackup(chosen);
      location.reload(); // every view reads the restored data afresh
    } catch (e) {
      error = describeError(e);
      restoring = false;
      confirming = false;
    }
  }

  async function act(action: () => Promise<void>) {
    error = null;
    try {
      await action();
    } catch (e) {
      error = describeError(e);
    }
  }
</script>

<p class="statement">{t("settings.privacy.statement")}</p>
<Row label={t("settings.privacy.export")} hint={t("settings.privacy.export.hint")}>
  <Button icon={Download} disabled={busy} onclick={() => void exportData()}>{t("settings.privacy.exportButton")}</Button>
</Row>
{#if saved.length > 0}
  <div role="status" class="saved">
    <p>{t("settings.privacy.exported")}</p>
    <ul>{#each saved as file (file)}<li>{file}</li>{/each}</ul>
  </div>
{/if}
<Row label={t("restore.settings")} hint={t("restore.settings.hint")}>
  {#if backups.length === 0}
    <span class="none">{t("restore.settings.none")}</span>
  {:else}
    <select bind:value={chosen} aria-label={t("restore.settings")} disabled={restoring}>
      <option value={null} disabled selected>…</option>
      {#each backups as name (name)}<option value={name}>{dayOf(name)}</option>{/each}
    </select>
    <Button icon={ArchiveRestore} disabled={!chosen || restoring} onclick={() => (confirming = true)}>{t("restore.restore")}</Button>
  {/if}
</Row>
{#if confirming && chosen}
  <div class="confirm" role="alertdialog" aria-label={t("restore.settings")}>
    <p>{tf("restore.confirm", { day: dayOf(chosen) })}</p>
    <Button variant="quiet" disabled={restoring} onclick={() => (confirming = false)}>{t("qa.cancel")}</Button>
    <Button variant="primary" disabled={restoring} onclick={() => void restore()}>{restoring ? t("restore.busy") : t("restore.restore")}</Button>
  </div>
{/if}
<div class="actions">
  <Button icon={FolderOpen} onclick={() => void act(() => api.openPlace("app_data"))}>{t("settings.privacy.appData")}</Button>
  <Button icon={FolderOpen} onclick={() => void act(() => api.openPlace("local_data"))}>{t("settings.privacy.localData")}</Button>
</div>
{#if error}<ErrorBanner {error} />{/if}

<style>
  .none { font-size: var(--text-sm); color: var(--color-text-muted); }
  select { font: inherit; font-size: var(--text-md); color: var(--color-text); background: var(--color-surface-raised); border: 1px solid var(--color-border); border-radius: var(--radius-md); padding: 2px var(--space-2); }
  .confirm { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-lg); margin: var(--space-2) 0; }
  .confirm p { flex: 1 1 100%; margin: 0; font-size: var(--text-md); }
  .statement { margin: var(--space-2) 0; font-size: var(--text-md); color: var(--color-text); }
  .saved { padding: var(--space-2) 0; font-size: var(--text-sm); }
  .saved p { margin: 0; color: var(--color-text-muted); }
  ul { margin: var(--space-1) 0 0; padding-left: var(--space-4); font-family: var(--font-mono); font-size: var(--text-xs); overflow-wrap: anywhere; }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; padding-top: var(--space-3); }
</style>
