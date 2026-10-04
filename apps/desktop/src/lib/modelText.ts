import type { Progress } from "./api";

const GB = 1024 ** 3;
const MB = 1024 ** 2;

/** Binary units, labelled GB/MB like Windows Explorer; one decimal for GB. */
export function formatBytes(n: number, lang: string): string {
  const fmt = (v: number, digits: number) => new Intl.NumberFormat(lang, { maximumFractionDigits: digits, minimumFractionDigits: digits }).format(v);
  return n >= GB ? `${fmt(n / GB, 1)} GB` : `${fmt(Math.floor(n / MB), 0)} MB`;
}

export function formatEta(secs: number | null, lang: string): string {
  if (secs === null) return "";
  const tr = lang.startsWith("tr");
  if (secs < 60) return tr ? "<1 dk kaldı" : "<1 min left";
  const min = Math.ceil(secs / 60);
  return tr ? `~${min} dk kaldı` : `~${min} min left`;
}

export function progressLine(p: Progress, lang: string): string {
  const speed = new Intl.NumberFormat(lang, { maximumFractionDigits: 1, minimumFractionDigits: 1 }).format(p.bytes_per_sec / MB);
  const unit = lang.startsWith("tr") ? "MB/sn" : "MB/s";
  return `${formatBytes(p.received, lang)} / ${formatBytes(p.total, lang)} · ${speed} ${unit} · ${formatEta(p.eta_secs, lang)}`;
}

const KINDS = ["network", "host", "https", "space", "checksum", "io", "http"];

/** i18n key for a download failure kind (IR-UI-006 texts live in i18n.ts). */
export function failureKey(kind: string): `model.error.${string}` {
  return `model.error.${KINDS.includes(kind) ? kind : "io"}`;
}
