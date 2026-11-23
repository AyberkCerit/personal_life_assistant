/** How often, at most, the window tells the backend the user is there. The daily maintenance waits
 * for five minutes away, so half a minute of slack is fine. */
export const ACTIVE_EVERY_MS = 30_000;

/** The window events that count as the user being there. */
export const ACTIVITY_EVENTS = ["keydown", "pointerdown", "pointermove", "wheel"] as const;

/** A handler for the activity events that calls `send` at most once every `every` ms. Off Windows
 * the backend measures idle time from these (the OS does not tell it; see `system.rs`). */
export function activityReporter(send: () => void, now: () => number = Date.now, every = ACTIVE_EVERY_MS): () => void {
  let last = -Infinity;
  return () => {
    const t = now();
    if (t - last < every) return;
    last = t;
    send();
  };
}
