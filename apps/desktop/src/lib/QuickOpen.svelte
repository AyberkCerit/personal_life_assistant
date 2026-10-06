<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { api, type QuickHit } from "./api";
  import { t } from "./i18n";
  import { BASE, motion } from "./ui/motion";

  // FR-EDT-005: Ctrl+O, type a few letters of a title or alias, Enter.
  let { onOpen, onClose, onCancel = () => {} }: { onOpen: (path: string) => void; onClose: () => void; onCancel?: () => void } = $props();

  function cancel() {
    onClose();
    onCancel(); // focus back where it was (a chosen note takes it instead)
  }

  let query = $state("");
  let hits = $state<QuickHit[]>([]);
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let request = 0;

  $effect(() => {
    const q = query;
    const mine = ++request;
    void api
      .quickOpen(q)
      .catch(() => [])
      .then((found) => {
        if (mine !== request) return;
        hits = found;
        active = 0;
      });
  });

  function choose(i: number) {
    const hit = hits[i];
    if (!hit) return;
    onClose();
    onOpen(hit.note_path);
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (hits.length) active = (active + (e.key === "ArrowDown" ? 1 : hits.length - 1)) % hits.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(active);
    } else if (e.key === "Tab") {
      e.preventDefault(); // one field: focus stays in the switcher
    }
  }

  const folder = (p: string) => p.split("/").slice(0, -1).join("/");
  onMount(() => input?.focus());
</script>

<div class="backdrop" transition:fade={{ duration: motion(BASE) }} onclick={cancel} aria-hidden="true"></div>
<div class="dialog" role="dialog" aria-modal="true" aria-label={t("quick.title")}>
  <input
    bind:this={input}
    bind:value={query}
    onkeydown={keydown}
    placeholder={t("quick.placeholder")}
    role="combobox"
    aria-expanded={hits.length > 0}
    aria-controls="quick-list"
    aria-activedescendant={hits.length ? `quick-${active}` : undefined}
    aria-autocomplete="list"
    spellcheck="false"
  />
  <ul id="quick-list" role="listbox" aria-label={t("quick.title")}>
    {#each hits as h, i (h.note_path)}
      <li id="quick-{i}" role="option" aria-selected={i === active} class:on={i === active} onmousedown={(e) => { e.preventDefault(); choose(i); }} onmousemove={() => (active = i)}>
        <span class="title">{h.title}</span>
        {#if h.alias}<span class="alias">{t("quick.alias")}: {h.alias}</span>{/if}
        <span class="path">{folder(h.note_path)}</span>
      </li>
    {:else}
      <li class="empty" role="presentation">{t("quick.empty")}</li>
    {/each}
  </ul>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: var(--color-scrim); z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; top: 14vh; left: 50%; transform: translateX(-50%); width: min(34rem, calc(100vw - 2 * var(--space-4)));
    background: var(--color-surface-reading); border: 1px solid var(--color-border); border-radius: var(--radius-lg); box-shadow: var(--shadow-raised);
    overflow: hidden;
  }
  input {
    width: 100%; box-sizing: border-box; border: 0; border-bottom: 1px solid var(--color-border); padding: var(--space-3) var(--space-4);
    background: transparent; color: var(--color-text); font: inherit; font-size: var(--text-lg); outline: none;
  }
  ul { list-style: none; margin: 0; padding: var(--space-1); max-height: 50vh; overflow-y: auto; }
  li { display: flex; align-items: baseline; gap: var(--space-2); padding: var(--space-2) var(--space-3); border-radius: var(--radius-md); cursor: pointer; }
  li.on { background: var(--color-accent-subtle); }
  .title { font-size: var(--text-md); color: var(--color-text); }
  .alias { font-size: var(--text-sm); color: var(--color-accent); }
  .path { margin-left: auto; font-size: var(--text-xs); color: var(--color-text-muted); }
  .empty { color: var(--color-text-muted); cursor: default; font-size: var(--text-md); }
</style>
