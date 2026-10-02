// When to save and when to hand a note to extraction (FR-EDT-003, FR-EXT-002, FR-EXT-003).

export const AUTOSAVE_MS = 2000;
export const EXTRACT_IDLE_MS = 45000;

export interface Clock {
  set(fn: () => void, ms: number): number;
  clear(id: number): void;
}

const browserClock: Clock = {
  set: (fn, ms) => window.setTimeout(fn, ms),
  clear: (id) => window.clearTimeout(id),
};

export class NoteSession {
  private saveTimer: number | null = null;
  private idleTimer: number | null = null;
  private editedSinceQueued = false;

  constructor(
    private readonly path: string,
    private readonly save: (path: string) => Promise<void>,
    private readonly queue: (path: string) => void,
    private readonly clock: Clock = browserClock,
  ) {}

  edited(): void {
    this.editedSinceQueued = true;
    this.restart("save", AUTOSAVE_MS, () => void this.flushSave());
    this.restart("idle", EXTRACT_IDLE_MS, () => this.queueIfEdited());
  }

  async blurred(): Promise<void> {
    await this.flushSave();
  }

  async close(): Promise<void> {
    this.stop("idle");
    await this.flushSave();
    this.queueIfEdited();
  }

  private async flushSave(): Promise<void> {
    if (this.saveTimer === null) return;
    this.stop("save");
    await this.save(this.path);
  }

  private queueIfEdited(): void {
    this.idleTimer = null;
    if (this.editedSinceQueued) {
      this.editedSinceQueued = false;
      this.queue(this.path);
    }
  }

  private restart(which: "save" | "idle", ms: number, fn: () => void): void {
    this.stop(which);
    const id = this.clock.set(fn, ms);
    if (which === "save") this.saveTimer = id;
    else this.idleTimer = id;
  }

  private stop(which: "save" | "idle"): void {
    const id = which === "save" ? this.saveTimer : this.idleTimer;
    if (id !== null) this.clock.clear(id);
    if (which === "save") this.saveTimer = null;
    else this.idleTimer = null;
  }
}
