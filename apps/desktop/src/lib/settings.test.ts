import { describe, expect, it } from "vitest";
import { SECTIONS, backupLine, settingsError, trapIndex } from "./settings";

describe("settings screen", () => {
  it("has the seven sections of FR-SET-011 in order", () => {
    expect(SECTIONS).toEqual(["general", "vault", "ai", "scheduler", "notifications", "privacy", "about"]);
  });

  it("keeps Tab inside the window", () => {
    expect(trapIndex(4, 5, false)).toBe(0);
    expect(trapIndex(0, 5, true)).toBe(4);
    expect(trapIndex(2, 5, false)).toBe(3);
    expect(trapIndex(-1, 5, false)).toBe(0); // focus was outside: start at the first
    expect(trapIndex(-1, 5, true)).toBe(4);
  });

  it("reads coded errors and names the folder", () => {
    expect(settingsError(String.raw`not_removed|C:\Modeller\g.gguf|access_denied`)).toEqual({
      key: "settings.error.not_removed",
      path: String.raw`C:\Modeller\g.gguf`,
      reason: "",
      reasonKey: "wizard.reason.access_denied",
    });
    expect(settingsError("model_busy")).toMatchObject({ key: "settings.error.model_busy" });
    expect(settingsError(String.raw`network|\\s\p`)).toMatchObject({ key: "wizard.error.network", path: String.raw`\\s\p` });
    expect(settingsError("boom")).toEqual({ key: "settings.error.other", path: "", reason: "boom" });
  });

  it("says when the backup last ran or why it failed", () => {
    const none = { last_success_at: null, last_attempt_at: null, last_error: null };
    expect(backupLine(none)).toEqual({ key: "settings.backup.never" });
    expect(backupLine({ ...none, last_success_at: "2026-10-06T23:10:00+03:00", last_attempt_at: "2026-10-06T23:10:00+03:00" })).toEqual({ key: "settings.backup.last", at: "2026-10-06T23:10:00+03:00" });
    expect(backupLine({ last_success_at: "2026-10-05T23:10:00+03:00", last_attempt_at: "2026-10-06T23:00:00+03:00", last_error: "disk full" })).toEqual({
      key: "settings.backup.failed",
      at: "2026-10-06T23:00:00+03:00",
      error: "disk full",
    });
  });
});
