"""Builds the README images in this folder from the numbers in research/*/RESULTS.md: python docs/readme/make_svgs.py"""
from pathlib import Path

OUT = Path(__file__).resolve().parent
OUT.mkdir(parents=True, exist_ok=True)

SANS = "'Segoe UI',-apple-system,BlinkMacSystemFont,'Helvetica Neue',Arial,sans-serif"
MONO = "'Cascadia Mono','SFMono-Regular',Consolas,'Liberation Mono',monospace"

# Chart surfaces follow the app's own tokens (apps/desktop/src/theme/tokens.css).
CHART_STYLE = f"""
<style>
  svg {{ --surface:#fbfbfc; --grid:#e3e6ec; --text:#2a2e37; --muted:#5d6472;
         --s1:#2a78d6; --s2:#eb6834; --accent:#0f7a63; --track:#d2f1e7; --dim:#b8bec9; --rule:#2a2e37; }}
  @media (prefers-color-scheme: dark) {{
    svg {{ --surface:#1f2228; --grid:#2f343d; --text:#d2d6de; --muted:#9199a7;
           --s1:#3987e5; --s2:#d95926; --accent:#5fbea1; --track:#243c3c; --dim:#4a5160; --rule:#d2d6de; }}
  }}
  text {{ font-family:{SANS}; fill:var(--text); }}
  .muted {{ fill:var(--muted); }}
  .t {{ font-size:15px; font-weight:600; }}
  .s {{ font-size:12px; }}
  .l {{ font-size:13px; }}
  .v {{ font-size:12px; font-variant-numeric:tabular-nums; }}
  .b {{ font-weight:650; }}
</style>"""


def bar(x, y, w, h, fill, r=4):
    """Horizontal bar, square at the baseline (left), 4px rounded data end (right)."""
    if w <= r:
        return f'<rect x="{x}" y="{y}" width="{max(w, 1):.1f}" height="{h}" fill="{fill}"/>'
    return (f'<path d="M{x},{y} h{w - r:.1f} a{r},{r} 0 0 1 {r},{r} v{h - 2 * r} '
            f'a{r},{r} 0 0 1 -{r},{r} h-{w - r:.1f} z" fill="{fill}"/>')


# ---------------------------------------------------------------- model shoot-out
# research/f1-model-eval/RESULTS.md, best prompt per model; Gemma rows with the low-memory settings.
MODELS = [
    ("Qwen2.5 1.5B", "Q4_K_M", 46, 61, 1.91, False),
    ("Qwen3 1.7B", "Q4_K_M", 64, 76, 2.42, False),
    ("Qwen3 4B 2507", "Q4_K_M", 84, 86, 5.04, False),
    ("Gemma 4 E2B", "Q4_0 QAT", 97, 95, 2.75, False),
    ("Gemma 4 E2B", "Q3_K_M · shipped", 96, 95, 1.85, True),
]


def shootout():
    W, H = 920, 470
    top, row = 124, 54
    lx = 18                  # model label column
    ax0, aw = 190, 400       # accuracy panel
    mx0, mw = 680, 200       # memory panel
    mmax = 5.5
    g = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" '
         f'aria-labelledby="t d"><title id="t">Model shoot-out: extraction accuracy and memory</title>'
         f'<desc id="d">Five local models on the same 200 Turkish and English notes. Gemma 4 E2B Q3_K_M reaches '
         f'96% (TR) and 95% (EN) field accuracy in 1.85 GB of working memory and is the model PLA ships.</desc>',
         CHART_STYLE,
         f'<rect width="{W}" height="{H}" rx="12" fill="var(--surface)"/>',
         f'<text x="{lx}" y="34" class="t">Extraction accuracy and memory: five local models on the same 200 notes</text>',
         f'<text x="{lx}" y="56" class="s muted">Field accuracy = expected items extracted exactly right. llama.cpp b11280, CPU only, 4 threads, '
         f'grammar-constrained JSON.</text>']
    # legend
    lgx = ax0
    for i, (name, var) in enumerate((("Turkish notes", "--s1"), ("English notes", "--s2"))):
        x = lgx + i * 130
        g.append(f'<rect x="{x}" y="74" width="12" height="12" rx="3" fill="var({var})"/>'
                 f'<text x="{x + 18}" y="84" class="s">{name}</text>')
    g.append(f'<text x="{mx0}" y="84" class="s b">Working memory</text>')
    plot_h = row * len(MODELS)
    # accuracy grid
    for p in (0, 25, 50, 75, 100):
        x = ax0 + aw * p / 100
        g.append(f'<line x1="{x}" y1="{top - 6}" x2="{x}" y2="{top + plot_h}" stroke="var(--grid)" stroke-width="1"/>'
                 f'<text x="{x}" y="{top + plot_h + 18}" class="v muted" text-anchor="middle">{p}%</text>')
    gx = ax0 + aw * 0.80
    g.append(f'<line x1="{gx}" y1="{top - 10}" x2="{gx}" y2="{top + plot_h}" stroke="var(--rule)" stroke-width="1.5"/>'
             f'<text x="{gx - 4}" y="{top - 14}" class="v muted" text-anchor="end">required: 80%</text>')
    # memory grid
    for gb in (0, 1, 2, 3, 4, 5):
        x = mx0 + mw * gb / mmax
        g.append(f'<line x1="{x}" y1="{top - 6}" x2="{x}" y2="{top + plot_h}" stroke="var(--grid)" stroke-width="1"/>'
                 f'<text x="{x}" y="{top + plot_h + 18}" class="v muted" text-anchor="middle">{gb}{" GB" if gb == 5 else ""}</text>')
    bx = mx0 + mw * 2 / mmax
    g.append(f'<line x1="{bx}" y1="{top - 10}" x2="{bx}" y2="{top + plot_h}" stroke="var(--rule)" stroke-width="1.5"/>'
             f'<text x="{bx + 4}" y="{top - 14}" class="v muted">budget 2 GB</text>')
    for i, (name, quant, tr, en, mem, chosen) in enumerate(MODELS):
        y = top + i * row
        cls = "l b" if chosen else "l"
        g.append(f'<text x="{lx}" y="{y + 20}" class="{cls}">{name}</text>'
                 f'<text x="{lx}" y="{y + 37}" class="s muted">{quant}</text>')
        for j, (val, var) in enumerate(((tr, "--s1"), (en, "--s2"))):
            by = y + 8 + j * 16
            w = aw * val / 100
            g.append(bar(ax0, by, w, 14, f"var({var})"))
            g.append(f'<text x="{ax0 + w + 6}" y="{by + 11}" class="v">{val}%</text>')
        w = mw * mem / mmax
        g.append(bar(mx0, y + 12, w, 20, "var(--accent)" if chosen else "var(--dim)"))
        g.append(f'<text x="{mx0 + w + 6}" y="{y + 26}" class="v{" b" if chosen else ""}">{mem:.2f} GB</text>')
    g.append(f'<text x="{lx}" y="{H - 18}" class="s muted">Dev set, 100 TR + 100 EN notes. On a separate held-out set written after the prompt was frozen, '
             f'the shipped model scores 91% / 91%.</text>')
    g.append("</svg>")
    (OUT / "model-shootout.svg").write_text("\n".join(g), encoding="utf-8")


# ---------------------------------------------------------------- performance budgets
# research/perf/RESULTS.md: p95 on the 10,000-note reference vault, model on 3 threads.
PERF = [
    ("Cold start to an editable note", 1.72, 3, "1.72 s", "3 s"),
    ("Open a 1 MB note", 72, 200, "72 ms", "200 ms"),
    ("Keystroke to frame, model busy", 7.7, 16, "7.7 ms", "16 ms"),
    ("Full-text search", 33, 300, "33 ms", "300 ms"),
    ("Task, metric and report queries", 12, 100, "12 ms", "100 ms"),
    ("Load the language model", 2.6, 5, "2.6 s", "5 s"),
    ("Open the window from the tray", 641, 3000, "641 ms", "3 s"),
    ("First index of 10,000 notes", 165, 300, "165 s", "300 s"),
    ("Memory, sitting in the tray", 27, 150, "27 MB", "150 MB"),
    ("Memory, window open and idle", 374, 450, "374 MB", "450 MB"),
]


def perf():
    W = 920
    top, row = 92, 34
    H = top + row * len(PERF) + 56
    lx, bx0, bw = 18, 300, 400
    g = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" '
         f'aria-labelledby="t d"><title id="t">Performance budgets</title>'
         f'<desc id="d">Ten measured p95 values against their limits on a 10,000-note vault. All ten are under budget.</desc>',
         CHART_STYLE,
         f'<rect width="{W}" height="{H}" rx="12" fill="var(--surface)"/>',
         f'<text x="{lx}" y="34" class="t">Measured p95 as a share of each limit</text>',
         f'<text x="{lx}" y="56" class="s muted">p95 on a generated vault: 10,000 notes, 900 MB of attachments, 20,000 tasks, '
         f'50,000 metrics. Release build.</text>']
    plot_h = row * len(PERF)
    for p in (0, 50, 100):
        x = bx0 + bw * p / 100
        g.append(f'<text x="{x}" y="{top - 10}" class="v muted" text-anchor="middle">{p}%</text>')
    for i, (label, val, lim, vs, ls) in enumerate(PERF):
        y = top + i * row
        frac = val / lim
        g.append(f'<text x="{lx}" y="{y + 17}" class="l">{label}</text>')
        g.append(f'<rect x="{bx0}" y="{y + 6}" width="{bw}" height="16" rx="4" fill="var(--track)"/>')
        g.append(bar(bx0, y + 6, bw * frac, 16, "var(--accent)"))
        g.append(f'<text x="{bx0 + bw + 14}" y="{y + 18}" class="v"><tspan class="b">{vs}</tspan>'
                 f'<tspan class="muted"> of {ls}  ·  {round(frac * 100)}%</tspan></text>')
    x100 = bx0 + bw
    g.append(f'<line x1="{x100}" y1="{top}" x2="{x100}" y2="{top + plot_h - 6}" stroke="var(--rule)" stroke-width="1.5"/>')
    g.append(f'<text x="{lx}" y="{H - 20}" class="s muted">Limits from the SRS (NFR-PERF-001…012). Model runs on 3 threads, '
             f'what PLA gives it on the minimum 4-core machine.</text>')
    g.append("</svg>")
    (OUT / "perf-budgets.svg").write_text("\n".join(g), encoding="utf-8")


# ---------------------------------------------------------------- hero
SPARK = ("M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5"
         "l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964"
         "L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z")


def hero():
    W, H = 1280, 640
    style = f"""
<style>
  text {{ font-family:{SANS}; }}
  .mono {{ font-family:{MONO}; }}
  .hl {{ animation: hl 9s ease-in-out infinite; }}
  .hl2 {{ animation: hl2 9s ease-in-out infinite; }}
  .flow {{ stroke-dasharray: 6 8; animation: flow 1.2s linear infinite; }}
  .c1 {{ animation: c1 9s ease-out infinite; }}
  .c2 {{ animation: c2 9s ease-out infinite; }}
  .chip {{ animation: chip 9s ease-in-out infinite; }}
  .caret {{ animation: caret 1s steps(1) infinite; }}
  @keyframes hl   {{ 0%,8% {{opacity:0}} 14%,40% {{opacity:1}} 50%,100% {{opacity:0}} }}
  @keyframes hl2  {{ 0%,40% {{opacity:0}} 46%,72% {{opacity:1}} 82%,100% {{opacity:0}} }}
  @keyframes c1   {{ 0%,22% {{opacity:0; transform:translateX(24px)}} 30%,92% {{opacity:1; transform:none}} 100% {{opacity:0}} }}
  @keyframes c2   {{ 0%,54% {{opacity:0; transform:translateX(24px)}} 62%,92% {{opacity:1; transform:none}} 100% {{opacity:0}} }}
  @keyframes chip {{ 0%,12% {{opacity:.35}} 18%,28% {{opacity:1}} 34%,44% {{opacity:.35}} 50%,60% {{opacity:1}} 66%,100% {{opacity:.35}} }}
  @keyframes flow {{ to {{ stroke-dashoffset:-14 }} }}
  @keyframes caret {{ 50% {{ opacity:0 }} }}
  @media (prefers-reduced-motion: reduce) {{
    .hl,.hl2,.flow,.c1,.c2,.chip,.caret {{ animation:none; }}
    .hl,.hl2 {{ opacity:0; }}
  }}
</style>"""
    bg, panel, raised = "#1f2228", "#1a1d22", "#262a32"
    text, muted, border = "#d2d6de", "#9199a7", "#343a45"
    accent, ai, ai_sub = "#5fbea1", "#94d4c0", "#213c36"
    g = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" aria-labelledby="t d">'
         f'<title id="t">PLA: a note becomes a task and a metric, on your own computer</title>'
         f'<desc id="d">A daily note says "Tomorrow at 8 I am hitting the gym, shoulder press, goal 40 kg. Slept 7 hours last night." '
         f'PLA, running a local model, adds a task for Friday at 08:00 and logs 7 hours of sleep, both marked as added by AI.</desc>',
         style,
         '<defs><radialGradient id="glow" cx="78%" cy="8%" r="70%"><stop offset="0" stop-color="#5fbea1" stop-opacity=".16"/>'
         '<stop offset="1" stop-color="#5fbea1" stop-opacity="0"/></radialGradient>'
         '<pattern id="dots" width="22" height="22" patternUnits="userSpaceOnUse"><circle cx="1" cy="1" r="1" fill="#ffffff" fill-opacity=".045"/></pattern></defs>',
         f'<rect width="{W}" height="{H}" rx="20" fill="{panel}"/>',
         f'<rect width="{W}" height="{H}" rx="20" fill="url(#dots)"/>',
         f'<rect width="{W}" height="{H}" rx="20" fill="url(#glow)"/>',
         # logo + wordmark
         '<g transform="translate(64,56) scale(.875)"><rect x="4" y="4" width="56" height="56" rx="14" fill="#5fbea1"/>'
         '<path d="M22 48V16h12a10 10 0 0 1 0 20H22" fill="none" stroke="#0b1712" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/>'
         '<circle cx="44" cy="46" r="4.5" fill="#0b1712"/></g>',
         f'<text x="134" y="92" font-size="34" font-weight="700" fill="{text}" letter-spacing="1">PLA</text>',
         f'<text x="214" y="91" font-size="17" fill="{muted}">Personal Life Assistant</text>',
         f'<text x="64" y="168" font-size="46" font-weight="700" fill="{text}">Your notes already hold your to-do list.</text>',
         f'<text x="64" y="214" font-size="24" fill="{muted}">PLA reads them with a small model that runs on your own CPU, offline.</text>']
    # note panel
    nx, ny, nw, nh = 64, 268, 560, 300
    g += [f'<rect x="{nx}" y="{ny}" width="{nw}" height="{nh}" rx="14" fill="{bg}" stroke="{border}"/>',
          f'<circle cx="{nx + 22}" cy="{ny + 24}" r="5" fill="#3a404b"/><circle cx="{nx + 40}" cy="{ny + 24}" r="5" fill="#3a404b"/>'
          f'<circle cx="{nx + 58}" cy="{ny + 24}" r="5" fill="#3a404b"/>',
          f'<text x="{nx + 80}" y="{ny + 29}" class="mono" font-size="14" fill="{muted}">daily/2026/2026-11-05.md</text>',
          f'<line x1="{nx}" y1="{ny + 46}" x2="{nx + nw}" y2="{ny + 46}" stroke="{border}"/>',
          f'<text x="{nx + 28}" y="{ny + 88}" font-size="22" font-weight="650" fill="{text}">Thursday</text>',
          f'<rect class="hl" x="{nx + 20}" y="{ny + 112}" width="{nw - 40}" height="64" rx="8" fill="{ai_sub}"/>',
          f'<text x="{nx + 28}" y="{ny + 138}" font-size="18" fill="{text}">Tomorrow at 8 I&#8217;m hitting the gym,</text>',
          f'<text x="{nx + 28}" y="{ny + 164}" font-size="18" fill="{text}">shoulder press, goal 40 kg.</text>',
          f'<rect class="hl2" x="{nx + 20}" y="{ny + 184}" width="{nw - 40}" height="38" rx="8" fill="{ai_sub}"/>',
          f'<text x="{nx + 28}" y="{ny + 209}" font-size="18" fill="{text}">Slept 7 hours last night, finally.</text>',
          f'<rect class="caret" x="{nx + 29}" y="{ny + 236}" width="2" height="22" fill="{accent}"/>']
    # flow + model chip
    fx = nx + nw
    g += [f'<path class="flow" d="M{fx + 8} {ny + 150} H{fx + 156}" stroke="{accent}" stroke-width="2" fill="none"/>',
          f'<path d="M{fx + 150} {ny + 143} l9 7 l-9 7" stroke="{accent}" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>',
          f'<g class="chip"><rect x="{fx + 22}" y="{ny + 172}" width="122" height="58" rx="10" fill="{raised}" stroke="{border}"/>'
          f'<text x="{fx + 83}" y="{ny + 196}" font-size="13" font-weight="650" fill="{text}" text-anchor="middle">Gemma 4 E2B</text>'
          f'<text x="{fx + 83}" y="{ny + 216}" font-size="12" fill="{muted}" text-anchor="middle">on-device</text></g>']
    # result cards
    cx, cw = fx + 172, 380

    def card(cls, y, kicker, title, meta):
        return (f'<g class="{cls}"><rect x="{cx}" y="{y}" width="{cw}" height="104" rx="14" fill="{raised}" stroke="{border}"/>'
                f'<text x="{cx + 22}" y="{y + 30}" font-size="13" font-weight="650" fill="{muted}" letter-spacing="1.2">{kicker}</text>'
                f'<rect x="{cx + cw - 102}" y="{y + 14}" width="84" height="26" rx="13" fill="{ai_sub}"/>'
                f'<g transform="translate({cx + cw - 94},{y + 19}) scale(.66)"><path d="{SPARK}" fill="{ai}"/></g>'
                f'<text x="{cx + cw - 72}" y="{y + 32}" font-size="13" font-weight="650" fill="{ai}">added</text>'
                f'<rect x="{cx + 22}" y="{y + 52}" width="18" height="18" rx="4" fill="none" stroke="#6b7383" stroke-width="1.6"/>'
                f'<text x="{cx + 52}" y="{y + 67}" font-size="19" font-weight="650" fill="{text}">{title}</text>'
                f'<text x="{cx + 52}" y="{y + 90}" font-size="15" fill="{muted}">{meta}</text></g>')
    g.append(card("c1", ny, "TASK", "Shoulder workout", "Fri 6 Nov · 08:00 · shoulder press 40 kg"))
    g.append(f'<g class="c2"><rect x="{cx}" y="{ny + 124}" width="{cw}" height="104" rx="14" fill="{raised}" stroke="{border}"/>'
             f'<text x="{cx + 22}" y="{ny + 154}" font-size="13" font-weight="650" fill="{muted}" letter-spacing="1.2">METRIC · SLEEP</text>'
             f'<rect x="{cx + cw - 102}" y="{ny + 138}" width="84" height="26" rx="13" fill="{ai_sub}"/>'
             f'<g transform="translate({cx + cw - 94},{ny + 143}) scale(.66)"><path d="{SPARK}" fill="{ai}"/></g>'
             f'<text x="{cx + cw - 72}" y="{ny + 156}" font-size="13" font-weight="650" fill="{ai}">added</text>'
             f'<text x="{cx + 22}" y="{ny + 204}" font-size="34" font-weight="700" fill="{text}">7 h</text>'
             f'<text x="{cx + 92}" y="{ny + 203}" font-size="15" fill="{muted}">Wed 4 Nov, the night before</text></g>')
    g.append(f'<text x="{cx}" y="{ny + 268}" font-size="15" fill="{muted}">Dates are computed in Rust, not guessed by the model.</text>')
    g.append(f'<text x="{cx}" y="{ny + 292}" font-size="15" fill="{muted}">Each added item has an Undo and a link to its line.</text>')
    g.append("</svg>")
    (OUT / "hero.svg").write_text("\n".join(g), encoding="utf-8")


shootout()
perf()
hero()
print("wrote", ", ".join(sorted(p.name for p in OUT.glob("*.svg"))))
