<script lang="ts">
  import type { Snippet } from "svelte";

  // One setting: what it is (and a hint) on the left, its control on the right.
  let { label, hint, forId, children }: { label: string; hint?: string; forId?: string; children: Snippet } = $props();
</script>

<div class="row">
  <div class="text">
    {#if forId}<label for={forId}>{label}</label>{:else}<span class="label">{label}</span>{/if}
    {#if hint}<p class="hint">{hint}</p>{/if}
  </div>
  <div class="control">{@render children()}</div>
</div>

<style>
  .row { display: flex; align-items: center; justify-content: space-between; gap: var(--space-4); padding: var(--space-3) 0; border-bottom: 1px solid var(--color-border); }
  .text { min-width: 0; }
  label, .label { font-size: var(--text-md); color: var(--color-text); }
  .hint { margin: var(--space-1) 0 0; font-size: var(--text-sm); color: var(--color-text-muted); }
  .control { flex: none; display: flex; gap: var(--space-2); align-items: center; }
  .control :global(select) {
    font: inherit; font-size: var(--text-md); color: var(--color-text); background: var(--color-surface-raised);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius-md); padding: var(--space-1) var(--space-2);
  }
  .control :global(select:focus-visible) { outline: 2px solid var(--color-accent); outline-offset: 1px; }
</style>
