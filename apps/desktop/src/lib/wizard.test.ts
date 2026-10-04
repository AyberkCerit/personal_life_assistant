import { describe, expect, it } from "vitest";
import { STEPS, folderAdvice, nextStep, prevStep, setupError } from "./wizard";

describe("first-run wizard", () => {
  it("walks language → vault → model → done", () => {
    expect(STEPS).toEqual(["language", "vault", "model", "done"]);
    expect(nextStep("language")).toBe("vault");
    expect(nextStep("done")).toBe("done");
    expect(prevStep("vault")).toBe("language");
    expect(prevStep("language")).toBe("language");
  });

  it("explains what choosing a folder will do", () => {
    const report = (state: string, md_files = 0, in_onedrive = false) => ({ check: { state, md_files }, in_onedrive }) as Parameters<typeof folderAdvice>[0];
    expect(folderAdvice(report("missing"))).toEqual([{ key: "wizard.folder.missing", tone: "info", usable: true }]);
    expect(folderAdvice(report("notes", 42))).toEqual([{ key: "wizard.folder.notes", tone: "info", usable: true, count: 42 }]);
    expect(folderAdvice(report("network"))[0]).toMatchObject({ tone: "danger", usable: false });
    expect(folderAdvice(report("not_folder"))[0]).toMatchObject({ tone: "danger", usable: false });
    const synced = folderAdvice(report("empty", 0, true));
    expect(synced.map((a) => a.key)).toEqual(["wizard.folder.empty", "wizard.folder.onedrive"]);
    expect(synced[1]).toMatchObject({ tone: "warning", usable: true });
  });

  it("reads setup errors that name the folder", () => {
    expect(setupError("not_writable|C:\Program Files\Kasa|Erişim engellendi. (os error 5)")).toEqual({
      key: "wizard.error.not_writable",
      path: "C:\Program Files\Kasa",
      reason: "Erişim engellendi. (os error 5)",
    });
    expect(setupError("network|\\sunucu\paylasim")).toEqual({ key: "wizard.error.network", path: "\\sunucu\paylasim", reason: "" });
    expect(setupError("something else")).toEqual({ key: "wizard.error.other", path: "", reason: "something else" });
  });
});
