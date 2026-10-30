# Bundled PDFium

This folder ships with PLA as a Tauri resource (`tauri.conf.json > bundle.resources`).
PDF import reads PDFs with it. Its files are not in git; fill it with:

    cd apps/desktop
    npm run fetch-pdfium                          # Windows x64, what the installer ships
    npm run fetch-pdfium -- --platform linux-x64  # for development on Linux

The script downloads the pinned build of bblanchon/pdfium-binaries and checks its SHA-256
(`scripts/release-lib.mjs`). Without it PLA still builds and runs; only PDF import is unavailable.
