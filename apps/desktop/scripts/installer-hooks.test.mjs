// The uninstaller hook's delete-unless-vault rule (NFR-SEC-010), compiled with the real makensis
// Tauri downloads and run on temporary folders: the real uninstaller only touches the real
// %APPDATA%, where a test must never delete anything.
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const hooks = resolve(dirname(fileURLToPath(import.meta.url)), "../src-tauri/installer/hooks.nsh");
const makensis = join(process.env.LOCALAPPDATA ?? "", "tauri", "NSIS", "makensis.exe");

// An uninstaller that runs the hook's macro on two folders, as the real one does on PLA's.
const script = (dir) => `Unicode true
!include "LogicLib.nsh"
Var UpdateMode
Var DeleteAppDataCheckboxState
!define BUNDLEID "pla.hook.test"
!include "${hooks}"
Name "PLA hook test"
OutFile "${join(dir, "install.exe")}"
RequestExecutionLevel user
SilentInstall silent
SilentUnInstall silent
Section
  WriteUninstaller "${join(dir, "uninstall.exe")}"
SectionEnd
Section "Uninstall"
  StrCpy $UpdateMode 0
  StrCpy $DeleteAppDataCheckboxState 1
  !insertmacro PLA_DELETE_UNLESS_VAULT "${join(dir, "data")}"
  !insertmacro PLA_DELETE_UNLESS_VAULT "${join(dir, "with-vault")}"
SectionEnd
`;

describe.skipIf(process.platform !== "win32" || !existsSync(makensis))("uninstaller hook", () => {
  it("deletes PLA's folder but keeps one that holds a vault", () => {
    const dir = mkdtempSync(join(tmpdir(), "pla-hook-"));
    try {
      mkdirSync(join(dir, "data", "vaults", "0123"), { recursive: true });
      writeFileSync(join(dir, "data", "settings.json"), "{}");
      mkdirSync(join(dir, "with-vault", "Notlar", ".pla"), { recursive: true });
      writeFileSync(join(dir, "with-vault", "Notlar", "Not.md"), "benim notum");
      writeFileSync(join(dir, "test.nsi"), script(dir));

      const built = spawnSync(makensis, ["/V2", join(dir, "test.nsi")], { encoding: "utf8" });
      expect(built.status, built.stdout + built.stderr).toBe(0);
      expect(spawnSync(join(dir, "install.exe")).status).toBe(0);
      // `_?=` runs the uninstaller in place and waits for it
      expect(spawnSync(join(dir, "uninstall.exe"), [`_?=${dir}`]).status).toBe(0);

      expect(existsSync(join(dir, "data"))).toBe(false);
      expect(existsSync(join(dir, "with-vault", "Notlar", "Not.md"))).toBe(true);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 60_000);
});
