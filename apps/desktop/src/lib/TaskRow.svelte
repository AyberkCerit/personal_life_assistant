<script lang="ts">
  import { tick } from "svelte";
  import type { Task, TaskInput } from "./api";
  import Bell from "@lucide/svelte/icons/bell";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { aiMark, isOverdue, isReminder } from "./tasks";
  import Badge from "./ui/Badge.svelte";
  import Button from "./ui/Button.svelte";
  import Checkbox from "./ui/Checkbox.svelte";
  import Icon from "./ui/Icon.svelte";
  import IconButton from "./ui/IconButton.svelte";
  import { BASE, motion } from "./ui/motion";
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
    /** Resolves to whether the change was saved. */
    onDone: (done: boolean) => Promise<boolean>;
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
  let form = $state<TaskInput & { remind: boolean }>({ title: "", date: "", time: "", details: "", remind: false });
  let confirmButton: HTMLButtonElement | undefined = $state();

  function askDelete() {
    confirming = true;
    void backTo(() => confirmButton); // keyboard focus lands on "Delete" (IR-UI-003)
  }
  let leaving = $state(false);
  const mark = $derived(aiMark(task));

  /** A completed task fades before the list moves it (spec § 6). */
  async function toggle(done: boolean) {
    if (done) {
      leaving = true;
      await new Promise((r) => setTimeout(r, motion(BASE)));
    }
    if (!(await onDone(done))) leaving = false; // not saved: the row comes back (deferred design minor)
  }

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

<li class="task" class:done={task.status === "done"} class:leaving>
  {#if editing}
    <form class="edit" onsubmit={save} use:escapeCancels={cancelEdit}>
      <input bind:value={form.title} aria-label={t("tasks.title")} required maxlength="200" use:focusOnMount />
      <div class="when">
        <input type="date" bind:value={form.date} aria-label={t("tasks.date")} />
        <input type="time" bind:value={form.time} aria-label={t("tasks.time")} />
      </div>
      <textarea bind:value={form.details} aria-label={t("tasks.details")} rows="2"></textarea>
      <Checkbox bind:checked={form.remind} label={t("tasks.remind")} disabled={!form.date} />
      <div class="actions">
        <Button type="submit" variant="primary">{t("tasks.save")}</Button>
        <Button variant="quiet" onclick={cancelEdit}>{t("tasks.cancel")}</Button>
      </div>
    </form>
  {:else if confirming}
    <div class="confirm" role="alertdialog" aria-label={t("tasks.delete")} tabindex="-1" onkeydown={(e) => onEscape(e, cancelDelete)}>
      <span>'{task.title}' {t("tasks.confirmDelete")}</span>
      <Button variant="danger" icon={Trash2} bind:element={confirmButton} onclick={onDelete}>{t("tasks.delete")}</Button>
      <Button variant="quiet" onclick={cancelDelete}>{t("tasks.cancel")}</Button>
    </div>
  {:else}
    <Checkbox checked={task.status === "done" || leaving} label={t("tasks.done")} hideLabel onchange={(c) => void toggle(c)} />
    <div class="body">
      <button class="title" bind:this={titleButton} onclick={startEdit} title={t("tasks.edit")}>{task.title}</button>
      <div class="meta">
        {#if task.time}<span>{task.time}</span>{/if}
        {#if isReminder(task)}<Icon icon={Bell} size="sm" label={t("tasks.reminderLabel")} />{/if}
        {#if isOverdue(task, today)}<span class="overdue">{t("tasks.overdue")}</span>{/if}
        {#if mark === "ai"}
          <Badge kind="ai" label={t("tasks.aiLabel")}>{t("tasks.ai")}</Badge>
        {/if}
        {#if task.note_path}
          <button class="link" disabled={task.source_missing} onclick={onOpenSource}>
            {task.source_missing ? t("tasks.sourceMissing") : t("tasks.source")}
          </button>
        {/if}
      </div>
    </div>
    <span class="del"><IconButton icon={Trash2} label={t("tasks.delete")} bind:element={deleteButton} onclick={askDelete} /></span>
  {/if}
</li>

<style>
  .task {
    display: flex; gap: var(--space-2); align-items: flex-start;
    padding: var(--space-2) var(--space-1); border-bottom: 1px solid var(--color-border);
    transition: opacity var(--duration-base) var(--ease-standard);
  }
  .task > :global(.cb) { padding-top: 0.1875rem; }
  .task.leaving { opacity: 0.4; }
  .task.done .title { text-decoration: line-through; color: var(--color-text-muted); }
  .body { flex: 1; min-width: 0; }
  .title { background: none; border: 0; padding: 0; text-align: left; cursor: pointer; width: 100%; overflow-wrap: anywhere; color: var(--color-text); }
  .meta { display: flex; gap: var(--space-2); align-items: center; font-size: var(--text-sm); color: var(--color-text-muted); flex-wrap: wrap; margin-top: 0.125rem; }
  .overdue { color: var(--color-danger); font-weight: 600; }
  .link { background: none; border: 0; padding: 0; color: var(--color-link); cursor: pointer; font-size: var(--text-sm); }
  .link:disabled { color: var(--color-text-muted); cursor: default; }
  .del { opacity: 0; transition: opacity var(--duration-fast) var(--ease-standard); }
  .task:hover .del, .task:focus-within .del { opacity: 1; }
  .edit, .confirm { display: flex; flex-direction: column; gap: var(--space-2); width: 100%; }
  .confirm { flex-direction: row; flex-wrap: wrap; align-items: center; }
  .confirm > span { flex: 1 1 100%; min-width: 0; overflow-wrap: anywhere; } /* long unbroken titles wrap (final review I5) */
  .when, .actions { display: flex; gap: var(--space-2); }
  .when input { flex: 1; }
</style>
