<script lang="ts">
  import type { WorkerStatus } from "./api";
  import { t, type Key } from "./i18n";

  let { status, onResume }: { status: WorkerStatus; onResume: () => void } = $props();
</script>

<footer class="status" role="status" aria-live="polite">
  <span>{t(`status.model.${status.model}` as Key)}</span>
  {#if status.paused}<span>{t("status.paused")}</span> <button class="resume" onclick={onResume}>{t("status.resume")}</button>{/if}
  {#if status.busy}<span>{t("status.busy")}</span>{/if}
  <span>{status.queued} {t("status.queued")}</span>
  <span>{status.added} {t("status.added")}</span>
  {#if status.last_error}<span class="error" title={status.last_error}>⚠ {status.last_error}</span>{/if}
</footer>

<style>
  .status { grid-column: 1 / -1; display: flex; gap: 16px; align-items: center; padding: 0 12px; font-size: 12px; color: var(--muted); border-top: 1px solid var(--border); background: var(--panel); }
  .resume { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 0 6px; cursor: pointer; font-size: 12px; }
  .error { color: var(--danger); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
