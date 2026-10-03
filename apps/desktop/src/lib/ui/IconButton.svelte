<script lang="ts">
  import type { Component } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";

  let {
    icon,
    label,
    element = $bindable(),
    ...rest
  }: { icon: Component<any>; label: string; element?: HTMLButtonElement } & HTMLButtonAttributes = $props();
</script>

<button type="button" {...rest} class="icon-btn" aria-label={label} title={label} bind:this={element}>
  <Icon {icon} size="md" />
</button>

<style>
  .icon-btn {
    display: inline-grid; place-items: center; flex: none;
    width: 1.75rem; height: 1.75rem; padding: 0;
    border: 0; border-radius: var(--radius-md);
    background: transparent; color: var(--color-text-muted); cursor: pointer;
    transition: background-color var(--duration-fast) var(--ease-standard), color var(--duration-fast) var(--ease-standard);
  }
  .icon-btn:hover:not(:disabled) { background: var(--color-surface-hover); color: var(--color-text); }
  .icon-btn:disabled { opacity: 0.5; cursor: default; }
</style>
