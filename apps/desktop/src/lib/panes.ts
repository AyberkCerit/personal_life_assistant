// Resizable side panels (like Obsidian): the user drags the left or right border, the widths are
// kept per window between runs. The right panel keeps two widths: the assistant tab is wider than
// tasks and links (owner decision 2026-10-07), and each remembers its own.

export interface PaneWidths {
  left: number;
  right: number; // tasks and links
  rightWide: number; // the assistant tab
}

export const DEFAULT_WIDTHS: PaneWidths = { left: 240, right: 320, rightWide: 520 };
export const LIMITS = { left: [180, 480], right: [260, 760] } as const;
/** The note in the middle never gets narrower than this. */
export const MIN_CENTER = 360;

const KEY = "pla.paneWidths";

/** A side's width within its limits, leaving the middle at least MIN_CENTER of `window`. */
export function clampWidth(side: "left" | "right", width: number, other: number, window: number): number {
  const [min, max] = LIMITS[side];
  const room = window - other - MIN_CENTER;
  return Math.round(Math.max(min, Math.min(width, max, room)));
}

export function loadWidths(store: Pick<Storage, "getItem"> | null = safeStorage()): PaneWidths {
  try {
    const saved = JSON.parse(store?.getItem(KEY) ?? "null") as Partial<PaneWidths> | null;
    const pick = (k: keyof PaneWidths) => (typeof saved?.[k] === "number" && Number.isFinite(saved[k]) ? saved[k] : DEFAULT_WIDTHS[k]);
    return { left: pick("left"), right: pick("right"), rightWide: pick("rightWide") };
  } catch {
    return { ...DEFAULT_WIDTHS };
  }
}

export function saveWidths(widths: PaneWidths, store: Pick<Storage, "setItem"> | null = safeStorage()) {
  try {
    store?.setItem(KEY, JSON.stringify(widths));
  } catch {
    // no storage: the widths last until the app closes
  }
}

function safeStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
