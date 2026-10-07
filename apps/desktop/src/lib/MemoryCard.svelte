<script lang="ts">
  import { onMount } from "svelte";
  import Download from "@lucide/svelte/icons/download";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import { api, type DownloadState, type MemoryStatus } from "./api";
  import { lang, t, tf, type Key } from "./i18n";
  import { failureKey, formatBytes, progressLine } from "./modelText";
  import { describeError } from "./settings";
  import Button from "./ui/Button.svelte";

  // FR-MEM-003/004, owner decision A: the embedding model downloads after the language model or by
  // this button; then the notes are read in the background and the progress shows here.
  let { status }: { status: MemoryStatus } = $props();
  let download = $state<DownloadState | null>(null);
  let progress = $state<{ pending: number; total: number } | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    download = status.download;
    progress = { pending: status.pending, total: status.total };
  });

  const failure = $derived(
    download?.state === "failed"
      ? t(failureKey(download.failure.kind) as Key)
          .replace("{needed}", formatBytes(download.failure.needed ?? 0, lang))
          .replace("{available}", formatBytes(download.failure.available ?? 0, lang))
          .replace("{status}", String(download.failure.status ?? ""))
      : "",
  );

  async function start() {
    error = null;
    try {
      await api.memoryDownloadStart();
    } catch (e) {
      error = describeError(e);
    }
  }

  onMount(() => {
    const unDl = api.onMemoryDownload((s) => (download = s));
    const unProgress = api.onMemoryProgress((p) => (progress = p));
    return () => {
      void unDl.then((f) => f());
      void unProgress.then((f) => f());
    };
  });
</script>

<div class="card">
  {#if status.installed || download?.state === "done"}
    {#if progress && progress.pending > 0}
      <div class="bar" role="progressbar" aria-label={t("memory.title")} aria-valuemin="0" aria-valuemax={progress.total} aria-valuenow={progress.total - progress.pending}>
        <span style:width={`${((progress.total - progress.pending) / Math.max(progress.total, 1)) * 100}%`}></span>
      </div>
      <p class="line">{tf("memory.indexing", { done: progress.total - progress.pending, total: progress.total })}</p>
    {:else}
      <p role="status">{t("memory.ready")} {#if progress?.total}<span class="line">{tf("memory.indexed", { total: progress.total })}</span>{/if}</p>
    {/if}
  {:else if download?.state === "running" && download.progress.total > 0}
    {@const p = download.progress}
    <div class="bar" role="progressbar" aria-label={t("memory.title")} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.floor((p.received / p.total) * 100)}>
      <span style:width={`${(p.received / p.total) * 100}%`}></span>
    </div>
    <p class="line">{progressLine(p, lang)}</p>
    <div class="actions"><Button icon={Pause} onclick={() => void api.memoryDownloadPause()}>{t("memory.pause")}</Button></div>
  {:else}
    {#if download?.state === "paused"}<p class="line">{formatBytes(download.received, lang)} {t("memory.paused")}</p>{/if}
    {#if failure}<p class="error" role="alert">{failure}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      {#if download?.state === "paused" || download?.state === "failed"}
        <Button variant="primary" icon={Play} onclick={() => void start()}>{t("memory.resume")}</Button>
      {:else}
        <Button variant="primary" icon={Download} onclick={() => void start()}>{tf("memory.download", { size: formatBytes(status.entry.size, lang) })}</Button>
      {/if}
    </div>
    <p class="line">{status.entry.name} · {status.entry.source} · <a href={status.entry.licence_url} target="_blank" rel="noreferrer">{status.entry.licence}</a></p>
  {/if}
</div>

<style>
  .card { display: flex; flex-direction: column; gap: var(--space-2); margin-top: var(--space-2); }
  p { margin: 0; }
  .line { color: var(--color-text-muted); font-size: var(--text-sm); }
  .error { color: var(--color-danger); }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; }
  a { color: var(--color-link); }
  .bar { height: 0.375rem; border-radius: var(--radius-full); background: var(--color-surface-selected); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--color-accent); transition: width var(--duration-base) var(--ease-standard); }
</style>
