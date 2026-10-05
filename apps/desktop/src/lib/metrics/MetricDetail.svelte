<script lang="ts">
  import { onMount } from "svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import FileText from "@lucide/svelte/icons/file-text";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import TrendingDown from "@lucide/svelte/icons/trending-down";
  import TrendingUp from "@lucide/svelte/icons/trending-up";
  import Minus from "@lucide/svelte/icons/minus";
  import { api, type MetricRecord, type Summary } from "../api";
  import { lang, t, tf, type Key } from "../i18n";
  import { fillDays, formatValue, metricError, trendDirection, type MetricKind } from "../metrics";
  import { todayIso } from "../tasks";
  import Badge from "../ui/Badge.svelte";
  import Banner from "../ui/Banner.svelte";
  import Button from "../ui/Button.svelte";
  import Icon from "../ui/Icon.svelte";
  import IconButton from "../ui/IconButton.svelte";
  import LineChart from "./LineChart.svelte";

  let {
    kind,
    version,
    onBack,
    onEdit,
    onOpenSource,
  }: { kind: MetricKind; version: number; onBack: () => void; onEdit: (r: MetricRecord) => void; onOpenSource: (path: string, text: string | null) => void } = $props();

  const PERIODS = [7, 30, 90];
  let days = $state(30);
  let summary = $state<Summary | null>(null);
  let records = $state<MetricRecord[]>([]);
  let confirming = $state<string | null>(null);
  let error = $state<string | null>(null);

  function addDays(iso: string, n: number): string {
    const [y, m, d] = iso.split("-").map(Number);
    return new Date(Date.UTC(y, m - 1, d + n)).toISOString().slice(0, 10);
  }

  async function load() {
    try {
      [summary, records] = await Promise.all([api.metricsSummary(kind, days), api.metricRecords(kind, days)]);
    } catch (e) {
      error = tf(metricError(String(e)).key, { detail: String(e) });
    }
  }

  async function act(action: () => Promise<void>) {
    error = null;
    try {
      await action();
    } catch (e) {
      const parsed = metricError(String(e));
      error = tf(parsed.key, { detail: parsed.detail ?? "" });
    }
    confirming = null;
    await load();
  }

  $effect(() => {
    void [kind, days, version];
    void load();
  });

  const chartDays = $derived(summary ? fillDays(summary.days, addDays(todayIso(), 1 - days), days) : []);
  const conflictDays = $derived(summary ? summary.days.filter((d) => d.conflict).map((d) => d.date) : []);
  const fmt = (v: number | null) => (v === null ? "—" : formatValue(kind, v, lang));
  const longDate = (iso: string) => new Intl.DateTimeFormat(lang, { weekday: "short", day: "numeric", month: "short" }).format(new Date(`${iso}T12:00:00`));
  const trend = $derived(summary ? trendDirection(summary.trend, summary.average) : "flat");
  const describe = (r: MetricRecord) => {
    if (kind !== "workout") return r.value === null ? "—" : formatValue(kind, r.value, lang);
    const parts = [r.exercise, r.sets !== null && r.reps !== null ? `${r.sets}×${r.reps}` : r.sets !== null ? `${r.sets} ${t("metrics.sets")}` : null, r.value ? `${r.value} kg` : null];
    return parts.filter(Boolean).join(" · ") || "—";
  };

  onMount(() => {
    const un = api.onMetricsChanged(() => void load());
    return () => void un.then((f) => f());
  });
</script>

<div class="detail">
  <header>
    <Button variant="quiet" icon={ArrowLeft} onclick={onBack}>{t("metrics.back")}</Button>
    <h2>{t(`metrics.kind.${kind}` as Key)}</h2>
    <div class="periods" role="radiogroup" aria-label={t(`metrics.kind.${kind}` as Key)}>
      {#each PERIODS as p (p)}
        <button role="radio" aria-checked={p === days} class:on={p === days} onclick={() => (days = p)}>{tf("metrics.period", { n: p })}</button>
      {/each}
    </div>
  </header>

  {#each conflictDays as date (date)}
    <Banner kind="warning" icon={CircleAlert} role="region" label={tf("metrics.conflict", { date: longDate(date) })}>
      <p class="text">{tf("metrics.conflict", { date: longDate(date) })}</p>
      <div class="choices">
        {#each records.filter((r) => r.date === date) as r (r.metric_id)}
          <Button onclick={() => void act(() => api.metricResolve(r.metric_id))}>{t("metrics.keep")}: {describe(r)}</Button>
        {/each}
      </div>
    </Banner>
  {/each}
  {#if error}<Banner kind="danger" icon={CircleAlert} role="alert">{error}</Banner>{/if}

  {#if summary}
    {#if summary.days.length === 0}
      <p class="empty">{t("metrics.empty")}</p>
    {:else}
      <dl class="stats">
        <div><dt>{t("metrics.average")}</dt><dd>{fmt(summary.average)}</dd></div>
        <div><dt>{t("metrics.min")}</dt><dd>{fmt(summary.min)}</dd></div>
        <div><dt>{t("metrics.max")}</dt><dd>{fmt(summary.max)}</dd></div>
        <div>
          <dt>{t("metrics.trend")}</dt>
          <dd class="trend {trend}">
            <Icon icon={trend === "up" ? TrendingUp : trend === "down" ? TrendingDown : Minus} />
            {summary.trend === null ? "—" : `${summary.trend > 0 ? "+" : ""}${fmt(summary.trend)}`}
          </dd>
        </div>
      </dl>
      <LineChart days={chartDays} {kind} />
      <section>
        <h3>{t("metrics.weekly")}</h3>
        <ul class="weeks">
          {#each summary.weekly as w (w.week_start)}
            <li><span>{tf("metrics.weekOf", { date: longDate(w.week_start) })}</span><span>{fmt(w.average)}</span></li>
          {/each}
        </ul>
      </section>
    {/if}
  {/if}

  <section>
    <h3>{t("metrics.records")}</h3>
    <ul class="records">
      {#each records as r (r.metric_id)}
        <li>
          <span class="date">{longDate(r.date)}</span>
          <span class="value">{describe(r)}</span>
          {#if r.origin === "extracted" && !r.user_modified}<Badge kind="ai"><Icon icon={Sparkles} size="sm" />AI</Badge>{/if}
          <span class="actions">
            {#if confirming === r.metric_id}
              <span class="ask">{t("metrics.confirmDelete")}</span>
              <Button variant="danger" onclick={() => void act(() => api.metricDelete(r.metric_id))}>{t("metrics.delete")}</Button>
              <Button variant="quiet" onclick={() => (confirming = null)}>{t("metrics.cancel")}</Button>
            {:else}
              {#if r.note_path}<IconButton icon={FileText} label={t("metrics.source")} onclick={() => onOpenSource(r.note_path as string, r.block_text)} />{/if}
              <IconButton icon={Pencil} label={t("metrics.edit")} onclick={() => onEdit(r)} />
              <IconButton icon={Trash2} label={t("metrics.delete")} onclick={() => (confirming = r.metric_id)} />
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  </section>
</div>

<style>
  .detail { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-3) var(--space-4) var(--space-6); }
  header { display: flex; align-items: center; gap: var(--space-3); flex-wrap: wrap; }
  h2 { margin: 0; font-size: var(--text-xl); font-weight: 600; flex: 1; }
  h3 { margin: 0 0 var(--space-1); font-size: var(--text-md); font-weight: 600; }
  .periods { display: flex; gap: 2px; background: var(--color-surface-panel); border-radius: var(--radius-md); padding: 2px; }
  .periods button { border: 0; background: none; padding: var(--space-1) var(--space-2); border-radius: var(--radius-sm); color: var(--color-text-muted); font: inherit; font-size: var(--text-sm); cursor: pointer; }
  .periods button.on { background: var(--color-surface-raised); color: var(--color-text); }
  .periods button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  .detail > :global(.banner) { margin: 0; }
  .text { margin: 0 0 var(--space-2); }
  .choices { display: flex; gap: var(--space-2); flex-wrap: wrap; }
  .empty { color: var(--color-text-muted); margin: 0; }
  .stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--space-2); margin: 0; }
  .stats div { background: var(--color-surface-panel); border-radius: var(--radius-md); padding: var(--space-2) var(--space-3); min-width: 0; }
  dt { font-size: var(--text-xs); color: var(--color-text-muted); }
  dd { margin: var(--space-1) 0 0; font-size: var(--text-lg); font-weight: 600; display: flex; align-items: center; gap: var(--space-1); }
  .trend.up { color: var(--color-accent); }
  .trend.down { color: var(--color-warning); }
  ul { list-style: none; margin: 0; padding: 0; }
  .weeks li, .records li { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-1) 0; border-bottom: 1px solid var(--color-border); font-size: var(--text-md); }
  .weeks li span:last-child { margin-left: auto; font-variant-numeric: tabular-nums; }
  .date { color: var(--color-text-muted); min-width: 7rem; }
  .value { font-variant-numeric: tabular-nums; }
  .actions { margin-left: auto; display: flex; gap: var(--space-1); align-items: center; }
  .ask { font-size: var(--text-sm); color: var(--color-text-muted); }
</style>
