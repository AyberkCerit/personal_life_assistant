<script lang="ts">
  import Hash from "@lucide/svelte/icons/hash";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import type { FrontMatter } from "./api";
  import { t, tf } from "./i18n";

  // FR-EDT-006/007: the note's tags and aliases from its frontmatter, read-only (owner decision B:
  // the YAML stays text in the editor; a tag opens the search on it).
  let { meta, onTag }: { meta: FrontMatter; onTag: (tag: string) => void } = $props();
</script>

{#if meta.state === "invalid"}
  <div class="props" role="status">
    <div class="column invalid"><TriangleAlert size={14} strokeWidth={2} aria-hidden="true" /><span>{t("props.invalid")}</span></div>
  </div>
{:else if meta.tags.length || meta.aliases.length}
  <div class="props" role="group" aria-label={t("props.label")}>
    <div class="column">
    {#if meta.tags.length}
      <ul aria-label={t("props.tags")}>
        {#each meta.tags as tag (tag)}
          <li>
            <button class="chip tag" title={tf("props.tagSearch", { tag })} onclick={() => onTag(tag)}>
              <Hash size={12} strokeWidth={2} aria-hidden="true" />{tag}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if meta.aliases.length}
      <ul aria-label={t("props.aliases")}>
        <li class="label">{t("props.aliases")}</li>
        {#each meta.aliases as alias (alias)}
          <li><span class="chip">{alias}</span></li>
        {/each}
      </ul>
    {/if}
    </div>
  </div>
{/if}

<style>
  .props { border-bottom: 1px solid var(--color-border); background: var(--color-surface-reading); }
  /* the editor's text column (72ch at its font size), so the chips line up with the note */
  .column {
    font-size: var(--text-lg); max-width: 72ch; box-sizing: border-box; margin: 0 auto; padding: var(--space-2) var(--space-8);
    display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2) var(--space-4);
  }
  .column > :global(*) { font-size: var(--text-sm); }
  .invalid { color: var(--color-warning); gap: var(--space-2); flex-wrap: nowrap; }
  ul { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-1); margin: 0; padding: 0; list-style: none; }
  .label { color: var(--color-text-muted); margin-right: var(--space-1); }
  .chip {
    display: inline-flex; align-items: center; gap: 2px; padding: 1px var(--space-2);
    border-radius: var(--radius-sm); background: var(--color-surface-panel); color: var(--color-text);
    font: inherit; border: 1px solid var(--color-border);
  }
  button.tag { color: var(--color-link); cursor: pointer; }
  button.tag:hover { background: var(--color-accent-subtle); }
  button.tag:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
</style>
