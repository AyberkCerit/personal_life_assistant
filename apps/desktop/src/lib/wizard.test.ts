import { describe, expect, it } from "vitest";
import { STEPS, folderAdvice, modelSummary, nextStep, prevStep, setupError, type FolderState } from "./wizard";

describe("first-run wizard", () => {
  it("walks language → vault → model → done", () => {
    expect(STEPS).toEqual(["language", "vault", "model", "done"]);
    expect(nextStep("language")).toBe("vault");
    expect(nextStep("done")).toBe("done");
    expect(prevStep("vault")).toBe("language");
    expect(prevStep("language")).toBe("language");
  });

  it("explains what choosing a folder will do", () => {
    const report = (state: FolderState, md_files = 0, in_onedrive = false, more = false) => ({ check: { state, md_files, more }, in_onedrive });
    expect(folderAdvice(report("missing"))).toEqual([{ key: "wizard.folder.missing", tone: "info", usable: true }]);
    expect(folderAdvice(report("notes", 42))).toEqual([{ key: "wizard.folder.notes", tone: "info", usable: true, count: "42" }]);
    expect(folderAdvice(report("notes", 10000, false, true))[0].count).toBe("10000+");
    expect(folderAdvice(report("network"))[0]).toMatchObject({ tone: "danger", usable: false });
    expect(folderAdvice(report("not_folder"))[0]).toMatchObject({ tone: "danger", usable: false });
    expect(folderAdvice(report("relative"))[0]).toMatchObject({ key: "wizard.folder.relative", tone: "danger", usable: false });
    const synced = folderAdvice(report("empty", 0, true));
    expect(synced.map((a) => a.key)).toEqual(["wizard.folder.empty", "wizard.folder.onedrive"]);
    expect(synced[1]).toMatchObject({ tone: "warning", usable: true });
  });

  it("reads setup errors that name the folder", () => {
    expect(setupError(String.raw`not_writable|C:\Program Files\Kasa|Disk dolu`)).toEqual({
      key: "wizard.error.not_writable",
      path: String.raw`C:\Program Files\Kasa`,
      reason: "Disk dolu",
    });
    expect(setupError(String.raw`not_writable|C:\Windows\Kasa|access_denied`)).toMatchObject({ reason: "", reasonKey: "wizard.reason.access_denied" });
    expect(setupError(String.raw`network|\\sunucu\paylasim`)).toEqual({ key: "wizard.error.network", path: String.raw`\\sunucu\paylasim`, reason: "" });
    expect(setupError("relative|Notlar")).toMatchObject({ key: "wizard.error.relative", path: "Notlar" });
    expect(setupError("something else")).toEqual({ key: "wizard.error.other", path: "", reason: "something else" });
  });

  it("sums up the model without calling a paused download running", () => {
    // final review M1
    expect(modelSummary(true, null)).toBe("ready");
    expect(modelSummary(false, { state: "running", progress: { received: 1, total: 2, bytes_per_sec: 1, eta_secs: 1 } })).toBe("downloading");
    expect(modelSummary(false, { state: "paused", received: 1, total: 2 })).toBe("paused");
    expect(modelSummary(false, { state: "failed", failure: { kind: "network", needed: null, available: null, status: null, detail: "" }, received: 1, total: 2 })).toBe("off");
    expect(modelSummary(false, null)).toBe("off");
  });
});
