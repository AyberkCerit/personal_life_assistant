<script lang="ts">
  import { onMount } from "svelte";
  import Hash from "@lucide/svelte/icons/hash";
  import X from "@lucide/svelte/icons/x";
  import { api, type SearchHit, type TagCount, type TreeEntry } from "./api";
  import { t, tf } from "./i18n";
  import { snippetParts } from "./links";
  import { fold } from "./media";
  import IconButton from "./ui/IconButton.svelte";

  // FR-EDT-012/014/015: full-text search with folder and tag filters, and the tag list.
  let {
    entries,
    onOpen,
    onClose,
  }: { entries: TreeEntry[]; onOpen: (path: string, line: string | null) => void; onClose: () => void } = $props();

  let query = $state("");
  let folder = $state("");
  let tag = $state("");
  let hits = $state<SearchHit[]>([]);
  let tags = $state<TagCount[]>([]);
  let input: HTMLInputElement | undefined = $state();
  let loading = $state(false);
  let revision = $state(0); // bumped when the index changes: results follow edits (M1)
  let request = 0;

  const folders = $derived(entries.filter((e) => e.is_dir && e.depth === 0).map((e) => e.path));
  const searching = $derived(query.trim() !== "" || tag !== "");

  $effect(() => {
    const [q, f, g] = [query, folder, tag];
    void revision;
    const mine = ++request;
    if (q.trim() === "" && g === "") {
      hits = [];
      loading = false;
      return;
    }
    loading = true;
    const timer = setTimeout(() => {
      void api
        .searchNotes(q, f || null, g || null)
        .catch(() => [])
        .then((found) => {
          if (mine !== request) return;
          hits = found;
          loading = false;
        });
    }, 200);
    return () => clearTimeout(timer);
  });

  async function loadTags() {
    tags = await api.listTags().catch(() => []);
  }

  /** FR-EDT-006: a tag chip in the properties strip opens the search on that tag. */
  export function showTag(name: string) {
    tag = fold(name); // as the index keeps tags, so the list shows it selected
    query = "";
  }

  export function focus() {
    input?.focus();
    input?.select();
  }

  onMount(() => {
    focus();
    void loadTags();
    const un = api.onIndexChanged(() => {
      void loadTags();
      revision++;
    });
    return () => void un.then((f) => f());
  });
</script>

<section class="search" aria-label={t("search.title")}>
  <header>
    <input
      bind:this={input}
      bind:value={query}
      type="search"
      placeholder={t("search.placeholder")}
      aria-label={t("search.placeholder")}
      spellcheck="false"
      onkeydown={(e) => e.key === "Escape" && (query ? (query = "") : onClose())}
    />
    <IconButton icon={X} label={t("search.close")} onclick={onClose} />
  </header>
  <div class="filters">
    <select bind:value={folder} aria-label={t("search.folder")}>
      <option value="">{t("search.allFolders")}</option>
      {#each folders as f (f)}<option value={f}>{f}</option>{/each}
    </select>
    <select bind:value={tag} aria-label={t("search.tag")}>
      <option value="">{t("search.allTags")}</option>
      {#each tags as tg (tg.tag)}<option value={tg.tag}>#{tg.tag}</option>{/each}
    </select>
  </div>

  {#if searching}
    <p class="count" role="status">{hits.length ? tf("search.count", { n: hits.length }) : loading ? "" : t("search.none")}</p>
    <ul class="hits">
      {#each hits as h (h.note_path)}
        <li>
          <button onclick={() => onOpen(h.note_path, h.line_text)}>
            <span class="title">{h.title}</span>
            <span class="snippet">{#each snippetParts(h.snippet) as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</span>
            <span class="path">{h.note_path}</span>
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <h3>{t("search.tags")}</h3>
    {#if tags.length === 0}
      <p class="count">{t("search.noTags")}</p>
    {:else}
      <ul class="tags">
        {#each tags as tg (tg.tag)}
          <li><button onclick={() => (tag = tg.tag)}><Hash size={12} strokeWidth={2} aria-hidden="true" />{tg.tag}<span class="n">{tg.count}</span></button></li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .search { display: flex; flex-direction: column; gap: var(--space-2); padding: var(--space-2); min-height: 0; }
  header { display: flex; gap: var(--space-1); align-items: center; }
  input, select {
    font: inherit; font-size: var(--text-md); color: var(--color-text); background: var(--color-surface-raised);
    border: 1px solid var(--color-border-strong); border-radius: var(--radius-md); padding: var(--space-1) var(--space-2); min-width: 0;
  }
  input { flex: 1; }
  input:focus-visible, select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
  .filters { display: flex; gap: var(--space-1); }
  .filters select { flex: 1; font-size: var(--text-sm); }
  .count { margin: 0; font-size: var(--text-xs); color: var(--color-text-muted); }
  h3 { margin: var(--space-2) 0 0; font-size: var(--text-sm); font-weight: 600; color: var(--color-text-muted); }
  ul { list-style: none; margin: 0; padding: 0; }
  .hits { display: flex; flex-direction: column; gap: 2px; }
  .hits button {
    width: 100%; display: flex; flex-direction: column; gap: 2px; text-align: left; padding: var(--space-2);
    border: 0; border-radius: var(--radius-md); background: none; color: var(--color-text); font: inherit; cursor: pointer;
  }
  .hits button:hover { background: var(--color-surface-hover); }
  .hits button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  .title { font-size: var(--text-md); font-weight: 600; }
  .snippet { font-size: var(--text-sm); color: var(--color-text-muted); overflow-wrap: anywhere; }
  mark { background: var(--color-accent-subtle); color: var(--color-text); border-radius: 2px; padding: 0 1px; }
  .path { font-size: var(--text-xs); color: var(--color-text-muted); }
  .tags { display: flex; flex-wrap: wrap; gap: var(--space-1); }
  .tags button {
    display: inline-flex; align-items: center; gap: 2px; padding: 2px var(--space-2); border: 1px solid var(--color-border);
    border-radius: var(--radius-full); background: none; color: var(--color-text); font: inherit; font-size: var(--text-sm); cursor: pointer;
  }
  .tags button:hover { background: var(--color-surface-hover); }
  .n { margin-left: var(--space-1); color: var(--color-text-muted); font-size: var(--text-xs); }
</style>
