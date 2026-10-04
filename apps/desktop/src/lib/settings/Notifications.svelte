<script lang="ts">
  import { onMount } from "svelte";
  import Bell from "@lucide/svelte/icons/bell";
  import BellOff from "@lucide/svelte/icons/bell-off";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { api } from "../api";
  import { t } from "../i18n";
  import { describeError } from "../settings";
  import Banner from "../ui/Banner.svelte";
  import Button from "../ui/Button.svelte";
  import Icon from "../ui/Icon.svelte";

  // FR-TSK-015: say when Windows hides PLA's notifications, and where to turn them on.
  let on = $state<boolean | null>(null);
  let sent = $state(false);
  let error = $state<string | null>(null);

  async function act(action: () => Promise<void>) {
    error = null;
    try {
      await action();
    } catch (e) {
      error = describeError(e);
    }
  }

  onMount(() => {
    const check = () => void api.notificationStatus().then((v) => (on = v));
    check();
    window.addEventListener("focus", check); // back from Windows settings
    return () => window.removeEventListener("focus", check);
  });
</script>

{#if on !== null}
  <p class="state" class:off={!on} role="status"><Icon icon={on ? Bell : BellOff} />{on ? t("settings.notify.on") : t("settings.notify.off")}</p>
{/if}
<div class="actions">
  <Button icon={ExternalLink} onclick={() => void act(() => api.openPlace("notification_settings"))}>{t("settings.notify.openWindows")}</Button>
  <Button onclick={() => void act(async () => { await api.sendTestNotification(); sent = true; })}>{t("settings.notify.test")}</Button>
</div>
{#if sent}<p class="hint" role="status">{t("settings.notify.sent")}</p>{/if}
{#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}

<style>
  .state { display: flex; gap: var(--space-2); align-items: flex-start; margin: var(--space-2) 0 var(--space-3); font-size: var(--text-md); }
  .state :global(svg) { flex: none; margin-top: 2px; color: var(--color-accent); }
  .state.off :global(svg) { color: var(--color-warning); }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; }
  .hint { margin: var(--space-2) 0 0; color: var(--color-text-muted); font-size: var(--text-sm); }
</style>
