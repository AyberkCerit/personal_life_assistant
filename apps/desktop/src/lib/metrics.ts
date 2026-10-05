// Metrics view helpers: formatting and chart layout only. Every statistic comes from the Rust core
// (FR-MET-010); nothing here averages or merges records.
import type { Key } from "./i18n";

export const KINDS = ["sleep", "weight", "steps", "water", "workout"] as const;
export type MetricKind = (typeof KINDS)[number];

export interface DayValue { date: string; value: number; sets: number | null; conflict: boolean }

const UNITS: Record<"tr" | "en", Record<Exclude<MetricKind, "workout">, string>> = {
  tr: { sleep: "sa", weight: "kg", steps: "adım", water: "ml" },
  en: { sleep: "h", weight: "kg", steps: "steps", water: "ml" },
};

function num(value: number, lang: string, digits: number): string {
  return new Intl.NumberFormat(lang, { maximumFractionDigits: digits }).format(value);
}

/** A day's value in its kind's unit; a workout day is its sessions (and their sets). */
export function formatValue(kind: MetricKind, value: number, lang: string, sets: number | null = null): string {
  const l = lang.startsWith("tr") ? "tr" : "en";
  if (kind === "workout") {
    const sessions = l === "tr" ? `${num(value, lang, 0)} oturum` : `${num(value, lang, 0)} session${value === 1 ? "" : "s"}`;
    return sets === null ? sessions : `${sessions} · ${num(sets, lang, 0)} ${l === "tr" ? "set" : sets === 1 ? "set" : "sets"}`;
  }
  if (kind === "water" && value >= 1000) return `${num(value / 1000, lang, 2)} L`;
  return `${num(value, lang, kind === "steps" || kind === "water" ? 0 : 1)} ${UNITS[l][kind]}`;
}

/** A typed amount: "7,5" and "7.5" are the same; for steps a dot or comma groups thousands. */
export function parseAmount(text: string, kind: MetricKind): number | null {
  const t = text.trim().replace(/\s/g, "");
  if (t === "") return null;
  const n = kind === "steps" ? Number(t.replace(/[.,]/g, "")) : Number(t.replace(",", "."));
  return Number.isFinite(n) ? n : null;
}

export interface ChartDay { date: string; value: number | null; conflict: boolean }

function addDays(iso: string, n: number): string {
  const [y, m, d] = iso.split("-").map(Number);
  const date = new Date(Date.UTC(y, m - 1, d + n));
  return date.toISOString().slice(0, 10);
}

/** The core returns only days with a value; the chart needs every day of the period. */
export function fillDays(days: DayValue[], from: string, count: number): ChartDay[] {
  const byDate = new Map(days.map((d) => [d.date, d]));
  return Array.from({ length: count }, (_, i) => {
    const date = addDays(from, i);
    const day = byDate.get(date);
    return { date, value: day ? day.value : null, conflict: day?.conflict ?? false };
  });
}

export interface Point { x: number; y: number }

/** Where each value sits in a `width` × `height` box; the line breaks where a day is missing. */
export function chartGeometry(values: (number | null)[], width: number, height: number, pad: number): { points: (Point | null)[]; path: string } {
  const known = values.filter((v): v is number => v !== null);
  if (known.length === 0) return { points: values.map(() => null), path: "" };
  const [lo, hi] = [Math.min(...known), Math.max(...known)];
  const step = values.length > 1 ? (width - 2 * pad) / (values.length - 1) : 0;
  const points = values.map((v, i) => {
    if (v === null) return null;
    const x = values.length > 1 ? pad + i * step : width / 2;
    const y = hi === lo ? height / 2 : pad + ((hi - v) / (hi - lo)) * (height - 2 * pad);
    return { x, y };
  });
  const r = (n: number) => String(Math.round(n * 10) / 10);
  const path = points
    .map((p, i) => (p ? `${i > 0 && points[i - 1] ? "L" : "M"}${r(p.x)},${r(p.y)}` : ""))
    .filter(Boolean)
    .join(" ");
  return { points, path };
}

/** Up or down only when the change is more than 2 % of the average. */
export function trendDirection(trend: number | null, average: number | null): "up" | "down" | "flat" {
  if (trend === null) return "flat";
  const threshold = Math.max(Math.abs(average ?? 0) * 0.02, 1e-9);
  return Math.abs(trend) < threshold ? "flat" : trend > 0 ? "up" : "down";
}

const CODES = ["out_of_range", "missing_value", "empty_workout", "invalid_date", "not_found", "no_vault"];

/** Reads the metrics commands' errors, e.g. `out_of_range|450` (ask the user to confirm). */
export function metricError(err: string): { key: Key; value: number | null; detail?: string } {
  const [code, value] = err.split("|");
  if (CODES.includes(code)) return { key: `metrics.error.${code}` as Key, value: value !== undefined ? Number(value) : null };
  return { key: "metrics.error.other", value: null, detail: err };
}
