import { describe, expect, it } from "vitest";
import { LLAMA_FILES, installerName, llamaProblems, signConfig, versionProblems } from "./release-lib.mjs";

describe("release checks", () => {
  it("names the llama-server files that are missing", () => {
    // without them the installed PLA cannot run a language model
    const all = [...LLAMA_FILES, "ggml-cpu-haswell.dll", "README.md"];
    expect(llamaProblems(all)).toEqual([]);
    expect(llamaProblems(all.filter((f) => f !== "llama.dll"))).toEqual(["llama.dll"]);
    expect(llamaProblems(LLAMA_FILES)).toEqual(["ggml-cpu-*.dll"]);
    expect(llamaProblems(["README.md"])).toHaveLength(LLAMA_FILES.length + 1);
  });

  it("wants one version in every manifest", () => {
    expect(versionProblems({ "tauri.conf.json": "0.1.0", "package.json": "0.1.0", "Cargo.toml": "0.1.0" })).toEqual([]);
    expect(versionProblems({ "tauri.conf.json": "0.2.0", "package.json": "0.1.0", "Cargo.toml": "0.2.0" })).toEqual(["package.json has 0.1.0, tauri.conf.json has 0.2.0"]);
  });

  it("names the installer as Tauri does", () => {
    expect(installerName("0.1.0")).toBe("PLA_0.1.0_x64-setup.exe");
  });

  it("signs only when a sign command is set", () => {
    // owner decision: unsigned until there is a certificate (NFR-SEC-009)
    expect(signConfig(undefined)).toBeNull();
    expect(signConfig("  ")).toBeNull();
    expect(JSON.parse(signConfig("signtool sign /a %1"))).toEqual({ bundle: { windows: { signCommand: "signtool sign /a %1" } } });
    expect(() => signConfig("signtool sign /a")).toThrow(/%1/);
  });
});
