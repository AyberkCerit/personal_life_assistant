import { describe, expect, it } from "vitest";
import { ancestors, followMove, moveTargets, openAfterMove, parentOf, systemFolder, treeError, visibleEntries } from "./tree";
import { tIn, type Key } from "./i18n";
import type { TreeEntry } from "./api";

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

  // M6 performance: 10 000 notes as one always-open list were 90 000 DOM nodes and 850 MB
  const dir = (path: string, depth: number): TreeEntry => ({ path, name: path.split("/").pop()!, is_dir: true, depth });
  const note = (path: string, depth: number): TreeEntry => ({ path, name: path.split("/").pop()!, is_dir: false, depth });
  const vault = [dir("daily", 0), dir("daily/2026", 1), note("daily/2026/a.md", 2), note("daily/b.md", 1), dir("notes", 0), note("notes/c.md", 1), note("kök.md", 0)];

  it("shows a folder's contents only when it and every folder above it are open", () => {
    const paths = (open: string[]) => visibleEntries(vault, new Set(open)).map((e) => e.path);
    expect(paths([])).toEqual(["daily", "notes", "kök.md"]);
    expect(paths(["daily"])).toEqual(["daily", "daily/2026", "daily/b.md", "notes", "kök.md"]);
    expect(paths(["daily/2026"])).toEqual(["daily", "notes", "kök.md"]); // a closed parent hides an open child
    expect(paths(["daily", "daily/2026", "notes"])).toEqual(vault.map((e) => e.path));
  });

  it("names the folders above a path", () => {
    expect(ancestors("daily/2026/a.md")).toEqual(["daily", "daily/2026"]);
    expect(ancestors("kök.md")).toEqual([]);
  });

  it("keeps a renamed or moved folder open, with the folders inside it", () => {
    const open = new Set(["proje", "proje/eski", "notes"]);
    expect([...openAfterMove(open, "proje", "arşiv/proje")].sort()).toEqual(["arşiv/proje", "arşiv/proje/eski", "notes"]);
  });

  // FR-I18N-004 (M6 acceptance): system folders keep English names on disk, the tree says them in
  // the interface language
  it("knows the vault's own folders by their configured names, at the top only", () => {
    const folders = { daily: "daily", inbox: "Gelen", notes: "notes", reports: "reports", attachments: "attachments", templates: "templates", memory: "memory" };
    expect(systemFolder("daily", folders)).toBe("daily");
    expect(systemFolder("Gelen", folders)).toBe("inbox");
    expect(systemFolder("daily/2026", folders)).toBeNull();
    expect(systemFolder("projeler", folders)).toBeNull();
  });

  it("shows the memory folder as Hafıza in the Turkish interface", () => {
    const folders = { daily: "daily", inbox: "inbox", notes: "notes", reports: "reports", attachments: "attachments", templates: "templates", memory: "memory" };
    const kind = systemFolder("memory", folders);
    expect(kind).toBe("memory");
    expect(tIn("tr", `folder.${kind}` as Key)).toBe("Hafıza");
    expect(tIn("en", `folder.${kind}` as Key)).toBe("Memory");
  });
});
