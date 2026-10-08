import { describe, expect, it } from "vitest";
import { LLAMA_FILES, LLAMA_REQUIRED, installerName, llamaProblems, signConfig, strayFiles, versionProblems } from "./release-lib.mjs";

describe("release checks", () => {
  it("names the llama-server files that are missing", () => {
    // without them the installed PLA cannot run a language model, or ships without its licence
    const all = [...LLAMA_REQUIRED, "ggml-cpu-haswell.dll", "README.md"];
    expect(LLAMA_REQUIRED).toContain("LICENSE-llama.cpp.txt");
    expect(llamaProblems(all)).toEqual([]);
    expect(llamaProblems(all.filter((f) => f !== "llama.dll"))).toEqual(["llama.dll"]);
    expect(llamaProblems(LLAMA_REQUIRED)).toEqual(["ggml-cpu-*.dll"]);
    expect(llamaProblems(["README.md"])).toHaveLength(LLAMA_REQUIRED.length + 1);
  });

  it("finds files that do not belong in the installer", () => {
    // final review M6: the whole llama/ folder is bundled
    expect(strayFiles([...LLAMA_REQUIRED, "ggml-cpu-haswell.dll", "README.md"])).toEqual([]);
    expect(strayFiles(["llama.dll", "gemma.gguf", "notlar.txt"])).toEqual(["gemma.gguf", "notlar.txt"]);
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
    expect(signConfig("signtool sign /a %1")).toEqual({ bundle: { windows: { signCommand: "signtool sign /a %1" } } });
    expect(() => signConfig("signtool sign /a")).toThrow(/%1/);
    expect(() => signConfig("cmd /C echo %1>>log")).toThrow(/separate/);
  });
});
