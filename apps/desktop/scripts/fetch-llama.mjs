// Fills src-tauri/llama/ with the llama-server files PLA bundles (model-manager spec § 5).
// Usage: npm run fetch-llama [-- --from <folder with llama.cpp b11280 Windows x64 CPU binaries>]
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const BUILD = "11280";
const here = dirname(fileURLToPath(import.meta.url));
const desktop = resolve(here, "..");
const fromArg = process.argv.indexOf("--from");
const source = fromArg > 0 ? resolve(process.argv[fromArg + 1]) : resolve(desktop, "../../research/f1-model-eval/bin");
const target = join(desktop, "src-tauri", "llama");
// Measured: llama-server does not start without any of these (plan Task 3).
const FILES = ["llama-server.exe", "llama-server-impl.dll", "llama.dll", "llama-common.dll", "ggml.dll", "ggml-base.dll", "mtmd.dll", "libomp.dll", "LICENSE-LLVM-OpenMP"];

if (!existsSync(join(source, "llama-server.exe"))) {
  console.error(`No llama-server.exe in ${source}. Pass --from <folder> with llama.cpp b${BUILD} (Windows x64, CPU).`);
  process.exit(1);
}
rmSync(target, { recursive: true, force: true });
mkdirSync(target, { recursive: true });
const cpu = readdirSync(source).filter((f) => /^ggml-cpu-.*\.dll$/.test(f));
for (const f of [...FILES, ...cpu]) copyFileSync(join(source, f), join(target, f));
for (const f of readdirSync(join(desktop, "src-tauri", "llama-licenses"))) copyFileSync(join(desktop, "src-tauri", "llama-licenses", f), join(target, f));
// llama-server prints its version on stderr.
const run = spawnSync(join(target, "llama-server.exe"), ["--version"], { encoding: "utf8" });
const version = `${run.stdout ?? ""}${run.stderr ?? ""}`;
if (!version.includes(`build ${BUILD}`)) {
  console.error(`Expected llama.cpp build ${BUILD}, got:\n${version}`);
  process.exit(1);
}
// `tauri dev` runs target/debug/pla-desktop.exe; bundled_server() also looks next to the exe.
const devDir = resolve(desktop, "../../target/debug");
if (existsSync(devDir)) {
  rmSync(join(devDir, "llama"), { recursive: true, force: true });
  mkdirSync(join(devDir, "llama"), { recursive: true });
  for (const f of readdirSync(target)) copyFileSync(join(target, f), join(devDir, "llama", f));
}
console.log(`llama-server b${BUILD}: ${FILES.length + cpu.length} files in ${target}`);
