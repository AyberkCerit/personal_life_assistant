// Weekly routines for the Routines tab (spec: docs/superpowers/specs/2026-10-18-rutinler-design.md). No framework code.
import { splitError, tf, tIn, type Key, type Lang } from "./i18n";

export type DayKey = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";
/** ISO week order, Monday first. */
export const WEEK: DayKey[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

export interface RoutineDay { day: DayKey; time: string | null }
export interface RoutineInput { title: string; days: RoutineDay[] }

export interface Routine {
  routine_id: string;
  title: string;
  days: RoutineDay[];
  paused: boolean;
  origin: "manual" | "extracted" | "assistant";
  note_path: string | null;
  block_text: string | null;
  source_missing: boolean;
  user_modified: boolean;
  week_done: number;
  week_due: number;
}

/**
 * "Pzt · Cum · Paz — 20:00" when every day has the same time, else the time sits beside each day
 * ("Pzt 20:00 · Cum 18:00"). Days run Monday to Sunday.
 */
export function dayLabels(days: RoutineDay[], lang: Lang): string {
  const sorted = [...days].sort((a, b) => WEEK.indexOf(a.day) - WEEK.indexOf(b.day));
  const name = (d: DayKey) => tIn(lang, `day.short.${d}` as Key);
  const first = sorted[0]?.time ?? null;
  if (sorted.every((d) => d.time === first)) {
    const names = sorted.map((d) => name(d.day)).join(" · ");
    return first ? `${names} — ${first}` : names;
  }
  return sorted.map((d) => (d.time ? `${name(d.day)} ${d.time}` : name(d.day))).join(" · ");
}

/** "bu hafta 1/5" / "this week 1/5". */
export function weekCount(r: Pick<Routine, "week_done" | "week_due">): string {
  return tf("routines.week", { done: r.week_done, due: r.week_due });
}

const SHARED_CODES = ["bad_title", "bad_time", "not_found"];

/** Reads the routine commands' `<code>|<detail>|<reason>` errors; task codes reuse the task texts. */
export function routineError(err: string): { key: Key; detail: string } {
  const { code, detail } = splitError(err);
  if (code === "no_days") return { key: "routines.error.no_days", detail };
  if (SHARED_CODES.includes(code)) return { key: `tasks.error.${code}` as Key, detail };
  return { key: "tasks.error.other", detail: err };
}
