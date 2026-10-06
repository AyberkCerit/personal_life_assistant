<script lang="ts">
  import { onMount } from "svelte";
  import FileText from "@lucide/svelte/icons/file-text";
  import Link2 from "@lucide/svelte/icons/link-2";
  import { api, type Backlink } from "./api";
  import { t, tf } from "./i18n";
  import EmptyState from "./ui/EmptyState.svelte";
  import Icon from "./ui/Icon.svelte";

  // FR-EDT-011: every note linking to the open one, with the line the link stands on.
  let { path, onOpen }: { path: string | null; onOpen: (path: string, line: string) => void } = $props();

  let links = $state<Backlink[]>([]);
  let request = 0;

  async function load() {
    const mine = ++request;
    const found = path ? await api.backlinks(path).catch(() => []) : [];
    if (mine === request) links = found;
  }

  $effect(() => {
    void path;
    void load();
  });

  onMount(() => {
    const un = api.onIndexChanged(() => void load());
    return () => void un.then((f) => f());
  });

  const title = $derived(path ? (path.split("/").pop() ?? path).replace(/\.md$/i, "") : "");
  const groups = $derived(
    links.reduce<{ source: string; title: string; lines: Backlink[] }[]>((acc, l) => {
      const last = acc[acc.length - 1];
      if (last && last.source === l.source_path) last.lines.push(l);
      else acc.push({ source: l.source_path, title: l.source_title, lines: [l] });
      return acc;
    }, []),
  );
</script>

<section class="backlinks" aria-label={t("links.backlinks")}>
  {#if !path}
    <EmptyState icon={Link2} title={t("links.noNote")} />
  {:else if groups.length === 0}
    <EmptyState icon={Link2} title={tf("links.none", { title })} />
  {:else}
    <h3>{t("links.backlinks")}</h3>
    <ul>
      {#each groups as g (g.source)}
        <li>
          <p class="source"><Icon icon={FileText} size="sm" />{g.title}</p>
          {#each g.lines as l (l.line)}
            <button class="line" onclick={() => onOpen(l.source_path, l.line_text)}>{l.line_text}</button>
          {/each}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .backlinks { padding: var(--space-3); }
  h3 { margin: 0 0 var(--space-2); font-size: var(--text-sm); font-weight: 600; color: var(--color-text-muted); }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--space-3); }
  .source { display: flex; align-items: center; gap: var(--space-1); margin: 0 0 var(--space-1); font-size: var(--text-md); font-weight: 600; }
  .line {
    display: block; width: 100%; text-align: left; margin: 0 0 var(--space-1); padding: var(--space-1) var(--space-2);
    border: 0; border-left: 2px solid var(--color-border-strong); border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    background: none; color: var(--color-text-muted); font: inherit; font-size: var(--text-sm); cursor: pointer;
    overflow-wrap: anywhere;
  }
  .line:hover { background: var(--color-surface-hover); color: var(--color-text); border-left-color: var(--color-accent); }
  .line:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
</style>
