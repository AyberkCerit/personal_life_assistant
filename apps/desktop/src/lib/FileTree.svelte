<script lang="ts">
  import type { TreeEntry } from "./api";
  import { t } from "./i18n";

  let {
    entries,
    selected,
    onOpen,
    onCreate,
  }: { entries: TreeEntry[]; selected: string | null; onOpen: (path: string) => void; onCreate: (title: string) => void } = $props();

  let title = $state("");

  function submit(e: SubmitEvent) {
    e.preventDefault();
    const name = title.trim();
    if (name) {
      onCreate(name);
      title = "";
    }
  }
</script>

<nav class="tree" aria-label={t("tree.label")}>
  <form onsubmit={submit}>
    <input bind:value={title} placeholder={t("tree.newNote")} aria-label={t("tree.newNote")} />
  </form>
  <ul>
    {#each entries as entry (entry.path)}
      <li style:padding-left="{entry.depth * 14 + 8}px">
        {#if entry.is_dir}
          <span class="dir">{entry.name}</span>
        {:else}
          <button class:active={entry.path === selected} onclick={() => onOpen(entry.path)}>
            {entry.name.replace(/\.md$/i, "")}
          </button>
        {/if}
      </li>
    {/each}
  </ul>
</nav>

<style>
  .tree { padding: 8px 0; }
  form { padding: 0 8px 8px; }
  input { width: 100%; box-sizing: border-box; background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 4px 8px; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { line-height: 26px; }
  .dir { color: var(--muted); font-weight: 600; }
  button { background: none; border: 0; padding: 0 6px; cursor: pointer; text-align: left; width: 100%; border-radius: 4px; }
  button:hover, button.active { background: var(--border); }
</style>
