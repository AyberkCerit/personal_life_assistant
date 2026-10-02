<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AddedItem } from "./api";
  import { t } from "./i18n";

  const SHOW_MS = 10_000; // decision 4 (FR-EXT-015: at least 8 s)
  let { onChanged }: { onChanged: () => void } = $props();
  let items = $state<Array<AddedItem & { key: number }>>([]);
  let next = 0;

  function dismiss(key: number) {
    items = items.filter((i) => i.key !== key);
  }

  async function undo(item: AddedItem & { key: number }) {
    dismiss(item.key);
    await api.undoItem(item.kind, item.id);
    onChanged();
  }

  onMount(() => {
    const un = api.onItemsAdded((added) => {
      for (const a of added) {
        const key = next++;
        items = [...items, { ...a, key }];
        setTimeout(() => dismiss(key), SHOW_MS);
      }
      onChanged();
    });
    return () => void un.then((f) => f());
  });
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each items as item (item.key)}
    <div class="toast">
      <span>{t("toast.added")}: <strong>{item.title}</strong></span>
      <button onclick={() => void undo(item)}>{t("toast.undo")}</button>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: 16px; bottom: 40px; display: flex; flex-direction: column; gap: 8px; z-index: 10; }
  .toast { background: var(--panel); border: 1px solid var(--border); border-radius: 8px; padding: 8px 12px; display: flex; gap: 12px; align-items: center; box-shadow: 0 4px 16px rgb(0 0 0 / 25%); }
  .toast button { background: var(--bg); border: 1px solid var(--accent); color: var(--accent); border-radius: 6px; padding: 2px 8px; cursor: pointer; }
</style>
