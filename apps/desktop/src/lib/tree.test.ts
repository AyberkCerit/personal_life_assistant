import { describe, expect, it } from "vitest";
import { followMove, moveTargets, parentOf, treeError } from "./tree";

describe("tree helpers", () => {
  it("reads the tree commands' error codes", () => {
    expect(treeError("bad_name|?|")).toEqual({ key: "tree.error.bad_name", detail: "?", reason: "" });
    expect(treeError("exists|notes/a.md|")).toEqual({ key: "tree.error.exists", detail: "notes/a.md", reason: "" });
    expect(treeError("io||erişim engellendi")).toEqual({ key: "tree.error.io", detail: "", reason: "erişim engellendi" });
    expect(treeError("reserved|CON|")).toEqual({ key: "tree.error.reserved", detail: "CON", reason: "" });
    expect(treeError("boom")).toEqual({ key: "tree.error.io", detail: "", reason: "boom" });
  });

  it("knows a path's folder", () => {
    expect(parentOf("notes/alt/a.md")).toBe("notes/alt");
    expect(parentOf("a.md")).toBe("");
  });

  it("follows the open note when it or its folder moves", () => {
    expect(followMove("notes/a.md", "notes/a.md", "arşiv/a.md")).toBe("arşiv/a.md");
    expect(followMove("notes/proje/a.md", "notes/proje", "arşiv/proje")).toBe("arşiv/proje/a.md");
    expect(followMove("notes/projeler/a.md", "notes/proje", "arşiv/proje")).toBe("notes/projeler/a.md"); // a name prefix only
    expect(followMove(null, "a", "b")).toBeNull();
  });

  it("offers every other folder as a move target, not the item or its inside", () => {
    const entries = [
      { path: "inbox", name: "inbox", is_dir: true, depth: 0 },
      { path: "notes", name: "notes", is_dir: true, depth: 0 },
      { path: "notes/proje", name: "proje", is_dir: true, depth: 1 },
      { path: "notes/proje/alt", name: "alt", is_dir: true, depth: 2 },
      { path: "notes/a.md", name: "a.md", is_dir: false, depth: 1 },
    ];
    expect(moveTargets(entries, "notes/a.md")).toEqual(["", "inbox", "notes/proje", "notes/proje/alt"]); // not where it already is
    expect(moveTargets(entries, "notes/proje")).toEqual(["", "inbox"]); // not into itself, not where it is
  });
});
