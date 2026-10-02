import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { mockBackend, mockListeners } from "./mock";

export interface TaskInput { title: string; details?: string | null; date?: string | null; time?: string | null }
export interface ReviewItem {
  review_id: string;
  reason: string;
  title: string | null;
  is_action: boolean;
  payload_json: string;
  note_path: string | null;
  block_text: string | null;
  created_at: string;
}
export interface AddedItem { kind: "task" | "metric"; id: string; title: string }
export interface VaultInfo { path: string; inbox: string }
export type { Task } from "./tasks";
import type { Task, TaskList } from "./tasks";

export interface TreeEntry { path: string; name: string; is_dir: boolean; depth: number }
export interface NoteFile { text: string; hash: string; read_only: boolean }
export type SaveResult = { kind: "saved"; hash: string } | { kind: "conflict"; current_hash: string } | { kind: "missing" };
export interface StartupInfo {
  vault_path: string | null;
  first_run: boolean;
  settings_recovered: boolean;
  theme: "dark" | "light";
  error: string | null;
  inbox: string | null;
}
export interface WorkerStatus {
  queued: number;
  model: "not_installed" | "off" | "running";
  busy: boolean;
  last_error: string | null;
  added: number;
}

/** Inside the Tauri window; false in a plain browser (Vite dev server), where a mock backend answers. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return inTauri ? invoke<T>(cmd, args) : mockBackend<T>(cmd, args ?? {});
}

function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (inTauri) return listen<T>(event, (e) => cb(e.payload));
  const entry = cb as (payload: unknown) => void;
  mockListeners.set(event, [...(mockListeners.get(event) ?? []), entry]);
  return Promise.resolve(() => mockListeners.set(event, (mockListeners.get(event) ?? []).filter((f) => f !== entry)));
}

export const api = {
  startup: () => call<StartupInfo>("startup"),
  openVault: (path: string) => call<VaultInfo>("open_vault", { path }),
  listTree: () => call<TreeEntry[]>("list_tree"),
  readNote: (path: string) => call<NoteFile>("read_note", { path }),
  saveNote: (path: string, text: string, expectedHash: string | null) =>
    call<SaveResult>("save_note", { path, text, expectedHash }),
  saveCopy: (path: string, text: string) => call<string>("save_copy", { path, text }),
  createNote: (folder: string, title: string) => call<string>("create_note", { folder, title }),
  queueNote: (path: string) => call<void>("queue_note", { path }),
  onStatus: (cb: (s: WorkerStatus) => void) => on<WorkerStatus>("worker-status", cb),
  onTreeChanged: (cb: () => void) => on<unknown>("tree-changed", () => cb()),
  /** A note changed outside PLA (Obsidian, OneDrive…). */
  onNoteChanged: (cb: (path: string) => void) => on<string>("note-changed", cb),
  vaultInfo: () => call<VaultInfo>("vault_info"),
  workerStatus: () => call<WorkerStatus>("worker_status"),
  listTasks: (list: TaskList) => call<Task[]>("list_tasks", { list }),
  addTask: (input: TaskInput) => call<string>("add_task", { input }),
  editTask: (id: string, input: TaskInput) => call<void>("edit_task", { id, input }),
  setTaskDone: (id: string, done: boolean) => call<void>("set_task_done", { id, done }),
  deleteTask: (id: string) => call<void>("delete_task", { id }),
  undoItem: (kind: string, id: string) => call<void>("undo_item", { kind, id }),
  listReview: () => call<ReviewItem[]>("list_review"),
  acceptReview: (id: string, input: TaskInput) => call<string>("accept_review", { id, input }),
  rejectReview: (id: string) => call<void>("reject_review", { id }),
  onItemsAdded: (cb: (items: AddedItem[]) => void) => on<AddedItem[]>("items-added", cb),
};
