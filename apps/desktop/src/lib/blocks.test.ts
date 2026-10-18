import { describe, expect, it } from "vitest";
import { convertLine, fold, matchBlocks, MENU_KINDS, parseLine, slashQuery, type MenuKind } from "./blocks";

const LABELS = {
  p: ["Metin", "Text"],
  h1: ["Başlık 1", "Heading 1"],
  h2: ["Başlık 2", "Heading 2"],
  h3: ["Başlık 3", "Heading 3"],
  bullet: ["Madde listesi", "Bulleted list"],
  numbered: ["Numaralı liste", "Numbered list"],
  task: ["Yapılacak", "To-do"],
  quote: ["Alıntı", "Quote"],
} satisfies Record<MenuKind, string[]>;

/** The line after turning it into `kind`. */
function turn(text: string, kind: MenuKind): string {
  const { length, insert } = convertLine(text, kind);
  return insert + text.slice(length);
}

describe("parseLine", () => {
  it("tells the line types apart", () => {
    expect(parseLine("Düz metin").kind).toBe("p");
    expect(parseLine("# Başlık").kind).toBe("h1");
    expect(parseLine("### Üç").kind).toBe("h3");
    expect(parseLine("###### Altı").kind).toBe("h6");
    expect(parseLine("- madde").kind).toBe("bullet");
    expect(parseLine("  * iç madde").kind).toBe("bullet");
    expect(parseLine("- [ ] süt al").kind).toBe("task");
    expect(parseLine("- [x] bitti").kind).toBe("task");
    expect(parseLine("3. üçüncü").kind).toBe("numbered");
    expect(parseLine("> söz").kind).toBe("quote");
  });

  it("is not fooled by text that only starts like a marker", () => {
    expect(parseLine("#etiket").kind).toBe("p");
    expect(parseLine("**kalın**").kind).toBe("p");
    expect(parseLine("---").kind).toBe("p");
    expect(parseLine("-eksi").kind).toBe("p");
    expect(parseLine("2026. yıl").kind).toBe("numbered"); // Markdown reads it so too
  });

  it("splits marker and body", () => {
    expect(parseLine("  - [x] bitti")).toEqual({ kind: "task", indent: "  ", marker: "- [x] ", body: "bitti" });
    expect(parseLine("## İki")).toEqual({ kind: "h2", indent: "", marker: "## ", body: "İki" });
  });
});

describe("convertLine", () => {
  it("turns plain text into each type", () => {
    expect(turn("Alışveriş", "h1")).toBe("# Alışveriş");
    expect(turn("Alışveriş", "h3")).toBe("### Alışveriş");
    expect(turn("Alışveriş", "bullet")).toBe("- Alışveriş");
    expect(turn("Alışveriş", "numbered")).toBe("1. Alışveriş");
    expect(turn("Alışveriş", "task")).toBe("- [ ] Alışveriş");
    expect(turn("Alışveriş", "quote")).toBe("> Alışveriş");
    expect(turn("Alışveriş", "p")).toBe("Alışveriş");
  });

  it("swaps one marker for another and back to text", () => {
    expect(turn("# Başlık", "h2")).toBe("## Başlık");
    expect(turn("#### Dört", "h1")).toBe("# Dört");
    expect(turn("> söz", "p")).toBe("söz");
    expect(turn("- [ ] süt", "bullet")).toBe("- süt");
    expect(turn("## Plan", "task")).toBe("- [ ] Plan");
  });

  it("keeps a list item's nesting, bullet character, number and tick", () => {
    expect(turn("  * madde", "task")).toBe("  * [ ] madde");
    expect(turn("  - [x] bitti", "bullet")).toBe("  - bitti");
    expect(turn("- [x] bitti", "task")).toBe("- [x] bitti");
    expect(turn("4) dört", "numbered")).toBe("4) dört");
    expect(turn("  - iç", "h2")).toBe("## iç");
  });

  it("works on an empty line", () => {
    expect(turn("", "h1")).toBe("# ");
    expect(turn("# ", "p")).toBe("");
  });
});

describe("slashQuery", () => {
  it("finds a slash at the start of the line or right after its marker", () => {
    expect(slashQuery("/")).toEqual({ offset: 0, query: "" });
    expect(slashQuery("/baş")).toEqual({ offset: 0, query: "baş" });
    expect(slashQuery("- /")).toEqual({ offset: 2, query: "" });
    expect(slashQuery("## /me")).toEqual({ offset: 3, query: "me" });
  });

  it("ignores slashes inside text and paths", () => {
    expect(slashQuery("ve/veya")).toBeNull();
    expect(slashQuery("Not /")).toBeNull();
    expect(slashQuery("/usr/bin")).toBeNull();
  });
});

describe("matchBlocks", () => {
  it("offers everything for a bare slash", () => {
    expect(matchBlocks("", LABELS)).toEqual([...MENU_KINDS]);
  });

  it("finds by label in either language, without Turkish letters, and by Markdown words", () => {
    expect(matchBlocks("baslik", LABELS)).toEqual(["h1", "h2", "h3"]);
    expect(matchBlocks("Başlık 2", LABELS)).toEqual(["h2"]);
    expect(matchBlocks("baslik2", LABELS)).toEqual(["h2"]);
    expect(matchBlocks("heading", LABELS)).toEqual(["h1", "h2", "h3"]);
    expect(matchBlocks("h3", LABELS)).toEqual(["h3"]);
    expect(matchBlocks("görev", LABELS)).toEqual(["task"]);
    expect(matchBlocks("todo", LABELS)).toEqual(["task"]);
    expect(matchBlocks("liste", LABELS)).toEqual(["bullet", "numbered"]);
    expect(matchBlocks("zzz", LABELS)).toEqual([]);
  });

  it("folds Turkish capitals", () => {
    expect(fold("İSTANBUL Işık")).toBe("istanbulisik");
  });
});
