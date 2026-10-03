<script lang="ts">
  let { size = 24 }: { size?: number } = $props();
</script>

<!-- The P monogram (spec § 8); colours follow the theme. -->
<svg width={size} height={size} viewBox="0 0 64 64" role="img" aria-label="PLA">
  <rect x="4" y="4" width="56" height="56" rx="14" class="tile" />
  <path d="M22 48V16h12a10 10 0 0 1 0 20H22" class="mark" />
  <circle cx="44" cy="46" r="4.5" class="dot" />
</svg>

<style>
  .tile { fill: var(--color-accent); }
  .mark { fill: none; stroke: var(--color-on-accent); stroke-width: 7; stroke-linecap: round; stroke-linejoin: round; }
  .dot { fill: var(--color-on-accent); }
</style>
