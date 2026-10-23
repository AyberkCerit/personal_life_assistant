"""The interface's performance measurements (SRS §5.1, M6 performance plan § 2) in the real window, on
the reference data set made by `make_reference`: NFR-PERF-001, -002, -003, -006, -007 and the time to
open the window from the tray. Run with the app built (target/debug/pla-desktop.exe) and no PLA open:
the single-instance plugin would hand the measured starts to it and the memory counts go by process
name, so the script stops if one runs; it closes only the PLA it started.

    python research/perf/perf_ui.py <reference out dir> [runs]

Writes research/perf/ui.json and prints a table. p95 of `runs` (default 20).
"""
import json, math, os, subprocess, sys, time
from playwright.sync_api import sync_playwright

REF = os.path.abspath(sys.argv[1])
RUNS = int(sys.argv[2]) if len(sys.argv) > 2 else 20
# PLA_EXE: the release build (target/release, its page inside) is what users run; debug loads Vite
EXE = os.environ.get("PLA_EXE", r"C:\dev\PLA\target\debug\pla-desktop.exe")
MODEL = r"C:\dev\PLA\research\f1-model-eval\models\gemma-4-E2B-it-Q3_K_M.gguf"
HERE = os.path.dirname(os.path.abspath(__file__))
ENV = dict(os.environ, APPDATA=REF, LOCALAPPDATA=os.path.join(REF, "Local"), PLA_MODEL=MODEL, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333")
os.makedirs(ENV["LOCALAPPDATA"], exist_ok=True)


def p95(values):
    v = sorted(values)
    return v[max(0, math.ceil(len(v) * 0.95) - 1)]


started = None  # the PLA this script started; its tree holds the model and the webview


def start():
    global started
    started = subprocess.Popen([EXE], env=ENV, creationflags=subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP)
    return started


def kill():
    global started
    if started is not None and started.poll() is None:
        subprocess.run(["taskkill", "/PID", str(started.pid), "/T", "/F"], capture_output=True)
        time.sleep(2)
    started = None


def running(image):
    out = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {image}", "/NH"], capture_output=True, text=True).stdout
    return image.lower() in out.lower()


PS_MEM = r"""$ids = @(Get-Process pla-desktop, llama-server -ErrorAction SilentlyContinue | % Id)
$ids += Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | ? { $_.CommandLine -match 'dev\.ayberkcerit\.pla' } | % ProcessId
$p = Get-Process -Id $ids -ErrorAction SilentlyContinue
$by = $p | Group-Object ProcessName | % { '{0}={1:N0}/{2:N0}' -f $_.Name, (($_.Group | Measure-Object PrivateMemorySize64 -Sum).Sum/1MB), (($_.Group | Measure-Object WorkingSet64 -Sum).Sum/1MB) }
'{0:N0}|{1:N0}|{2}' -f (($p | Measure-Object PrivateMemorySize64 -Sum).Sum/1MB), (($p | Measure-Object WorkingSet64 -Sum).Sum/1MB), ($by -join ' ')"""


def memory():
    out = subprocess.run(["powershell", "-NoProfile", "-Command", PS_MEM], capture_output=True, text=True).stdout.strip()
    private, working, detail = out.split("|", 2)
    return float(private.replace(",", "")), float(working.replace(",", "")), detail


def connect(p, timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        try:
            b = p.chromium.connect_over_cdp("http://127.0.0.1:9333")
            if b.contexts and b.contexts[0].pages:
                page = b.contexts[0].pages[0]
                page.wait_for_function("window.__TAURI_INTERNALS__ !== undefined && document.querySelector('.tree') !== null", timeout=60000)
                # every start queues the 10 000 never-extracted notes (FR-VLT-014): a backlog of days
                # on this machine; paused, it does not stand in for the steady state measured here
                inv(page, "set_paused", {"paused": True})
                return b, page
            b.close()
        except Exception:
            pass
        time.sleep(0.05)
    raise RuntimeError("no window")


def inv(page, c, a=None):
    return page.evaluate("([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a ?? {})", [c, a])


def hide(page):
    """Closes the window to the tray: the window goes with the call, its answer may not come back."""
    try:
        inv(page, "hide_to_tray")
    except Exception:
        pass


def open_note(page, rel, marker):
    """Emits what a click on a note does and waits until the editor shows it; ms."""
    return page.evaluate(
        """async ([rel, marker]) => {
            // the vault as PLA spells it: the page compares the two strings
            const v = (await window.__TAURI_INTERNALS__.invoke('vault_info', {})).path;
            const t = performance.now();
            await window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'open-note', payload: { path: rel, vault: v } });
            while (!(document.querySelector('.cm-content')?.textContent ?? '').includes(marker)) {
                if (performance.now() - t > 30000) throw new Error('the note did not open: ' + rel);
                await new Promise(r => setTimeout(r, 2));
            }
            await new Promise(r => requestAnimationFrame(() => r()));
            return performance.now() - t;
        }""",
        [rel, marker],
    )


others = [i for i in ("pla-desktop.exe", "llama-server.exe") if running(i)]
if others:
    sys.exit(f"close {', '.join(others)} first: the measurements need the machine to themselves")
rows = []
print("started", flush=True)
with sync_playwright() as p:
    # warm-up: the first start indexes 10 000 notes; the measured starts find the index made
    start()
    b, page = connect(p, 120)
    time.sleep(120)  # the indexer's first pass on a new reference (NFR-PERF-011 measures it: 91 s)
    b.close()
    kill()

    # NFR-PERF-001: cold start to an editable note
    cold = []
    for i in range(RUNS):
        t0 = time.perf_counter()
        start()
        b, page = connect(p)
        open_note(page, "daily/2026/2026-10-10.md", "# 2026-10-10")
        cold.append((time.perf_counter() - t0) * 1000)
        b.close()
        kill()
    rows.append(("NFR-PERF-001", "cold start to an editable note", p95(cold), "ms", 3000)); print(rows[-1], flush=True)

    start()
    b, page = connect(p)
    # NFR-PERF-002: a 1 MB note
    big = []
    for i in range(RUNS):
        open_note(page, "daily/2026/2026-10-09.md", "# 2026-10-09")
        big.append(open_note(page, "notes/Büyük not.md", "# Büyük not"))
    rows.append(("NFR-PERF-002", "open a 1 MB note", p95(big), "ms", 200)); print(rows[-1], flush=True)

    # NFR-PERF-003: typing while the model answers (4 threads busy). Each keystroke: from the event's
    # time to the frame that shows it (the animation frame after the editor's input handling). The
    # Event Timing API counts to the frame's presentation, ~16 ms at 60 Hz by itself: not usable here.
    open_note(page, "daily/2026/2026-10-09.md", "# 2026-10-09")
    page.evaluate("""() => {
        window.__lat = [];
        document.addEventListener('keydown', e => {
            const t = e.timeStamp;
            requestAnimationFrame(() => window.__lat.push(performance.now() - t));
        }, true);
    }""")

    def typing(text):
        page.evaluate("window.__lat = []")
        page.locator(".cm-content").first.click()
        page.keyboard.press("Control+End")
        page.keyboard.type(text, delay=40)
        time.sleep(0.5)
        return page.evaluate("window.__lat")

    text = "Arka planda model çalışırken yazıyorum. " * 5
    idle = typing(text)
    asked = inv(page, "qa_ask", {"question": "Bu hafta neler yaptım, ayrıntılı anlat?", "newTopic": True})
    time.sleep(3)
    busy = typing(text)
    rows.append(("NFR-PERF-003", f"keystroke to frame while the model answers ({len(busy)} keys, worst {max(busy):.0f} ms; model idle: p95 {p95(idle):.0f} ms)", p95(busy), "ms", 16)); print(rows[-1], flush=True)
    # NFR-PERF-007: the language model while it answers
    time.sleep(2)
    _, _, detail = memory()
    llama = [d for d in detail.split() if d.startswith("llama-server=")]
    private_llama = float(llama[0].split("=")[1].split("/")[0].replace(",", "")) if llama else 0
    working_llama = float(llama[0].split("/")[1].replace(",", "")) if llama else 0
    rows.append(("NFR-PERF-007", f"language model while answering (private; working set {working_llama:.0f} MB)", private_llama, "MB", 2048)); print(rows[-1], flush=True)
    # the turn is written when the answer ends
    for _ in range(600):
        if any(t["turn_id"] == asked for t in inv(page, "qa_history")):
            break
        time.sleep(1)

    # NFR-PERF-006: idle, no model loaded (it stops after 60 s idle), window open, then in the tray
    time.sleep(75)
    private, working, detail = memory()
    rows.append(("NFR-PERF-006", f"idle, window open ({detail}; working set {working:.0f} MB)", private, "MB", 450)); print(rows[-1], flush=True)
    hide(page)
    b.close()
    time.sleep(10)
    private, working, detail = memory()
    rows.append(("NFR-PERF-006", f"idle, in the tray ({detail}; working set {working:.0f} MB)", private, "MB", 150)); print(rows[-1], flush=True)
    # the window again from the tray (a second start is what the single-instance plugin turns into show)
    reopen = []
    for i in range(5):
        t0 = time.perf_counter()
        subprocess.Popen([EXE], env=ENV)
        b, page = connect(p)
        reopen.append((time.perf_counter() - t0) * 1000)
        hide(page)
        b.close()
        time.sleep(3)
    rows.append(("—", "open the window from the tray (5 runs)", p95(reopen), "ms", 3000)); print(rows[-1], flush=True)
    kill()

print("\n| SRS | measured | p95 | limit | |\n| --- | --- | --- | --- | --- |")
out = []
for sid, what, value, unit, limit in rows:
    ok = value <= limit
    print(f"| {sid} | {what} | {value:.0f} {unit} | {limit} | {'✓' if ok else '✗'} |")
    out.append({"id": sid, "what": what, "value": value, "unit": unit, "limit": limit, "pass": ok})
json.dump(out, open(os.path.join(HERE, "ui.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=2)
