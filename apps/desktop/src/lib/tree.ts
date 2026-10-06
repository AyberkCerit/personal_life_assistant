// Folder tree helpers (FR-EDT-004): error codes, paths after a move, where an item can go.
import type { TreeEntry } from "./api";
import type { Key } from "./i18n";

const CODES = ["bad_name", "exists", "missing", "bad_path", "read_only", "conflict", "io", "no_vault"];

/** Reads `<code>|<detail>|<reason>` from the tree commands. */
export function treeError(err: string): { key: Key; detail: string; reason: string } {
  const [code, detail = "", ...rest] = err.split("|");
  if (!CODES.includes(code)) return { key: "tree.error.io", detail: "", reason: err };
  return { key: `tree.error.${code}` as Key, detail, reason: rest.join("|") };
}

export function parentOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

/** Where the open note is after `from` (a note or a folder) moved to `to`. */
export function followMove(current: string | null, from: string, to: string): string | null {
  if (current === null) return null;
  if (current === from) return to;
  if (current.startsWith(`${from}/`)) return `${to}${current.slice(from.length)}`;
  return current;
}

/** The folders `path` can move to (`""` is the vault root): not where it is, not into itself. */
export function moveTargets(entries: TreeEntry[], path: string): string[] {
  const here = parentOf(path);
  const folders = ["", ...entries.filter((e) => e.is_dir).map((e) => e.path)];
  return folders.filter((f) => f !== here && f !== path && !f.startsWith(`${path}/`));
}

/** What the tree asks the app to do; the app saves the open note first and reports errors. */
export interface TreeActions {
  createNote: (folder: string, title: string, template: string | null) => Promise<void>;
  createFolder: (parent: string, name: string) => Promise<void>;
  renameNote: (path: string, name: string, updateLinks: boolean) => Promise<void>;
  renameFolder: (path: string, name: string) => Promise<void>;
  move: (path: string, folder: string) => Promise<void>;
  remove: (path: string) => Promise<void>;
  linkCount: (path: string) => Promise<number>;
  templates: () => Promise<string[]>;
}
