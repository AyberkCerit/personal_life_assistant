import { describe, expect, it } from "vitest";
import { EDITOR_THEME, HIGHLIGHT, WIKILINK } from "./editorTheme";

const COLOUR_KEYS = ["color", "backgroundColor", "borderLeftColor", "caretColor", "outline", "borderBottom", "borderTop", "border", "textDecorationColor"];

function colourValues(spec: object): string[] {
  const out: string[] = [];
  for (const [k, v] of Object.entries(spec)) {
    if (k === "tag") continue; // lezer Tag objects reference themselves
    if (v && typeof v === "object") out.push(...colourValues(v));
    else if (COLOUR_KEYS.includes(k) && typeof v === "string" && v !== "none" && v !== "transparent") out.push(`${k}: ${v}`);
  }
  return out;
}

describe("editor theme", () => {
  it("takes every colour from the theme tokens", () => {
    const values = [...colourValues(EDITOR_THEME), ...HIGHLIGHT.flatMap((h) => colourValues(h))];
    expect(values.length).toBeGreaterThan(10);
    expect(values.filter((v) => !v.includes("var(--color-"))).toEqual([]);
  });

  it("keeps the centred text column full width (a flex item with auto margins shrinks to one character)", () => {
    expect(EDITOR_THEME[".cm-content"]).toMatchObject({ width: "100%", maxWidth: "72ch", boxSizing: "border-box" });
  });

  it("finds wikilinks, also with aliases and Turkish letters", () => {
    const text = "Bkz. [[Sağlık]] ve [[Projeler/PLA|PLA]], ama [tek] ve [[yarım\nsatır]] değil.";
    expect(text.match(WIKILINK)).toEqual(["[[Sağlık]]", "[[Projeler/PLA|PLA]]"]);
  });
});
