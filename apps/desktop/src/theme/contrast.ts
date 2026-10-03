/** WCAG 2.1 contrast maths and a small parser for the theme blocks in tokens.css. */

export type ThemeName = "dark" | "light";

function luminance(hex: string): number {
  const h = hex.replace("#", "");
  const [r, g, b] = [0, 2, 4].map((i) => {
    const c = parseInt(h.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio of two `#rrggbb` colours, 1–21. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** The `--color-*: #rrggbb` declarations of the dark and light blocks. */
export function parseThemes(css: string): Record<ThemeName, Record<string, string>> {
  const block = (name: ThemeName) => {
    const start = css.indexOf(`[data-theme="${name}"]`);
    const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
    const vars: Record<string, string> = {};
    for (const m of body.matchAll(/(--color-[a-z-]+)\s*:\s*(#[0-9a-fA-F]{6})\s*;/g)) vars[m[1]] = m[2].toLowerCase();
    return vars;
  };
  return { dark: block("dark"), light: block("light") };
}
