<script lang="ts">
  import { tick } from "svelte";
  import Pause from "@lucide/svelte/icons/pause";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Play from "@lucide/svelte/icons/play";
  import Repeat from "@lucide/svelte/icons/repeat";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { Routine, RoutineInput } from "./api";
  import { lang, t, tf, type Key } from "./i18n";
  import { dayLabels, weekCount, WEEK, type DayKey } from "./routines";
  import Button from "./ui/Button.svelte";
  import Checkbox from "./ui/Checkbox.svelte";
  import EmptyState from "./ui/EmptyState.svelte";
  import Icon from "./ui/Icon.svelte";
  import IconButton from "./ui/IconButton.svelte";

  let {
    routines,
    onSave,
    onPause,
    onDelete,
    onOpenSource,
  }: {
    routines: Routine[];
    /** Resolves to whether the change was saved. */
    onSave: (id: string, input: RoutineInput) => Promise<boolean>;
    onPause: (id: string, paused: boolean) => void;
    onDelete: (id: string) => void;
    onOpenSource: (path: string, blockText: string | null) => void;
  } = $props();

  type DayForm = Record<DayKey, { on: boolean; time: string }>;
  let editingId = $state<string | null>(null);
  let confirmingId = $state<string | null>(null);
  let title = $state("");
  let days = $state<DayForm>(emptyDays());

  function emptyDays(): DayForm {
    return Object.fromEntries(WEEK.map((d) => [d, { on: false, time: "" }])) as DayForm;
  }

  /** Puts keyboard focus on an element as soon as it appears. */
  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  async function focusAfter(selector: string) {
    await tick();
    document.querySelector<HTMLElement>(selector)?.focus();
  }

  function startEdit(r: Routine) {
    title = r.title;
    days = emptyDays();
    for (const d of r.days) days[d.day] = { on: true, time: d.time ?? "" };
    confirmingId = null;
    editingId = r.routine_id;
  }

  function cancelEdit() {
    const id = editingId;
    editingId = null;
    void focusAfter(`[data-edit="${id}"]`);
  }

  function cancelDelete() {
    const id = confirmingId;
    confirmingId = null;
    void focusAfter(`[data-delete="${id}"]`);
  }

  function onEscape(e: KeyboardEvent, cancel: () => void) {
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    }
  }

  /** Escape cancels the form it is attached to. */
  function escapeCancels(node: HTMLElement, cancel: () => void) {
    const handler = (e: KeyboardEvent) => onEscape(e, cancel);
    node.addEventListener("keydown", handler);
    return { destroy: () => node.removeEventListener("keydown", handler) };
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    const id = editingId;
    if (!id) return;
    const input: RoutineInput = {
      title: title.trim(),
      days: WEEK.filter((d) => days[d].on).map((d) => ({ day: d, time: days[d].time || null })),
    };
    if (await onSave(id, input)) {
      editingId = null;
      void focusAfter(`[data-edit="${id}"]`);
    }
  }
</script>

<ul>
  {#each routines as r (r.routine_id)}
    <li class="routine" class:paused={r.paused}>
      {#if editingId === r.routine_id}
        <form class="edit" onsubmit={save} use:escapeCancels={cancelEdit}>
          <input bind:value={title} aria-label={t("routines.title")} required maxlength="200" use:focusOnMount />
          {#each WEEK as d (d)}
            <div class="dayrow">
              <Checkbox bind:checked={days[d].on} label={t(`day.${d}` as Key)} />
              <input type="time" bind:value={days[d].time} disabled={!days[d].on} aria-label={`${t(`day.${d}` as Key)}: ${t("routines.time")}`} />
            </div>
          {/each}
          <div class="actions">
            <Button type="submit" variant="primary">{t("routines.save")}</Button>
            <Button variant="quiet" onclick={cancelEdit}>{t("routines.cancel")}</Button>
          </div>
        </form>
      {:else if confirmingId === r.routine_id}
        <div class="confirm" role="alertdialog" aria-label={t("routines.delete")} tabindex="-1" onkeydown={(e) => onEscape(e, cancelDelete)}>
          <span>{tf("routines.deleteConfirm", { title: r.title })}</span>
          <Button variant="danger" icon={Trash2} onclick={() => onDelete(r.routine_id)}>{t("routines.delete")}</Button>
          <Button variant="quiet" onclick={cancelDelete}>{t("routines.cancel")}</Button>
        </div>
      {:else}
        <div class="body">
          <span class="title"><Icon icon={Repeat} size="sm" />{r.title}</span>
          <div class="meta">
            <span>{dayLabels(r.days, lang)}</span>
            <span>{weekCount(r)}</span>
            {#if r.paused}<span class="pausedtag">{t("routines.paused")}</span>{/if}
            {#if r.note_path}
              <button class="link" disabled={r.source_missing} onclick={() => onOpenSource(r.note_path!, r.block_text)}>
                {r.source_missing ? t("tasks.sourceMissing") : t("tasks.source")}
              </button>
            {/if}
          </div>
        </div>
        <span class="tools">
          <IconButton icon={Pencil} label={t("routines.edit")} data-edit={r.routine_id} onclick={() => startEdit(r)} />
          <IconButton
            icon={r.paused ? Play : Pause}
            label={r.paused ? t("routines.resume") : t("routines.pause")}
            onclick={() => onPause(r.routine_id, !r.paused)}
          />
          <IconButton icon={Trash2} label={t("routines.delete")} data-delete={r.routine_id} onclick={() => (confirmingId = r.routine_id)} />
        </span>
      {/if}
    </li>
  {:else}
    <EmptyState icon={Repeat} title={t("routines.empty")} hint={t("routines.emptyHint")} />
  {/each}
</ul>

<style>
  ul { list-style: none; margin: 0; padding: 0; }
  .routine {
    display: flex; gap: var(--space-2); align-items: flex-start;
    padding: var(--space-2) var(--space-1); border-bottom: 1px solid var(--color-border);
  }
  .routine.paused .title { color: var(--color-text-muted); }
  .body { flex: 1; min-width: 0; }
  .title { display: inline-flex; gap: var(--space-1); align-items: center; overflow-wrap: anywhere; color: var(--color-text); }
  .meta { display: flex; gap: var(--space-2); align-items: center; font-size: var(--text-sm); color: var(--color-text-muted); flex-wrap: wrap; margin-top: 0.125rem; }
  .pausedtag { font-weight: 600; }
  .link { background: none; border: 0; padding: 0; color: var(--color-link); cursor: pointer; font-size: var(--text-sm); }
  .link:disabled { color: var(--color-text-muted); cursor: default; }
  .tools { display: flex; gap: 0.125rem; opacity: 0; transition: opacity var(--duration-fast) var(--ease-standard); }
  .routine:hover .tools, .routine:focus-within .tools { opacity: 1; }
  .edit, .confirm { display: flex; flex-direction: column; gap: var(--space-2); width: 100%; }
  .confirm { flex-direction: row; flex-wrap: wrap; align-items: center; }
  .confirm > span { flex: 1 1 100%; min-width: 0; overflow-wrap: anywhere; }
  .dayrow { display: flex; gap: var(--space-2); align-items: center; justify-content: space-between; }
  .actions { display: flex; gap: var(--space-2); }
</style>
