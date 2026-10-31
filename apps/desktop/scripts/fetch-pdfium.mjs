// Fills src-tauri/pdfium/ with the PDFium library PDF import reads PDFs with, and its licences.
// Usage: npm run fetch-pdfium [-- --platform linux-x64]   (default win-x64, the platform PLA ships for)
// Downloads the pinned bblanchon/pdfium-binaries build and refuses a file whose SHA-256 differs.
import { createHash } from "node:crypto";
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PDFIUM_ASSETS, PDFIUM_BUILD } from "./release-lib.mjs";

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const flag = process.argv.indexOf("--platform");
const platform = flag > 0 ? process.argv[flag + 1] : "win-x64";
const asset = PDFIUM_ASSETS[platform];
if (!asset) {
  console.error(`Unknown platform ${platform}. Known: ${Object.keys(PDFIUM_ASSETS).join(", ")}`);
  process.exit(1);
}

const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/${PDFIUM_BUILD}/pdfium-${platform}.tgz`;
const response = await fetch(url);
if (!response.ok) {
  console.error(`Download failed (${response.status}): ${url}`);
  process.exit(1);
}
const bytes = Buffer.from(await response.arrayBuffer());
const digest = createHash("sha256").update(bytes).digest("hex");
if (digest !== asset.sha256) {
  console.error(`pdfium-${platform}.tgz has SHA-256 ${digest}, expected ${asset.sha256}. Not using it.`);
  process.exit(1);
}

const work = mkdtempSync(join(tmpdir(), "pla-pdfium-"));
try {
  const archive = join(work, "pdfium.tgz");
  writeFileSync(archive, bytes);
  const untar = spawnSync("tar", ["xzf", archive, "-C", work], { encoding: "utf8" });
  if (untar.status !== 0) {
    console.error(`tar could not unpack the archive:\n${untar.stderr}`);
    process.exit(1);
  }
  const target = join(desktop, "src-tauri", "pdfium");
  mkdirSync(target, { recursive: true });
  // Keep the committed README; replace everything else.
  for (const f of readdirSync(target)) if (f !== "README.md") rmSync(join(target, f), { recursive: true, force: true });
  copyFileSync(join(work, asset.lib), join(target, basename(asset.lib)));
  copyFileSync(join(work, "LICENSE"), join(target, "LICENSE"));
  cpSync(join(work, "licenses"), join(target, "licenses"), { recursive: true });
  // `tauri dev` runs target/debug/pla-desktop; the app also looks for pdfium/ next to the exe.
  const devDir = resolve(desktop, "../../target/debug");
  if (existsSync(devDir)) {
    rmSync(join(devDir, "pdfium"), { recursive: true, force: true });
    cpSync(target, join(devDir, "pdfium"), { recursive: true, filter: (f) => basename(f) !== "README.md" });
  }
  console.log(`PDFium ${PDFIUM_BUILD} (${platform}) in ${target}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
