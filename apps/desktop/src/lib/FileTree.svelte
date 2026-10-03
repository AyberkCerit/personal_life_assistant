<script lang="ts">
  import FileText from "@lucide/svelte/icons/file-text";
  import Folder from "@lucide/svelte/icons/folder";
  import type { TreeEntry } from "./api";
  import { t } from "./i18n";
  import Icon from "./ui/Icon.svelte";

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
      <li style:padding-left={`calc(${entry.depth} * var(--space-4))`}>
        {#if entry.is_dir}
          <span class="row dir"><Icon icon={Folder} size="sm" /><span class="name">{entry.name}</span></span>
        {:else}
          {@const name = entry.name.replace(/\.md$/i, "")}
          <button
            class="row"
            class:active={entry.path === selected}
            aria-current={entry.path === selected ? "page" : undefined}
            title={name}
            onclick={() => onOpen(entry.path)}
          >
            <Icon icon={FileText} size="sm" /><span class="name">{name}</span>
          </button>
        {/if}
      </li>
    {/each}
  </ul>
</nav>

<style>
  .tree { padding: var(--space-2) 0; }
  form { padding: 0 var(--space-2) var(--space-2); }
  input { width: 100%; box-sizing: border-box; }
  ul { list-style: none; margin: 0; padding: 0 var(--space-1); }
  .row { display: flex; align-items: center; gap: var(--space-2); min-height: 1.75rem; padding: 0 var(--space-2); box-sizing: border-box; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dir { color: var(--color-text-muted); font-weight: 600; }
  button {
    width: 100%; background: none; border: 0; text-align: left; cursor: pointer;
    border-radius: var(--radius-md); color: var(--color-text-muted);
    box-shadow: inset 2px 0 0 transparent;
    transition: background-color var(--duration-fast) var(--ease-standard), color var(--duration-fast) var(--ease-standard);
  }
  button:hover { background: var(--color-surface-hover); color: var(--color-text); }
  button.active { background: var(--color-surface-selected); color: var(--color-text); box-shadow: inset 2px 0 0 var(--color-accent); }
</style>
