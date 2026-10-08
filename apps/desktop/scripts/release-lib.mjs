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
