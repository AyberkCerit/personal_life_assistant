import { describe, expect, it } from "vitest";
import { ToastQueue } from "./toasts";
import type { Clock } from "./triggers";

class FakeClock implements Clock {
  now = 0;
  private next = 1;
  private timers = new Map<number, { at: number; fn: () => void }>();
  set(fn: () => void, ms: number): number {
    const id = this.next++;
    this.timers.set(id, { at: this.now + ms, fn });
    return id;
  }
  clear(id: number): void {
    this.timers.delete(id);
  }
  advance(ms: number): void {
    const end = this.now + ms;
    for (;;) {
      const due = [...this.timers.entries()].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
      if (!due) break;
      this.timers.delete(due[0]);
      this.now = due[1].at;
      due[1].fn();
    }
    this.now = end;
  }
}

describe("ToastQueue (Final review I5)", () => {
  it("shows at most three and counts the rest", () => {
    const clock = new FakeClock();
    const q = new ToastQueue<string>(10_000, 3, clock, () => clock.now);
    for (const t of ["a", "b", "c", "d", "e"]) q.add(t);
    expect(q.visible().map((i) => i.item)).toEqual(["a", "b", "c"]);
    expect(q.hiddenCount()).toBe(2);
  });

  it("expires each toast after its time and moves the next one up", () => {
    const clock = new FakeClock();
    const q = new ToastQueue<string>(10_000, 1, clock, () => clock.now);
    q.add("a");
    q.add("b");
    clock.advance(9_999);
    expect(q.visible().map((i) => i.item)).toEqual(["a"]);
    clock.advance(1);
    expect(q.visible().map((i) => i.item)).toEqual(["b"]);
    clock.advance(10_000);
    expect(q.visible()).toEqual([]);
  });

  it("pauses while the user hovers or focuses, then gives the remaining time", () => {
    const clock = new FakeClock();
    const q = new ToastQueue<string>(10_000, 3, clock, () => clock.now);
    q.add("a");
    clock.advance(6_000);
    q.pause();
    clock.advance(60_000);
    expect(q.visible().map((i) => i.item)).toEqual(["a"]);
    q.resume();
    clock.advance(3_999);
    expect(q.visible()).toHaveLength(1);
    clock.advance(1);
    expect(q.visible()).toEqual([]);
  });

  it("dismissing removes the toast and tells listeners", () => {
    const clock = new FakeClock();
    let changes = 0;
    const q = new ToastQueue<string>(10_000, 3, clock, () => clock.now, () => changes++);
    const key = q.add("a");
    q.dismiss(key);
    expect(q.visible()).toEqual([]);
    expect(changes).toBe(2);
  });
});
