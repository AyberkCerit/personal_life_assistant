<script lang="ts">
  import { api } from "./api";
  import { failure, lang, t, tf, type Failure } from "./i18n";
  import { offerText, type RestoreOffer } from "./settings";
  import Button from "./ui/Button.svelte";
  import ErrorBanner from "./ui/ErrorBanner.svelte";

  // FR-BKP-004/008, FR-LOG-007 (M6 acceptance): asked once, before the vault is used. Background AI
  // waits meanwhile (the session started paused for it).
  let { offer, onDone }: { offer: RestoreOffer; onDone: () => void } = $props();

  const said = $derived(offerText(offer));
  const day = $derived.by(() => {
    if (!said.day) return "";
    const [y, m, d] = said.day.split("-").map(Number);
    return new Intl.DateTimeFormat(lang, { day: "numeric", month: "long", year: "numeric" }).format(new Date(y, m - 1, d));
  });
  let busy = $state(false);
  let error = $state<Failure | null>(null);

  async function restore() {
    if (!offer.backup) return;
    busy = true;
    error = null;
    try {
      await api.restoreBackup(offer.backup);
      location.reload(); // every view reads the restored data afresh
    } catch (e) {
      error = failure(e);
      busy = false;
    }
  }

  async function keepEmpty() {
    await api.keepEmpty().catch(() => {});
    onDone();
  }
</script>

<div class="backdrop" aria-hidden="true"></div>
<div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="restore-title" aria-describedby="restore-body">
  <h2 id="restore-title">{t("restore.title")}</h2>
  <p id="restore-body">{tf(said.key, { day })}</p>
  {#if said.canRestore}<p class="muted">{t("restore.notesSafe")}</p>{/if}
  {#if error}<ErrorBanner {error} />{/if}
  <footer>
    {#if said.canRestore}
      <Button variant="quiet" disabled={busy} onclick={() => void keepEmpty()}>{t("restore.empty")}</Button>
      <Button variant="primary" disabled={busy} onclick={() => void restore()}>{busy ? t("restore.busy") : t("restore.restore")}</Button>
    {:else}
      <Button variant="primary" onclick={() => void keepEmpty()}>{t("restore.ok")}</Button>
    {/if}
  </footer>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: var(--color-scrim); z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; top: 20vh; left: 50%; transform: translateX(-50%); width: min(30rem, calc(100vw - 2 * var(--space-4)));
    background: var(--color-surface-reading); border: 1px solid var(--color-border); border-radius: var(--radius-lg); box-shadow: var(--shadow-raised);
    padding: var(--space-4); display: flex; flex-direction: column; gap: var(--space-3);
  }
  h2 { margin: 0; font-size: var(--text-lg); font-weight: 600; }
  p { margin: 0; font-size: var(--text-md); }
  .muted { color: var(--color-text-muted); font-size: var(--text-sm); }
  footer { display: flex; justify-content: flex-end; gap: var(--space-2); }
</style>
