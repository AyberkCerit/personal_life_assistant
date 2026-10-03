<script lang="ts">
  import { onMount } from "svelte";
  import { api, type DueReminder } from "./api";
  import { t } from "./i18n";
  import AlarmClock from "@lucide/svelte/icons/alarm-clock";
  import Check from "@lucide/svelte/icons/check";
  import Clock from "@lucide/svelte/icons/clock";
  import History from "@lucide/svelte/icons/history";
  import X from "@lucide/svelte/icons/x";
  import Banner from "./ui/Banner.svelte";
  import Button from "./ui/Button.svelte";
  import IconButton from "./ui/IconButton.svelte";

  // `version` changes whenever tasks change in the panel, so answered reminders disappear here too.
  let { version, onChanged }: { version: number; onChanged: () => void } = $props();
  let due = $state<DueReminder[]>([]);
  let missed = $state<DueReminder[]>([]);

  // The list lives in the app, not only in events: the scheduler's first tick may come before this
  // banner listens (final review C1), and a reminder answered elsewhere must go (M2).
  async function refresh() {
    const lists = await api.pendingReminders();
    due = lists.due;
    missed = lists.missed;
  }

  async function act(id: string, action: "done" | "snooze") {
    due = due.filter((r) => r.task_id !== id);
    missed = missed.filter((r) => r.task_id !== id);
    await (action === "done" ? api.reminderDone(id) : api.reminderSnooze(id));
    onChanged();
  }

  async function dismiss() {
    missed = [];
    await api.dismissMissed();
  }

  $effect(() => {
    void version;
    void refresh();
  });

  onMount(() => {
    const subs = [api.onReminderDue(() => void refresh()), api.onMissedReminders(() => void refresh()), api.onTasksChanged(() => void refresh())];
    return () => subs.forEach((s) => void s.then((f) => f()));
  });
</script>

{#snippet item(r: DueReminder)}
  <li>
    <span class="what"><strong>{r.title}</strong> <span class="time">{r.notify_at.slice(11, 16)}</span></span>
    <span class="acts">
      <Button variant="primary" icon={Check} aria-label={`${t("reminder.done")}: ${r.title}`} onclick={() => void act(r.task_id, "done")}>{t("reminder.done")}</Button>
      <Button icon={Clock} aria-label={`${t("reminder.snooze")}: ${r.title}`} onclick={() => void act(r.task_id, "snooze")}>{t("reminder.snooze")}</Button>
    </span>
  </li>
{/snippet}

{#if due.length}
  <Banner kind="reminder" icon={AlarmClock} role="alert">
    <ul>{#each due as r (r.task_id)}{@render item(r)}{/each}</ul>
  </Banner>
{/if}
{#if missed.length}
  <Banner kind="reminder" icon={History} role="region" label={t("reminder.missed")}>
    <h3>{t("reminder.missed")} ({missed.length})</h3>
    <ul>{#each missed as r (r.task_id)}{@render item(r)}{/each}</ul>
    {#snippet actions()}
      <IconButton icon={X} label={t("reminder.dismiss")} onclick={() => void dismiss()} />
    {/snippet}
  </Banner>
{/if}

<style>
  h3 { margin: 0 0 var(--space-1); font-size: var(--text-sm); font-weight: 600; color: var(--color-text-muted); }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--space-2); }
  li { display: flex; gap: var(--space-2); align-items: center; flex-wrap: wrap; justify-content: space-between; }
  .what { min-width: 0; overflow-wrap: anywhere; }
  .time { color: var(--color-text-muted); font-size: var(--text-sm); }
  .acts { display: flex; gap: var(--space-2); }
</style>
