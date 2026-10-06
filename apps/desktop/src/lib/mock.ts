// Browser-only stand-in for the Rust commands, so the UI can be checked with `npm run dev`.
const files = new Map<string, string>([
  ["daily/2026/2026-10-06.md", "Yarın 9'da dişçi var.\n\nDün 7 saat uyudum.\n"],
  ["notes/Fikirler.md", "# Fikirler\n\n- [ ] Blog yazısı #yazı\n\nDişçi notu: [[2026-10-06]] günü.\n"],
  ["inbox/Hoş geldin.md", "Bu, PLA'nın tarayıcıdaki deneme kasası.\n"],
]);
import type { Task } from "./tasks";
import { todayIso } from "./tasks";

export const mockListeners = new Map<string, Array<(payload: unknown) => void>>();

// Model manager stand-in: a fake 2.4 GB download that advances 10 % every 300 ms.
const RECOMMENDED = { id: "gemma-4-e2b-it-q3km", name: "Gemma 4 E2B (Q3_K_M)", file_name: "gemma-4-E2B-it-Q3_K_M.gguf", size: 2_536_786_016, sha256: "086e…", url: "https://huggingface.co/…", source: "Hugging Face · unsloth", licence: "Apache-2.0", licence_url: "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF" };
let installed: { name: string; size: number; path: string; local: boolean } | null = null;
let download: unknown = null;
let timer = 0;
let received = 0;
const emit = (event: string, payload: unknown) => { for (const cb of mockListeners.get(event) ?? []) cb(payload); };

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
let setupDone = false;
let mockLang: "tr" | "en" | null = null;
let mockTheme: "dark" | "light" = "dark";
let mockAutostart = false;
let lastBackup: string | null = null;

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

// Metrics stand-in for the browser check. The real merge rules and statistics live in Rust; this
// only imitates their shape with simple rules (last value per day, water summed, workouts counted).
type MockMetric = { metric_id: string; kind: string; date: string; value: number | null; unit: string | null; exercise: string | null; sets: number | null; reps: number | null; origin: string; user_modified: boolean; note_path: string | null; block_text: string | null; created_at: string };
let metricsDb: MockMetric[] = (() => {
  const out: MockMetric[] = [];
  const rec = (kind: string, offset: number, value: number | null, extra: Partial<MockMetric> = {}) =>
    out.push({ metric_id: `m${out.length}`, kind, date: day(-offset), value, unit: null, exercise: null, sets: null, reps: null, origin: offset % 3 === 0 ? "extracted" : "manual", user_modified: false, note_path: offset % 3 === 0 ? "daily/2026/2026-10-06.md" : null, block_text: offset % 3 === 0 ? "Dün 7 saat uyudum." : null, created_at: `${day(-offset)}T08:00:00+03:00`, ...extra });
  for (let i = 0; i < 40; i++) {
    if (i % 5 !== 4) rec("sleep", i, 6 + ((i * 7) % 5) / 2);
    if (i % 2 === 0) rec("weight", i, 80 - i * 0.05);
    rec("steps", i, 4000 + ((i * 1337) % 7000));
    if (i < 12) rec("water", i, 250 * (2 + (i % 4)));
    if (i % 3 === 1) rec("workout", i, null, { exercise: "şınav", sets: 3, reps: 12 });
  }
  rec("sleep", 2, 5, { origin: "extracted", created_at: `${day(-2)}T09:00:00+03:00` }); // a conflict
  out[out.length - 1].origin = "extracted";
  return out;
})();

function mockDaily(kind: string, from: string, to: string) {
  const days = new Map<string, { date: string; value: number; sets: number | null; conflict: boolean }>();
  for (const r of [...metricsDb].filter((r) => r.kind === kind && r.date >= from && r.date <= to).sort((a, b) => a.created_at.localeCompare(b.created_at))) {
    const d = days.get(r.date);
    if (kind === "water") days.set(r.date, { date: r.date, value: (d?.value ?? 0) + (r.value ?? 0), sets: null, conflict: false });
    else if (kind === "workout") days.set(r.date, { date: r.date, value: (d?.value ?? 0) + 1, sets: (d?.sets ?? 0) + (r.sets ?? 0), conflict: false });
    else days.set(r.date, { date: r.date, value: r.value ?? 0, sets: null, conflict: !!d && d.value !== r.value });
  }
  return [...days.values()].sort((a, b) => a.date.localeCompare(b.date));
}
const avg = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);

function mockMetrics(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "metrics_overview":
      return ["sleep", "weight", "steps", "water", "workout"].map((kind) => {
        const values = mockDaily(kind, day(-13), day(0));
        const spark = Array.from({ length: 14 }, (_, i) => values.find((v) => v.date === day(i - 13))?.value ?? null);
        return { kind, last: values.at(-1) ?? null, average7: avg(values.filter((v) => v.date >= day(-6)).map((v) => v.value)), spark, conflicts: values.filter((v) => v.conflict).length };
      });
    case "metrics_summary": {
      const n = Number(args.days);
      const values = mockDaily(String(args.kind), day(1 - n), day(0));
      const nums = values.map((v) => v.value);
      return { kind: args.kind, days: values, average: avg(nums), min: nums.length ? Math.min(...nums) : null, max: nums.length ? Math.max(...nums) : null, trend: 0.3, weekly: [{ week_start: day(-6), average: avg(nums) ?? 0 }], conflicts: values.filter((v) => v.conflict).length };
    }
    case "metric_records": {
      const n = Number(args.days);
      return metricsDb.filter((r) => r.kind === args.kind && r.date >= day(1 - n)).sort((a, b) => b.date.localeCompare(a.date) || b.created_at.localeCompare(a.created_at));
    }
    case "metric_log":
    case "metric_edit": {
      const input = args.input as { kind: string; date: string; value: number | null; unit: string | null; exercise: string | null; sets: number | null; reps: number | null; confirmed: boolean };
      const value = input.kind === "water" && input.unit === "glass" ? (input.value ?? 0) * 250 : input.value;
      if (input.kind !== "workout" && value === null) throw "missing_value";
      if (input.kind === "weight" && value !== null && value > 400 && !input.confirmed) throw `out_of_range|${value}`;
      const fields = { kind: input.kind, date: input.date, value, exercise: input.exercise, sets: input.sets, reps: input.reps };
      if (cmd === "metric_edit") {
        metricsDb = metricsDb.map((r) => (r.metric_id === args.id ? { ...r, ...fields, user_modified: true } : r));
      } else {
        metricsDb.push({ metric_id: `m${metricsDb.length + 100}`, unit: null, origin: "manual", user_modified: false, note_path: null, block_text: null, created_at: new Date().toISOString(), ...fields });
      }
      emit("metrics-changed", null);
      return "ok";
    }
    case "metric_delete":
      metricsDb = metricsDb.filter((r) => r.metric_id !== args.id);
      emit("metrics-changed", null);
      return undefined;
    case "metric_resolve": {
      const keep = metricsDb.find((r) => r.metric_id === args.keepId);
      metricsDb = metricsDb.filter((r) => !(keep && r.kind === keep.kind && r.date === keep.date && r.metric_id !== keep.metric_id && r.origin === "extracted"));
      emit("metrics-changed", null);
      return undefined;
    }
  }
  return undefined;
}

// Links and search stand-in: plain substring search over the mock files, same shapes as Rust.
function mockLinks(cmd: string, args: Record<string, unknown>): unknown {
  const title = (p: string) => p.split("/").pop()!.replace(/\.md$/i, "");
  const fold = (s: string) => s.toLocaleLowerCase("tr");
  switch (cmd) {
    case "search_notes": {
      const words = fold(String(args.query)).split(/\s+/).filter(Boolean);
      if (!words.length && !args.tag) return [];
      return [...files.entries()]
        .filter(([p, text]) => (!args.folder || p.startsWith(`${args.folder}/`)) && (!args.tag || fold(text).includes(`#${fold(String(args.tag))}`)) && words.every((w) => fold(text + " " + title(p)).includes(w)))
        .map(([p, text]) => {
          const line = text.split("\n").find((l) => words.some((w) => fold(l).includes(w))) ?? null;
          const snippet = words.reduce((s, w) => s.replace(new RegExp(w, "i"), (m) => `\u0002${m}\u0003`), (line ?? text).slice(0, 100));
          return { note_path: p, title: title(p), snippet, line_text: line };
        });
    }
    case "quick_open": {
      const q = fold(String(args.query));
      return [...files.keys()].filter((p) => fold(p).includes(q)).slice(0, 20).map((p) => ({ note_path: p, title: title(p), alias: null }));
    }
    case "backlinks": {
      const name = fold(title(String(args.path)));
      const out: unknown[] = [];
      for (const [p, text] of files) {
        if (p === args.path) continue;
        text.split("\n").forEach((l, i) => {
          if (fold(l).includes(`[[${name}`)) out.push({ source_path: p, source_title: title(p), line: i + 1, line_text: l });
        });
      }
      return out;
    }
    case "list_tags": {
      const counts = new Map<string, number>();
      for (const text of files.values()) for (const m of new Set(text.match(/#[\p{L}\p{N}_/-]+/gu) ?? [])) counts.set(fold(m.slice(1)), (counts.get(fold(m.slice(1))) ?? 0) + 1);
      return [...counts].sort().map(([tag, count]) => ({ tag, count }));
    }
    case "open_link": {
      const target = fold(String(args.target).split("#")[0]);
      const hit = [...files.keys()].find((p) => fold(title(p)) === target || fold(p.replace(/\.md$/i, "")) === target);
      if (hit) return { path: hit, created: false };
      const path = `inbox/${String(args.target).split("#")[0].split("/").pop()}.md`;
      files.set(path, "");
      emit("tree-changed", null);
      return { path, created: true };
    }
  }
  return undefined;
}

export async function mockBackend<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const path = String(args.path ?? "");
  switch (cmd) {
    case "startup":
      return { vault_path: vault, first_run: vault === null, show_wizard: !setupDone, language: mockLang, settings_recovered: false, theme: "dark", error: null, inbox: vault ? "inbox" : null } as T;
    case "wizard_defaults":
      return { suggested_vault: String.raw`C:\Users\ayse\OneDrive\Belgeler\PLA Vault` } as T;
    case "inspect_vault_folder": {
      // Browser check: the name picks the state ("notlar" = notes, "\\\\" = network, ".txt" = file).
      const p = String(args.path).trim();
      const state = p.startsWith("\\\\") ? "network" : !/^[A-Za-z]:[\\/]/.test(p) ? "relative" : p.endsWith(".txt") ? "not_folder" : /notlar/i.test(p) ? "notes" : "missing";
      return { check: { state, md_files: state === "notes" ? 128 : 0, more: false }, in_onedrive: /onedrive/i.test(p) } as T;
    }
    case "setup_vault":
      if (String(args.path).includes("Program Files")) throw `not_writable|${args.path}|Erişim engellendi. (os error 5)`;
      vault = String(args.path).trim();
      return { path: vault, inbox: "inbox" } as T;
    case "set_language":
      mockLang = args.lang as "tr" | "en";
      return undefined as T;
    case "metrics_overview":
    case "metrics_summary":
    case "metric_records":
    case "metric_log":
    case "metric_edit":
    case "metric_delete":
    case "metric_resolve":
      return mockMetrics(cmd, args) as T;
    case "search_notes":
    case "quick_open":
    case "backlinks":
    case "list_tags":
    case "open_link":
      return mockLinks(cmd, args) as T;
    case "settings_get":
      return { language: mockLang, theme: mockTheme, autostart: mockAutostart, paused: false, vault_path: vault } as T;
    case "settings_set":
      if (args.key === "language") mockLang = args.value as "tr" | "en" | null;
      else if (args.key === "theme") mockTheme = args.value as "dark" | "light";
      return { language: mockLang, theme: mockTheme, autostart: mockAutostart, paused: false, vault_path: vault } as T;
    case "set_autostart":
      mockAutostart = Boolean(args.enabled);
      emit("autostart-changed", mockAutostart);
      return undefined as T;
    case "send_test_notification":
      return undefined as T;
    case "notification_status":
      return false as T;
    case "model_remove":
      installed = null;
      emit("model-removed", null);
      return undefined as T;
    case "backup_now":
      setTimeout(() => {
        lastBackup = new Date().toISOString();
        emit("backup-changed", null);
      }, 600);
      return undefined as T;
    case "backup_status":
      return { job: { last_success_at: lastBackup, last_attempt_at: lastBackup, last_error: null }, backups: lastBackup ? ["pla-2026-10-05.db"] : [] } as T;
    case "export_data":
      return [`${args.dir}/PLA-gorevler.csv`, `${args.dir}/PLA-olcumler.csv`] as T;
    case "open_place":
      return undefined as T;
    case "about_info":
      return { version: "0.1.0", model: installed?.name ?? null, model_licence: installed ? "Apache-2.0" : null, components: [{ name: "llama.cpp", licence: "MIT", url: "https://github.com/ggml-org/llama.cpp" }, { name: "Svelte", licence: "MIT", url: "https://svelte.dev" }] } as T;
    case "finish_setup":
      setupDone = true;
      return undefined as T;
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
      const copy = path.replace(/\.md$/i, " (conflict).md");
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
      pending.due = pending.due.filter((r) => r.task_id !== args.id);
      pending.missed = pending.missed.filter((r) => r.task_id !== args.id);
      return undefined as T;
    case "hide_to_tray":
      return undefined as T;
    case "pending_reminders": {
      const open = (r: { task_id: string }) => !tasks.some((t) => t.task_id === r.task_id && t.status === "done");
      pending = { due: pending.due.filter(open), missed: pending.missed.filter(open) };
      return { due: [...pending.due], missed: [...pending.missed] } as T;
    }
    case "dismiss_missed":
      pending.missed = [];
      return undefined as T;
    case "set_paused":
      for (const cb of mockListeners.get("paused-changed") ?? []) cb(Boolean(args.paused));
      return undefined as T;
    case "model_status":
      return { installed, recommended: RECOMMENDED, download, models_dir: "C:/Users/me/AppData/Local/PLA/models" } as T;
    case "model_download_start":
      clearInterval(timer);
      timer = window.setInterval(() => {
        received = Math.min(RECOMMENDED.size, received + RECOMMENDED.size / 10);
        download = { state: "running", progress: { received, total: RECOMMENDED.size, bytes_per_sec: 8_703_180, eta_secs: Math.round((RECOMMENDED.size - received) / 8_703_180) } };
        emit("model-download", download);
        if (received >= RECOMMENDED.size) {
          clearInterval(timer);
          download = { state: "done", path: "C:/Users/me/AppData/Local/PLA/models/gemma-4-E2B-it-Q3_K_M.gguf" };
          emit("model-download", download);
          installed = { name: RECOMMENDED.name, size: RECOMMENDED.size, path: "C:/…/gemma-4-E2B-it-Q3_K_M.gguf", local: false };
          emit("model-changed", installed);
        }
      }, 300);
      return undefined as T;
    case "model_download_pause":
      clearInterval(timer);
      download = { state: "paused", received, total: RECOMMENDED.size };
      emit("model-download", download);
      return undefined as T;
    case "model_use_local": {
      const path = String(args.path);
      if (!path.toLowerCase().endsWith(".gguf")) throw new Error("not_gguf");
      installed = { name: path.split(/[\\/]/).pop() ?? path, size: 2_536_786_016, path, local: true };
      emit("model-changed", installed);
      return installed as T;
    }
    default:
      throw new Error(`mock: unknown command ${cmd}`);
  }
}

// Dev helpers for the browser check: simulate the scheduler.
type MockReminder = { task_id: string; title: string; notify_at: string };
let pending: { due: MockReminder[]; missed: MockReminder[] } = { due: [], missed: [] };
(globalThis as Record<string, unknown>).__plaMockDue = (title: string) => {
  const r = { task_id: "t1", title, notify_at: "2026-10-06T09:00" };
  pending.due = [...pending.due.filter((x) => x.task_id !== r.task_id), r];
  for (const cb of mockListeners.get("reminder-due") ?? []) cb(r);
};
(globalThis as Record<string, unknown>).__plaMockMissed = () => {
  const list = [
    { task_id: "t2", title: "Fatura öde", notify_at: "2026-10-05T09:00" },
    { task_id: "t3", title: "Kitap oku", notify_at: "2026-10-05T20:00" },
  ];
  pending.missed = list;
  for (const cb of mockListeners.get("missed-reminders") ?? []) cb(list);
};
(globalThis as Record<string, unknown>).__plaMockFail = (kind: string) => { clearInterval(timer); download = { state: "failed", failure: { kind, needed: 6_700_000_000, available: 4_400_000_000, status: 503, detail: kind }, received, total: RECOMMENDED.size }; emit("model-download", download); };
