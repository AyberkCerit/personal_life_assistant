// The checks `npm run release` makes before it builds the installer (packaging plan § 5).

// Measured: llama-server does not start without any of these (model-manager plan Task 3), plus at
// least one ggml-cpu-*.dll for the processor. fetch-llama copies them from the llama.cpp build.
export const LLAMA_FILES = ["llama-server.exe", "llama-server-impl.dll", "llama.dll", "llama-common.dll", "ggml.dll", "ggml-base.dll", "mtmd.dll", "libomp.dll", "LICENSE-LLVM-OpenMP"];
export const LLAMA_BUILD = "11280";
/** What src-tauri/llama/ must hold before it ships: the files above and llama.cpp's MIT licence. */
export const LLAMA_REQUIRED = [...LLAMA_FILES, "LICENSE-llama.cpp.txt"];

const isCpuDll = (f) => /^ggml-cpu-.*\.dll$/.test(f);

/** The llama-server files missing from `files` (the names in src-tauri/llama/). */
export function llamaProblems(files) {
  const missing = LLAMA_REQUIRED.filter((f) => !files.includes(f));
  if (!files.some(isCpuDll)) missing.push("ggml-cpu-*.dll");
  return missing;
}

/** Files in src-tauri/llama/ that are not llama-server's: the whole folder goes into the installer. */
export function strayFiles(files) {
  return files.filter((f) => f !== "README.md" && !LLAMA_REQUIRED.includes(f) && !isCpuDll(f));
}

// PDF import reads PDFs with PDFium (Chromium's PDF engine), built by bblanchon/pdfium-binaries.
// The build matches pdfium-render's `pdfium_latest` bindings; the digests are GitHub's for each asset.
export const PDFIUM_BUILD = "7881";
export const PDFIUM_ASSETS = {
  "win-x64": { lib: "bin/pdfium.dll", sha256: "73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08" },
  "linux-x64": { lib: "lib/libpdfium.so", sha256: "1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d" },
};
/** What src-tauri/pdfium/ must hold before the Windows installer ships: the library and its licences. */
export const PDFIUM_REQUIRED = ["pdfium.dll", "LICENSE", "licenses"];

/** The PDFium files missing from `files` (the names in src-tauri/pdfium/). */
export function pdfiumProblems(files) {
  return PDFIUM_REQUIRED.filter((f) => !files.includes(f));
}

/** `{ file: version }` → what disagrees with tauri.conf.json, the version the installer carries. */
export function versionProblems(versions) {
  const want = versions["tauri.conf.json"];
  return Object.entries(versions)
    .filter(([, v]) => v !== want)
    .map(([file, v]) => `${file} has ${v}, tauri.conf.json has ${want}`);
}

/** The NSIS installer's name in target/release/bundle/nsis/. */
export function installerName(version) {
  return `PLA_${version}_x64-setup.exe`;
}

/** The config `tauri build --config` merges for signing when `PLA_SIGN_COMMAND` is set (NFR-SEC-009); `%1` is the file. */
export function signConfig(command) {
  if (!command?.trim()) return null;
  // Tauri puts the file in place of a "%1" that stands as an argument of its own
  if (!command.trim().split(/\s+/).includes("%1")) throw new Error("PLA_SIGN_COMMAND must name the file to sign as a separate %1 argument");
  return { bundle: { windows: { signCommand: command } } };
}
