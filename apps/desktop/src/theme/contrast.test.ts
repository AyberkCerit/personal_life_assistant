import { describe, expect, it } from "vitest";
import css from "./tokens.css?raw";
import appCss from "../app.css?raw";
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

  // WCAG 1.4.11: what identifies a control (outline, focus ring) needs 3:1 (final review I4).
  const NON_TEXT: [string, string][] = [
    ...["surface-reading", "surface-panel", "surface-raised", "surface-hover"].map((b) => ["border-strong", b] as [string, string]),
    ...["surface-reading", "surface-panel", "surface-raised"].map((b) => ["accent", b] as [string, string]),
  ];

  for (const name of ["dark", "light"] as const) {
    it(`gives controls 3:1 non-text contrast in the ${name} theme`, () => {
      const t = themes[name];
      const failures = NON_TEXT.filter(([f, b]) => contrast(t[`--color-${f}`], t[`--color-${b}`]) < 3).map(
        ([f, b]) => `${f} on ${b}: ${contrast(t[`--color-${f}`], t[`--color-${b}`]).toFixed(2)}`,
      );
      expect(failures).toEqual([]);
    });

    it(`shows selected text clearly and readably in the ${name} theme`, () => {
      // final review C1: the selection must stand out from the page and keep body text readable
      const t = themes[name];
      expect(contrast(t["--color-selection"], t["--color-surface-reading"])).toBeGreaterThanOrEqual(1.3);
      expect(contrast(t["--color-text"], t["--color-selection"])).toBeGreaterThanOrEqual(4.5);
    });
  }

  it("keeps the active-line tint see-through so the selection under it shows", () => {
    for (const name of ["dark", "light"]) {
      const start = css.indexOf(`[data-theme="${name}"]`);
      const block = css.slice(start, css.indexOf("}", start));
      expect(block, name).toMatch(/--color-active-line:\s*rgb\([^)]*\/\s*0?\.\d+\)/);
    }
  });

  it("styles placeholders and input outlines from the tokens (final review I3/I4)", () => {
    expect(appCss).toMatch(/::placeholder\s*\{[^}]*color:\s*var\(--color-text-muted\)[^}]*opacity:\s*1/);
    expect(appCss).toMatch(/input:not\(\[type="checkbox"\]\), select, textarea \{[^}]*border: 1px solid var\(--color-border-strong\)/);
  });

  it("defines the same colour tokens in both themes", () => {
    expect(Object.keys(themes.light).sort()).toEqual(Object.keys(themes.dark).sort());
  });

  it("switches motion off for reduced motion", () => {
    const block = css.slice(css.indexOf("prefers-reduced-motion"));
    expect(block).toMatch(/--duration-fast:\s*0ms/);
    expect(block).toMatch(/--duration-base:\s*0ms/);
  });
});
