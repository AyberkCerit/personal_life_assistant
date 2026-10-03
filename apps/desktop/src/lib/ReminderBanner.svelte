<script lang="ts">
  import { onMount } from "svelte";
  import { api, type DueReminder } from "./api";
  import { t } from "./i18n";

  let { onChanged }: { onChanged: () => void } = $props();
  let due = $state<DueReminder[]>([]);
  let missed = $state<DueReminder[]>([]);

  function drop(id: string) {
    due = due.filter((r) => r.task_id !== id);
    missed = missed.filter((r) => r.task_id !== id);
  }

  async function act(id: string, action: "done" | "snooze") {
    drop(id);
    await (action === "done" ? api.reminderDone(id) : api.reminderSnooze(id));
    onChanged();
  }

  onMount(() => {
    const a = api.onReminderDue((r) => (due = [...due.filter((x) => x.task_id !== r.task_id), r]));
    const b = api.onMissedReminders((list) => (missed = [...missed, ...list.filter((r) => !missed.some((m) => m.task_id === r.task_id))]));
    return () => {
      void a.then((f) => f());
      void b.then((f) => f());
    };
  });
</script>

{#snippet item(r: DueReminder)}
  <li>
    <span>⏰ <strong>{r.title}</strong> <span class="time">{r.notify_at.slice(11, 16)}</span></span>
    <button onclick={() => void act(r.task_id, "done")}>{t("reminder.done")}</button>
    <button onclick={() => void act(r.task_id, "snooze")}>{t("reminder.snooze")}</button>
  </li>
{/snippet}

{#if due.length}
  <section class="reminders" role="alert">
    <ul>{#each due as r (r.task_id)}{@render item(r)}{/each}</ul>
  </section>
{/if}
{#if missed.length}
  <section class="reminders missed" aria-label={t("reminder.missed")}>
    <h3>{t("reminder.missed")} ({missed.length})</h3>
    <ul>{#each missed as r (r.task_id)}{@render item(r)}{/each}</ul>
    <button class="close" onclick={() => (missed = [])}>{t("reminder.dismiss")}</button>
  </section>
{/if}

<style>
  .reminders { border-bottom: 1px solid var(--border); background: var(--panel); padding: 8px 12px; }
  .missed h3 { margin: 0 0 4px; font-size: 13px; color: var(--muted); }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  li { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .time { color: var(--muted); font-size: 12px; }
  button { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 2px 8px; cursor: pointer; }
  .close { margin-top: 6px; }
</style>
