import { describe, expect, it } from "vitest";
import css from "./tokens.css?raw";
import { contrast, parseThemes } from "./contrast";

const themes = parseThemes(css);

// Every text-on-surface pair the UI uses (spec § 7). [foreground, background]
const PAIRS: [string, string][] = [
  ...["text", "text-muted"].flatMap((f) =>
    ["surface-reading", "surface-panel", "surface-raised", "surface-hover", "surface-selected"].map((b) => [f, b] as [string, string]),
  ),
  ...["accent", "link", "danger", "warning"].flatMap((f) => ["surface-reading", "surface-panel"].map((b) => [f, b] as [string, string])),
  ["on-accent", "accent"],
  ["ai", "ai-subtle"],
  ["danger", "danger-subtle"],
  ["warning", "warning-subtle"],
  ["text", "banner"],
  ["text-muted", "banner"],
  ["text", "accent-subtle"],
];

describe("theme tokens", () => {
  it("computes WCAG contrast like the reference values", () => {
    expect(contrast("#000000", "#ffffff")).toBeCloseTo(21, 1);
    expect(contrast("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
  });

  for (const name of ["dark", "light"] as const) {
    it(`meets WCAG AA in the ${name} theme`, () => {
      const t = themes[name];
      const failures = PAIRS.filter(([f, b]) => {
        const fg = t[`--color-${f}`];
        const bg = t[`--color-${b}`];
        expect(fg, `--color-${f} missing in ${name}`).toBeDefined();
        expect(bg, `--color-${b} missing in ${name}`).toBeDefined();
        return contrast(fg, bg) < 4.5;
      }).map(([f, b]) => `${f} on ${b}: ${contrast(t[`--color-${f}`], t[`--color-${b}`]).toFixed(2)}`);
      expect(failures).toEqual([]);
    });
  }

  it("defines the same colour tokens in both themes", () => {
    expect(Object.keys(themes.light).sort()).toEqual(Object.keys(themes.dark).sort());
  });

  it("switches motion off for reduced motion", () => {
    const block = css.slice(css.indexOf("prefers-reduced-motion"));
    expect(block).toMatch(/--duration-fast:\s*0ms/);
    expect(block).toMatch(/--duration-base:\s*0ms/);
  });
});
