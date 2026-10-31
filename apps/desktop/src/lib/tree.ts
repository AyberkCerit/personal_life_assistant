// Folder tree helpers (FR-EDT-004): error codes, paths after a move, where an item can go.
import type { TreeEntry } from "./api";
import { splitError, type Key } from "./i18n";

const CODES = ["bad_name", "exists", "missing", "bad_path", "read_only", "conflict", "io", "no_vault", "reserved"];

/** Reads `<code>|<detail>|<reason>` from the tree commands. */
export function treeError(err: string): { key: Key; detail: string; reason: string } {
  const { code, detail, reason } = splitError(err);
  if (!CODES.includes(code)) return { key: "tree.error.io", detail: "", reason: err };
  return { key: `tree.error.${code}` as Key, detail, reason };
}

export function parentOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

/** The vault's own folders as `.pla/config` names them (pla-core `Folders`). */
export interface Folders { daily: string; inbox: string; notes: string; reports: string; attachments: string; templates: string; memory: string; sources: string }

/** Which of the vault's own folders `path` is, at the vault's top (FR-I18N-004: the tree says it in
 *  the interface language; on disk it keeps its name). */
export function systemFolder(path: string, folders: Folders): keyof Folders | null {
  const kinds = Object.keys(folders) as (keyof Folders)[];
  return kinds.find((k) => folders[k] === path) ?? null;
}

/** The folders above `path`, outermost first. */
export function ancestors(path: string): string[] {
  const parts = path.split("/").slice(0, -1);
  return parts.map((_, i) => parts.slice(0, i + 1).join("/"));
}

/** The rows to draw: an entry shows when every folder above it is open (M6: a vault of 10 000
 *  notes drawn whole was 90 000 DOM nodes and 850 MB). `entries` come depth-first. */
export function visibleEntries(entries: TreeEntry[], open: Set<string>): TreeEntry[] {
  return entries.filter((e) => ancestors(e.path).every((a) => open.has(a)));
}

/** Where the open note is after `from` (a note or a folder) moved to `to`. */
export function followMove(current: string | null, from: string, to: string): string | null {
  if (current === null) return null;
  if (current === from) return to;
  if (current.startsWith(`${from}/`)) return `${to}${current.slice(from.length)}`;
  return current;
}

/** The open folders after `from` became `to` (renamed or moved): it and those inside stay open. */
export function openAfterMove(open: Set<string>, from: string, to: string): Set<string> {
  return new Set([...open].map((p) => followMove(p, from, to) ?? p));
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
