import { describe, expect, it } from "vitest";
import { DEFAULT_WIDTHS, MIN_CENTER, clampWidth, loadWidths, saveWidths } from "./panes";

function memory() {
  const data = new Map<string, string>();
  return { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
}

describe("pane widths", () => {
  it("keeps a side within its limits", () => {
    expect(clampWidth("left", 50, 320, 1920)).toBe(180);
    expect(clampWidth("left", 900, 320, 1920)).toBe(480);
    expect(clampWidth("right", 400.4, 240, 1920)).toBe(400);
  });

  it("never squeezes the note below its minimum", () => {
    const width = clampWidth("right", 700, 240, 1200);
    expect(1200 - 240 - width).toBe(MIN_CENTER);
  });

  it("falls back to the minimum when the window is too small for both", () => {
    expect(clampWidth("left", 300, 600, 800)).toBe(180);
  });

  it("remembers the widths and ignores broken data", () => {
    const store = memory();
    expect(loadWidths(store)).toEqual(DEFAULT_WIDTHS);
    saveWidths({ left: 300, right: 280, rightWide: 600 }, store);
    expect(loadWidths(store)).toEqual({ left: 300, right: 280, rightWide: 600 });
    store.setItem("pla.paneWidths", "{not json");
    expect(loadWidths(store)).toEqual(DEFAULT_WIDTHS);
    store.setItem("pla.paneWidths", JSON.stringify({ left: "wide", right: 290 }));
    expect(loadWidths(store)).toEqual({ ...DEFAULT_WIDTHS, right: 290 });
  });
});
