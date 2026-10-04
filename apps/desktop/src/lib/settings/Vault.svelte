<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Cloud from "@lucide/svelte/icons/cloud";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Info from "@lucide/svelte/icons/info";
  import { api, inTauri } from "../api";
  import { t, tf } from "../i18n";
  import { describeError } from "../settings";
  import { folderAdvice, type Advice } from "../wizard";
  import Banner from "../ui/Banner.svelte";
  import Button from "../ui/Button.svelte";
  import Row from "./Row.svelte";

  // FR-SET-014: one vault at a time; switching closes this one and opens the chosen folder with
  // the same checks as the wizard.
  let { onSwitchVault }: { onSwitchVault: (path: string) => Promise<void> } = $props();

  let current = $state<string | null>(null);
  let picked = $state<string | null>(null);
  let advice = $state<Advice[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function choose() {
    error = null;
    const path = inTauri ? await open({ directory: true }) : "C:/Notlar";
    if (typeof path !== "string") return;
    picked = path;
    advice = folderAdvice(await api.inspectVaultFolder(path));
  }

  async function switchTo() {
    if (!picked) return;
    busy = true;
    error = null;
    try {
      await onSwitchVault(picked);
      current = picked;
      picked = null;
    } catch (e) {
      error = describeError(e);
    } finally {
      busy = false;
    }
  }

  async function show() {
    try {
      await api.openPlace("vault");
    } catch (e) {
      error = describeError(e);
    }
  }

  onMount(() => void api.settingsGet().then((v) => (current = v.vault_path)));
</script>

<Row label={t("settings.vault.current")} hint={current ?? "—"}>
  {#if current}<Button icon={FolderOpen} onclick={() => void show()}>{t("settings.vault.show")}</Button>{/if}
</Row>
<Row label={t("settings.vault.switch")}>
  <Button onclick={() => void choose()} disabled={busy}>{t("wizard.vault.browse")}</Button>
</Row>
{#if picked}
  <div class="confirm">
    <p class="path">{picked}</p>
    {#each advice as a (a.key)}
      <Banner kind={a.tone === "info" ? "reminder" : a.tone} icon={a.tone === "danger" ? CircleAlert : a.tone === "warning" ? Cloud : Info}>{tf(a.key, { count: a.count ?? 0 })}</Banner>
    {/each}
    {#if advice.every((a) => a.usable)}
      <p>{tf("settings.vault.confirm", { path: current ?? "" })}</p>
      <div class="actions">
        <Button variant="primary" disabled={busy} onclick={() => void switchTo()}>{t("settings.vault.open")}</Button>
        <Button variant="quiet" disabled={busy} onclick={() => (picked = null)}>{t("settings.cancel")}</Button>
      </div>
    {/if}
  </div>
{/if}
{#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}

<style>
  .confirm { display: flex; flex-direction: column; gap: var(--space-2); padding: var(--space-3) 0; }
  .confirm :global(.banner) { margin: 0; }
  p { margin: 0; font-size: var(--text-md); }
  .path { font-family: var(--font-mono); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .actions { display: flex; gap: var(--space-2); }
</style>
