"""Klasordeki diyagram HTML'lerini PNG'ye cevirir (yalniz <svg> alani).

Calistirma: C:\\Users\\ayber\\miniconda3\\python.exe export_png.py [olcek]
"""
import pathlib
import sys

from playwright.sync_api import sync_playwright

HERE = pathlib.Path(__file__).parent
scale = float(sys.argv[1]) if len(sys.argv) > 1 else 2.5

with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_page(device_scale_factor=scale, viewport={"width": 1400, "height": 1000})
    for src in sorted(HERE.glob("pla-*.html")):
        page.goto(src.resolve().as_uri())
        page.wait_for_load_state("networkidle")
        page.evaluate("document.fonts.ready")
        out = src.with_suffix(".png")
        page.locator("svg").first.screenshot(path=str(out), omit_background=False)
        print("png:", out.name)
    browser.close()
