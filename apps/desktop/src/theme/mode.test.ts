import { afterEach, describe, expect, it, vi } from "vitest";
import { applyTheme, resolveTheme } from "./mode";

describe("theme mode", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("follows the system only when asked", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
    expect(resolveTheme("light", true)).toBe("light");
  });

  it("repaints when Windows switches mode, until another choice is made", () => {
    const listeners = new Set<(e: { matches: boolean }) => void>();
    const query = {
      matches: false,
      addEventListener: (_: string, f: (e: { matches: boolean }) => void) => listeners.add(f),
      removeEventListener: (_: string, f: (e: { matches: boolean }) => void) => listeners.delete(f),
    };
    vi.stubGlobal("matchMedia", () => query);
    const root = { dataset: {} as Record<string, string> } as unknown as HTMLElement;

    applyTheme("system", root);
    expect(root.dataset.theme).toBe("light");
    for (const f of listeners) f({ matches: true });
    expect(root.dataset.theme).toBe("dark");

    applyTheme("light", root);
    expect(root.dataset.theme).toBe("light");
    expect(listeners.size).toBe(0);
  });
});
