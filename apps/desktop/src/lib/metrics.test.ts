import { describe, expect, it } from "vitest";
import { KINDS, chartGeometry, fillDays, formatValue, metricError, parseAmount, trendDirection } from "./metrics";

describe("metrics view helpers", () => {
  it("lists the kinds in the overview order", () => {
    expect(KINDS).toEqual(["sleep", "weight", "steps", "water", "workout"]);
  });

  it("formats values in each kind's unit and the UI language", () => {
    expect(formatValue("sleep", 7.5, "tr")).toBe("7,5 sa");
    expect(formatValue("sleep", 7.5, "en")).toBe("7.5 h");
    expect(formatValue("weight", 79.83, "tr")).toBe("79,8 kg");
    expect(formatValue("steps", 8432, "tr")).toBe("8.432 adım");
    expect(formatValue("steps", 8432, "en")).toBe("8,432 steps");
    expect(formatValue("water", 750, "tr")).toBe("750 ml");
    expect(formatValue("water", 1250, "tr")).toBe("1,25 L");
    expect(formatValue("workout", 2, "tr", 7)).toBe("2 oturum · 7 set");
    expect(formatValue("workout", 1, "en")).toBe("1 session");
  });

  it("reads amounts typed the Turkish or English way", () => {
    expect(parseAmount("7,5", "sleep")).toBe(7.5);
    expect(parseAmount(" 7.5 ", "sleep")).toBe(7.5);
    expect(parseAmount("8.432", "steps")).toBe(8432); // a thousands dot, not a decimal
    expect(parseAmount("8,432", "steps")).toBe(8432);
    expect(parseAmount("", "sleep")).toBeNull();
    expect(parseAmount("abc", "weight")).toBeNull();
  });

  it("lays the days out on a continuous axis with gaps", () => {
    const days = [
      { date: "2026-10-01", value: 7, sets: null, conflict: false },
      { date: "2026-10-03", value: 8, sets: null, conflict: true },
    ];
    expect(fillDays(days, "2026-10-01", 4)).toEqual([
      { date: "2026-10-01", value: 7, conflict: false },
      { date: "2026-10-02", value: null, conflict: false },
      { date: "2026-10-03", value: 8, conflict: true },
      { date: "2026-10-04", value: null, conflict: false },
    ]);
  });

  it("scales a chart and breaks the line where a day is missing", () => {
    const g = chartGeometry([6, null, 8, 7], 300, 100, 10);
    expect(g.points.map((p) => (p ? [Math.round(p.x), Math.round(p.y)] : null))).toEqual([[10, 90], null, [197, 10], [290, 50]]);
    expect(g.path).toBe("M10,90 M196.7,10 L290,50");
    expect(chartGeometry([null, null], 300, 100, 10).path).toBe("");
    const flat = chartGeometry([5, 5], 300, 100, 10);
    expect(flat.points[0]?.y).toBe(50); // one value: the middle, not a division by zero
  });

  it("calls a trend flat when it is too small to matter", () => {
    expect(trendDirection(0.5, 7)).toBe("up");
    expect(trendDirection(-0.5, 7)).toBe("down");
    expect(trendDirection(0.01, 7)).toBe("flat");
    expect(trendDirection(null, 7)).toBe("flat");
  });

  it("reads the commands' error codes", () => {
    expect(metricError("out_of_range|450")).toEqual({ key: "metrics.error.out_of_range", value: 450 });
    expect(metricError("missing_value")).toEqual({ key: "metrics.error.missing_value", value: null });
    expect(metricError("boom")).toEqual({ key: "metrics.error.other", value: null, detail: "boom" });
  });
});
