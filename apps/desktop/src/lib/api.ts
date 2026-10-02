import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { mockBackend } from "./mock";

export interface TreeEntry { path: string; name: string; is_dir: boolean; depth: number }
export interface NoteFile { text: string; hash: string; read_only: boolean }
export type SaveResult = { kind: "saved"; hash: string } | { kind: "conflict"; current_hash: string } | { kind: "missing" };
export interface StartupInfo {
  vault_path: string | null;
  first_run: boolean;
  settings_recovered: boolean;
  theme: "dark" | "light";
  error: string | null;
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
  return inTauri ? listen<T>(event, (e) => cb(e.payload)) : Promise.resolve(() => {});
}

export const api = {
  startup: () => call<StartupInfo>("startup"),
  openVault: (path: string) => call<string>("open_vault", { path }),
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
};
