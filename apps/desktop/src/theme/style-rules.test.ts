import { describe, expect, it } from "vitest";

// Every component, stylesheet and script outside the theme folder and the tests.
const sources = import.meta.glob(["../**/*.svelte", "../**/*.css", "../**/*.ts", "!../theme/**", "!../**/*.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Not migrated yet; each task removes the files it moves onto the tokens. Task 8 deletes this set. */
export const PENDING = new Set([
  "../lib/TaskPanel.svelte",
  "../lib/TaskRow.svelte",
  "../lib/ReminderBanner.svelte",
  "../lib/AddedToast.svelte",
]);

const RULES: [string, RegExp][] = [
  ["colour literal (use a --color-* token)", /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\(/],
  ["emoji used as an icon (use <Icon>)", /[🔔⏰✕⚠✦]/u],
  ["legacy variable (use the --color-* token)", /var\(--(?:bg|panel|border|text|muted|accent|danger)\)/],
];

describe("style rules", () => {
  it("finds the sources", () => {
    expect(Object.keys(sources)).toContain("../lib/TaskRow.svelte");
  });

  it("keeps colours in the theme and icons in Lucide", () => {
    const problems: string[] = [];
    for (const [file, text] of Object.entries(sources)) {
      if (PENDING.has(file) || file === "../app.css") continue;
      text.split("\n").forEach((line, i) => {
        for (const [what, re] of RULES) if (re.test(line)) problems.push(`${file}:${i + 1} ${what}: ${line.trim()}`);
      });
    }
    expect(problems).toEqual([]);
  });
});
