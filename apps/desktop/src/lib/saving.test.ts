import { describe, expect, it } from "vitest";
import { NoteDoc, type SaveApi, type SaveOutcome } from "./saving";

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

function fake(outcomes: Array<SaveOutcome | Error>) {
  const saves: Array<{ text: string; hash: string | null }> = [];
  const copies: string[] = [];
  const api: SaveApi = {
    async save(_path, text, hash) {
      saves.push({ text, hash });
      const next = outcomes.shift() ?? { kind: "saved", hash: `h${saves.length}` };
      if (next instanceof Error) throw next;
      return next;
    },
    async saveCopy(path, text) {
      copies.push(text);
      return path.replace(".md", " (conflict).md");
    },
  };
  return { api, saves, copies };
}

describe("NoteDoc", () => {
  it("serializes saves so each one sends the hash the previous one produced", async () => {
    let text = "a";
    const first = deferred<SaveOutcome>();
    const hashes: Array<string | null> = [];
    const api: SaveApi = {
      save: (_p, _t, hash) => {
        hashes.push(hash);
        return hashes.length === 1 ? first.promise : Promise.resolve({ kind: "saved", hash: "h2" });
      },
      saveCopy: async () => "x",
    };
    const doc = new NoteDoc("n.md", "", "h0", () => text, api);
    const one = doc.save();
    await new Promise((r) => setTimeout(r, 0)); // the first save is now in flight
    text = "ab";
    const two = doc.save();
    first.resolve({ kind: "saved", hash: "h1" });
    await Promise.all([one, two]);
    expect(hashes).toEqual(["h0", "h1"]);
    expect(doc.dirty).toBe(false);
  });

  it("leaving a clean note writes nothing", async () => {
    const { api, saves, copies } = fake([]);
    const doc = new NoteDoc("n.md", "metin", "h0", () => "metin", api);
    expect(await doc.leave()).toBeNull();
    expect(saves).toEqual([]);
    expect(copies).toEqual([]);
  });

  it("leaving after an outside change keeps the user's text as a copy (Final review I1)", async () => {
    const { api, copies } = fake([{ kind: "conflict", current_hash: "dış" }]);
    const doc = new NoteDoc("n.md", "eski", "h0", () => "benim", api);
    await doc.save();
    expect(doc.problem).toBe("conflict");
    expect(await doc.leave()).toBe("n (conflict).md");
    expect(copies).toEqual(["benim"]);
  });

  it("leaving after a failed save keeps a copy too", async () => {
    const { api, copies } = fake([new Error("sharing violation"), new Error("sharing violation")]);
    const doc = new NoteDoc("n.md", "eski", "h0", () => "benim", api);
    await doc.save();
    expect(doc.error).toContain("sharing violation");
    expect(await doc.leave()).toBe("n (conflict).md");
    expect(copies).toEqual(["benim"]);
  });

  it("a note deleted outside is never recreated, the text goes to a copy (Final review I2)", async () => {
    const { api, saves, copies } = fake([{ kind: "missing" }]);
    const doc = new NoteDoc("n.md", "eski", "h0", () => "benim", api);
    await doc.save();
    expect(doc.problem).toBe("missing");
    await doc.save();
    expect(saves.length).toBe(1);
    await doc.leave();
    expect(copies).toEqual(["benim"]);
  });

  it("an outside change reloads a clean note and flags an edited one (Final review I3)", () => {
    const { api } = fake([]);
    let text = "aynı";
    const doc = new NoteDoc("n.md", "aynı", "h0", () => text, api);
    expect(doc.externalChange()).toBe("reload");
    text = "değişti";
    expect(doc.externalChange()).toBe("conflict");
    expect(doc.problem).toBe("conflict");
  });
});
