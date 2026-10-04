<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import { api, type BackupStatus } from "../api";
  import { lang, t, tf } from "../i18n";
  import { backupLine, describeError } from "../settings";
  import Banner from "../ui/Banner.svelte";
  import Button from "../ui/Button.svelte";

  let status = $state<BackupStatus | null>(null);
  let running = $state(false);
  let error = $state<string | null>(null);

  const when = (iso: string) => new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));

  async function refresh() {
    try {
      status = await api.backupStatus();
    } catch (e) {
      error = describeError(e);
    }
  }

  async function backupNow() {
    error = null;
    running = true;
    try {
      await api.backupNow(); // the result arrives as "backup-changed"
    } catch (e) {
      running = false;
      error = describeError(e);
    }
  }

  async function openFolder() {
    try {
      await api.openPlace("backups");
    } catch (e) {
      error = describeError(e);
    }
  }

  onMount(() => {
    void refresh();
    const un = api.onBackupChanged(() => {
      running = false;
      void refresh();
    });
    return () => void un.then((f) => f());
  });

  const line = $derived(status ? backupLine(status.job) : null);
</script>

<h4>{t("settings.backup.title")}</h4>
<p class="hint">{t("settings.backup.hint")}</p>
{#if line}
  <p class:failed={line.key === "settings.backup.failed"} role="status">{tf(line.key, { at: line.at ? when(line.at) : "", error: line.error ?? "" })}</p>
  {#if status && status.backups.length > 0}<p class="hint">{tf("settings.backup.count", { count: status.backups.length })}</p>{/if}
{/if}
<div class="actions">
  <Button variant="primary" disabled={running} onclick={() => void backupNow()}>{running ? t("settings.backup.running") : t("settings.backup.now")}</Button>
  {#if status && status.backups.length > 0}<Button icon={FolderOpen} onclick={() => void openFolder()}>{t("settings.backup.open")}</Button>{/if}
</div>
{#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}

<style>
  h4 { margin: var(--space-2) 0 var(--space-1); font-size: var(--text-md); font-weight: 600; }
  p { margin: 0 0 var(--space-2); font-size: var(--text-md); }
  .hint { color: var(--color-text-muted); font-size: var(--text-sm); }
  .failed { color: var(--color-danger); }
  .actions { display: flex; gap: var(--space-2); }
</style>
