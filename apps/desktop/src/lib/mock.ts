// Browser-only stand-in for the Rust commands, so the UI can be checked with `npm run dev`.
const files = new Map<string, string>([
  ["daily/2026/2026-10-06.md", "Yarın 9'da dişçi var.\n\nDün 7 saat uyudum.\n"],
  ["notes/Fikirler.md", "# Fikirler\n\n- [ ] Blog yazısı\n"],
  ["inbox/Hoş geldin.md", "Bu, PLA'nın tarayıcıdaki deneme kasası.\n"],
]);
let vault: string | null = null;

function hash(s: string): string {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) | 0;
  return String(h);
}

function tree() {
  const dirs = new Set<string>();
  for (const p of files.keys()) {
    const parts = p.split("/");
    for (let i = 1; i < parts.length; i++) dirs.add(parts.slice(0, i).join("/"));
  }
  const entries: { path: string; name: string; is_dir: boolean; depth: number }[] = [];
  const children = (prefix: string) => {
    const depth = prefix ? prefix.split("/").length : 0;
    const under = (p: string) => (prefix ? p.startsWith(prefix + "/") : true) && p.split("/").length === depth + 1;
    const byName = (a: string, b: string) => a.toLowerCase().localeCompare(b.toLowerCase());
    for (const d of [...dirs].filter(under).sort(byName)) {
      entries.push({ path: d, name: d.split("/").pop()!, is_dir: true, depth });
      children(d);
    }
    for (const f of [...files.keys()].filter(under).sort(byName)) {
      entries.push({ path: f, name: f.split("/").pop()!, is_dir: false, depth });
    }
  };
  children("");
  return entries;
}

export async function mockBackend<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const path = String(args.path ?? "");
  switch (cmd) {
    case "startup":
      return { vault_path: vault, first_run: vault === null, settings_recovered: false, theme: "dark", error: null } as T;
    case "open_vault":
      vault = path;
      return vault as T;
    case "list_tree":
      return tree() as T;
    case "read_note": {
      const text = files.get(path) ?? "";
      return { text, hash: hash(text), read_only: false } as T;
    }
    case "save_note": {
      const text = String(args.text);
      files.set(path, text);
      return { kind: "saved", hash: hash(text) } as T;
    }
    case "save_copy": {
      const copy = path.replace(/\.md$/i, " (çakışma).md");
      files.set(copy, String(args.text));
      return copy as T;
    }
    case "create_note": {
      const rel = `${args.folder}/${args.title}.md`;
      files.set(rel, "");
      return rel as T;
    }
    case "queue_note":
      return undefined as T;
    default:
      throw new Error(`mock: unknown command ${cmd}`);
  }
}
