import { describe, expect, it } from "vitest";
import { AUTOSAVE_MS, EXTRACT_IDLE_MS, NoteSession, type Clock } from "./triggers";

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

function setup() {
  const clock = new FakeClock();
  const saves: string[] = [];
  const queued: string[] = [];
  const session = new NoteSession("notes/a.md", async (p) => void saves.push(p), (p) => void queued.push(p), clock);
  return { clock, saves, queued, session };
}

describe("NoteSession", () => {
  it("autosaves once, 2 s after the last keystroke", () => {
    const { clock, saves, session } = setup();
    session.edited();
    clock.advance(1500);
    session.edited();
    clock.advance(AUTOSAVE_MS - 1);
    expect(saves).toEqual([]);
    clock.advance(1);
    expect(saves).toEqual(["notes/a.md"]);
  });

  it("never queues extraction because of autosave alone (FR-EXT-003)", () => {
    const { clock, queued, session } = setup();
    session.edited();
    clock.advance(EXTRACT_IDLE_MS - 1);
    expect(queued).toEqual([]);
  });

  it("queues after 45 s without typing (FR-EXT-002)", () => {
    const { clock, queued, session } = setup();
    session.edited();
    clock.advance(EXTRACT_IDLE_MS);
    expect(queued).toEqual(["notes/a.md"]);
    clock.advance(EXTRACT_IDLE_MS * 2);
    expect(queued).toEqual(["notes/a.md"]);
  });

  it("closing saves pending text and queues only an edited note", async () => {
    const { saves, queued, session } = setup();
    await session.close();
    expect(queued).toEqual([]);
    session.edited();
    await session.close();
    expect(saves).toEqual(["notes/a.md"]);
    expect(queued).toEqual(["notes/a.md"]);
  });

  it("blur saves at once without queueing", async () => {
    const { clock, saves, queued, session } = setup();
    session.edited();
    await session.blurred();
    expect(saves).toEqual(["notes/a.md"]);
    clock.advance(AUTOSAVE_MS);
    expect(saves).toEqual(["notes/a.md"]);
    expect(queued).toEqual([]);
  });
});
