"""High-priority SRS requirements that no code, test or script mentions by ID (a rough gap list)."""
import glob, os, re, sys

os.chdir(r"C:\dev\PLA")
src = "".join(open(f, encoding="utf-8").read() for f in glob.glob("docs/srs/build/content/*.js"))
row = re.compile(r'\["((?:FR|NFR)-[A-Z0-9]+-\d{3})", "[A-Z]", "((?:[^"\\]|\\.)*)", "([YOD])", "([^"]+)"')
reqs = row.findall(src)
code = ""
for pat in ["crates/**/*.rs", "apps/desktop/src-tauri/src/**/*.rs", "apps/desktop/src/**/*.ts", "apps/desktop/src/**/*.svelte",
            "apps/desktop/scripts/**/*.mjs", "research/**/*.py", "apps/desktop/src-tauri/installer/*.nsh"]:
    for f in glob.glob(pat, recursive=True):
        try:
            code += open(f, encoding="utf-8").read()
        except Exception:
            pass
high = [r for r in reqs if r[2] == "Y"]
missing = [r for r in high if r[0] not in code]
print(f"requirements: {len(reqs)}, high: {len(high)}, high not named in code: {len(missing)}")
show = "--all" in sys.argv
by = {}
for r in missing:
    by.setdefault(r[0].rsplit("-", 1)[0], []).append(r)
for k, v in sorted(by.items()):
    print(f"{k} ({len(v)}): " + " ".join(x[0].rsplit("-", 1)[1] for x in v))
    if show:
        for x in v:
            print(f"   {x[0]} [{x[3]}] {x[1][:150]}")
