<script lang="ts">
  import type { Snippet } from "svelte";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Icon from "./Icon.svelte";

  type Kind = "ai" | "neutral" | "warning" | "danger";
  let { kind = "neutral", label, children }: { kind?: Kind; label?: string; children: Snippet } = $props();
</script>

<span class="badge {kind}" title={label}>
  {#if kind === "ai"}<Icon icon={Sparkles} size="sm" />{/if}
  <span aria-hidden={label ? "true" : undefined}>{@render children()}</span>
  {#if label}<span class="sr-only">{label}</span>{/if}
</span>

<style>
  .badge {
    display: inline-flex; align-items: center; gap: 0.1875rem;
    padding: 0 var(--space-2); border-radius: var(--radius-full);
    font-size: var(--text-xs); font-weight: 600; line-height: 1.6;
    background: var(--color-surface-selected); color: var(--color-text-muted);
  }
  .badge :global(svg) { width: 0.6875rem; height: 0.6875rem; }
  .ai { background: var(--color-ai-subtle); color: var(--color-ai); }
  .warning { background: var(--color-warning-subtle); color: var(--color-warning); }
  .danger { background: var(--color-danger-subtle); color: var(--color-danger); }
</style>
