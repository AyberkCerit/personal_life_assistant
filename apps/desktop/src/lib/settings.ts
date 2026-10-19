// Settings screen rules that need no backend (FR-SET-011).
import { t, tf, type Key, errorText, failure, splitError, type Failure } from "./i18n";
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
  const { code, detail: path, reason } = splitError(err);
  // `missing` names a folder of its own; with none it is the general "not found" (M6 final review)
  if (code === "missing" && !path) return { key: "settings.error.other", path: "", reason: err };
  // the previous vault's own failure is a code too: said, not shown raw (M6 final review)
  if (code === "vault_lost") return { key: "settings.error.vault_lost", path, reason: errorText(reason).text };
  if (OWN_CODES.includes(code)) {
    const parsed = setupError(`not_writable|${path}|${reason}`);
    return { ...parsed, key: `settings.error.${code}` as Key };
  }
  const folder = setupError(err);
  return folder.key === "wizard.error.other" ? { key: "settings.error.other", path: "", reason: err } : folder;
}

export interface JobStatus { last_success_at: string | null; last_attempt_at: string | null; last_error: string | null }

/** The Scheduler section's line about the daily backup. */
export function backupLine(job: JobStatus): { key: Key; at?: string; error?: string } {
  // the job keeps a code (M6 translations); an older English reason reads as unexpected
  if (job.last_error && job.last_attempt_at) return { key: "settings.backup.failed", at: job.last_attempt_at, error: errorText(job.last_error).text };
  if (job.last_success_at) return { key: "settings.backup.last", at: job.last_success_at };
  return { key: "settings.backup.never" };
}

/** FR-SCH-011: when the weekly report was last written, or why it failed. */
export function reportLine(job: JobStatus): { key: Key; at?: string; error?: string } {
  if (job.last_error && job.last_attempt_at) return { key: "settings.report.failed", at: job.last_attempt_at, error: errorText(job.last_error).text };
  if (job.last_success_at) return { key: "settings.report.last", at: job.last_success_at };
  return { key: "settings.report.never" };
}

/** The weekly report's days, Monday first (0 = Monday, as the settings file keeps them). */
export const WEEKDAYS: Key[] = ["day.mon", "day.tue", "day.wed", "day.thu", "day.fri", "day.sat", "day.sun"];

/** Runs a settings action; resolves to its error as the user reads it, or `null` when it worked. */
export async function attempt(action: () => Promise<void>): Promise<string | Failure | null> {
  try {
    await action();
    return null;
  } catch (e) {
    return describeError(e);
  }
}

/** A `YYYY-MM-DD` day written out ("19 Ekim 2026"), for the backups the restore offers. */
export function longDay(iso: string, language: string): string {
  const [y, m, d] = iso.split("-").map(Number);
  return new Intl.DateTimeFormat(language, { day: "numeric", month: "long", year: "numeric" }).format(new Date(y, m - 1, d));
}

/** An error from a settings command as text for the user, naming the folder and the reason. */
export function describeError(e: unknown): string | Failure {
  const { key, path, reason, reasonKey } = settingsError(String(e));
  if (key === "settings.error.other") return failure(e); // M6 translations: no English reason inline
  return tf(key, { path, reason: reasonKey ? t(reasonKey) : reason });
}

/** What the vault offers when it opens (FR-BKP-004/008, FR-LOG-007). */
export interface RestoreOffer { backup: string | null; damaged: boolean }

/** The day a backup was taken, from its name `pla-<YYYY-MM-DD>.db`. */
export function backupDay(name: string): string | null {
  const m = /^pla-(\d{4}-\d{2}-\d{2})\.db$/.exec(name);
  return m ? m[1] : null;
}

/** The restore dialog's message: no data here, or data damaged by an unclean stop. */
export function offerText(offer: RestoreOffer): { key: Key; day: string; canRestore: boolean } {
  const day = offer.backup ? (backupDay(offer.backup) ?? "") : "";
  if (!offer.damaged) return { key: "restore.missing", day, canRestore: true };
  return offer.backup ? { key: "restore.damaged", day, canRestore: true } : { key: "restore.damagedNone", day: "", canRestore: false };
}
