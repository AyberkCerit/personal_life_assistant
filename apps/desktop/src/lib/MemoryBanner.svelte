<script lang="ts">
  import { onMount } from "svelte";
  import Brain from "@lucide/svelte/icons/brain";
  import X from "@lucide/svelte/icons/x";
  import { api, type MemoryStatus } from "./api";
  import { t } from "./i18n";
  import MemoryCard from "./MemoryCard.svelte";
  import Banner from "./ui/Banner.svelte";
  import IconButton from "./ui/IconButton.svelte";

  // Owner decision A: someone with the language model but not the embedding model is offered it
  // here (and in Settings > AI); a download started after the language model shows here too.
  let { languageModel }: { languageModel: boolean } = $props();
  let status = $state<MemoryStatus | null>(null);
  let dismissed = $state(false);

  async function refresh() {
    status = await api.memoryStatus().catch(() => null);
  }

  onMount(() => {
    void refresh();
    const unChanged = api.onMemoryChanged(() => void refresh());
    const unModel = api.onModelChanged(() => void refresh());
    const unDl = api.onMemoryDownload((s) => {
      if (s.state === "failed" || s.state === "running") dismissed = false;
      if (s.state === "done") setTimeout(() => void refresh(), 3000);
    });
    return () => {
      void unChanged.then((f) => f());
      void unModel.then((f) => f());
      void unDl.then((f) => f());
    };
  });
</script>

{#if status && languageModel && !status.installed && !dismissed}
  <Banner kind="warning" icon={Brain} role="region" label={t("memory.title")}>
    <p class="text">{t("memory.banner")}</p>
    <MemoryCard {status} />
    {#snippet actions()}
      <IconButton icon={X} label={t("memory.close")} onclick={() => (dismissed = true)} />
    {/snippet}
  </Banner>
{/if}

<style>
  .text { margin: 0; }
</style>
