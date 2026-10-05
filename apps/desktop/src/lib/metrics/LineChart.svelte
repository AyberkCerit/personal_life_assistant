<script lang="ts">
  import { lang, t, type Key } from "../i18n";
  import { chartGeometry, formatValue, type ChartDay, type MetricKind } from "../metrics";

  // FR-MET-009: daily values over the period; gaps where nothing was recorded, conflicts marked.
  let { days, kind }: { days: ChartDay[]; kind: MetricKind } = $props();

  let measured = $state(600);
  const H = 180;
  const PAD = 14;
  const width = $derived(Math.max(measured, 2 * PAD + 1)); // a hidden pane measures 0
  const g = $derived(chartGeometry(days.map((d) => d.value), width, H, PAD));
  const known = $derived(days.filter((d) => d.value !== null).map((d) => d.value as number));
  const hi = $derived(known.length ? Math.max(...known) : null);
  const lo = $derived(known.length ? Math.min(...known) : null);
  const short = (iso: string) => new Intl.DateTimeFormat(lang, { day: "numeric", month: "short" }).format(new Date(`${iso}T12:00:00`));
  const label = $derived(
    `${t(`metrics.kind.${kind}` as Key)}: ${known.length} / ${days.length}` +
      (lo !== null && hi !== null ? ` · ${formatValue(kind, lo, lang)} – ${formatValue(kind, hi, lang)}` : ""),
  );
</script>

<figure bind:clientWidth={measured}>
  <svg viewBox="0 0 {width} {H}" width={width} height={H} role="img" aria-label={label}>
    <line class="grid" x1={PAD} x2={width - PAD} y1={PAD} y2={PAD} />
    <line class="grid" x1={PAD} x2={width - PAD} y1={H - PAD} y2={H - PAD} />
    <path d={g.path} />
    {#each g.points as p, i (i)}
      {#if p}
        <circle cx={p.x} cy={p.y} r={days[i].conflict ? 4 : 2.5} class:conflict={days[i].conflict}>
          <title>{short(days[i].date)}: {formatValue(kind, days[i].value as number, lang)}</title>
        </circle>
      {/if}
    {/each}
  </svg>
  <figcaption>
    <span>{days.length ? short(days[0].date) : ""}</span>
    {#if hi !== null && lo !== null}<span class="range">{formatValue(kind, lo, lang)} – {formatValue(kind, hi, lang)}</span>{/if}
    <span>{days.length ? short(days[days.length - 1].date) : ""}</span>
  </figcaption>
</figure>

<style>
  figure { margin: 0; min-width: 0; }
  svg { display: block; }
  .grid { stroke: var(--color-border); stroke-width: 1; }
  path { fill: none; stroke: var(--color-accent); stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
  circle { fill: var(--color-accent); }
  circle.conflict { fill: var(--color-warning); }
  figcaption { display: flex; justify-content: space-between; font-size: var(--text-xs); color: var(--color-text-muted); padding: var(--space-1) var(--space-1) 0; }
</style>
