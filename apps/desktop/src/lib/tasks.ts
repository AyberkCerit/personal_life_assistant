// Task grouping and labels for the task panel (FR-TSK-001, -007, -008). No framework code.

export interface Task {
  task_id: string;
  title: string;
  details: string | null;
  date: string | null;
  time: string | null;
  notify_at: string | null;
  status: "open" | "done" | "cancelled";
  origin: "manual" | "extracted" | "assistant";
  note_path: string | null;
  block_text: string | null;
  source_missing: boolean;
  user_modified: boolean;
}

export type TaskList = "today" | "upcoming" | "completed";
export type OriginFilter = "all" | "manual" | "extracted";

export interface Labels {
  today: string;
  tomorrow: string;
  noDate: string;
  overdue: string;
}

export interface Group {
  key: string;
  label: string;
  tasks: Task[];
}

const pad = (n: number) => String(n).padStart(2, "0");

/** The user's local date as YYYY-MM-DD. */
export function todayIso(d: Date = new Date()): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function addDays(iso: string, days: number): string {
  const [y, m, d] = iso.split("-").map(Number);
  return todayIso(new Date(y, m - 1, d + days));
}

export function isOverdue(task: Task, today: string): boolean {
  return task.status === "open" && task.date !== null && task.date < today;
}

export function filterByOrigin(tasks: Task[], filter: OriginFilter): Task[] {
  return filter === "all" ? tasks : tasks.filter((t) => t.origin === filter);
}

export function dayLabel(iso: string, today: string, labels: Labels, lang: string): string {
  if (iso === today) return labels.today;
  if (iso === addDays(today, 1)) return labels.tomorrow;
  const [y, m, d] = iso.split("-").map(Number);
  return new Intl.DateTimeFormat(lang, { weekday: "short", day: "numeric", month: "short" }).format(new Date(y, m - 1, d));
}

export function groupTasks(tasks: Task[], list: TaskList, today: string, labels: Labels, lang: string): Group[] {
  if (list === "completed") return tasks.length ? [{ key: "completed", label: "", tasks }] : [];
  if (list === "today") {
    const overdue = tasks.filter((t) => isOverdue(t, today));
    const due = tasks.filter((t) => !isOverdue(t, today));
    return [
      ...(overdue.length ? [{ key: "overdue", label: labels.overdue, tasks: overdue }] : []),
      ...(due.length ? [{ key: "today", label: labels.today, tasks: due }] : []),
    ];
  }
  const groups = new Map<string, Task[]>();
  for (const t of tasks) {
    const key = t.date ?? "no-date";
    groups.set(key, [...(groups.get(key) ?? []), t]);
  }
  const keys = [...groups.keys()].filter((k) => k !== "no-date").sort();
  if (groups.has("no-date")) keys.push("no-date");
  return keys.map((key) => ({
    key,
    label: key === "no-date" ? labels.noDate : dayLabel(key, today, labels, lang),
    tasks: groups.get(key)!,
  }));
}

/** Milliseconds until the local date changes (the panel refreshes "today" then, Final review I4). */
export function msUntilNextDay(now: Date = new Date()): number {
  const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return next.getTime() - now.getTime();
}

/**
 * Where a source block sits in the editor's text. CodeMirror shows CRLF files with LF line breaks
 * and blocks are stored with LF, so the search runs on LF text (Final review I3).
 */
export function revealRange(fileText: string, block: string): { anchor: number; head: number } | null {
  const text = fileText.replace(/\r\n?/g, "\n");
  const at = text.indexOf(block);
  return at < 0 ? null : { anchor: at, head: at + block.length };
}

export function isReminder(task: Task): boolean {
  return task.notify_at !== null;
}

/** How an item's AI origin shows (IR-UI-007): a badge until the user edits it, then a quiet note. */
export function aiMark(task: Pick<Task, "origin" | "user_modified">): "ai" | "edited" | null {
  if (task.origin !== "extracted") return null;
  return task.user_modified ? "edited" : "ai";
}
