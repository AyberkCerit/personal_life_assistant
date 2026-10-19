import { describe, expect, it } from "vitest";
import { aiMark, reviewReason, taskError, dayLabel, filterByOrigin, groupTasks, isOverdue, isReminder, msUntilNextDay, revealRange, todayIso, type Labels, type Task } from "./tasks";

const L: Labels = { today: "Bugün", tomorrow: "Yarın", noDate: "Tarihsiz", overdue: "Gecikmiş" };
const TODAY = "2026-10-06";

function task(title: string, date: string | null, extra: Partial<Task> = {}): Task {
  return {
    task_id: title, title, details: null, date, time: null, notify_at: null, status: "open", origin: "manual",
    note_path: null, block_text: null, source_missing: false, user_modified: false, routine_id: null, ...extra,
  };
}

describe("tasks", () => {
  it("marks AI items with a badge wherever they are listed (IR-UI-007)", () => {
    expect(aiMark(task("a", null, { origin: "extracted" }))).toBe("ai");
    // user_modified is also set by ticking off or snoozing, so it cannot mean "edited" (final review I2)
    expect(aiMark(task("b", null, { origin: "extracted", user_modified: true, status: "done" }))).toBe("ai");
    expect(aiMark(task("c", null, { origin: "manual", user_modified: true }))).toBeNull();
  });

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

describe("review reasons", () => {
  it("are said in the interface language, old English rows included", () => {
    expect(reviewReason("reminder without a date")).toBe("review.reason.no_date");
    expect(reviewReason("block too long for the model")).toBe("review.reason.too_long");
    expect(reviewReason("llama-server did not answer in time")).toBe("review.reason.model");
    expect(reviewReason("Sleep value 30 is outside its plausible range")).toBe("review.reason.out_of_range");
    expect(reviewReason("invalid calendar date: 2026-02-30")).toBe("review.reason.bad_date");
    expect(reviewReason("repeat without a day")).toBe("review.reason.bad_repeat");
    expect(reviewReason("task or reminder without a title")).toBe("review.reason.no_title");
    expect(reviewReason("metric without a value")).toBe("review.reason.incomplete");
    expect(reviewReason("EOF while parsing a value at line 1 column 20")).toBe("review.reason.unreadable");
  });
});

describe("task errors", () => {
  it("turns the commands' codes into the UI's own sentences", () => {
    // architecture review A2: the panel showed the backend's English sentence
    expect(taskError("bad_title||")).toEqual({ key: "tasks.error.bad_title", detail: "" });
    expect(taskError("bad_date|31.02.2026|")).toEqual({ key: "tasks.error.bad_date", detail: "31.02.2026" });
    expect(taskError("no_vault||")).toEqual({ key: "tasks.error.no_vault", detail: "" });
    expect(taskError("io||disk full")).toEqual({ key: "tasks.error.other", detail: "disk full" });
    expect(taskError("something new")).toEqual({ key: "tasks.error.other", detail: "something new" });
  });
});
