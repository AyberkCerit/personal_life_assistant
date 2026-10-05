<script lang="ts">
  import { chartGeometry } from "../metrics";

  // Two weeks at a glance on an overview card; decorative, the card's text says the numbers.
  let { values }: { values: (number | null)[] } = $props();
  const W = 120;
  const H = 32;
  const g = $derived(chartGeometry(values, W, H, 3));
  // Unlike the detail chart, the sparkline joins across missing days: it shows the shape, not gaps.
  const path = $derived(
    g.points
      .filter((p) => p !== null)
      .map((p, i) => `${i ? "L" : "M"}${p.x.toFixed(1)},${p.y.toFixed(1)}`)
      .join(" "),
  );
  const last = $derived(g.points.findLast((p) => p !== null) ?? null);
</script>

<svg viewBox="0 0 {W} {H}" width={W} height={H} aria-hidden="true">
  <path d={path} />
  {#if last}<circle cx={last.x} cy={last.y} r="2" />{/if}
</svg>

<style>
  svg { display: block; overflow: visible; }
  path { fill: none; stroke: var(--color-accent); stroke-width: 1.5; stroke-linejoin: round; stroke-linecap: round; }
  circle { fill: var(--color-accent); }
</style>
