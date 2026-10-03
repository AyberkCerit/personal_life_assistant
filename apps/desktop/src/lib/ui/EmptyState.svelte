<script lang="ts">
  import type { Component, Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let { icon, title, hint, children }: { icon: Component<any>; title: string; hint?: string; children?: Snippet } = $props();
</script>

<div class="empty-state">
  <span class="art"><Icon {icon} size="lg" /></span>
  <p class="title">{title}</p>
  {#if hint}<p class="hint">{hint}</p>{/if}
  {#if children}{@render children()}{/if}
</div>

<style>
  .empty-state { margin: auto; padding: var(--space-6) var(--space-4); display: flex; flex-direction: column; align-items: center; gap: var(--space-2); text-align: center; }
  .art { display: inline-grid; place-items: center; width: 2.5rem; height: 2.5rem; border-radius: var(--radius-full); background: var(--color-surface-selected); color: var(--color-text-muted); }
  p { margin: 0; }
  .title { color: var(--color-text); font-weight: 600; }
  .hint { color: var(--color-text-muted); font-size: var(--text-sm); max-width: 18rem; }
</style>
