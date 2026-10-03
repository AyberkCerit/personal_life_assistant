<script lang="ts">
  import type { Component, Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";

  type Variant = "primary" | "secondary" | "quiet" | "danger";
  let {
    variant = "secondary",
    icon,
    element = $bindable(),
    children,
    ...rest
  }: { variant?: Variant; icon?: Component<any>; element?: HTMLButtonElement; children: Snippet } & HTMLButtonAttributes = $props();
</script>

<button type="button" {...rest} class="btn {variant}" bind:this={element}>
  {#if icon}<Icon {icon} size="sm" />{/if}
  {@render children()}
</button>

<style>
  .btn {
    display: inline-flex; align-items: center; gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border); border-radius: var(--radius-md);
    background: var(--color-surface-reading); color: var(--color-text);
    font-size: var(--text-sm); line-height: 1.4; cursor: pointer; white-space: nowrap;
    transition: background-color var(--duration-fast) var(--ease-standard), color var(--duration-fast) var(--ease-standard);
  }
  .btn:hover:not(:disabled) { background: var(--color-surface-hover); }
  .primary { background: var(--color-accent); border-color: transparent; color: var(--color-on-accent); font-weight: 600; }
  .primary:hover:not(:disabled) { background: var(--color-accent); filter: brightness(1.08); }
  .quiet { background: transparent; border-color: transparent; color: var(--color-text-muted); }
  .quiet:hover:not(:disabled) { color: var(--color-text); }
  .danger { color: var(--color-danger); border-color: var(--color-danger-subtle); }
  .btn:disabled { opacity: 0.5; cursor: default; }
</style>
