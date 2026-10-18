import { afterEach, describe, expect, it } from "vitest";
import { setLang } from "./i18n";
import { dayLabels, routineError, weekCount, type Routine } from "./routines";

describe("dayLabels", () => {
  it("one time for all days is written once, in week order", () => {
    expect(dayLabels([{ day: "sun", time: "20:00" }, { day: "mon", time: "20:00" }, { day: "fri", time: "20:00" }], "tr")).toBe("Pzt · Cum · Paz — 20:00");
  });
  it("different times are written per day", () => {
    expect(dayLabels([{ day: "mon", time: "20:00" }, { day: "fri", time: "18:00" }], "tr")).toBe("Pzt 20:00 · Cum 18:00");
  });
  it("a routine without a time has no time", () => {
    expect(dayLabels([{ day: "mon", time: null }], "en")).toBe("Mon");
  });
  it("a mix of timed and untimed days is written per day", () => {
    expect(dayLabels([{ day: "mon", time: "08:00" }, { day: "wed", time: null }], "en")).toBe("Mon 08:00 · Wed");
  });
});

describe("weekCount", () => {
  afterEach(() => setLang("tr"));
  const r = { week_done: 1, week_due: 5 } as Routine;
  it("says this week's progress in the UI language", () => {
    setLang("tr");
    expect(weekCount(r)).toBe("bu hafta 1/5");
    setLang("en");
    expect(weekCount(r)).toBe("this week 1/5");
  });
});

describe("routineError", () => {
  it("maps routine and shared task codes to keys", () => {
    expect(routineError("no_days||")).toEqual({ key: "routines.error.no_days", detail: "" });
    expect(routineError("bad_time|25:00|")).toEqual({ key: "tasks.error.bad_time", detail: "25:00" });
    expect(routineError("db||boom").key).toBe("tasks.error.other");
  });
});
