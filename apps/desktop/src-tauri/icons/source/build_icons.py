"""Builds PLA's app icons from the logo SVGs (design spec § 8).

Run from apps/desktop/src-tauri with the miniconda Python (Pillow + Playwright):
    C:\\Users\\ayber\\miniconda3\\python.exe icons/source/build_icons.py
"""
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image
from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
ICONS = HERE.parent
NPX = "npx.cmd" if shutil.which("npx.cmd") else "npx"


def render(svg: Path, png: Path, size: int) -> None:
    markup = svg.read_text(encoding="utf-8").replace("<svg ", f'<svg width="{size}" height="{size}" ', 1)
    with sync_playwright() as p:
        browser = p.chromium.launch()
        page = browser.new_page(viewport={"width": size, "height": size})
        page.set_content(f'<html><body style="margin:0;background:transparent">{markup}</body></html>')
        page.screenshot(path=str(png), omit_background=True, clip={"x": 0, "y": 0, "width": size, "height": size})
        browser.close()


def tauri_icon(png: Path, out: Path) -> None:
    subprocess.run([NPX, "tauri", "icon", str(png), "--output", str(out)], check=True, cwd=ICONS.parent)
    for mobile in ("android", "ios"):
        shutil.rmtree(out / mobile, ignore_errors=True)


def ico_frames(ico: Path) -> dict[int, Image.Image]:
    im = Image.open(ico)
    frames = {}
    for size in im.info["sizes"]:
        im.size = size
        im.load()
        frames[size[0]] = im.copy().convert("RGBA")
    return frames


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        render(HERE / "logo.svg", tmp / "logo.png", 1024)
        render(HERE / "logo-16.svg", tmp / "logo-16.png", 1024)
        tauri_icon(tmp / "logo.png", ICONS)  # full set: PNGs, icon.icns, icon.ico
        small_dir = tmp / "small"
        tauri_icon(tmp / "logo-16.png", small_dir)

        shutil.copyfile(small_dir / "32x32.png", ICONS / "32x32.png")
        shutil.copyfile(small_dir / "32x32.png", ICONS / "tray.png")

        small, full = ico_frames(small_dir / "icon.ico"), ico_frames(ICONS / "icon.ico")
        frames = [small[s] for s in sorted(small) if s <= 32] + [full[s] for s in sorted(full) if s > 32]
        frames[-1].save(ICONS / "icon.ico", format="ICO", sizes=[f.size for f in frames], append_images=frames[:-1])
    print("icons written to", ICONS)


if __name__ == "__main__":
    main()
