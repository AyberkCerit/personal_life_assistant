// Builds PLA's Windows installer (packaging plan § 5): checks the bundled llama-server and the
// versions, runs `tauri build`, and leaves the installer and its SHA-256 in <repo>/dist-release/.
// Usage: npm run release   (PLA_SIGN_COMMAND="signtool sign ... %1" signs it, NFR-SEC-009)
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { LLAMA_BUILD, installerName, llamaProblems, signConfig, versionProblems } from "./release-lib.mjs";

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repo = resolve(desktop, "../..");
const tauri = join(desktop, "src-tauri");

function fail(message) {
  console.error(`release: ${message}`);
  process.exit(1);
}

// 1. The llama-server PLA ships, complete and at the build the app was tested with.
const llama = join(tauri, "llama");
const missing = llamaProblems(readdirSync(llama));
if (missing.length) fail(`src-tauri/llama/ is missing ${missing.join(", ")}. Run: npm run fetch-llama -- --from <llama.cpp b${LLAMA_BUILD} folder>`);
const run = spawnSync(join(llama, "llama-server.exe"), ["--version"], { encoding: "utf8" });
if (!`${run.stdout ?? ""}${run.stderr ?? ""}`.includes(`build ${LLAMA_BUILD}`)) fail(`src-tauri/llama/ is not llama.cpp b${LLAMA_BUILD}. Run npm run fetch-llama again.`);

// 2. One version everywhere.
const version = JSON.parse(readFileSync(join(tauri, "tauri.conf.json"), "utf8")).version;
const cargoVersion = (file) => readFileSync(file, "utf8").match(/^version = "([^"]+)"/m)?.[1];
const problems = versionProblems({
  "tauri.conf.json": version,
  "package.json": JSON.parse(readFileSync(join(desktop, "package.json"), "utf8")).version,
  "Cargo.toml": cargoVersion(join(tauri, "Cargo.toml")),
  "pla-core/Cargo.toml": cargoVersion(join(repo, "crates/pla-core/Cargo.toml")),
});
if (problems.length) fail(`versions disagree:\n  ${problems.join("\n  ")}`);

// 3. Build.
const sign = signConfig(process.env.PLA_SIGN_COMMAND);
console.log(`release: building PLA ${version} (${sign ? "signed" : "unsigned"})`);
const args = ["tauri", "build", ...(sign ? ["--config", sign] : [])];
const build = spawnSync("npx", args, { cwd: desktop, stdio: "inherit", shell: true });
if (build.status !== 0) fail(`tauri build failed (exit ${build.status})`);

// 4. The installer and its checksum, side by side.
const name = installerName(version);
const out = join(repo, "dist-release");
mkdirSync(out, { recursive: true });
copyFileSync(join(repo, "target/release/bundle/nsis", name), join(out, name));
const sha = createHash("sha256").update(readFileSync(join(out, name))).digest("hex");
writeFileSync(join(out, `${name}.sha256`), `${sha}  ${name}\n`);
console.log(`release: ${join(out, name)}\nrelease: sha256 ${sha}`);
