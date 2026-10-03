<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Icon from "./Icon.svelte";

  let {
    checked = $bindable(false),
    label,
    hideLabel = false,
    disabled = false,
    onchange,
  }: { checked?: boolean; label: string; hideLabel?: boolean; disabled?: boolean; onchange?: (checked: boolean) => void } = $props();

  function change(e: Event & { currentTarget: HTMLInputElement }) {
    checked = e.currentTarget.checked;
    onchange?.(checked);
  }
</script>

<!-- The native input stays in the page (keyboard, screen readers); the box is drawn over it. -->
<label class="cb" class:disabled>
  <input type="checkbox" {checked} {disabled} aria-label={hideLabel ? label : undefined} onchange={change} />
  <span class="box" class:on={checked} aria-hidden="true">{#if checked}<Icon icon={Check} size="sm" />{/if}</span>
  {#if !hideLabel}<span class="text">{label}</span>{/if}
</label>

<style>
  .cb { position: relative; display: inline-flex; align-items: center; gap: var(--space-2); cursor: pointer; font-size: var(--text-md); }
  .cb.disabled { opacity: 0.5; cursor: default; }
  input { position: absolute; opacity: 0; width: 1px; height: 1px; margin: 0; }
  .box {
    display: inline-grid; place-items: center; flex: none;
    width: 1rem; height: 1rem; box-sizing: border-box;
    border: 1.5px solid var(--color-border-strong); border-radius: var(--radius-sm);
    transition: background-color var(--duration-fast) var(--ease-standard), border-color var(--duration-fast) var(--ease-standard);
  }
  .box.on { background: var(--color-accent); border-color: var(--color-accent); color: var(--color-on-accent); }
  .box :global(svg) { width: 0.75rem; height: 0.75rem; stroke-width: 3; }
  input:focus-visible + .box { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  .cb:hover .box:not(.on) { border-color: var(--color-text-muted); }
</style>
