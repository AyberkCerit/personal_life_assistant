<script lang="ts">
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Play from "@lucide/svelte/icons/play";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import type { DownloadState, WorkerStatus } from "./api";
  import { lang, t, tf, type Key } from "./i18n";
  import Badge from "./ui/Badge.svelte";
  import Button from "./ui/Button.svelte";
  import Icon from "./ui/Icon.svelte";
  import IconButton from "./ui/IconButton.svelte";

  let {
    status,
    onResume,
    download = null,
    onShowModel = () => {},
    onSettings = () => {},
    memory = null,
  }: {
    status: WorkerStatus;
    onResume: () => void;
    download?: DownloadState | null;
    onShowModel?: () => void;
    onSettings?: () => void;
    /** FR-MEM-003: chunks left to embed and all chunks. */
    memory?: { pending: number; total: number } | null;
  } = $props();
</script>

<footer class="status" role="status" aria-live="polite">
  <span class="model {status.model}" class:busy={status.busy}><span class="dot" aria-hidden="true"></span>{t(`status.model.${status.model}` as Key)}</span>
  {#if download?.state === "running" && download.progress.received < download.progress.total}
    <button class="link" onclick={onShowModel}>{t("model.status")} {new Intl.NumberFormat(lang, { style: "percent" }).format(Math.floor((download.progress.received / download.progress.total) * 100) / 100)}</button>
  {/if}
  {#if status.paused}
    <Badge kind="warning">{t("status.paused")}</Badge>
    <Button variant="quiet" icon={Play} onclick={onResume}>{t("status.resume")}</Button>
  {/if}
  {#if status.busy}<span>{t("status.busy")}</span>{/if}
  <span>{status.queued} {t("status.queued")}</span>
  <span>{status.added} {t("status.added")}</span>
  {#if memory && memory.pending > 0}<span>{tf("memory.status", { done: memory.total - memory.pending, total: memory.total })}{#if status.paused} ({t("memory.paused")}){/if}</span>{/if}
  {#if status.last_error}
    <span class="error" title={status.last_error}><Icon icon={CircleAlert} size="sm" />{status.last_error}</span>
  {/if}
  <span class="end">
    <IconButton icon={SettingsIcon} label={t("status.settings")} onclick={onSettings} />
  </span>
</footer>

<style>
  .status {
    grid-column: 1 / -1; display: flex; gap: var(--space-4); align-items: center;
    padding: 0 var(--space-3); font-size: var(--text-sm); color: var(--color-text-muted);
    border-top: 1px solid var(--color-border); background: var(--color-surface-panel); min-width: 0;
  }
  .status :global(.btn) { padding-block: 0; }
  .model { display: inline-flex; align-items: center; gap: var(--space-2); white-space: nowrap; }
  .dot { width: 0.5rem; height: 0.5rem; border-radius: var(--radius-full); background: var(--color-text-muted); }
  .running .dot { background: var(--color-accent); }
  /* only while it is working, not all the time the model is loaded (deferred design minor) */
  .running.busy .dot { animation: pulse 1.6s var(--ease-standard) infinite; }
  .error { display: inline-flex; gap: var(--space-1); align-items: center; color: var(--color-danger); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .end { margin-left: auto; display: inline-flex; }
  /* the status row is 1.75rem tall with a border: a smaller gear keeps it from pushing the page */
  .end :global(.icon-btn) { width: 1.5rem; height: 1.5rem; }
  .link { background: none; border: 0; padding: 0; color: var(--color-link); cursor: pointer; font-size: var(--text-sm); }
  @keyframes pulse { 50% { opacity: 0.35; } }
</style>
