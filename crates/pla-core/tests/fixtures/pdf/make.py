"""Regenerates the PDF import fixtures from their HTML with Chromium (Playwright):
    python3 crates/pla-core/tests/fixtures/pdf/make.py
ders.pdf: lecture notes with headings, code, lists, running header and page numbers, an outline.
slayt.pdf: a slide deck whose bullets are drawn. tarama.pdf: a page with no text, like a scan."""
from pathlib import Path
from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_page()
    page.goto((HERE / "ders.html").as_uri())
    page.pdf(path=HERE / "ders.pdf", format="A4", margin={"top": "22mm", "bottom": "20mm", "left": "20mm", "right": "20mm"},
             display_header_footer=True, outline=True, tagged=True,
             header_template='<div style="font-size:8pt;width:100%;text-align:center;color:#777">Nesne Yönelimli Programlama – Ders Notları</div>',
             footer_template='<div style="font-size:8pt;width:100%;text-align:center;color:#777">Sayfa <span class="pageNumber"></span> / <span class="totalPages"></span></div>')
    page.goto((HERE / "slayt.html").as_uri())
    page.pdf(path=HERE / "slayt.pdf", prefer_css_page_size=True, print_background=True)
    page.goto((HERE / "tarama.html").as_uri())
    page.pdf(path=HERE / "tarama.pdf", format="A4", print_background=True)
    browser.close()
