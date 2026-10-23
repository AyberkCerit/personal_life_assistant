// Svelte actions and key handlers the inline forms share (task rows, routines).

/** Puts keyboard focus on an element as soon as it appears (IR-UI-003). */
export function focusOnMount(node: HTMLElement) {
  node.focus();
}

/** Calls `cancel` on Escape. */
export function onEscape(e: KeyboardEvent, cancel: () => void) {
  if (e.key === "Escape") {
    e.preventDefault();
    cancel();
  }
}

/** Escape cancels the form it is attached to. */
export function escapeCancels(node: HTMLElement, cancel: () => void) {
  const handler = (e: KeyboardEvent) => onEscape(e, cancel);
  node.addEventListener("keydown", handler);
  return { destroy: () => node.removeEventListener("keydown", handler) };
}
