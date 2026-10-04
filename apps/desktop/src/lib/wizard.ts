// First-run wizard rules that need no backend (FR-SET-001…009).
import type { VaultInfo } from "./api";
import type { Key } from "./i18n";

export const STEPS = ["language", "vault", "model", "done"] as const;
export type Step = (typeof STEPS)[number];

/** Wizard progress, kept by App so a language switch (a re-mount) does not lose it. */
export interface Progress { step: Step; path: string; opened: VaultInfo | null }

export function nextStep(step: Step): Step {
  return STEPS[Math.min(STEPS.indexOf(step) + 1, STEPS.length - 1)];
}

export function prevStep(step: Step): Step {
  return STEPS[Math.max(STEPS.indexOf(step) - 1, 0)];
}

export type FolderState = "missing" | "empty" | "notes" | "other" | "network" | "not_folder";
export interface FolderReport { check: { state: FolderState; md_files: number }; in_onedrive: boolean }
export interface Advice { key: Key; tone: "info" | "warning" | "danger"; usable: boolean; count?: number }

/** What choosing this folder will do, shown before the user commits to it (FR-SET-005…007). */
export function folderAdvice(report: FolderReport): Advice[] {
  const { state, md_files } = report.check;
  const usable = state !== "network" && state !== "not_folder";
  const first: Advice = { key: `wizard.folder.${state}` as Key, tone: usable ? "info" : "danger", usable };
  if (state === "notes") first.count = md_files;
  const advice = [first];
  if (usable && report.in_onedrive) advice.push({ key: "wizard.folder.onedrive", tone: "warning", usable: true });
  return advice;
}

/** Reads `setup_vault`'s `<code>|<path>|<reason>` errors; anything else is shown as it came. */
export function setupError(err: string): { key: Key; path: string; reason: string } {
  const [code, path = "", ...rest] = err.split("|");
  if (code === "network" || code === "not_folder" || code === "not_writable") {
    return { key: `wizard.error.${code}` as Key, path, reason: rest.join("|") };
  }
  return { key: "wizard.error.other", path: "", reason: err };
}
