// Settings screen rules that need no backend (FR-SET-011).
import { t, tf, type Key } from "./i18n";
import { setupError } from "./wizard";

export const SECTIONS = ["general", "vault", "ai", "scheduler", "notifications", "privacy", "about"] as const;
export type Section = (typeof SECTIONS)[number];

/** The next element for Tab (or Shift+Tab) inside the settings window, wrapping at the ends. */
export function trapIndex(current: number, count: number, shift: boolean): number {
  if (current < 0) return shift ? count - 1 : 0;
  return (current + (shift ? count - 1 : 1)) % count;
}

const OWN_CODES = ["not_removed", "not_found", "model_busy", "invalid_setting", "no_vault", "download_running", "missing", "vault_lost"];

/** Reads the commands' `<code>|<path>|<reason>` errors; folder errors share the wizard's texts. */
export function settingsError(err: string): { key: Key; path: string; reason: string; reasonKey?: Key } {
  const [code, path = "", ...rest] = err.split("|");
  if (OWN_CODES.includes(code)) {
    const parsed = setupError(`not_writable|${path}|${rest.join("|")}`);
    return { ...parsed, key: `settings.error.${code}` as Key };
  }
  const folder = setupError(err);
  return folder.key === "wizard.error.other" ? { key: "settings.error.other", path: "", reason: err } : folder;
}

export interface JobStatus { last_success_at: string | null; last_attempt_at: string | null; last_error: string | null }

/** The Scheduler section's line about the daily backup. */
export function backupLine(job: JobStatus): { key: Key; at?: string; error?: string } {
  if (job.last_error && job.last_attempt_at) return { key: "settings.backup.failed", at: job.last_attempt_at, error: job.last_error };
  if (job.last_success_at) return { key: "settings.backup.last", at: job.last_success_at };
  return { key: "settings.backup.never" };
}

/** An error from a settings command as text for the user, naming the folder and the reason. */
export function describeError(e: unknown): string {
  const { key, path, reason, reasonKey } = settingsError(String(e));
  return tf(key, { path, reason: reasonKey ? t(reasonKey) : reason });
}
