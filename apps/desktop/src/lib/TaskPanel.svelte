<script lang="ts">
  import { api, type ReviewItem, type Task, type TaskInput } from "./api";
  import { onMount } from "svelte";
  import { filterByOrigin, groupTasks, msUntilNextDay, todayIso, type OriginFilter, type TaskList } from "./tasks";
  import TaskRow from "./TaskRow.svelte";
  import { lang, t } from "./i18n";

  let { version, onOpenSource }: { version: number; onOpenSource: (path: string, blockText: string | null) => void } = $props();

  type Tab = TaskList | "review";
  const TABS: Tab[] = ["today", "upcoming", "completed", "review"];
  let tab = $state<Tab>("today");
  let filter = $state<OriginFilter>("all");
  let tasks = $state<Task[]>([]);
  let review = $state<ReviewItem[]>([]);
  let error = $state<string | null>(null);
  let newTitle = $state("");
  let newDate = $state("");
  let newTime = $state("");
  let newRemind = $state(false);
  let reviewForms = $state<Record<string, TaskInput>>({});
  let today = $state(todayIso()); // refreshed on every load and at local midnight
  const labels = $derived({ today: t("tasks.today"), tomorrow: t("tasks.tomorrow"), noDate: t("tasks.noDate"), overdue: t("tasks.overdue") });
  const groups = $derived(tab === "review" ? [] : groupTasks(filterByOrigin(tasks, filter), tab, today, labels, lang));

  async function run(action: () => Promise<unknown>) {
    try {
      await action();
      error = null;
    } catch (e) {
      error = String(e);
    }
    await load();
  }

  async function load() {
    today = todayIso();
    try {
      if (tab === "review") {
        review = await api.listReview();
        reviewForms = Object.fromEntries(review.map((r) => [r.review_id, reviewForms[r.review_id] ?? { title: r.title ?? "", date: "", time: "" }]));
      } else {
        tasks = await api.listTasks(tab);
      }
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    void version; // reload when the worker reports changes
    void tab;
    void load();
  });

  onMount(() => {
    let timer = 0;
    const atMidnight = () => {
      timer = window.setTimeout(() => {
        void load(); // "today", "overdue" and "tomorrow" move with the date
        atMidnight();
      }, msUntilNextDay() + 1000);
    };
    atMidnight();
    return () => window.clearTimeout(timer);
  });

  function onTabKey(e: KeyboardEvent) {
    const i = TABS.indexOf(tab);
    if (e.key === "ArrowRight") tab = TABS[(i + 1) % TABS.length];
    if (e.key === "ArrowLeft") tab = TABS[(i + TABS.length - 1) % TABS.length];
  }

  function add(e: SubmitEvent) {
    e.preventDefault();
    const title = newTitle.trim();
    if (!title) return;
    void run(() => api.addTask({ title, date: newDate || null, time: newTime || null, remind: newRemind })).then(() => {
      if (!error) {
        newTitle = newDate = newTime = "";
        newRemind = false;
      }
    });
  }
</script>

<section class="panel" aria-label={t("tasks.today")}>
  <div class="tabs" role="tablist" tabindex="-1" onkeydown={onTabKey}>
    {#each TABS as name (name)}
      <button role="tab" aria-selected={tab === name} tabindex={tab === name ? 0 : -1} onclick={() => (tab = name)}>
        {t(`tasks.${name}` as const)}
      </button>
    {/each}
  </div>

  {#if error}<div class="banner danger" role="alert">{t("error.generic")}: {error}</div>{/if}

  {#if tab !== "review"}
    <div class="tools">
      <label>
        {t("tasks.filter")}
        <select bind:value={filter}>
          <option value="all">{t("tasks.filterAll")}</option>
          <option value="manual">{t("tasks.filterManual")}</option>
          <option value="extracted">{t("tasks.filterExtracted")}</option>
        </select>
      </label>
    </div>
    {#if tab !== "completed"}
      <form class="add" onsubmit={add}>
        <input bind:value={newTitle} placeholder={t("tasks.add")} aria-label={t("tasks.add")} maxlength="200" />
        <div class="when">
          <input type="date" bind:value={newDate} aria-label={t("tasks.date")} />
          <input type="time" bind:value={newTime} aria-label={t("tasks.time")} />
        </div>
        <label class="check"><input type="checkbox" bind:checked={newRemind} /> {t("tasks.remind")}</label>
      </form>
    {/if}
    {#each groups as group (group.key)}
      {#if group.label}<h3 class:overdue={group.key === "overdue"}>{group.label}</h3>{/if}
      <ul>
        {#each group.tasks as task (task.task_id)}
          <TaskRow
            {task}
            {today}
            onDone={(done) => void run(() => api.setTaskDone(task.task_id, done))}
            onSave={(input) => run(() => api.editTask(task.task_id, input))}
            onDelete={() => void run(() => api.deleteTask(task.task_id))}
            onOpenSource={() => task.note_path && onOpenSource(task.note_path, task.block_text)}
          />
        {/each}
      </ul>
    {:else}
      <p class="empty-list">{t("tasks.empty")}</p>
    {/each}
  {:else}
    <ul>
      {#each review as item (item.review_id)}
        <li class="review">
          <div class="reason">{item.reason}</div>
          {#if item.block_text}<blockquote>{item.block_text}</blockquote>{/if}
          {#if item.is_action}
            <form
              onsubmit={(e) => {
                e.preventDefault();
                void run(() => api.acceptReview(item.review_id, reviewForms[item.review_id]));
              }}
            >
              <input bind:value={reviewForms[item.review_id].title} aria-label={t("tasks.title")} required maxlength="200" />
              <div class="when">
                <input type="date" bind:value={reviewForms[item.review_id].date} aria-label={t("tasks.date")} />
                <input type="time" bind:value={reviewForms[item.review_id].time} aria-label={t("tasks.time")} />
              </div>
              <div class="actions">
                <button type="submit">{t("review.accept")}</button>
                <button type="button" onclick={() => void run(() => api.rejectReview(item.review_id))}>{t("review.reject")}</button>
              </div>
            </form>
          {:else}
            <p class="hint">{t("review.metricHint")}</p>
            <button onclick={() => void run(() => api.rejectReview(item.review_id))}>{t("review.reject")}</button>
          {/if}
          {#if item.note_path}
            <button class="link" onclick={() => onOpenSource(item.note_path!, item.block_text)}>{t("tasks.source")}</button>
          {/if}
        </li>
      {:else}
        <p class="empty-list">{t("review.empty")}</p>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .panel { display: flex; flex-direction: column; gap: 8px; }
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--border); }
  .tabs button { background: none; border: 0; padding: 6px 8px; cursor: pointer; color: var(--muted); border-bottom: 2px solid transparent; }
  .tabs button[aria-selected="true"] { color: var(--text); border-bottom-color: var(--accent); }
  .tools { display: flex; justify-content: flex-end; font-size: 12px; color: var(--muted); }
  .tools select { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; margin-left: 6px; }
  .add { display: flex; flex-direction: column; gap: 4px; }
  .when, .actions { display: flex; gap: 6px; }
  .check { display: flex; gap: 6px; align-items: center; font-size: 13px; color: var(--muted); }
  input, select { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 4px 6px; min-width: 0; }
  ul { list-style: none; margin: 0; padding: 0; }
  h3 { font-size: 12px; text-transform: uppercase; letter-spacing: .04em; color: var(--muted); margin: 10px 0 2px; }
  h3.overdue { color: var(--danger); }
  .empty-list, .hint { color: var(--muted); font-size: 13px; }
  .review { border-bottom: 1px solid var(--border); padding: 8px 0; display: flex; flex-direction: column; gap: 6px; }
  .reason { color: var(--danger); font-size: 12px; }
  blockquote { margin: 0; padding-left: 8px; border-left: 2px solid var(--border); color: var(--muted); }
  .review form { display: flex; flex-direction: column; gap: 6px; }
  .review button, .actions button { background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 2px 8px; cursor: pointer; align-self: flex-start; }
  .link { background: none !important; border: 0 !important; padding: 0 !important; color: var(--accent); cursor: pointer; font-size: 12px; }
</style>
