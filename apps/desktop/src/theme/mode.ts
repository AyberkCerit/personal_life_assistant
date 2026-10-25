/** The saved theme choice; "system" follows the Windows light/dark mode. */
export type ThemePref = "system" | "dark" | "light";

const DARK_QUERY = "(prefers-color-scheme: dark)";

/** The palette to draw with: tokens.css has only "dark" and "light". */
export function resolveTheme(pref: ThemePref, systemDark: boolean): "dark" | "light" {
  if (pref === "system") return systemDark ? "dark" : "light";
  return pref;
}

let stopFollowing: (() => void) | null = null;

/** Sets `data-theme` on the page; for "system" it also follows later changes of the Windows mode. */
export function applyTheme(pref: ThemePref, root: HTMLElement = document.documentElement) {
  stopFollowing?.();
  stopFollowing = null;
  const query = typeof matchMedia === "function" ? matchMedia(DARK_QUERY) : null;
  root.dataset.theme = resolveTheme(pref, query?.matches ?? true);
  if (pref !== "system" || !query) return;
  const follow = (e: MediaQueryListEvent) => (root.dataset.theme = resolveTheme("system", e.matches));
  query.addEventListener("change", follow);
  stopFollowing = () => query.removeEventListener("change", follow);
}
