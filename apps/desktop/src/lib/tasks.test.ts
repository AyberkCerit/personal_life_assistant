import { describe, expect, it } from "vitest";
import { dayLabel, filterByOrigin, groupTasks, isOverdue, isReminder, msUntilNextDay, revealRange, todayIso, type Labels, type Task } from "./tasks";

const L: Labels = { today: "Bugün", tomorrow: "Yarın", noDate: "Tarihsiz", overdue: "Gecikmiş" };
const TODAY = "2026-10-06";

function task(title: string, date: string | null, extra: Partial<Task> = {}): Task {
  return {
    task_id: title, title, details: null, date, time: null, notify_at: null, status: "open", origin: "manual",
    note_path: null, block_text: null, source_missing: false, user_modified: false, ...extra,
  };
}

describe("tasks", () => {
  it("knows today's date in local time", () => {
    expect(todayIso(new Date(2026, 9, 6, 23, 59))).toBe("2026-10-06");
  });

  it("marks only open past tasks as overdue", () => {
    expect(isOverdue(task("a", "2026-10-05"), TODAY)).toBe(true);
    expect(isOverdue(task("b", TODAY), TODAY)).toBe(false);
    expect(isOverdue(task("c", "2026-10-05", { status: "done" }), TODAY)).toBe(false);
    expect(isOverdue(task("d", null), TODAY)).toBe(false);
  });

  it("labels today and tomorrow by name, other days with their date", () => {
    expect(dayLabel(TODAY, TODAY, L, "tr")).toBe("Bugün");
    expect(dayLabel("2026-10-07", TODAY, L, "tr")).toBe("Yarın");
    expect(dayLabel("2026-10-09", TODAY, L, "tr")).toContain("9");
  });

  it("puts overdue first in Today and undated last in Upcoming", () => {
    const today = groupTasks([task("geç", "2026-10-04"), task("şimdi", TODAY)], "today", TODAY, L, "tr");
    expect(today.map((g) => [g.label, g.tasks.map((t) => t.title)])).toEqual([
      ["Gecikmiş", ["geç"]],
      ["Bugün", ["şimdi"]],
    ]);
    const upcoming = groupTasks([task("cuma", "2026-10-09"), task("yarın", "2026-10-07"), task("boş", null)], "upcoming", TODAY, L, "tr");
    expect(upcoming.map((g) => g.key)).toEqual(["2026-10-07", "2026-10-09", "no-date"]);
    expect(upcoming[0].label).toBe("Yarın");
    expect(upcoming.at(-1)!.label).toBe("Tarihsiz");
  });

  it("filters by origin", () => {
    const list = [task("a", null), task("b", null, { origin: "extracted" })];
    expect(filterByOrigin(list, "extracted").map((t) => t.title)).toEqual(["b"]);
    expect(filterByOrigin(list, "all")).toHaveLength(2);
  });

  it("knows how long until the local day changes (Final review I4)", () => {
    expect(msUntilNextDay(new Date(2026, 9, 6, 23, 59, 30))).toBe(30_000);
    expect(msUntilNextDay(new Date(2026, 9, 6, 0, 0, 0))).toBe(24 * 3600 * 1000);
  });

  it("finds the source block in a CRLF note at the editor's offsets (Final review I3)", () => {
    const file = "# Başlık\r\n\r\nYarın dişçi.\r\nİkinci satır.\r\n";
    const block = "Yarın dişçi.\nİkinci satır.";
    const range = revealRange(file, block)!;
    const editorText = file.replace(/\r\n/g, "\n");
    expect(editorText.slice(range.anchor, range.head)).toBe(block);
    expect(revealRange(file, "yok")).toBeNull();
  });
  it("knows a reminder by its notification time", () => {
    expect(isReminder(task("a", TODAY, { notify_at: "2026-10-06T09:00" }))).toBe(true);
    expect(isReminder(task("b", TODAY))).toBe(false);
  });
});
