<script lang="ts">
  import { onMount } from "svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Plus from "@lucide/svelte/icons/plus";
  import { api, type MetricCard, type MetricRecord } from "../api";
  import { lang, t, tf, type Key } from "../i18n";
  import { formatValue, type MetricKind } from "../metrics";
  import Button from "../ui/Button.svelte";
  import Icon from "../ui/Icon.svelte";
  import MetricDetail from "./MetricDetail.svelte";
  import Sparkline from "./Sparkline.svelte";

  // SRS S-06: the middle pane shows this instead of the editor (owner decision A: overview first,
  // a card opens the detail).
  let {
    selected = $bindable(),
    version,
    onQuick,
    onEdit,
    onOpenSource,
  }: {
    selected: MetricKind | null;
    version: number;
    onQuick: () => void;
    onEdit: (r: MetricRecord) => void;
    onOpenSource: (path: string, text: string | null) => void;
  } = $props();

  let cards = $state<MetricCard[]>([]);

  async function load() {
    cards = await api.metricsOverview();
  }

  $effect(() => {
    void version;
    void load();
  });

  onMount(() => {
    const un = api.onMetricsChanged(() => void load());
    return () => void un.then((f) => f());
  });
</script>

{#if selected}
  <MetricDetail kind={selected} {version} onBack={() => (selected = null)} {onEdit} {onOpenSource} />
{:else}
  <div class="overview">
    <header>
      <h2>{t("metrics.title")}</h2>
      <Button variant="primary" icon={Plus} onclick={onQuick}>{t("metrics.add")}</Button>
    </header>
    <ul class="cards">
      {#each cards as c (c.kind)}
        <li>
          <button class="card" onclick={() => (selected = c.kind)}>
            <span class="name">{t(`metrics.kind.${c.kind}` as Key)}{#if c.conflicts > 0}<span class="flag" title={tf("metrics.conflict", { date: "" })}><Icon icon={CircleAlert} size="sm" /></span>{/if}</span>
            {#if c.last}
              <span class="value">{formatValue(c.kind, c.last.value, lang, c.last.sets)}</span>
              <span class="meta">{t("metrics.last")} · {new Intl.DateTimeFormat(lang, { day: "numeric", month: "short" }).format(new Date(`${c.last.date}T12:00:00`))}</span>
              {#if c.average7 !== null}<span class="meta">{t("metrics.avg7")}: {formatValue(c.kind, c.average7, lang)}</span>{/if}
              <Sparkline values={c.spark} />
            {:else}
              <span class="meta none">{t("metrics.none")}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  </div>
{/if}

<style>
  .overview { padding: var(--space-3) var(--space-4) var(--space-6); display: flex; flex-direction: column; gap: var(--space-3); }
  header { display: flex; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: var(--text-xl); font-weight: 600; }
  .cards { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr)); gap: var(--space-3); }
  .card {
    width: 100%; height: 100%; display: flex; flex-direction: column; align-items: flex-start; gap: var(--space-1); text-align: left;
    padding: var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-lg);
    background: var(--color-surface-panel); color: var(--color-text); font: inherit; cursor: pointer;
  }
  .card:hover { background: var(--color-surface-hover); }
  .card:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .name { display: flex; align-items: center; gap: var(--space-1); font-size: var(--text-sm); color: var(--color-text-muted); }
  .flag { color: var(--color-warning); display: inline-flex; }
  .value { font-size: var(--text-xl); font-weight: 600; font-variant-numeric: tabular-nums; }
  .meta { font-size: var(--text-xs); color: var(--color-text-muted); }
  .none { margin-top: var(--space-2); }
  .card :global(svg) { margin-top: auto; padding-top: var(--space-2); }
</style>
