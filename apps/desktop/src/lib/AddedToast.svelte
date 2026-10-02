<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AddedItem } from "./api";
  import { ToastQueue, type Toast } from "./toasts";
  import { t } from "./i18n";

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
    <div class="toast">
      <span>{t("toast.added")}: <strong>{toast.item.title}</strong></span>
      <button onclick={() => void undo(toast)}>{t("toast.undo")}</button>
    </div>
  {/each}
  {#if hidden > 0}<div class="more">+{hidden} {t("toast.more")}</div>{/if}
</div>

<style>
  .toasts { position: fixed; right: 16px; bottom: 40px; display: flex; flex-direction: column; gap: 8px; z-index: 10; }
  .toast { background: var(--panel); border: 1px solid var(--border); border-radius: 8px; padding: 8px 12px; display: flex; gap: 12px; align-items: center; box-shadow: 0 4px 16px rgb(0 0 0 / 25%); }
  .toast button { background: var(--bg); border: 1px solid var(--accent); color: var(--accent); border-radius: 6px; padding: 2px 8px; cursor: pointer; }
  .more { align-self: flex-end; font-size: 12px; color: var(--muted); }
</style>
