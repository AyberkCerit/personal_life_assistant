// "Added by AI" toasts (FR-EXT-015): each stays a fixed time, at most a few are shown at once, and
// the timers stop while the user hovers or focuses them so Undo stays reachable (WCAG 2.2.1).

import type { Clock } from "./triggers";

export interface Toast<T> {
  key: number;
  item: T;
}

interface Entry<T> extends Toast<T> {
  remaining: number;
  startedAt: number;
  timer: number | null;
}

const browserClock: Clock = {
  set: (fn, ms) => window.setTimeout(fn, ms),
  clear: (id) => window.clearTimeout(id),
};

export class ToastQueue<T> {
  private entries: Entry<T>[] = [];
  private next = 0;
  private paused = false;

  constructor(
    private readonly showMs: number,
    private readonly max: number,
    private readonly clock: Clock = browserClock,
    private readonly now: () => number = () => Date.now(),
    private readonly onChange: () => void = () => {},
  ) {}

  add(item: T): number {
    const key = this.next++;
    this.entries.push({ key, item, remaining: this.showMs, startedAt: 0, timer: null });
    this.schedule();
    this.onChange();
    return key;
  }

  visible(): Toast<T>[] {
    return this.entries.slice(0, this.max).map(({ key, item }) => ({ key, item }));
  }

  hiddenCount(): number {
    return Math.max(0, this.entries.length - this.max);
  }

  dismiss(key: number): void {
    const entry = this.entries.find((e) => e.key === key);
    if (!entry) return;
    if (entry.timer !== null) this.clock.clear(entry.timer);
    this.entries = this.entries.filter((e) => e.key !== key);
    this.schedule();
    this.onChange();
  }

  pause(): void {
    if (this.paused) return;
    this.paused = true;
    for (const e of this.entries) {
      if (e.timer === null) continue;
      this.clock.clear(e.timer);
      e.timer = null;
      e.remaining -= this.now() - e.startedAt;
    }
  }

  resume(): void {
    if (!this.paused) return;
    this.paused = false;
    this.schedule();
  }

  /** Only the visible toasts count down; the rest wait for their turn. */
  private schedule(): void {
    if (this.paused) return;
    for (const e of this.entries.slice(0, this.max)) {
      if (e.timer !== null) continue;
      e.startedAt = this.now();
      e.timer = this.clock.set(() => {
        e.timer = null;
        this.dismiss(e.key);
      }, e.remaining);
    }
  }
}
