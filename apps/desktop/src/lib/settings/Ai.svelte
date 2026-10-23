<script lang="ts">
  import type { Failure } from "../i18n";
  import ErrorBanner from "../ui/ErrorBanner.svelte";
  import { onMount } from "svelte";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { api, type ModelStatus, type Persona, type PersonaKey } from "../api";
  import { lang, t, tf } from "../i18n";
  import { formatBytes } from "../modelText";
  import { attempt, describeError } from "../settings";
  import ModelCard from "../ModelCard.svelte";
  import MemoryCard from "../MemoryCard.svelte";
  import type { MemoryStatus } from "../api";
  import Button from "../ui/Button.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Row from "./Row.svelte";

  let status = $state<ModelStatus | null>(null);
  let paused = $state(false);
  let confirming = $state(false);
  let busy = $state(false);
  let notice = $state<string | null>(null);
  let error = $state<string | Failure | null>(null);

  let memoryStatus = $state<MemoryStatus | null>(null);
  let persona = $state<Persona | null>(null);

  async function refresh() {
    status = await api.modelStatus();
    memoryStatus = await api.memoryStatus().catch(() => null);
  }

  async function remove() {
    busy = true;
    error = null;
    try {
      await api.modelRemove();
      confirming = false;
      notice = t("settings.ai.removed");
      await refresh();
    } catch (e) {
      error = describeError(e);
    } finally {
      busy = false;
    }
  }

  async function act(action: () => Promise<void>) {
    error = null;
    error = await attempt(action);
  }

  /** Saved as soon as the field is left; the field shows the text as it was kept (one line, 40 characters). */
  async function setPersona(key: PersonaKey, field: HTMLInputElement | HTMLSelectElement) {
    await act(async () => {
      persona = (await api.settingsSet(key, field.value)).persona;
      field.value = key === "assistant_name" ? persona.name : key === "user_address" ? persona.address : persona.formality;
    });
  }

  onMount(() => {
    void refresh();
    void api.settingsGet().then((v) => {
      paused = v.paused;
      persona = v.persona;
    });
    const unChanged = api.onModelChanged(() => {
      notice = null;
      void refresh();
    });
    const unRemoved = api.onModelRemoved(() => void refresh());
    const unPaused = api.onPausedChanged((p) => (paused = p));
    return () => {
      void unChanged.then((f) => f());
      void unRemoved.then((f) => f());
      void unPaused.then((f) => f());
    };
  });
</script>

{#if status?.installed}
  {@const m = status.installed}
  <dl>
    <dt>{m.name}</dt><dd></dd>
    <dt>{t("settings.ai.size")}</dt><dd>{formatBytes(m.size, lang)}</dd>
    {#if !m.local}<dt>{t("settings.ai.licence")}</dt><dd><a href={status.recommended.licence_url} target="_blank" rel="noreferrer">{status.recommended.licence}</a></dd>{/if}
    <dt>{t("settings.ai.location")}</dt><dd class="path">{m.path}</dd>
  </dl>
  {#if confirming}
    <div class="confirm" role="group" aria-label={t("settings.ai.remove")}>
      <p>{m.local ? t("settings.ai.confirmUnlink") : tf("settings.ai.confirmDelete", { size: formatBytes(m.size, lang) })}</p>
      <div class="actions">
        <Button variant="danger" icon={Trash2} disabled={busy} onclick={() => void remove()}>{t("settings.ai.remove")}</Button>
        <Button variant="quiet" disabled={busy} onclick={() => (confirming = false)}>{t("settings.cancel")}</Button>
      </div>
    </div>
  {:else}
    <div class="actions">
      <Button icon={FolderOpen} onclick={() => void act(() => api.openPlace("model_folder"))}>{t("settings.vault.show")}</Button>
      <Button icon={Trash2} onclick={() => (confirming = true)}>{t("settings.ai.remove")}</Button>
    </div>
  {/if}
{:else if status}
  {#if notice}<p role="status">{notice}</p>{/if}
  <p class="hint">{t("model.banner")}</p>
  <ModelCard {status} onDone={() => void refresh()} />
{/if}
{#if memoryStatus}
  <h3 class="sub">{t("memory.title")}</h3>
  <MemoryCard status={memoryStatus} />
{/if}
<Row label={t("settings.ai.pause")} hint={t("settings.ai.pause.hint")}>
  <Checkbox checked={paused} label={t("settings.ai.pause")} hideLabel onchange={(on) => void act(() => api.setPaused(on))} />
</Row>
{#if persona}
  <h3 class="sub">{t("settings.ai.persona")}</h3>
  <Row label={t("settings.ai.name")} hint={t("settings.ai.name.hint")} forId="set-ai-name">
    <input id="set-ai-name" maxlength="40" placeholder="PLA" value={persona.name} onchange={(e) => void setPersona("assistant_name", e.currentTarget)} />
  </Row>
  <Row label={t("settings.ai.address")} hint={t("settings.ai.address.hint")} forId="set-ai-address">
    <input id="set-ai-address" maxlength="40" placeholder={t("settings.ai.address.placeholder")} value={persona.address} onchange={(e) => void setPersona("user_address", e.currentTarget)} />
  </Row>
  <Row label={t("settings.ai.formality")} hint={t("settings.ai.formality.hint")} forId="set-ai-formality">
    <select id="set-ai-formality" value={persona.formality} onchange={(e) => void setPersona("formality", e.currentTarget)}>
      <option value="informal">{t("settings.ai.formality.informal")}</option>
      <option value="formal">{t("settings.ai.formality.formal")}</option>
    </select>
  </Row>
{/if}
{#if error}<ErrorBanner {error} />{/if}

<style>
  dl { display: grid; grid-template-columns: max-content 1fr; gap: var(--space-1) var(--space-3); margin: var(--space-2) 0; font-size: var(--text-md); }
  dt { color: var(--color-text-muted); }
  dt:first-child { color: var(--color-text); font-weight: 600; }
  dd { margin: 0; overflow-wrap: anywhere; }
  .path { font-family: var(--font-mono); font-size: var(--text-sm); }
  a { color: var(--color-link); }
  p { margin: 0 0 var(--space-2); font-size: var(--text-md); }
  .hint { color: var(--color-text-muted); }
  .sub { margin: var(--space-3) 0 0; font-size: var(--text-md); font-weight: 600; }
  input { width: 14rem; }
  .confirm { padding: var(--space-2) 0; }
  .actions { display: flex; gap: var(--space-2); padding-bottom: var(--space-3); border-bottom: 1px solid var(--color-border); }
</style>
