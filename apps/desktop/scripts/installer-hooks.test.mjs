// The uninstaller hook's rules (NFR-SEC-010), compiled with the real makensis Tauri downloads and
// run on temporary folders: the real uninstaller only touches the real %APPDATA%, where a test must
// never delete anything.
import { existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const hooks = resolve(dirname(fileURLToPath(import.meta.url)), "../src-tauri/installer/hooks.nsh");
const makensis = join(process.env.LOCALAPPDATA ?? "", "tauri", "NSIS", "makensis.exe");

// An uninstaller that runs the hook's PLA_REMOVE_DATA on folders of `dir`, as the real one does on
// PLA's, with the box, Tauri's /UPDATE and our upgrade detection set as given.
const script = (dir, { box, update, upgrade, appdata }) => `Unicode true
!include "LogicLib.nsh"
LoadLanguageFile "\${NSISDIR}\\Contrib\\Language files\\English.nlf"
LangString plaFolderKept \${LANG_ENGLISH} "kept"
Var UpdateMode
Var DeleteAppDataCheckboxState
!define BUNDLEID "pla.hook.test"
!define PRODUCTNAME "PLA hook test"
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
  StrCpy $UpdateMode ${update}
  StrCpy $DeleteAppDataCheckboxState ${box}
  StrCpy $PlaUpgrade ${upgrade}
  !insertmacro PLA_REMOVE_DATA "${join(dir, appdata)}" "${join(dir, "local")}" "${join(dir, "webview")}"
SectionEnd
`;

/** Builds the folders, compiles and runs the uninstaller; returns what is left. */
function uninstall(options) {
  const dir = mkdtempSync(join(tmpdir(), "pla-hook-"));
  try {
    mkdirSync(join(dir, "data", "vaults", "0123"), { recursive: true });
    writeFileSync(join(dir, "data", "settings.json"), "{}");
    mkdirSync(join(dir, "with-vault", "Notlar", ".pla"), { recursive: true });
    writeFileSync(join(dir, "with-vault", "Notlar", "Not.md"), "benim notum");
    // models moved to another drive with a junction (final review I1)
    mkdirSync(join(dir, "elsewhere"));
    writeFileSync(join(dir, "elsewhere", "precious.txt"), "not PLA's");
    mkdirSync(join(dir, "local", "models"), { recursive: true });
    writeFileSync(join(dir, "local", "pla-desktop.exe"), "");
    symlinkSync(join(dir, "elsewhere"), join(dir, "local", "linked"), "junction");
    mkdirSync(join(dir, "webview", "EBWebView"), { recursive: true });
    writeFileSync(join(dir, "test.nsi"), script(dir, { box: 0, update: 0, upgrade: 0, appdata: "data", ...options }));

    const built = spawnSync(makensis, ["/V2", join(dir, "test.nsi")], { encoding: "utf8" });
    expect(built.status, built.stdout + built.stderr).toBe(0);
    expect(spawnSync(join(dir, "install.exe")).status).toBe(0);
    // `_?=` runs the uninstaller in place and waits for it
    expect(spawnSync(join(dir, "uninstall.exe"), [`_?=${dir}`]).status).toBe(0);
    const left = (p) => existsSync(join(dir, p));
    return {
      data: left("data"),
      vault: left("with-vault/Notlar/Not.md"),
      local: left("local"),
      precious: left("elsewhere/precious.txt"),
      webview: left("webview"),
    };
  } finally {
    rmSync(join(dir, "local", "linked"), { force: true, recursive: false });
    rmSync(dir, { recursive: true, force: true });
  }
}

describe.skipIf(process.platform !== "win32" || !existsSync(makensis))("uninstaller hook", () => {
  it("with the box ticked deletes PLA's data, never what lies past a junction", () => {
    expect(uninstall({ box: 1 })).toEqual({ data: false, vault: true, local: true, precious: true, webview: false });
  }, 60_000);

  it("with the box empty keeps the data and clears only WebView2's cache", () => {
    expect(uninstall({ box: 0 })).toEqual({ data: true, vault: true, local: true, precious: true, webview: false });
  }, 60_000);

  it("keeps everything when a newer installer replaces PLA, whatever the box says", () => {
    // final review I2: a manual upgrade runs the old uninstaller with its box
    const all = { data: true, vault: true, local: true, precious: true, webview: true };
    expect(uninstall({ box: 1, upgrade: 1 })).toEqual(all);
    expect(uninstall({ box: 1, update: 1 })).toEqual(all);
  }, 120_000);

  it("keeps a folder that holds a vault", () => {
    expect(uninstall({ box: 1, appdata: "with-vault" })).toMatchObject({ vault: true });
  }, 60_000);
});
