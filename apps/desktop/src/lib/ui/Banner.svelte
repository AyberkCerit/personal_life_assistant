<script lang="ts">
  import type { Component, Snippet } from "svelte";
  import { slide } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { BASE, motion } from "./motion";

  let {
    kind,
    icon,
    role,
    label,
    children,
    actions,
  }: {
    kind: "reminder" | "warning" | "danger";
    icon: Component<any>;
    role?: "alert" | "status" | "region";
    label?: string;
    children: Snippet;
    actions?: Snippet;
  } = $props();
</script>

<section class="banner {kind}" {role} aria-label={label} transition:slide={{ duration: motion(BASE) }}>
  <span class="lead"><Icon {icon} size="md" /></span>
  <div class="body">{@render children()}</div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</section>

<style>
  .banner {
    display: flex; gap: var(--space-3); align-items: flex-start;
    margin: var(--space-3) var(--space-3) 0; padding: var(--space-2) var(--space-3);
    border: 1px solid; border-radius: var(--radius-lg); font-size: var(--text-md);
  }
  .lead { display: inline-flex; padding-top: 0.125rem; }
  .body { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .actions { display: flex; gap: var(--space-2); flex-wrap: wrap; }
  .reminder { background: var(--color-banner); border-color: var(--color-banner-border); }
  .reminder .lead { color: var(--color-accent); }
  .warning { background: var(--color-warning-subtle); border-color: transparent; }
  .warning .lead { color: var(--color-warning); }
  .danger { background: var(--color-danger-subtle); border-color: transparent; }
  .danger .lead { color: var(--color-danger); }
</style>
