<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Download from "@lucide/svelte/icons/download";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import { api, inTauri } from "../api";
  import { lang, t } from "../i18n";
  import { describeError } from "../settings";
  import Banner from "../ui/Banner.svelte";
  import Button from "../ui/Button.svelte";
  import Row from "./Row.svelte";

  let saved = $state<string[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);

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
<div class="actions">
  <Button icon={FolderOpen} onclick={() => void act(() => api.openPlace("app_data"))}>{t("settings.privacy.appData")}</Button>
  <Button icon={FolderOpen} onclick={() => void act(() => api.openPlace("local_data"))}>{t("settings.privacy.localData")}</Button>
</div>
{#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}

<style>
  .statement { margin: var(--space-2) 0; font-size: var(--text-md); color: var(--color-text); }
  .saved { padding: var(--space-2) 0; font-size: var(--text-sm); }
  .saved p { margin: 0; color: var(--color-text-muted); }
  ul { margin: var(--space-1) 0 0; padding-left: var(--space-4); font-family: var(--font-mono); font-size: var(--text-xs); overflow-wrap: anywhere; }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; padding-top: var(--space-3); }
</style>
