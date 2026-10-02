// One open note's saving rules: saves never overwrite an outside change (FR-VLT-013), never bring
// back a note deleted outside PLA, run one at a time, and leaving the note never loses text.

export type SaveOutcome = { kind: "saved"; hash: string } | { kind: "conflict"; current_hash: string } | { kind: "missing" };

export interface SaveApi {
  save(path: string, text: string, expectedHash: string | null): Promise<SaveOutcome>;
  saveCopy(path: string, text: string): Promise<string>;
}

export class NoteDoc {
  /** Set when the file changed or vanished outside PLA; saving stops until the user decides. */
  problem: "conflict" | "missing" | null = null;
  error: string | null = null;
  private saved: string;
  private chain: Promise<void> = Promise.resolve();

  constructor(
    private readonly path: string,
    text: string,
    private hash: string | null,
    private readonly getText: () => string,
    private readonly api: SaveApi,
    private readonly onChange: () => void = () => {},
  ) {
    this.saved = text;
  }

  get dirty(): boolean {
    return this.getText() !== this.saved;
  }

  /** Queued after any save in flight, so each save sends the hash the previous one produced. */
  save(): Promise<void> {
    this.chain = this.chain.then(() => this.saveNow());
    return this.chain;
  }

  /** Leaving the note (switch, close): returns the path of a safety copy if the text had to go there. */
  async leave(): Promise<string | null> {
    await this.save();
    if (!this.dirty) return null;
    const text = this.getText();
    const copy = await this.api.saveCopy(this.path, text);
    this.saved = text;
    return copy;
  }

  /** The watcher saw this note change outside PLA. */
  externalChange(): "reload" | "conflict" {
    if (!this.dirty) return "reload";
    this.problem = "conflict";
    this.onChange();
    return "conflict";
  }

  private async saveNow(): Promise<void> {
    if (this.problem || !this.dirty) return;
    const text = this.getText();
    try {
      const result = await this.api.save(this.path, text, this.hash);
      if (result.kind === "saved") {
        this.hash = result.hash;
        this.saved = text;
        this.error = null;
      } else {
        this.problem = result.kind;
      }
    } catch (e) {
      this.error = String(e);
    }
    this.onChange();
  }
}
