import { describe, expect, it } from "vitest";
import { answerParts, apply, qaError, started, toolLine, type ToolRecord } from "./qa";

const added: ToolRecord = {
  tool: "add_task",
  args: { title: "Doktor randevusu" },
  ok: true,
  result: { task_id: "t1", title: "Doktor randevusu", date: "2026-10-07", time: "10:00", remind: true },
  error: null,
  undo: { kind: "task", id: "t1" },
};

describe("assistant panel helpers", () => {
  it("builds the live turn from its events", () => {
    // FR-QA-003/008
    let t = started("a", "Yarın 10'da doktor", false);
    t = apply(t, { kind: "working", turn_id: "a", tool: "add_task" });
    expect(t.working).toBe("add_task");
    t = apply(t, { kind: "tool", turn_id: "a", record: added });
    t = apply(t, { kind: "token", turn_id: "a", text: "Ekledim" });
    t = apply(t, { kind: "token", turn_id: "b", text: " başka" });
    t = apply(t, { kind: "token", turn_id: "a", text: "." });
    expect([t.answer, t.tools.length, t.working, t.status]).toEqual(["Ekledim.", 1, undefined, "running"]);
    const turn = { turn_id: "a", question: "q", answer: "Ekledim.", tools: [added], created_at: "x", status: "done" as const, new_topic: false };
    t = apply(t, { kind: "done", turn_id: "a", turn, sources: ["notes/a.md"] });
    expect([t.status, t.sources]).toEqual(["done", ["notes/a.md"]]);
    const f = apply(started("c", "q", false), { kind: "failed", turn_id: "c", code: "no_model", detail: "" });
    expect([f.status, f.error?.code]).toEqual(["failed", "no_model"]);
  });

  it("finds the links in an answer", () => {
    // FR-QA-010
    expect(answerParts("Bkz. [[PLA]] ve [[PLA Kararları#Hedef|kararlar]].")).toEqual([
      { text: "Bkz. ", link: null },
      { text: "PLA", link: "PLA" },
      { text: " ve ", link: null },
      { text: "kararlar", link: "PLA Kararları" },
      { text: ".", link: null },
    ]);
  });

  it("says what each tool did", () => {
    expect(toolLine(added)).toEqual({ key: "qa.tool.reminder", values: { title: "Doktor randevusu", when: "2026-10-07 10:00" } });
    expect(toolLine({ ...added, ok: false, error: "a time needs a date" }).key).toBe("qa.tool.failed");
    expect(qaError("no_model")).toBe("qa.error.no_model");
    expect(qaError("weird")).toBe("qa.error.model");
  });
});
