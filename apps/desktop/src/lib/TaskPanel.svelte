<script lang="ts">
  import { failure, type Failure } from "./i18n";
  import ErrorBanner from "./ui/ErrorBanner.svelte";
  import { api, type ReviewItem, type Routine, type Task, type TaskInput } from "./api";
  import { onMount } from "svelte";
  import { filterByOrigin, groupTasks, msUntilNextDay, taskError, todayIso, type OriginFilter, type TaskList } from "./tasks";
  import RoutineList from "./RoutineList.svelte";
  import { routineError } from "./routines";
  import TaskRow from "./TaskRow.svelte";
  import { lang, t, tf } from "./i18n";
  import CalendarCheck from "@lucide/svelte/icons/calendar-check";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import Inbox from "@lucide/svelte/icons/inbox";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import Button from "./ui/Button.svelte";
  import Checkbox from "./ui/Checkbox.svelte";
  import EmptyState from "./ui/EmptyState.svelte";
  import Icon from "./ui/Icon.svelte";

  let tabsEl: HTMLDivElement | undefined = $state();
  let indicator = $state({ x: 0, w: 0 });

  // The accent underline slides to the selected tab (spec § 6), and follows it when a narrow panel shortens the tabs.
  function measureTab() {
    const el = tabsEl?.querySelector<HTMLElement>('[aria-selected="true"]');
    if (el) indicator = { x: el.offsetLeft, w: el.offsetWidth };
  }
  $effect(() => {
    void tab;
    measureTab();
  });
  $effect(() => {
    if (!tabsEl) return;
    const observer = new ResizeObserver(measureTab);
    observer.observe(tabsEl);
    return () => observer.disconnect();
  });

  let {
    version,
    onOpenSource,
    onChanged = () => {},
  }: { version: number; onOpenSource: (path: string, blockText: string | null) => void; onChanged?: () => void } = $props();

  type Tab = TaskList | "routines" | "review";
  const TABS: Tab[] = ["today", "upcoming", "completed", "routines", "review"];
  let tab = $state<Tab>("today");
  let filter = $state<OriginFilter>("all");
  let tasks = $state<Task[]>([]);
  let review = $state<ReviewItem[]>([]);
  let routines = $state<Routine[]>([]);
  let error = $state<string | Failure | null>(null);
  let newTitle = $state("");
  let newDate = $state("");
  let newTime = $state("");
  let newRemind = $state(false);
  let reviewForms = $state<Record<string, TaskInput>>({});
  let today = $state(todayIso()); // refreshed on every load and at local midnight
  const labels = $derived({ today: t("tasks.today"), tomorrow: t("tasks.tomorrow"), noDate: t("tasks.noDate"), overdue: t("tasks.overdue") });
  const groups = $derived(tab === "review" || tab === "routines" ? [] : groupTasks(filterByOrigin(tasks, filter), tab, today, labels, lang));

  /** The commands' error codes in the UI language (architecture review A2). */
  function errorText(e: unknown): string | Failure {
    const { key, detail } = tab === "routines" ? routineError(String(e)) : taskError(String(e));
    return key === "tasks.error.other" ? failure(e) : tf(key, { detail }); // M6: no English inline
  }

  async function run(action: () => Promise<unknown>): Promise<boolean> {
    let ok = true;
    try {
      await action();
      error = null;
      onChanged(); // e.g. a reminder ticked off here leaves the reminder banner
    } catch (e) {
      error = errorText(e);
      ok = false;
    }
    await load();
    return ok;
  }

  async function load() {
    today = todayIso();
    try {
      if (tab === "review") {
        review = await api.listReview();
        reviewForms = Object.fromEntries(review.map((r) => [r.review_id, reviewForms[r.review_id] ?? { title: r.title ?? "", date: "", time: "" }]));
      } else if (tab === "routines") {
        routines = await api.listRoutines();
      } else {
        tasks = await api.listTasks(tab);
      }
    } catch (e) {
      error = errorText(e);
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
  <div class="tabs" role="tablist" tabindex="-1" onkeydown={onTabKey} bind:this={tabsEl}>
    {#each TABS as name (name)}
      <button role="tab" aria-selected={tab === name} tabindex={tab === name ? 0 : -1} title={t(`tasks.${name}` as const)} onclick={() => (tab = name)}>
        {t(`tasks.${name}` as const)}
      </button>
    {/each}
    <span class="indicator" aria-hidden="true" style:transform={`translateX(${indicator.x}px)`} style:width={`${indicator.w}px`}></span>
  </div>

  {#if error}<ErrorBanner {error} />{/if}

  {#if tab === "routines"}
    <RoutineList
      {routines}
      onSave={(id, input) => run(() => api.editRoutine(id, input))}
      onPause={(id, paused) => void run(() => api.setRoutinePaused(id, paused))}
      onDelete={(id) => void run(() => api.deleteRoutine(id))}
      {onOpenSource}
    />
  {:else if tab !== "review"}
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
        <span title={newDate ? undefined : t("tasks.remindNeedsDate")}>
          <Checkbox bind:checked={newRemind} label={t("tasks.remind")} disabled={!newDate} />
        </span>
      </form>
    {/if}
    {#each groups as group (group.key)}
      {#if group.label}<h3 class:overdue={group.key === "overdue"}>{group.label}</h3>{/if}
      <ul>
        {#each group.tasks as task (task.task_id)}
          <TaskRow
            {task}
            {today}
            onDone={(done) => run(() => api.setTaskDone(task.task_id, done))}
            onSave={async (input) => void (await run(() => api.editTask(task.task_id, input)))}
            onDelete={() => void run(() => api.deleteTask(task.task_id))}
            onOpenSource={() => task.note_path && onOpenSource(task.note_path, task.block_text)}
          />
        {/each}
      </ul>
    {:else}
      {#if tab === "completed"}
        <EmptyState icon={CircleCheck} title={t("tasks.empty")} />
      {:else}
        <EmptyState icon={CalendarCheck} title={t("tasks.empty")} hint={t("tasks.emptyHint")} />
      {/if}
    {/each}
  {:else}
    <ul class="cards">
      {#each review as item (item.review_id)}
        <li class="card">
          <p class="reason"><Icon icon={Sparkles} size="sm" />{item.reason}</p>
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
                <Button type="submit" variant="primary">{t("review.accept")}</Button>
                <Button variant="quiet" onclick={() => void run(() => api.rejectReview(item.review_id))}>{t("review.reject")}</Button>
              </div>
            </form>
          {:else}
            <p class="hint">{t("review.metricHint")}</p>
            <div class="actions"><Button variant="quiet" onclick={() => void run(() => api.rejectReview(item.review_id))}>{t("review.reject")}</Button></div>
          {/if}
          {#if item.note_path}
            <button class="link" onclick={() => onOpenSource(item.note_path!, item.block_text)}>{t("tasks.source")}</button>
          {/if}
        </li>
      {:else}
        <EmptyState icon={Inbox} title={t("review.empty")} />
      {/each}
    </ul>
  {/if}
</section>

<style>
  .panel { display: flex; flex-direction: column; gap: var(--space-2); min-width: 0; }
  /* Five tabs fit the 20rem side panel; a narrower one shortens the labels (full text in the title) instead of scrolling. */
  .tabs { position: relative; display: flex; border-bottom: 1px solid var(--color-border); }
  .tabs button {
    flex: 0 1 auto; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    background: none; border: 0; padding: var(--space-2) var(--space-1); cursor: pointer;
    color: var(--color-text-muted); font-size: var(--text-md);
    transition: color var(--duration-fast) var(--ease-standard);
  }
  .tabs button:hover, .tabs button[aria-selected="true"] { color: var(--color-text); }
  .indicator {
    position: absolute; left: 0; bottom: -1px; height: 2px; background: var(--color-accent); border-radius: var(--radius-full);
    transition: transform var(--duration-base) var(--ease-standard), width var(--duration-base) var(--ease-standard);
  }
  .tools { display: flex; justify-content: flex-end; font-size: var(--text-sm); color: var(--color-text-muted); }
  .tools select { margin-left: var(--space-2); }
  .add { display: flex; flex-direction: column; gap: var(--space-2); }
  .when, .actions { display: flex; gap: var(--space-2); }
  .when input { flex: 1; }
  ul { list-style: none; margin: 0; padding: 0; }
  h3 { font-size: var(--text-xs); text-transform: uppercase; letter-spacing: 0.06em; color: var(--color-text-muted); margin: var(--space-3) 0 var(--space-1); }
  h3.overdue { color: var(--color-danger); }
  .hint { color: var(--color-text-muted); font-size: var(--text-sm); margin: 0; }
  .cards { display: flex; flex-direction: column; gap: var(--space-2); }
  .card {
    display: flex; flex-direction: column; gap: var(--space-2); padding: var(--space-3);
    background: var(--color-surface-raised); border: 1px solid var(--color-border); border-radius: var(--radius-lg);
  }
  .card form { display: flex; flex-direction: column; gap: var(--space-2); }
  .reason { display: flex; gap: var(--space-1); align-items: center; margin: 0; color: var(--color-ai); font-size: var(--text-sm); font-weight: 600; }
  blockquote { margin: 0; padding-left: var(--space-2); border-left: 2px solid var(--color-border-strong); color: var(--color-text-muted); overflow-wrap: anywhere; }
  .link { align-self: flex-start; background: none; border: 0; padding: 0; color: var(--color-link); cursor: pointer; font-size: var(--text-sm); }
</style>
