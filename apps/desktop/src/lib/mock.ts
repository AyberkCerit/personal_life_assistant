// Browser-only stand-in for the Rust commands, so the UI can be checked with `npm run dev`.
const files = new Map<string, string>([
  ["daily/2026/2026-10-06.md", "Yarın 9'da dişçi var.\n\nDün 7 saat uyudum.\n"],
  ["notes/Fikirler.md", "# Fikirler\n\n- [ ] Blog yazısı\n"],
  ["inbox/Hoş geldin.md", "Bu, PLA'nın tarayıcıdaki deneme kasası.\n"],
]);
import type { Task } from "./tasks";
import { todayIso } from "./tasks";

export const mockListeners = new Map<string, Array<(payload: unknown) => void>>();

const day = (offset: number) => {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return todayIso(d);
};
const base = { details: null, time: null, notify_at: null, status: "open" as const, note_path: null, block_text: null, source_missing: false, user_modified: false };
let tasks: Task[] = [
  { ...base, task_id: "t1", title: "Dişçi", date: day(1), time: "09:00", origin: "extracted", note_path: "daily/2026/2026-10-06.md", block_text: "Yarın 9'da dişçi var." },
  { ...base, task_id: "t2", title: "Fatura öde", date: day(-1), origin: "manual" },
  { ...base, task_id: "t3", title: "Kitap oku", date: null, origin: "manual" },
];
let review = [
  {
    review_id: "r1", reason: "reminder without a date", title: "Annemi ara", is_action: true,
    payload_json: '{"type":"reminder","title":"Annemi ara"}', note_path: "daily/2026/2026-10-06.md",
    block_text: "Dün 7 saat uyudum.", created_at: "",
  },
];
let nextId = 10;

function listTasks(list: string): Task[] {
  const today = todayIso();
  const byDate = (a: Task, b: Task) => (a.date ?? "9999").localeCompare(b.date ?? "9999") || (a.time ?? "99").localeCompare(b.time ?? "99");
  if (list === "today") return tasks.filter((t) => t.status === "open" && t.date !== null && t.date <= today).sort(byDate);
  if (list === "upcoming") return tasks.filter((t) => t.status === "open" && (t.date === null || t.date > today)).sort(byDate);
  return tasks.filter((t) => t.status === "done");
}

type MockInput = { title: string; date?: string | null; time?: string | null; details?: string | null; remind?: boolean | null };

function addMockTask(input: MockInput, origin: Task["origin"]): Task {
  const task: Task = { ...base, task_id: `t${nextId++}`, title: input.title.trim(), date: input.date || null, time: input.time || null, details: input.details || null, origin };
  if (input.remind && task.date) task.notify_at = `${task.date}T${task.time ?? "09:00"}`; // same rule as the Rust core
  tasks = [...tasks, task];
  return task;
}

// Dev helper for the browser check: simulates the worker adding an item.
(globalThis as Record<string, unknown>).__plaMockAdd = (title: string) => {
  const task = addMockTask({ title, date: day(1) }, "extracted");
  for (const cb of mockListeners.get("items-added") ?? []) cb([{ kind: "task", id: task.task_id, title }]);
};
let vault: string | null = null;

function hash(s: string): string {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) | 0;
  return String(h);
}

function tree() {
  const dirs = new Set<string>();
  for (const p of files.keys()) {
    const parts = p.split("/");
    for (let i = 1; i < parts.length; i++) dirs.add(parts.slice(0, i).join("/"));
  }
  const entries: { path: string; name: string; is_dir: boolean; depth: number }[] = [];
  const children = (prefix: string) => {
    const depth = prefix ? prefix.split("/").length : 0;
    const under = (p: string) => (prefix ? p.startsWith(prefix + "/") : true) && p.split("/").length === depth + 1;
    const byName = (a: string, b: string) => a.toLowerCase().localeCompare(b.toLowerCase());
    for (const d of [...dirs].filter(under).sort(byName)) {
      entries.push({ path: d, name: d.split("/").pop()!, is_dir: true, depth });
      children(d);
    }
    for (const f of [...files.keys()].filter(under).sort(byName)) {
      entries.push({ path: f, name: f.split("/").pop()!, is_dir: false, depth });
    }
  };
  children("");
  return entries;
}

export async function mockBackend<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const path = String(args.path ?? "");
  switch (cmd) {
    case "startup":
      return { vault_path: vault, first_run: vault === null, settings_recovered: false, theme: "dark", error: null, inbox: vault ? "inbox" : null } as T;
    case "open_vault":
      vault = path;
      return { path: vault, inbox: "inbox" } as T;
    case "list_tree":
      return tree() as T;
    case "read_note": {
      const text = files.get(path) ?? "";
      return { text, hash: hash(text), read_only: false } as T;
    }
    case "save_note": {
      const text = String(args.text);
      files.set(path, text);
      return { kind: "saved", hash: hash(text) } as T;
    }
    case "save_copy": {
      const copy = path.replace(/\.md$/i, " (çakışma).md");
      files.set(copy, String(args.text));
      return copy as T;
    }
    case "create_note": {
      const rel = `${args.folder}/${args.title}.md`;
      files.set(rel, "");
      return rel as T;
    }
    case "queue_note":
      return undefined as T;
    case "vault_info":
      return { path: vault, inbox: "inbox" } as T;
    case "worker_status":
      return { queued: 0, model: "not_installed", busy: false, last_error: null, added: 0, paused: false } as T;
    case "list_tasks":
      return listTasks(String(args.list)) as T;
    case "add_task":
      return addMockTask(args.input as MockInput, "manual").task_id as T;
    case "edit_task": {
      const input = args.input as MockInput;
      tasks = tasks.map((t) =>
        t.task_id === args.id ? { ...t, title: input.title, date: input.date || null, time: input.time || null, details: input.details || null, user_modified: true } : t,
      );
      return undefined as T;
    }
    case "set_task_done":
      tasks = tasks.map((t) => (t.task_id === args.id ? { ...t, status: args.done ? "done" : "open", user_modified: true } : t));
      return undefined as T;
    case "delete_task":
    case "undo_item":
      tasks = tasks.filter((t) => t.task_id !== args.id);
      return undefined as T;
    case "list_review":
      return review as T;
    case "accept_review":
      review = review.filter((r) => r.review_id !== args.id);
      return addMockTask(args.input as MockInput, "extracted").task_id as T;
    case "reject_review":
      review = review.filter((r) => r.review_id !== args.id);
      return undefined as T;
    case "reminder_done":
      tasks = tasks.map((t) => (t.task_id === args.id ? { ...t, status: "done" } : t));
      return undefined as T;
    case "reminder_snooze":
    case "hide_to_tray":
      return undefined as T;
    case "set_paused":
      for (const cb of mockListeners.get("paused-changed") ?? []) cb(Boolean(args.paused));
      return undefined as T;
    default:
      throw new Error(`mock: unknown command ${cmd}`);
  }
}

// Dev helpers for the browser check: simulate the scheduler.
(globalThis as Record<string, unknown>).__plaMockDue = (title: string) => {
  for (const cb of mockListeners.get("reminder-due") ?? []) cb({ task_id: "t1", title, notify_at: "2026-10-06T09:00" });
};
(globalThis as Record<string, unknown>).__plaMockMissed = () => {
  for (const cb of mockListeners.get("missed-reminders") ?? [])
    cb([
      { task_id: "t2", title: "Fatura öde", notify_at: "2026-10-05T09:00" },
      { task_id: "t3", title: "Kitap oku", notify_at: "2026-10-05T20:00" },
    ]);
};
