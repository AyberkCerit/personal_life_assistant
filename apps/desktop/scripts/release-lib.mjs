// The checks `npm run release` makes before it builds the installer (packaging plan § 5).

// Measured: llama-server does not start without any of these (model-manager plan Task 3), plus at
// least one ggml-cpu-*.dll for the processor.
export const LLAMA_FILES = ["llama-server.exe", "llama-server-impl.dll", "llama.dll", "llama-common.dll", "ggml.dll", "ggml-base.dll", "mtmd.dll", "libomp.dll", "LICENSE-LLVM-OpenMP"];
export const LLAMA_BUILD = "11280";

/** The llama-server files missing from `files` (the names in src-tauri/llama/). */
export function llamaProblems(files) {
  const missing = LLAMA_FILES.filter((f) => !files.includes(f));
  if (!files.some((f) => /^ggml-cpu-.*\.dll$/.test(f))) missing.push("ggml-cpu-*.dll");
  return missing;
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

/** `tauri build --config` for signing when `PLA_SIGN_COMMAND` is set (NFR-SEC-009); `%1` is the file. */
export function signConfig(command) {
  if (!command?.trim()) return null;
  if (!command.includes("%1")) throw new Error("PLA_SIGN_COMMAND must name the file to sign as %1");
  return JSON.stringify({ bundle: { windows: { signCommand: command } } });
}
