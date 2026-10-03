<script lang="ts">
  import { tick } from "svelte";
  import type { Task, TaskInput } from "./api";
  import { isOverdue, isReminder } from "./tasks";
  import { t } from "./i18n";

  let {
    task,
    today,
    onDone,
    onSave,
    onDelete,
    onOpenSource,
  }: {
    task: Task;
    today: string;
    onDone: (done: boolean) => void;
    onSave: (input: TaskInput) => Promise<void>;
    onDelete: () => void;
    onOpenSource: () => void;
  } = $props();

  let editing = $state(false);
  let confirming = $state(false);
  let titleButton: HTMLButtonElement | undefined = $state();
  let deleteButton: HTMLButtonElement | undefined = $state();

  /** Puts keyboard focus on an element as soon as it appears (IR-UI-003). */
  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  async function backTo(target: () => HTMLElement | undefined) {
    await tick();
    target()?.focus();
  }

  function cancelEdit() {
    editing = false;
    void backTo(() => titleButton);
  }

  function cancelDelete() {
    confirming = false;
    void backTo(() => deleteButton);
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
  let form = $state<TaskInput>({ title: "", date: "", time: "", details: "" });

  function startEdit() {
    form = { title: task.title, date: task.date ?? "", time: task.time ?? "", details: task.details ?? "", remind: isReminder(task) };
    editing = true;
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    await onSave(form);
    editing = false;
    void backTo(() => titleButton);
  }
</script>

<li class="task" class:done={task.status === "done"}>
  {#if editing}
    <form class="edit" onsubmit={save} use:escapeCancels={cancelEdit}>
      <input bind:value={form.title} aria-label={t("tasks.title")} required maxlength="200" use:focusOnMount />
      <div class="when">
        <input type="date" bind:value={form.date} aria-label={t("tasks.date")} />
        <input type="time" bind:value={form.time} aria-label={t("tasks.time")} />
      </div>
      <textarea bind:value={form.details} aria-label={t("tasks.details")} rows="2"></textarea>
      <label class="check"><input type="checkbox" bind:checked={form.remind} disabled={!form.date} /> {t("tasks.remind")}</label>
      <div class="actions">
        <button type="submit">{t("tasks.save")}</button>
        <button type="button" onclick={cancelEdit}>{t("tasks.cancel")}</button>
      </div>
    </form>
  {:else if confirming}
    <div class="confirm" role="alertdialog" aria-label={t("tasks.delete")} tabindex="-1" onkeydown={(e) => onEscape(e, cancelDelete)}>
      <span>'{task.title}' {t("tasks.confirmDelete")}</span>
      <button class="danger" onclick={onDelete} use:focusOnMount>{t("tasks.delete")}</button>
      <button onclick={cancelDelete}>{t("tasks.cancel")}</button>
    </div>
  {:else}
    <input
      type="checkbox"
      checked={task.status === "done"}
      aria-label={t("tasks.done")}
      onchange={(e) => onDone((e.currentTarget as HTMLInputElement).checked)}
    />
    <div class="body">
      <button class="title" bind:this={titleButton} onclick={startEdit} title={t("tasks.edit")}>{task.title}</button>
      <div class="meta">
        {#if task.time}<span>{task.time}</span>{/if}
        {#if isReminder(task)}<span role="img" title={t("tasks.reminderLabel")} aria-label={t("tasks.reminderLabel")}>🔔</span>{/if}
        {#if isOverdue(task, today)}<span class="overdue">{t("tasks.overdue")}</span>{/if}
        {#if task.origin === "extracted"}
          <span class="badge" title={t("tasks.aiLabel")} aria-label={t("tasks.aiLabel")}>{t("tasks.ai")}</span>
        {/if}
        {#if task.note_path}
          <button class="link" disabled={task.source_missing} onclick={onOpenSource}>
            {task.source_missing ? t("tasks.sourceMissing") : t("tasks.source")}
          </button>
        {/if}
      </div>
    </div>
    <button class="icon" bind:this={deleteButton} aria-label={t("tasks.delete")} title={t("tasks.delete")} onclick={() => (confirming = true)}>✕</button>
  {/if}
</li>

<style>
  .task { display: flex; gap: 8px; align-items: flex-start; padding: 6px 4px; border-bottom: 1px solid var(--border); }
  .task.done .title { text-decoration: line-through; color: var(--muted); }
  .body { flex: 1; min-width: 0; }
  .title { background: none; border: 0; padding: 0; text-align: left; cursor: pointer; width: 100%; overflow-wrap: anywhere; }
  .meta { display: flex; gap: 8px; align-items: center; font-size: 12px; color: var(--muted); flex-wrap: wrap; }
  .overdue { color: var(--danger); }
  .badge { border: 1px solid var(--accent); color: var(--accent); border-radius: 4px; padding: 0 4px; font-size: 11px; }
  .link { background: none; border: 0; padding: 0; color: var(--accent); cursor: pointer; font-size: 12px; }
  .link:disabled { color: var(--muted); cursor: default; }
  .icon { background: none; border: 0; color: var(--muted); cursor: pointer; }
  .edit, .confirm { display: flex; flex-direction: column; gap: 6px; width: 100%; }
  .confirm { flex-direction: row; flex-wrap: wrap; align-items: center; }
  .when, .actions { display: flex; gap: 6px; }
  .check { display: flex; gap: 6px; align-items: center; font-size: 13px; }
  input:not([type="checkbox"]), textarea { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 4px 6px; min-width: 0; }
  .actions button, .confirm button { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 2px 8px; cursor: pointer; }
  .danger { color: var(--danger); }
</style>
