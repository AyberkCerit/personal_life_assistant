<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import Download from "@lucide/svelte/icons/download";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Pause from "@lucide/svelte/icons/pause";
  import Play from "@lucide/svelte/icons/play";
  import { api, inTauri, type DownloadState, type ModelStatus } from "./api";
  import { lang, t, type Key } from "./i18n";
  import { failureKey, formatBytes, progressLine } from "./modelText";
  import Button from "./ui/Button.svelte";

  let { status, onDone }: { status: ModelStatus; onDone: () => void } = $props();
  let confirming = $state(false);
  // Starts from the status the banner fetched; later changes arrive as events.
  let download = $state<DownloadState | null>(untrack(() => status.download));
  let localError = $state<string | null>(null);
  const m = $derived(status.recommended);

  function fill(text: string, values: Record<string, string>) {
    return text.replace(/\{(\w+)\}/g, (_, k) => values[k] ?? "");
  }

  const failureText = $derived(
    download?.state === "failed"
      ? fill(t(failureKey(download.failure.kind) as Key), {
          needed: formatBytes(download.failure.needed ?? 0, lang),
          available: formatBytes(download.failure.available ?? 0, lang),
        })
      : "",
  );

  async function start() {
    confirming = false;
    localError = null;
    await api.modelDownloadStart();
  }

  async function chooseFile() {
    localError = null;
    const picked = inTauri ? await open({ filters: [{ name: "GGUF", extensions: ["gguf"] }] }) : "C:/Modeller/gemma.gguf";
    if (typeof picked !== "string") return;
    try {
      await api.modelUseLocal(picked);
      onDone();
    } catch (e) {
      const code = String(e).includes("not_found") ? "not_found" : "not_gguf";
      localError = t(`model.error.${code}` as Key);
    }
  }

  onMount(() => {
    const un = api.onModelDownload((s) => {
      download = s;
      if (s.state === "done") setTimeout(onDone, 3000);
    });
    return () => void un.then((f) => f());
  });
</script>

<div class="card">
  {#if download?.state === "running"}
    {@const p = download.progress}
    {#if p.received >= p.total}
      <p role="status">{t("model.verifying")}</p>
    {:else}
      <div class="bar" role="progressbar" aria-label={t("model.status")} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.floor((p.received / p.total) * 100)}>
        <span style:width={`${(p.received / p.total) * 100}%`}></span>
      </div>
      <p class="line">{progressLine(p, lang)}</p>
      <div class="actions"><Button icon={Pause} onclick={() => void api.modelDownloadPause()}>{t("model.pause")}</Button></div>
    {/if}
  {:else if download?.state === "done"}
    <p role="status">{t("model.ready")}</p>
  {:else if confirming}
    <dl>
      <dt>{m.name}</dt><dd></dd>
      <dt>{t("model.size")}</dt><dd>{formatBytes(m.size, lang)}</dd>
      <dt>{t("model.source")}</dt><dd>{m.source}</dd>
      <dt>{t("model.licence")}</dt><dd><a href={m.licence_url} target="_blank" rel="noreferrer">{m.licence}</a></dd>
      {#if status.models_dir}<dt>{t("model.saveTo")}</dt><dd class="path">{status.models_dir}</dd>{/if}
    </dl>
    <div class="actions">
      <Button variant="primary" icon={Download} onclick={() => void start()}>{t("model.start")}</Button>
      <Button variant="quiet" onclick={() => (confirming = false)}>{t("model.cancel")}</Button>
    </div>
  {:else}
    {#if download?.state === "paused"}
      <p class="line">{formatBytes(download.received, lang)} {t("model.paused")}</p>
    {:else if download?.state === "failed"}
      <p class="error" role="alert">{failureText}</p>
    {/if}
    {#if localError}<p class="error" role="alert">{localError}</p>{/if}
    <div class="actions">
      {#if download?.state === "paused" || download?.state === "failed"}
        <Button variant="primary" icon={Play} onclick={() => void start()}>{t("model.resume")}</Button>
      {:else}
        <Button variant="primary" icon={Download} onclick={() => (confirming = true)}>{t("model.download")}</Button>
      {/if}
      <Button icon={FolderOpen} onclick={() => void chooseFile()}>{t("model.chooseFile")}</Button>
    </div>
  {/if}
</div>

<style>
  .card { display: flex; flex-direction: column; gap: var(--space-2); margin-top: var(--space-2); }
  p { margin: 0; }
  .line { color: var(--color-text-muted); font-size: var(--text-sm); }
  .error { color: var(--color-danger); }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: var(--space-1) var(--space-3); margin: 0; font-size: var(--text-sm); }
  dt { color: var(--color-text-muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .path { font-family: var(--font-mono); font-size: var(--text-xs); }
  a { color: var(--color-link); }
  .bar { height: 0.375rem; border-radius: var(--radius-full); background: var(--color-surface-selected); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--color-accent); transition: width var(--duration-base) var(--ease-standard); }
</style>
