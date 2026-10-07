<script lang="ts">
  import { onMount } from "svelte";
  import CalendarDays from "@lucide/svelte/icons/calendar-days";
  import FileText from "@lucide/svelte/icons/file-text";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { api, type DayInfo } from "./api";
  import { lang, t, tf, type Key } from "./i18n";
  import { answerParts } from "./qa";
  import Button from "./ui/Button.svelte";

  // FR-MEM-006, owner decision B: a daily note shows its day's summary above it; nothing is
  // written into the note (FR-MEM-007).
  let { path, onOpenNote, onOpenCited }: { path: string; onOpenNote: (path: string) => void; onOpenCited: (target: string) => void } = $props();
  let day = $state<DayInfo | null>(null);
  const summary = $derived(day?.summary ?? null);
  let working = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    day = await api.daySummary(path).catch(() => null);
  }

  async function again() {
    error = null;
    working = true;
    try {
      await api.summaryRegenerate(path);
    } catch (e) {
      working = false;
      error = String(e);
    }
  }

  const name = (p: string) => p.split("/").pop()!.replace(/\.md$/i, "");
  const when = (iso: string) => new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" }).format(new Date(iso));

  onMount(() => {
    void load();
    const unSummary = api.onSummaryChanged((e) => {
      if (!path.endsWith(`${e.date}.md`)) return;
      working = false;
      error = e.error ? t(`summary.error.${e.error}` as Key) : null; // final review I2: never stuck
      void load();
    });
    // the day's notes changed: maybe stale now (once things settle, not on every save)
    let timer: ReturnType<typeof setTimeout> | undefined;
    const unIndex = api.onIndexChanged(() => {
      clearTimeout(timer);
      timer = setTimeout(() => void load(), 1500);
    });
    return () => {
      void unSummary.then((f) => f());
      void unIndex.then((f) => f());
    };
  });
</script>

{#if summary || working || day?.past}
  <details class="day" open>
    <summary><CalendarDays size={14} strokeWidth={2} aria-hidden="true" />{t("summary.title")}</summary>
    <div class="column">
      {#if summary}
        <ul class="points">
          {#each summary.text.split("\n").map((l) => l.replace(/^\s*[-*•]\s*/, "").trim()).filter(Boolean) as line, i (i)}
            <li>
              {#each answerParts(line) as part, j (j)}
                <!-- the day's own note is the one open: naming it after each point is noise -->
                {#if part.link && part.link !== summary.date}<button class="link" onclick={() => onOpenCited(part.link!)}>{part.text}</button>{:else if !part.link}{part.text}{/if}
              {/each}
            </li>
          {/each}
        </ul>
        <div class="meta">
          {#each summary.sources as src (src)}
            <button class="source" title={src} onclick={() => onOpenNote(src)}><FileText size={12} strokeWidth={2} aria-hidden="true" />{name(src)}</button>
          {/each}
          <span class="muted">{tf("summary.generated", { when: when(summary.generated_at) })}</span>
        </div>
        {#if summary.stale}<p class="muted">{t("summary.stale")}</p>{/if}
      {:else if !working}
        <p class="muted">{t("summary.none")}</p>
      {/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions">
        {#if working}
          <span class="muted" role="status">{t("summary.working")}</span>
        {:else}
          <Button variant="quiet" icon={RefreshCw} onclick={() => void again()}>{summary ? t("summary.again") : t("summary.make")}</Button>
        {/if}
      </div>
    </div>
  </details>
{/if}

<style>
  .day { border-bottom: 1px solid var(--color-border); background: var(--color-surface-panel); }
  summary {
    display: flex; align-items: center; gap: var(--space-2); max-width: 72ch; margin: 0 auto; box-sizing: border-box;
    padding: var(--space-2) var(--space-8); font-size: var(--text-sm); font-weight: 600; color: var(--color-text-muted); cursor: pointer; list-style: none;
  }
  summary::-webkit-details-marker { display: none; }
  summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  .column { max-width: 72ch; margin: 0 auto; box-sizing: border-box; padding: 0 var(--space-8) var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); }
  .points { margin: 0; padding-left: var(--space-5); font-size: var(--text-md); line-height: 1.55; }
  .meta { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-1) var(--space-2); font-size: var(--text-sm); }
  .muted { color: var(--color-text-muted); font-size: var(--text-sm); margin: 0; }
  .error { color: var(--color-danger); font-size: var(--text-sm); margin: 0; }
  .link { border: 0; padding: 0; background: none; font: inherit; color: var(--color-link); text-decoration: underline; text-decoration-color: var(--color-link-underline); text-underline-offset: 3px; cursor: pointer; }
  .source {
    display: inline-flex; align-items: center; gap: 2px; padding: 1px var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius-sm);
    background: var(--color-surface-reading); color: var(--color-link); font: inherit; cursor: pointer;
  }
  .actions { display: flex; }
</style>
