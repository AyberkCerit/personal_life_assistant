<script lang="ts">
  import { onMount } from "svelte";
  import Cpu from "@lucide/svelte/icons/cpu";
  import X from "@lucide/svelte/icons/x";
  import { api, type ModelStatus } from "./api";
  import { t } from "./i18n";
  import Banner from "./ui/Banner.svelte";
  import IconButton from "./ui/IconButton.svelte";
  import ModelCard from "./ModelCard.svelte";

  let { queued, forceOpen, onOpened }: { queued: number; forceOpen: boolean; onOpened: () => void } = $props();
  let status = $state<ModelStatus | null>(null);
  let dismissed = $state(false);

  async function refresh() {
    status = await api.modelStatus();
  }

  $effect(() => {
    if (forceOpen) {
      dismissed = false;
      onOpened();
    }
  });

  onMount(() => {
    void refresh();
    const un = api.onModelChanged(() => void refresh());
    const unRemoved = api.onModelRemoved(() => {
      dismissed = false; // AI just went off: offer the model again
      void refresh();
    });
    return () => {
      void un.then((f) => f());
      void unRemoved.then((f) => f());
    };
  });
</script>

{#if status && !status.installed && !dismissed}
  <Banner kind="warning" icon={Cpu} role="region" label={t("model.download")}>
    <p class="text">{t("model.banner")}</p>
    {#if queued > 0}<p class="text"><strong>{t("model.waiting")}: {queued}</strong></p>{/if}
    <ModelCard {status} onDone={() => void refresh()} />
    {#snippet actions()}
      <IconButton icon={X} label={t("model.close")} onclick={() => (dismissed = true)} />
    {/snippet}
  </Banner>
{/if}

<style>
  .text { margin: 0; }
</style>
