"""SRS diyagramlarini diagram-design stiliyle ureten betik.

Calistirma: python gen.py  -> ayni klasore <slug>.html dosyalari yazar.
PNG icin: python export_png.py (Playwright, miniconda python).
"""
from pathlib import Path

OUT = Path(__file__).parent

PAPER = "#f5f5f5"
INK = "#2d3142"
MUTED = "#4f5d75"
SOFT = "#7a8399"
ACCENT = "#eb6c36"
LINK = "#2e5aa8"

KIND = {
    "focal":    dict(fill="rgba(235,108,54,0.08)", stroke=ACCENT, dash=None),
    "step":     dict(fill="#ffffff", stroke=INK, dash=None),
    "store":    dict(fill="rgba(45,49,66,0.05)", stroke=MUTED, dash=None),
    "external": dict(fill="rgba(45,49,66,0.03)", stroke="rgba(45,49,66,0.30)", dash=None),
    "input":    dict(fill="rgba(79,93,117,0.10)", stroke=SOFT, dash=None),
    "optional": dict(fill="rgba(45,49,66,0.02)", stroke="rgba(45,49,66,0.20)", dash="4,3"),
}

FONT_LINK = ("https://fonts.googleapis.com/css2?family=Instrument+Serif:ital@0;1"
             "&family=Geist:wght@400;500;600&family=Geist+Mono:wght@400;500;600"
             "&family=Noto+Serif:ital@0;1&display=swap")

SANS = "'Geist', sans-serif"
MONO = "'Geist Mono', monospace"


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def tw(text, size=9):
    """Mono metin genisligi tahmini (px)."""
    return int(len(text) * size * 0.66) + 10


# ---------------------------------------------------------------- primitives

def rpath(points, r=8):
    """Ortogonal nokta listesinden yuvarlak koseli path 'd' degeri."""
    d = f"M {points[0][0]},{points[0][1]}"
    for i in range(1, len(points) - 1):
        (x0, y0), (x1, y1), (x2, y2) = points[i - 1], points[i], points[i + 1]
        dx1, dy1 = (x1 > x0) - (x1 < x0), (y1 > y0) - (y1 < y0)
        dx2, dy2 = (x2 > x1) - (x2 < x1), (y2 > y1) - (y2 < y1)
        l1 = abs(x1 - x0) + abs(y1 - y0)
        l2 = abs(x2 - x1) + abs(y2 - y1)
        rr = min(r, l1 / 2 if i > 1 else l1, l2 / 2 if i < len(points) - 2 else l2)
        ax, ay = x1 - dx1 * rr, y1 - dy1 * rr
        bx, by = x1 + dx2 * rr, y1 + dy2 * rr
        d += f" L {ax:g},{ay:g} Q {x1},{y1} {bx:g},{by:g}"
    d += f" L {points[-1][0]},{points[-1][1]}"
    return d


def conn(points, color="muted", dashed=False, arrow=True, width=1.2, open_head=False):
    stroke = {"muted": MUTED, "accent": ACCENT, "link": LINK, "ink": INK}[color]
    marker = ""
    if arrow:
        mid = "arrow-open" if open_head else {"muted": "arrow", "ink": "arrow",
                                              "accent": "arrow-accent", "link": "arrow-link"}[color]
        marker = f' marker-end="url(#{mid})"'
    dash = f' stroke-dasharray="{dashed if isinstance(dashed, str) else "5,4"}"' if dashed else ""
    return (f'<path d="{rpath(points)}" fill="none" stroke="{stroke}" '
            f'stroke-width="{width}"{dash}{marker}/>')


def label(x, y, text, anchor="middle", size=9, color=SOFT):
    """Maskeli etiket. (x, y) metnin taban cizgisi; anchor start/middle/end."""
    w = tw(text, size)
    if anchor == "middle":
        rx = x - w / 2
    elif anchor == "start":
        rx = x - 5
    else:
        rx = x - w + 5
    return (f'<rect x="{rx:g}" y="{y - size - 1}" width="{w}" height="{size + 4}" rx="2" fill="{PAPER}"/>'
            f'<text x="{x}" y="{y}" fill="{color}" font-size="{size}" font-family="{MONO}" '
            f'text-anchor="{anchor}" letter-spacing="0.06em">{esc(text)}</text>')


def lab_above(x1, x2, y, text):
    """Yatay segmentin ustune, 8px bosluklu etiket."""
    return label((x1 + x2) / 2, y - 11, text)


def lab_side(x, y1, y2, text, side="right"):
    """Dikey segmentin yanina, 8px bosluklu etiket."""
    cy = (y1 + y2) / 2 + 3
    if side == "right":
        return label(x + 13, cy, text, anchor="start")
    return label(x - 13, cy, text, anchor="end")


def box(x, y, w, h, name, sub=None, tag=None, kind="step", rx=6, name_size=12):
    k = KIND[kind]
    dash = f' stroke-dasharray="{k["dash"]}"' if k["dash"] else ""
    s = [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{PAPER}"/>',
         f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{k["fill"]}" '
         f'stroke="{k["stroke"]}" stroke-width="1"{dash}/>']
    cx = x + w / 2
    names = name.split("\n")
    subs = sub.split("\n") if sub else []
    lines = len(names) * 15 + len(subs) * 12
    top = y + h / 2 - lines / 2 + 11 + (4 if tag else 0)
    for i, n in enumerate(names):
        s.append(f'<text x="{cx:g}" y="{top + i * 15:g}" fill="{INK}" font-size="{name_size}" '
                 f'font-weight="600" font-family="{SANS}" text-anchor="middle">{esc(n)}</text>')
    base = top + len(names) * 15 - 2
    for i, sb in enumerate(subs):
        s.append(f'<text x="{cx:g}" y="{base + i * 12:g}" fill="{MUTED}" font-size="9" '
                 f'font-family="{MONO}" text-anchor="middle">{esc(sb)}</text>')
    if tag:
        tw_ = len(tag) * 5 + 10
        stroke = k["stroke"]
        s.append(f'<rect x="{x + 8}" y="{y + 6}" width="{tw_}" height="12" rx="2" fill="transparent" '
                 f'stroke="{stroke}" stroke-opacity="0.45" stroke-width="0.8"/>')
        s.append(f'<text x="{x + 8 + tw_ / 2:g}" y="{y + 15}" fill="{MUTED}" font-size="7" '
                 f'font-family="{MONO}" text-anchor="middle" letter-spacing="0.08em">{esc(tag)}</text>')
    return "\n".join(s)


def zone(x, y, w, h, text, security=False):
    if security:
        z = (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="8" fill="rgba(235,108,54,0.03)" '
             f'stroke="rgba(235,108,54,0.50)" stroke-width="0.8" stroke-dasharray="4,4"/>')
    else:
        z = (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="8" fill="rgba(45,49,66,0.02)" '
             f'stroke="rgba(45,49,66,0.14)" stroke-width="0.8"/>')
    lw = len(text) * 6 + 16
    z += (f'<rect x="{x + 12}" y="{y - 6}" width="{lw}" height="12" rx="2" fill="{PAPER}"/>'
          f'<text x="{x + 12 + lw / 2:g}" y="{y + 3}" fill="{SOFT}" font-size="8" font-family="{MONO}" '
          f'text-anchor="middle" letter-spacing="0.14em">{esc(text)}</text>')
    return z


def actor(cx, top, name):
    s = [f'<g stroke="{INK}" stroke-width="1.2" fill="none" stroke-linecap="round">',
         f'<circle cx="{cx}" cy="{top + 10}" r="10" fill="{PAPER}"/>',
         f'<line x1="{cx}" y1="{top + 20}" x2="{cx}" y2="{top + 50}"/>',
         f'<line x1="{cx - 18}" y1="{top + 30}" x2="{cx + 18}" y2="{top + 30}"/>',
         f'<line x1="{cx}" y1="{top + 50}" x2="{cx - 12}" y2="{top + 70}"/>',
         f'<line x1="{cx}" y1="{top + 50}" x2="{cx + 12}" y2="{top + 70}"/>', '</g>',
         f'<text x="{cx}" y="{top + 90}" fill="{INK}" font-size="12" font-weight="600" '
         f'font-family="{SANS}" text-anchor="middle">{esc(name)}</text>']
    return "\n".join(s)


def usecase(cx, cy, rx, ry, name, focal=False):
    fill, stroke = ("rgba(235,108,54,0.08)", ACCENT) if focal else ("#ffffff", INK)
    return (f'<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" fill="{PAPER}"/>'
            f'<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" fill="{fill}" stroke="{stroke}" stroke-width="1"/>'
            f'<text x="{cx}" y="{cy + 4}" fill="{INK}" font-size="12" font-weight="600" '
            f'font-family="{SANS}" text-anchor="middle">{esc(name)}</text>')


def state(x, y, w, h, name, focal=False, sub=None):
    return box(x, y, w, h, name, sub=sub, kind="focal" if focal else "step", rx=8)


def start_dot(cx, cy):
    return f'<circle cx="{cx}" cy="{cy}" r="6" fill="{INK}"/>'


def end_dot(cx, cy):
    return (f'<circle cx="{cx}" cy="{cy}" r="8" fill="{PAPER}" stroke="{INK}" stroke-width="1.2"/>'
            f'<circle cx="{cx}" cy="{cy}" r="5" fill="{INK}"/>')


def entity(x, y, w, name, fields, focal=False):
    h = 28 + len(fields) * 14 + 10
    k = KIND["focal" if focal else "step"]
    s = [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" fill="{PAPER}"/>',
         f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" fill="{k["fill"]}" stroke="{k["stroke"]}" stroke-width="1"/>',
         f'<line x1="{x}" y1="{y + 28}" x2="{x + w}" y2="{y + 28}" stroke="{k["stroke"]}" stroke-opacity="0.35" stroke-width="0.8"/>',
         f'<text x="{x + 12}" y="{y + 12}" fill="{SOFT}" font-size="7" font-family="{MONO}" letter-spacing="0.12em">ENTITY</text>',
         f'<text x="{x + 12}" y="{y + 23}" fill="{INK}" font-size="12" font-weight="600" font-family="{SANS}">{esc(name)}</text>']
    for i, f in enumerate(fields):
        color = INK if f.startswith("#") else (LINK if f.startswith("→") else MUTED)
        s.append(f'<text x="{x + 12}" y="{y + 44 + i * 14}" fill="{color}" font-size="9" '
                 f'font-family="{MONO}">{esc(f)}</text>')
    return "\n".join(s), h


def legend(y, items, x0=40, step=None, width=None):
    """items: (kind, text); kind: node kinds, 'line', 'dashed', 'link', 'accentline', 'start', 'end', 'actor', 'ellipse'."""
    s = [f'<line x1="{x0 - 10}" y1="{y - 18}" x2="{width - 30}" y2="{y - 18}" stroke="rgba(45,49,66,0.10)" stroke-width="0.8"/>',
         f'<text x="{x0 - 10}" y="{y + 3}" fill="{MUTED}" font-size="8" font-family="{MONO}" letter-spacing="0.14em">LEJANT</text>']
    x = x0 + 50
    for kind, text in items:
        if kind in KIND:
            k = KIND[kind]
            dash = f' stroke-dasharray="{k["dash"]}"' if k["dash"] else ""
            s.append(f'<rect x="{x}" y="{y - 6}" width="18" height="12" rx="2" fill="{k["fill"]}" stroke="{k["stroke"]}" stroke-width="1"{dash}/>')
            gx = 24
        elif kind == "plain":
            s.append(f'<line x1="{x}" y1="{y}" x2="{x + 22}" y2="{y}" stroke="{MUTED}" stroke-width="1.2"/>')
            gx = 30
        elif kind in ("line", "dashed", "link", "accentline", "open"):
            col = {"line": MUTED, "dashed": MUTED, "link": LINK, "accentline": ACCENT, "open": MUTED}[kind]
            dash = ' stroke-dasharray="5,4"' if kind in ("dashed", "open") else ""
            mk = {"line": "arrow", "dashed": "arrow", "link": "arrow-link", "accentline": "arrow-accent", "open": "arrow-open"}[kind]
            s.append(f'<line x1="{x}" y1="{y}" x2="{x + 22}" y2="{y}" stroke="{col}" stroke-width="1.2"{dash} marker-end="url(#{mk})"/>')
            gx = 30
        elif kind == "zone":
            s.append(f'<rect x="{x}" y="{y - 6}" width="18" height="12" rx="2" fill="rgba(235,108,54,0.03)" stroke="rgba(235,108,54,0.50)" stroke-width="0.8" stroke-dasharray="4,4"/>')
            gx = 24
        elif kind == "start":
            s.append(start_dot(x + 6, y))
            gx = 16
        elif kind == "end":
            s.append(end_dot(x + 8, y))
            gx = 20
        elif kind == "ellipse":
            s.append(f'<ellipse cx="{x + 12}" cy="{y}" rx="12" ry="6" fill="#ffffff" stroke="{INK}" stroke-width="1"/>')
            gx = 30
        elif kind == "ellipse-focal":
            s.append(f'<ellipse cx="{x + 12}" cy="{y}" rx="12" ry="6" fill="rgba(235,108,54,0.08)" stroke="{ACCENT}" stroke-width="1"/>')
            gx = 30
        elif kind == "actor":
            s.append(f'<g stroke="{INK}" stroke-width="1" fill="none"><circle cx="{x + 6}" cy="{y - 4}" r="3"/>'
                     f'<line x1="{x + 6}" y1="{y - 1}" x2="{x + 6}" y2="{y + 5}"/><line x1="{x + 1}" y1="{y + 1}" x2="{x + 11}" y2="{y + 1}"/></g>')
            gx = 18
        s.append(f'<text x="{x + gx}" y="{y + 3}" fill="{MUTED}" font-size="9" font-family="{SANS}">{esc(text)}</text>')
        x += gx + int(len(text) * 5.4) + 26
    return "\n".join(s)


def page(slug, eyebrow, title, desc, w, h, body, dy=0):
    if dy:
        body = f'<g transform="translate(0,-{dy})">\n{body}\n</g>'
        h -= dy
    svg = f'''<svg viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="{slug}-title {slug}-desc">
      <title id="{slug}-title">{esc(title)}</title>
      <desc id="{slug}-desc">{esc(desc)}</desc>
      <defs>
        <marker id="arrow" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><polygon points="0 0, 8 3, 0 6" fill="{MUTED}"/></marker>
        <marker id="arrow-accent" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><polygon points="0 0, 8 3, 0 6" fill="{ACCENT}"/></marker>
        <marker id="arrow-link" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><polygon points="0 0, 8 3, 0 6" fill="{LINK}"/></marker>
        <marker id="arrow-open" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto"><polyline points="0 0, 8 3, 0 6" fill="none" stroke="{MUTED}" stroke-width="1.2"/></marker>
      </defs>
      <rect width="100%" height="100%" fill="{PAPER}"/>
{body}
    </svg>'''
    html = f'''<!DOCTYPE html>
<html lang="tr">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{esc(title)}</title>
  <link href="{FONT_LINK.replace('&', '&amp;')}" rel="stylesheet">
  <style>
    *, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
    :root {{ --color-paper: {PAPER}; --color-ink: {INK}; --color-muted: {MUTED};
             --font-sans: 'Geist', system-ui, sans-serif; --font-serif: 'Instrument Serif', 'Noto Serif', serif;
             --font-mono: 'Geist Mono', ui-monospace, monospace; }}
    body {{ font-family: var(--font-sans); background: var(--color-paper); color: var(--color-ink);
           min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 3rem 2rem; }}
    .frame {{ max-width: 1200px; width: 100%; }}
    .eyebrow {{ font-family: var(--font-mono); font-size: 0.66rem; font-weight: 500; letter-spacing: 0.18em;
               text-transform: uppercase; color: var(--color-muted); margin-bottom: 0.5rem; }}
    h1 {{ font-family: var(--font-serif); font-size: clamp(1.5rem, 2.4vw + 0.75rem, 2rem); font-weight: 400;
         letter-spacing: -0.02em; line-height: 1.15; color: var(--color-ink); margin-bottom: 1.5rem; }}
    svg {{ width: 100%; min-width: 760px; display: block; }}
  </style>
</head>
<body>
  <div class="frame">
    <p class="eyebrow">{esc(eyebrow)} · PLA SRS</p>
    <h1>{esc(title)}</h1>
    {svg}
  </div>
</body>
</html>
'''
    (OUT / f"{slug}.html").write_text(html, encoding="utf-8")
    print("yazildi:", slug)


# ------------------------------------------------------------------ diagrams

def d1_context():
    W, H = 800, 560
    b = []
    b.append(zone(184, 104, 576, 372, "KULLANICI CİHAZI · ÇEVRİMDIŞI", security=True))
    # connectors
    b.append(conn([(144, 272), (296, 272)]))
    b.append(conn([(396, 216), (396, 80)], color="link"))
    b.append(conn([(496, 248), (600, 248)]))
    b.append(conn([(496, 312), (600, 312)]))
    b.append(conn([(672, 176), (672, 224)], dashed=True))
    b.append(conn([(396, 328), (396, 388)]))
    b.append(lab_above(144, 296, 272, "ETKİLEŞİM"))
    b.append(lab_side(396, 104, 200, "HTTPS İNDİRME"))
    b.append(lab_above(496, 600, 248, "OKU / YAZ"))
    b.append(lab_above(496, 600, 312, "SQLITE"))
    b.append(lab_side(672, 176, 224, "AYNI .md"))
    b.append(lab_side(396, 328, 388, "BİLDİRİM · TEPSİ"))
    # nodes
    b.append(box(40, 244, 104, 56, "Kullanıcı", sub="tek kullanıcı", kind="input"))
    b.append(box(296, 216, 200, 112, "PLA", sub="Tauri · Svelte · Rust\nllama.cpp (yerel)", tag="SİSTEM", kind="focal"))
    b.append(box(296, 24, 200, 56, "Model deposu", sub="huggingface.co", kind="external"))
    b.append(box(296, 388, 200, 56, "Windows 10/11", sub="bildirim · güç · çöp kutusu", kind="external"))
    b.append(box(600, 128, 144, 48, "Obsidian", sub="opsiyonel editör", kind="optional"))
    b.append(box(600, 224, 144, 48, "Vault", sub="Belgeler/PLA Vault", kind="store"))
    b.append(box(600, 288, 144, 48, "Uygulama verisi", sub="%APPDATA%/PLA", kind="store"))
    b.append(legend(528, [("focal", "Sistem"), ("input", "Kullanıcı"), ("store", "Veri deposu"),
                          ("external", "Dış sistem"), ("optional", "Opsiyonel"), ("link", "Ağ (HTTPS)"),
                          ("zone", "Cihaz sınırı")], width=W))
    page("pla-baglam", "Bağlam diyagramı", "PLA sistem bağlamı",
         "PLA'nın kullanıcı, Windows, vault klasörü, uygulama verisi, isteğe bağlı Obsidian ve yalnız model indirmek için erişilen model deposuyla ilişkisi.",
         W, H, "\n".join(b))


def d2_components():
    W, H = 800, 480
    b = []
    b.append(zone(48, 16, 704, 88, "SUNUM · WEBVIEW2"))
    b.append(zone(40, 132, 720, 104, "RUST ÇEKİRDEK"))
    b.append(zone(40, 300, 552, 96, "VERİ"))
    # connectors
    b.append(conn([(376, 88), (376, 116), (168, 116), (168, 160)]))
    b.append(conn([(424, 88), (424, 116), (492, 116), (492, 160)]))
    b.append(conn([(192, 188), (240, 188)]))
    b.append(conn([(608, 188), (560, 188)]))
    b.append(conn([(124, 216), (124, 324)]))
    b.append(conn([(308, 216), (308, 324)]))
    b.append(f'<path d="M 472,216 V 268 a 8,8 0 0,0 0,16 V 324" fill="none" stroke="{MUTED}" stroke-width="1.2" marker-end="url(#arrow)"/>')
    # extraction -> llama (hop over nothing) ; qa -> llama
    b.append(conn([(340, 216), (340, 276), (652, 276), (652, 324)], color="accent"))
    b.append(conn([(512, 216), (512, 260), (684, 260), (684, 324)]))
    b.append(label(272, 108, "IPC"))
    b.append(label(458, 108, "IPC"))
    b.append(label(216, 178, "KUYRUK", size=8))
    b.append(label(584, 178, "BAKIM", size=8))
    b.append(lab_side(124, 216, 324, "OKU / YAZ", side="right"))
    b.append(label(296, 294, "KAYIT", anchor="end"))
    b.append(label(460, 256, "ARA", anchor="end"))
    b.append(label(400, 292, "ÇIKARIM İSTEĞİ"))
    b.append(lab_above(560, 684, 260, "SORU · ARAÇ"))
    # nodes
    b.append(box(280, 40, 240, 48, "Svelte arayüzü", sub="CodeMirror · paneller · tepsi"))
    b.append(box(56, 160, 136, 56, "Vault ve indeks", sub="dosya · FTS"))
    b.append(box(240, 160, 136, 56, "Çıkarım hattı", sub="blok · doğrulama", kind="focal"))
    b.append(box(424, 160, 136, 56, "Soru-cevap", sub="RAG · araçlar · emb."))
    b.append(box(608, 160, 136, 56, "Zamanlayıcı", sub="bakım · rapor · telafi"))
    b.append(box(56, 324, 136, 48, "Vault (.md)", sub="notlar · raporlar", kind="store"))
    b.append(box(240, 324, 136, 48, "pla.db", sub="kullanıcı verisi", kind="store"))
    b.append(box(424, 324, 136, 48, "cache.db", sub="FTS · vektör · özet", kind="store"))
    b.append(box(608, 324, 136, 48, "llama-server", sub="127.0.0.1 · alt süreç", kind="external"))
    b.append(legend(444, [("focal", "Odak modül"), ("step", "Modül"), ("store", "Veri deposu"),
                          ("external", "Ayrı süreç"), ("accentline", "Çıkarım isteği")], width=W))
    page("pla-bilesenler", "Mimari diyagram", "PLA bileşen mimarisi",
         "Svelte arayüzü, dört Rust çekirdek modülü, üç veri deposu ve ayrı süreç olarak çalışan llama-server arasındaki bağlantılar.",
         W, H, "\n".join(b))


def d3_usecase():
    W, H = 800, 680
    b = []
    cx, rx, ry = 400, 136, 20
    ucs = [
        (80, "Kurulum ve model yönetimi", False),
        (140, "Not yaz ve düzenle", False),
        (200, "Otomatik veri çıkarımı", True),
        (260, "Notlarda ara ve gezin", False),
        (320, "Görev ve hatırlatıcı yönet", False),
        (380, "Metrik kaydet ve izle", False),
        (440, "Yedekle · geri yükle · dışa aktar", False),
        (500, "Asistana sor / komut ver", False),
        (560, "Günlük bakım ve haftalık rapor", False),
    ]
    left = cx - rx
    b.append(zone(248, 44, 304, 556, "PLA SİSTEMİ"))
    # Kullanici baglantilari (attach x=104)
    ax = 114
    ups = [(284, 80, 144), (296, 140, 160), (308, 260, 176)]
    downs = [(332, 380, 176), (344, 440, 160), (356, 500, 144)]
    for a, t, v in ups + downs:
        b.append(conn([(ax, a), (v, a), (v, t), (left, t)], arrow=False))
    b.append(conn([(ax, 320), (left, 320)], arrow=False))
    # extend: cikarim -> not yaz
    b.append(conn([(cx, 180), (cx, 160)], dashed=True))
    b.append(label(cx + 13, 173, "«extend»", anchor="start", size=8))
    # Dil modeli (kutu x=632..760, y=352..408) attach 364->200, 384->500, 400->560
    right = cx + rx
    b.append(conn([(632, 364), (584, 364), (584, 200), (right, 200)], arrow=False))
    b.append(conn([(632, 384), (576, 384), (576, 500), (right, 500)], arrow=False))
    e552 = int(cx + rx * (1 - (8 / ry) ** 2) ** 0.5)
    b.append(conn([(632, 400), (600, 400), (600, 552), (e552, 552)], arrow=False))
    b.append(conn([(632, 568), (e552, 568)], arrow=False))
    # nodes
    b.append(actor(76, 264, "Kullanıcı"))
    for y, name, focal in ucs:
        b.append(usecase(cx, y, rx, ry, name, focal))
    b.append(box(632, 352, 128, 56, "Yerel dil modeli", sub="llama-server", tag="SİSTEM", kind="external"))
    b.append(box(632, 544, 128, 48, "Zamanlayıcı", sub="zaman olayı", kind="external"))
    b.append(legend(644, [("actor", "Birincil aktör"), ("external", "İkincil aktör"), ("ellipse", "Kullanım senaryosu"),
                          ("ellipse-focal", "Odak senaryo"), ("dashed", "«extend»")], width=W))
    page("pla-use-case", "Kullanım senaryoları", "PLA kullanım senaryoları",
         "Kullanıcının yedi kullanım senaryosu; otomatik çıkarım not yazmayı genişletir, yerel dil modeli çıkarım, soru-cevap ve bakıma katılır, zamanlayıcı günlük bakımı tetikler.",
         W, H, "\n".join(b))


def seq_page(slug, eyebrow, title, desc, W, H, actors, msgs, frame=None, bars=(), legend_items=None):
    """actors: [(x, name, sub, kind)], msgs: list of dict(kind, frm, to, y, text)."""
    b = []
    top, bottom = 72, H - 90
    xs = {i: a[0] for i, a in enumerate(actors)}
    if frame:
        fx1, fy, fx2, fh, op, guards, divider, gx = frame
        b.append(f'<rect x="{fx1}" y="{fy}" width="{fx2 - fx1}" height="{fh}" rx="4" fill="rgba(45,49,66,0.02)" stroke="rgba(45,49,66,0.22)" stroke-width="1"/>')
        b.append(f'<rect x="{fx1}" y="{fy}" width="40" height="16" rx="2" fill="{PAPER}" stroke="rgba(45,49,66,0.22)" stroke-width="1"/>')
        b.append(f'<text x="{fx1 + 20}" y="{fy + 12}" fill="{MUTED}" font-size="8" font-family="{MONO}" text-anchor="middle" letter-spacing="0.12em">{op}</text>')
        for gy, gt in guards:
            b.append(f'<text x="{gx}" y="{gy}" fill="{MUTED}" font-size="9" font-family="{MONO}" letter-spacing="0.04em">{esc(gt)}</text>')
        if divider:
            b.append(f'<line x1="{fx1 + 8}" y1="{divider}" x2="{fx2 - 8}" y2="{divider}" stroke="rgba(45,49,66,0.20)" stroke-width="1" stroke-dasharray="4,3"/>')
    for i, (x, name, sub, kind) in enumerate(actors):
        b.append(f'<line x1="{x}" y1="{top}" x2="{x}" y2="{bottom}" stroke="rgba(45,49,66,0.20)" stroke-width="1" stroke-dasharray="3,3"/>')
    for i, y1, y2 in bars:
        x = xs[i]
        b.append(f'<rect x="{x - 4}" y="{y1}" width="8" height="{y2 - y1}" fill="rgba(45,49,66,0.06)" stroke="{MUTED}" stroke-width="0.8"/>')
    for m in msgs:
        k, f, t, y, text = m["kind"], m["frm"], m["to"], m["y"], m["text"]
        x1, x2 = xs[f], xs[t]
        if f == t:  # self message
            b.append(conn([(x1 + 4, y), (x1 + 36, y), (x1 + 36, y + 20), (x1 + 6, y + 20)]))
            b.append(label(x1 + 44, y + 14, text, anchor="start"))
            continue
        d = 1 if x2 > x1 else -1
        sx, ex = x1 + 4 * d, x2 - 4 * d
        if k == "call":
            b.append(conn([(sx, y), (ex, y)], color=m.get("color", "muted")))
        elif k == "ret":
            b.append(conn([(sx, y), (ex, y)], dashed=True, color=m.get("color", "muted")))
        elif k == "async":
            b.append(conn([(sx, y), (ex, y)], dashed=True, open_head=True))
        elif k == "head":
            b.append(conn([(sx, y), (ex, y)], color="accent"))
        b.append(label((x1 + x2) / 2, y - 9, text))
    for i, (x, name, sub, kind) in enumerate(actors):
        b.append(box(x - 72, 20, 144, 52, name, sub=sub, kind=kind))
    if legend_items:
        b.append(legend(H - 36, legend_items, width=W))
    page(slug, eyebrow, title, desc, W, H, "\n".join(b))


def d4_extraction_seq():
    W, H = 900, 720
    A = [(96, "Kullanıcı", None, "input"), (276, "Editör", "Svelte", "step"),
         (456, "Çıkarım hattı", "Rust", "focal"), (636, "llama-server", "alt süreç", "external"),
         (816, "pla.db", "SQLite", "store")]
    M = [
        dict(kind="call", frm=0, to=1, y=116, text="NOTU KAPATIR"),
        dict(kind="call", frm=1, to=2, y=152, text="IPC note_closed"),
        dict(kind="call", frm=2, to=2, y=184, text="BLOK EŞLEŞTİR · SEÇ"),
        dict(kind="call", frm=2, to=4, y=240, text="REDDEDİLENLERİ OKU"),
        dict(kind="ret", frm=4, to=2, y=268, text="İMZALAR"),
        dict(kind="call", frm=2, to=3, y=304, text="BLOK + TARİH + ŞEMA", color="link"),
        dict(kind="ret", frm=3, to=2, y=340, text="JSON (GRAMER)"),
        dict(kind="call", frm=2, to=2, y=372, text="MUTLAK TARİH · DOĞRULA"),
        dict(kind="call", frm=2, to=4, y=460, text="ÖĞE EKLE / GÜNCELLE"),
        dict(kind="head", frm=2, to=1, y=500, text="BİLDİRİM + GERİ AL"),
        dict(kind="call", frm=2, to=4, y=572, text="İNCELEME KUTUSU"),
        dict(kind="ret", frm=1, to=0, y=624, text="ROZETLİ ÖĞE"),
    ]
    frame = (196, 416, 872, 184, "ALT", [(440, "[doğrulandı]"), (548, "[doğrulanmadı]")], 528, 292)
    bars = [(1, 112, 156), (2, 152, 600), (3, 300, 344), (4, 236, 272), (4, 456, 464), (4, 568, 576), (1, 496, 628)]
    seq_page("pla-cikarim-sirasi", "Sıralama diyagramı", "Otonom çıkarım akışı",
             "Kullanıcı notu kapattığında değişen blokların eşleştirilip yerel modele gönderilmesi, dönen JSON'un doğrulanması ve öğenin eklenmesi ya da İnceleme kutusuna düşmesi.",
             W, H, A, M, frame, bars,
             legend_items=[("line", "Çağrı"), ("dashed", "Dönüş"), ("link", "Modele istek"), ("accentline", "Ana sonuç")])


def d5_qa_seq():
    W, H = 900, 720
    A = [(96, "Kullanıcı", None, "input"), (276, "Asistan paneli", "Svelte", "step"),
         (456, "Rust çekirdek", "RAG · araçlar", "focal"), (636, "Veri", "cache.db · pla.db", "store"),
         (816, "llama-server", "alt süreç", "external")]
    M = [
        dict(kind="call", frm=0, to=1, y=116, text="SORU"),
        dict(kind="call", frm=1, to=2, y=148, text="IPC qa_ask"),
        dict(kind="call", frm=2, to=3, y=184, text="HİBRİT ARAMA"),
        dict(kind="ret", frm=3, to=2, y=212, text="PARÇALAR"),
        dict(kind="call", frm=2, to=4, y=248, text="BAĞLAM + ARAÇLAR", color="link"),
        dict(kind="ret", frm=4, to=2, y=336, text="query_metrics()"),
        dict(kind="call", frm=2, to=3, y=368, text="SQL HESAPLA"),
        dict(kind="ret", frm=3, to=2, y=396, text="ORTALAMA 6,8 SA"),
        dict(kind="call", frm=2, to=4, y=432, text="ARAÇ SONUCU", color="link"),
        dict(kind="ret", frm=4, to=2, y=496, text="YANIT AKIŞI"),
        dict(kind="head", frm=2, to=1, y=532, text="TOKEN + KAYNAKLAR"),
        dict(kind="ret", frm=1, to=0, y=572, text="YANIT + BAĞLANTI"),
    ]
    frame = (376, 284, 872, 172, "OPT", [(304, "[model araç çağırırsa]")], None, 472)
    bars = [(1, 112, 152), (2, 144, 536), (3, 180, 216), (3, 364, 400), (4, 244, 340), (4, 428, 500), (1, 528, 576)]
    seq_page("pla-soru-cevap-sirasi", "Sıralama diyagramı", "Soru-cevap ve araç çağırma akışı",
             "Sorunun hibrit aramayla bağlama dönüştürülmesi, modelin gerekirse SQL ile hesaplanan bir aracı çağırması ve yanıtın ham not bağlantılarıyla akışla gösterilmesi.",
             W, H, A, M, frame, bars,
             legend_items=[("line", "Çağrı"), ("dashed", "Dönüş"), ("link", "Modele istek"), ("accentline", "Ana sonuç")])


def d6_item_states():
    W, H = 880, 500
    b = []
    # transitions
    b.append(conn([(54, 200), (96, 200)]))
    b.append(conn([(256, 200), (352, 200)]))
    b.append(conn([(176, 224), (176, 336)]))
    b.append(conn([(256, 360), (432, 360), (432, 224)]))
    b.append(conn([(176, 384), (176, 408), (704, 408), (704, 384)]))
    b.append(conn([(408, 176), (408, 152), (456, 152), (456, 176)]))
    b.append(conn([(512, 188), (568, 188), (568, 120), (624, 120)]))
    b.append(conn([(512, 212), (584, 212), (584, 280), (624, 280)]))
    b.append(conn([(480, 224), (480, 348), (624, 348)]))
    b.append(conn([(784, 360), (812, 360)]))
    b.append(label(304, 189, "GEÇERLİ"))
    b.append(lab_side(176, 224, 336, "GEÇERSİZ"))
    b.append(lab_above(256, 432, 360, "ONAYLANDI"))
    b.append(lab_above(176, 704, 408, "REDDEDİLDİ"))
    b.append(label(432, 141, "GÜNCELLENDİ"))
    b.append(label(560, 157, "DÜZENLENDİ", anchor="end"))
    b.append(label(576, 250, "BLOK YOK", anchor="end"))
    b.append(lab_above(480, 624, 348, "GERİ AL · SİL"))
    # states
    b.append(start_dot(48, 200))
    b.append(state(96, 176, 160, 48, "Doğrulanıyor", sub="şema + kurallar"))
    b.append(state(352, 176, 160, 48, "Eklendi (AI)", focal=True, sub="origin=extracted"))
    b.append(state(624, 96, 160, 48, "Kullanıcıya ait", sub="AI dokunmaz"))
    b.append(state(624, 256, 160, 48, "Kaynağı silindi", sub="öğe korunur"))
    b.append(state(96, 336, 160, 48, "İnceleme kutusu", sub="kullanıcı kararı"))
    b.append(state(624, 336, 160, 48, "Reddedildi", sub="imza kaydedildi"))
    b.append(end_dot(820, 360))
    b.append(legend(464, [("start", "Başlangıç"), ("step", "Durum"), ("focal", "Odak durum"), ("end", "Son durum")], width=W))
    page("pla-oge-durumlari", "Durum diyagramı", "Çıkarılan öğenin yaşam döngüsü",
         "Otomatik çıkarılan bir öğenin doğrulama, ekleme, güncelleme, kullanıcıya geçme, reddedilme ve kaynağının silinmesi durumları arasındaki geçişler.",
         W, H, "\n".join(b), dy=48)


def d7_model_states():
    W, H = 800, 480
    b = []
    b.append(conn([(54, 200), (96, 200)]))
    b.append(conn([(240, 200), (320, 200)]))
    b.append(conn([(480, 200), (560, 200)]))
    b.append(conn([(376, 224), (376, 312)]))
    b.append(conn([(424, 312), (424, 224)]))
    b.append(conn([(616, 176), (616, 88)]))
    b.append(conn([(664, 88), (664, 176)]))
    b.append(conn([(640, 224), (640, 312)]))
    b.append(conn([(640, 360), (640, 400), (152, 400), (152, 224)]))
    b.append(conn([(320, 336), (184, 336), (184, 224)]))
    b.append(lab_above(240, 320, 200, "AI İSTEĞİ"))
    b.append(lab_above(480, 560, 200, "SAĞLIK OK"))
    b.append(lab_side(376, 224, 312, "30 SN · ÇÖKME", side="left"))
    b.append(lab_side(424, 224, 312, "1 KEZ TEKRAR"))
    b.append(lab_side(616, 88, 176, "İSTEK", side="left"))
    b.append(lab_side(664, 88, 176, "YANIT"))
    b.append(lab_side(640, 224, 312, "60 SN BOŞTA"))
    b.append(lab_above(152, 640, 400, "SÜREÇ BİTTİ"))
    b.append(lab_above(184, 320, 336, "2. HATA"))
    b.append(start_dot(48, 200))
    b.append(state(96, 176, 144, 48, "Kapalı", sub="RAM boş"))
    b.append(state(320, 176, 160, 48, "Başlatılıyor", sub="port + API anahtarı"))
    b.append(state(560, 176, 160, 48, "Hazır (sıcak)", focal=True, sub="≤ 2 GB"))
    b.append(state(560, 40, 160, 48, "İstek işleniyor"))
    b.append(state(560, 312, 160, 48, "Kapatılıyor"))
    b.append(state(320, 312, 160, 48, "Hata", sub="işler bekletilir"))
    b.append(legend(444, [("start", "Başlangıç"), ("step", "Durum"), ("focal", "Odak durum")], width=W))
    page("pla-model-sureci", "Durum diyagramı", "Model sürecinin yaşam döngüsü",
         "llama-server alt sürecinin isteğe bağlı başlatılması, 60 saniye boşta kalınca kapatılması ve başlatma hatasında bir kez yeniden denenmesi.",
         W, H, "\n".join(b))


def d8_er():
    W, H = 960, 480
    b = []
    blk, bh = entity(380, 48, 200, "block", ["# block_id", "note_path", "position", "text_hash", "first_seen_at", "last_seen_at"], focal=True)
    tsk, th = entity(40, 48, 200, "task", ["# task_id", "title", "details", "date · time", "notify_at", "status", "origin", "user_modified", "→ block_id"])
    met, mh = entity(720, 48, 200, "metric_record", ["# metric_id", "type", "value_json · unit", "date", "origin", "user_modified", "→ block_id"])
    rej, _ = entity(40, 280, 184, "rejection", ["# rejection_id", "→ block_id", "item_signature", "rejected_at"])
    rev, _ = entity(272, 280, 184, "review_item", ["# review_id", "→ block_id", "payload_json", "reason", "created_at"])
    job, _ = entity(504, 280, 184, "job_run", ["# job_type", "last_success_at", "last_attempt_at", "last_error"])
    qa, _ = entity(736, 280, 184, "qa_turn", ["# turn_id", "question", "answer", "tool_calls_json", "created_at"])
    b.append(conn([(380, 100), (240, 100)], arrow=False))
    b.append(conn([(580, 100), (720, 100)], arrow=False))
    b.append(conn([(420, 172), (420, 240), (132, 240), (132, 280)], arrow=False))
    b.append(conn([(452, 172), (452, 256), (364, 256), (364, 280)], arrow=False))
    for x, y, t in [(366, 94, "1"), (252, 94, "N"), (594, 94, "1"), (708, 94, "N"),
                    (428, 186, "1"), (140, 274, "N"), (460, 186, "1"), (372, 274, "N")]:
        b.append(f'<text x="{x}" y="{y}" fill="{MUTED}" font-size="9" font-family="{MONO}" text-anchor="middle">{t}</text>')
    b.append(label(310, 89, "kaynak", size=8))
    b.append(label(650, 89, "kaynak", size=8))
    b += [blk, tsk, met, rej, rev, job, qa]
    b.append(legend(448, [("focal", "Merkez varlık"), ("step", "Varlık"), ("plain", "1 — N ilişki")], width=W))
    page("pla-veri-modeli", "ER diyagramı", "pla.db mantıksal veri modeli",
         "Kullanıcı verisini tutan pla.db'de blok varlığının görev, metrik, ret ve inceleme kayıtlarıyla bire-çok ilişkisi; iş kayıtları ve soru-cevap geçmişi bağımsız tablolardır.",
         W, H, "\n".join(b))


def d9_dfd():
    W, H = 920, 640
    b = []
    b.append(conn([(100, 276), (100, 116), (240, 116)]))
    b.append(conn([(400, 112), (480, 112)]))
    b.append(conn([(640, 112), (720, 112)]))
    b.append(conn([(800, 144), (800, 276)]))
    b.append(conn([(116, 332), (116, 480), (240, 480)]))
    b.append(conn([(240, 504), (84, 504), (84, 332)]))
    b.append(conn([(480, 488), (400, 488)]))
    b.append(conn([(800, 324), (800, 552), (320, 552), (320, 520)]))
    b.append(conn([(560, 136), (560, 276)]))
    b.append(conn([(560, 332), (560, 464)]))
    b.append(conn([(600, 276), (600, 136)]))
    b.append(lab_above(100, 240, 116, "NOT METNİ"))
    b.append(lab_above(400, 480, 112, "YAZ"))
    b.append(lab_above(640, 720, 112, "BLOKLAR"))
    b.append(lab_side(800, 144, 276, "GÖREV · METRİK"))
    b.append(lab_above(116, 240, 480, "SORU"))
    b.append(label(162, 521, "YANIT + KAYNAK"))
    b.append(lab_above(400, 480, 488, "PARÇALAR"))
    b.append(lab_above(320, 800, 552, "İSTATİSTİK"))
    b.append(lab_side(560, 136, 276, "NOTLAR", side="left"))
    b.append(lab_side(560, 332, 464, "ÖZET · VEKTÖR"))
    b.append(lab_side(600, 136, 276, "RAPOR"))
    b.append(box(40, 276, 120, 56, "Kullanıcı", tag="DIŞ", kind="input"))
    b.append(box(240, 88, 160, 56, "1 · Not yönetimi", sub="editör · vault", tag="SÜREÇ"))
    b.append(box(720, 88, 160, 56, "2 · Otonom çıkarım", sub="blok → öğe", tag="SÜREÇ", kind="focal"))
    b.append(box(240, 464, 160, 56, "3 · Soru-cevap", sub="RAG · araçlar", tag="SÜREÇ"))
    b.append(box(480, 276, 160, 56, "4 · Bakım ve rapor", sub="günlük · haftalık", tag="SÜREÇ"))
    b.append(box(480, 88, 160, 48, "D1 · Vault", sub=".md dosyaları", kind="store"))
    b.append(box(720, 276, 160, 48, "D2 · pla.db", sub="görev · metrik", kind="store"))
    b.append(box(480, 464, 160, 48, "D3 · cache.db", sub="FTS · vektör · özet", kind="store"))
    b.append(legend(604, [("input", "Dış varlık"), ("step", "Süreç"), ("focal", "Odak süreç"), ("store", "Veri deposu"),
                          ("line", "Veri akışı")], width=W))
    page("pla-veri-akisi", "Veri akış diyagramı", "PLA veri akış diyagramı (düzey 1)",
         "Kullanıcı ile dört süreç (not yönetimi, otonom çıkarım, soru-cevap, bakım ve rapor) ve üç veri deposu arasında akan veriler.",
         W, H, "\n".join(b), dy=56)


if __name__ == "__main__":
    d1_context()
    d2_components()
    d3_usecase()
    d4_extraction_seq()
    d5_qa_seq()
    d6_item_states()
    d7_model_states()
    d8_er()
    d9_dfd()
