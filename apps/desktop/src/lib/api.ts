import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { mockBackend, mockListeners } from "./mock";
import type { FolderReport } from "./wizard";
import type { JobStatus } from "./settings";
import type { DayValue, MetricKind } from "./metrics";

export interface TaskInput { title: string; details?: string | null; date?: string | null; time?: string | null; remind?: boolean | null }
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
  /** FR-SET-001: show the first-run wizard. */
  show_wizard: boolean;
  /** The language chosen in the wizard; null = follow the OS. */
  language: "tr" | "en" | null;
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
  paused: boolean;
}

export interface DueReminder { task_id: string; title: string; notify_at: string }
export interface CatalogEntry { id: string; name: string; file_name: string; size: number; sha256: string; url: string; source: string; licence: string; licence_url: string }
export interface InstalledModel { name: string; size: number; path: string; local: boolean }
export interface Progress { received: number; total: number; bytes_per_sec: number; eta_secs: number | null }
export interface Failure { kind: string; needed: number | null; available: number | null; status: number | null; detail: string }
export type DownloadState =
  | { state: "running"; progress: Progress }
  | { state: "paused"; received: number; total: number }
  | { state: "failed"; failure: Failure; received: number; total: number }
  | { state: "done"; path: string };
export interface ModelStatus { installed: InstalledModel | null; recommended: CatalogEntry; download: DownloadState | null; models_dir: string | null }
export interface WizardDefaults { suggested_vault: string | null }
export interface SettingsView { language: "tr" | "en" | null; theme: "dark" | "light"; autostart: boolean; paused: boolean; vault_path: string | null }
export interface BackupStatus { job: JobStatus; backups: string[] }
export interface Component { name: string; licence: string; url: string }
export interface About { version: string; model: string | null; model_licence: string | null; components: Component[] }
export type PlaceKind = "vault" | "backups" | "app_data" | "local_data" | "model_folder" | "notification_settings";
export interface MetricRecord {
  metric_id: string;
  kind: MetricKind;
  date: string;
  value: number | null;
  unit: string | null;
  exercise: string | null;
  sets: number | null;
  reps: number | null;
  origin: "manual" | "extracted" | "assistant";
  user_modified: boolean;
  note_path: string | null;
  block_text: string | null;
  source_missing: boolean;
  created_at: string;
}
export interface Summary {
  kind: MetricKind;
  days: DayValue[];
  average: number | null;
  min: number | null;
  max: number | null;
  trend: number | null;
  weekly: { week_start: string; average: number }[];
  conflicts: number;
}
export interface MetricCard { kind: MetricKind; last: DayValue | null; average7: number | null; spark: (number | null)[]; conflicts: number }
export type MetricUnit = "h" | "min" | "ml" | "l" | "glass" | "count" | "kg" | "lb";
export interface MetricInput {
  kind: MetricKind;
  date: string;
  value: number | null;
  unit: MetricUnit | null;
  exercise: string | null;
  sets: number | null;
  reps: number | null;
  confirmed: boolean;
}
export interface SearchHit { note_path: string; title: string; snippet: string; line_text: string | null }
export interface QuickHit { note_path: string; title: string; alias: string | null }
export interface Backlink { source_path: string; source_title: string; line: number; line_text: string }
export interface TagCount { tag: string; count: number }
/** FR-EDT-006/007: the open note's frontmatter, `lines` with both fences. */
export interface FrontMatter { state: "none" | "ok" | "invalid"; tags: string[]; aliases: string[]; lines: number }
export interface SavedImage { path: string; embed: string }
/** FR-MEM-003/004: the embedding model and how far the notes are embedded. */
export interface MemoryStatus { installed: boolean; entry: CatalogEntry; download: DownloadState | null; pending: number; total: number }
export interface MemoryProgress { pending: number; total: number }
/** FR-MEM-006: a day's summary (cache only). */
export interface DaySummary { date: string; text: string; sources: string[]; generated_at: string; stale: boolean }
import type { QaEvent, QaTurn } from "./qa";
export interface PendingReminders { due: DueReminder[]; missed: DueReminder[] }

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
  wizardDefaults: () => call<WizardDefaults>("wizard_defaults"),
  inspectVaultFolder: (path: string) => call<FolderReport>("inspect_vault_folder", { path }),
  /** Errors are `<code>|<path>|<reason>` (see `setupError`). */
  setupVault: (path: string, lang: string) => call<VaultInfo>("setup_vault", { path, lang }),
  setLanguage: (lang: string) => call<void>("set_language", { lang }),
  finishSetup: () => call<void>("finish_setup"),
  /** FR-QA-003: starts an answer; its progress comes as `qa-event`s with the returned turn id. */
  qaAsk: (question: string, newTopic: boolean) => call<string>("qa_ask", { question, newTopic }),
  qaStop: () => call<void>("qa_stop"),
  qaHistory: () => call<QaTurn[]>("qa_history"),
  qaClear: () => call<void>("qa_clear"),
  qaUndo: (turnId: string, index: number) => call<void>("qa_undo", { turnId, index }),
  onQaEvent: (cb: (e: QaEvent) => void) => on<QaEvent>("qa-event", cb),
  daySummary: (path: string) => call<DaySummary | null>("day_summary", { path }),
  summaryRegenerate: (path: string) => call<void>("summary_regenerate", { path }),
  onSummaryChanged: (cb: (date: string) => void) => on<string>("summary-changed", cb),
  memoryStatus: () => call<MemoryStatus>("memory_status"),
  memoryDownloadStart: () => call<void>("memory_download_start"),
  memoryDownloadPause: () => call<void>("memory_download_pause"),
  onMemoryDownload: (cb: (s: DownloadState) => void) => on<DownloadState>("memory-download", cb),
  onMemoryProgress: (cb: (p: MemoryProgress) => void) => on<MemoryProgress>("memory-progress", cb),
  onMemoryChanged: (cb: () => void) => on<null>("memory-changed", () => cb()),
  noteMeta: (text: string) => call<FrontMatter>("note_meta", { text }),
  /** FR-EDT-016: the image bytes; rejects `missing|<target>|` when the vault has no such image. */
  imageBytes: (target: string) => (inTauri ? invoke<ArrayBuffer>("image_bytes", { target }) : mockBackend<ArrayBuffer>("image_bytes", { target })),
  /** FR-EDT-017: the bytes go as the raw body, a dropped file's name in a header. */
  saveImage: (bytes: Uint8Array, name: string | null) =>
    inTauri
      ? invoke<SavedImage>("save_image", bytes, { headers: name ? { "x-name": encodeURIComponent(name) } : {} })
      : mockBackend<SavedImage>("save_image", { name }),
  linkCount: (path: string) => call<number>("link_count", { path }),
  renameNote: (path: string, name: string, updateLinks: boolean) => call<string>("rename_note", { path, name, updateLinks }),
  moveEntry: (path: string, folder: string) => call<string>("move_entry", { path, folder }),
  renameFolder: (path: string, name: string) => call<string>("rename_folder", { path, name }),
  deleteEntry: (path: string) => call<void>("delete_entry", { path }),
  createFolder: (parent: string, name: string) => call<string>("create_folder", { parent, name }),
  listTemplates: () => call<string[]>("list_templates"),
  createNoteIn: (folder: string, title: string, template: string | null) => call<string>("create_note_in", { folder, title, template }),
  openToday: (lang: string) => call<string>("open_today", { lang }),
  searchNotes: (query: string, folder: string | null, tag: string | null) => call<SearchHit[]>("search_notes", { query, folder, tag }),
  quickOpen: (query: string) => call<QuickHit[]>("quick_open", { query }),
  backlinks: (path: string) => call<Backlink[]>("backlinks", { path }),
  listTags: () => call<TagCount[]>("list_tags"),
  /** Resolves a wikilink; a missing target is created in the inbox (FR-EDT-009). */
  openLink: (target: string) => call<{ path: string; created: boolean }>("open_link", { target }),
  onIndexChanged: (cb: () => void) => on<unknown>("index-changed", () => cb()),
  metricsOverview: () => call<MetricCard[]>("metrics_overview"),
  metricsSummary: (kind: MetricKind, days: number) => call<Summary>("metrics_summary", { kind, days }),
  metricRecords: (kind: MetricKind, days: number) => call<MetricRecord[]>("metric_records", { kind, days }),
  /** Errors are codes, e.g. `out_of_range|450` (see `metricError`). */
  metricLog: (input: MetricInput) => call<string>("metric_log", { input }),
  metricEdit: (id: string, input: MetricInput) => call<void>("metric_edit", { id, input }),
  metricDelete: (id: string) => call<void>("metric_delete", { id }),
  metricResolve: (keepId: string) => call<void>("metric_resolve", { keepId }),
  onMetricsChanged: (cb: () => void) => on<unknown>("metrics-changed", () => cb()),
  onOpenQuickMetric: (cb: () => void) => on<unknown>("open-quick-metric", () => cb()),
  settingsGet: () => call<SettingsView>("settings_get"),
  /** Applies at once; errors are `invalid_setting|<key>|<value>`. */
  settingsSet: (key: "language" | "theme", value: string | null) => call<SettingsView>("settings_set", { key, value }),
  setAutostart: (enabled: boolean) => call<void>("set_autostart", { enabled }),
  sendTestNotification: () => call<void>("send_test_notification"),
  notificationStatus: () => call<boolean>("notification_status"),
  modelRemove: () => call<void>("model_remove"),
  backupNow: () => call<void>("backup_now"),
  backupStatus: () => call<BackupStatus>("backup_status"),
  exportData: (dir: string, lang: string) => call<string[]>("export_data", { dir, lang }),
  openPlace: (kind: PlaceKind) => call<void>("open_place", { kind }),
  aboutInfo: () => call<About>("about_info"),
  onOpenSettings: (cb: () => void) => on<unknown>("open-settings", () => cb()),
  onAutostartChanged: (cb: (on: boolean) => void) => on<boolean>("autostart-changed", cb),
  onBackupChanged: (cb: () => void) => on<unknown>("backup-changed", () => cb()),
  onModelRemoved: (cb: () => void) => on<unknown>("model-removed", () => cb()),
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
  reminderDone: (id: string) => call<void>("reminder_done", { id }),
  reminderSnooze: (id: string) => call<void>("reminder_snooze", { id }),
  setPaused: (paused: boolean) => call<void>("set_paused", { paused }),
  /** Reminders shown but not answered yet (kept by the app, so none is lost before the UI listens). */
  pendingReminders: () => call<PendingReminders>("pending_reminders"),
  modelStatus: () => call<ModelStatus>("model_status"),
  modelDownloadStart: () => call<void>("model_download_start"),
  modelDownloadPause: () => call<void>("model_download_pause"),
  modelUseLocal: (path: string) => call<InstalledModel>("model_use_local", { path }),
  onModelDownload: (cb: (s: DownloadState) => void) => on<DownloadState>("model-download", cb),
  onModelChanged: (cb: (m: InstalledModel) => void) => on<InstalledModel>("model-changed", cb),
  dismissMissed: () => call<void>("dismiss_missed"),
  hideToTray: () => call<void>("hide_to_tray"),
  onReminderDue: (cb: (r: DueReminder) => void) => on<DueReminder>("reminder-due", cb),
  onMissedReminders: (cb: (list: DueReminder[]) => void) => on<DueReminder[]>("missed-reminders", cb),
  onTasksChanged: (cb: () => void) => on<unknown>("tasks-changed", () => cb()),
  onNewNote: (cb: () => void) => on<unknown>("new-note", () => cb()),
  onPausedChanged: (cb: (paused: boolean) => void) => on<boolean>("paused-changed", cb),
};
