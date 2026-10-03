<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AddedItem } from "./api";
  import { ToastQueue, type Toast } from "./toasts";
  import { t } from "./i18n";
  import { fade, fly } from "svelte/transition";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Button from "./ui/Button.svelte";
  import Icon from "./ui/Icon.svelte";
  import { BASE, FAST, motion } from "./ui/motion";

  const SHOW_MS = 10_000; // decision 4 (FR-EXT-015: at least 8 s)
  const MAX_VISIBLE = 3;
  let { onChanged }: { onChanged: () => void } = $props();
  let shown = $state<Toast<AddedItem>[]>([]);
  let hidden = $state(0);
  const queue = new ToastQueue<AddedItem>(SHOW_MS, MAX_VISIBLE, undefined, undefined, () => {
    shown = queue.visible();
    hidden = queue.hiddenCount();
  });

  async function undo(toast: Toast<AddedItem>) {
    queue.dismiss(toast.key);
    try {
      await api.undoItem(toast.item.kind, toast.item.id);
    } finally {
      onChanged();
    }
  }

  onMount(() => {
    const un = api.onItemsAdded((added) => {
      for (const a of added) queue.add(a);
      onChanged();
    });
    return () => void un.then((f) => f());
  });
</script>

<!-- Timers stop while the pointer or keyboard focus is on the toasts, so Undo stays reachable. -->
<div
  class="toasts"
  role="status"
  aria-live="polite"
  onmouseenter={() => queue.pause()}
  onmouseleave={() => queue.resume()}
  onfocusin={() => queue.pause()}
  onfocusout={() => queue.resume()}
>
  {#each shown as toast (toast.key)}
    <div class="toast" in:fly={{ x: 24, duration: motion(BASE) }} out:fade={{ duration: motion(FAST) }}>
      <span class="lead"><Icon icon={Sparkles} size="md" /></span>
      <span class="text"><span class="label">{t("toast.added")}</span><strong>{toast.item.title}</strong></span>
      <Button variant="quiet" icon={Undo2} onclick={() => void undo(toast)}>{t("toast.undo")}</Button>
    </div>
  {/each}
  {#if hidden > 0}<div class="more">+{hidden} {t("toast.more")}</div>{/if}
</div>

<style>
  .toasts { position: fixed; right: var(--space-4); bottom: 2.5rem; display: flex; flex-direction: column; gap: var(--space-2); z-index: 10; max-width: 22rem; }
  .toast {
    display: flex; gap: var(--space-3); align-items: center;
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
    background: var(--color-surface-raised); border: 1px solid var(--color-border);
    border-radius: var(--radius-lg); box-shadow: var(--shadow-raised);
  }
  .lead { display: inline-grid; place-items: center; width: 1.75rem; height: 1.75rem; border-radius: var(--radius-full); background: var(--color-ai-subtle); color: var(--color-ai); flex: none; }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .label { font-size: var(--text-xs); color: var(--color-text-muted); }
  strong { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .more { align-self: flex-end; font-size: var(--text-sm); color: var(--color-text-muted); }
</style>
