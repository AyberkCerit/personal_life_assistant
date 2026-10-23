import { describe, expect, it } from "vitest";
import { SECTIONS, WEEKDAYS, attempt, backupDay, backupLine, describeError, longDay, offerText, reportLine, settingsError, trapIndex } from "./settings";
import { errorText } from "./i18n";

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
    // M6 final review: the old vault's own failure is said, not shown as its raw code
    expect(settingsError(String.raw`vault_lost|C:\Eski|missing|C:\Yeni|os error 2`)).toMatchObject({
      key: "settings.error.vault_lost",
      path: String.raw`C:\Eski`,
      reason: errorText(String.raw`missing|C:\Yeni|`).text,
    });
    expect(settingsError(String.raw`network|\\s\p`)).toMatchObject({ key: "wizard.error.network", path: String.raw`\\s\p` });
    expect(settingsError("boom")).toEqual({ key: "settings.error.other", path: "", reason: "boom" });
    // settings final review M8: no English text from the backend in a Turkish UI
    expect(settingsError("no_vault")).toMatchObject({ key: "settings.error.no_vault" });
    expect(settingsError("download_running")).toMatchObject({ key: "settings.error.download_running" });
    expect(settingsError("missing|Notlar|")).toMatchObject({ key: "settings.error.missing", path: "Notlar" });
    expect(settingsError("vault_lost|C:/Kasa|disk")).toMatchObject({ key: "settings.error.vault_lost", path: "C:/Kasa", reason: errorText("disk").text }); // an old plain reason reads as unexpected (M6)
  });

  it("says when the weekly report last ran or why it failed", () => {
    // FR-SCH-011
    const none = { last_success_at: null, last_attempt_at: null, last_error: null };
    expect(reportLine(none)).toEqual({ key: "settings.report.never" });
    expect(reportLine({ ...none, last_success_at: "2026-10-11T20:05:00+03:00", last_attempt_at: "2026-10-11T20:05:00+03:00" })).toEqual({ key: "settings.report.last", at: "2026-10-11T20:05:00+03:00" });
    expect(reportLine({ last_success_at: null, last_attempt_at: "2026-10-11T20:05:00+03:00", last_error: "disk full" }).key).toBe("settings.report.failed");
    expect(WEEKDAYS).toHaveLength(7);
  });

  it("says when the backup last ran or why it failed", () => {
    const none = { last_success_at: null, last_attempt_at: null, last_error: null };
    expect(backupLine(none)).toEqual({ key: "settings.backup.never" });
    expect(backupLine({ ...none, last_success_at: "2026-10-06T23:10:00+03:00", last_attempt_at: "2026-10-06T23:10:00+03:00" })).toEqual({ key: "settings.backup.last", at: "2026-10-06T23:10:00+03:00" });
    expect(backupLine({ last_success_at: "2026-10-05T23:10:00+03:00", last_attempt_at: "2026-10-06T23:00:00+03:00", last_error: "disk_full||No space left on device" })).toEqual({
      key: "settings.backup.failed",
      at: "2026-10-06T23:00:00+03:00",
      error: errorText("disk_full||").text, // M6 translations: said, not the stored English reason
    });
  });
});

describe("shared settings helpers", () => {
  it("say an action's error or nothing, and write a backup's day out", async () => {
    expect(await attempt(async () => {})).toBeNull();
    expect(await attempt(async () => Promise.reject("missing||"))).toEqual(describeError("missing||"));
    expect(longDay("2026-10-19", "en")).toBe("October 19, 2026");
  });
});

describe("settings errors (M6 final review)", () => {
  it("a missing thing with no folder named reads as the general message", () => {
    expect(describeError("missing||os error 2")).toMatchObject({ text: errorText("missing||").text, details: "os error 2" });
  });
});

describe("restore (FR-BKP-004/005/008, FR-LOG-007)", () => {
  it("reads a backup's day from its name, and nothing else", () => {
    expect(backupDay("pla-2026-10-05.db")).toBe("2026-10-05");
    expect(backupDay("benim.db")).toBeNull();
  });

  it("says why a backup is offered", () => {
    expect(offerText({ backup: "pla-2026-10-05.db", damaged: false })).toEqual({ key: "restore.missing", day: "2026-10-05", canRestore: true });
    expect(offerText({ backup: "pla-2026-10-05.db", damaged: true })).toEqual({ key: "restore.damaged", day: "2026-10-05", canRestore: true });
    expect(offerText({ backup: null, damaged: true })).toEqual({ key: "restore.damagedNone", day: "", canRestore: false });
  });
});
