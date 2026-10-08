; PLA's additions to Tauri's NSIS installer (tauri.conf.json > bundle.windows.nsis.installerHooks).
;
; NFR-SEC-010: uninstalling never deletes the vault and asks before deleting PLA's data. Tauri's
; own "delete app data" box (its text is ours, see Turkish.nsh / English.nsh) only knows the
; identifier folders; PLA keeps its data in %APPDATA%\PLA (settings, task and assistant databases)
; and %LOCALAPPDATA%\PLA (models, next to the program), so the box removes those too.

!include "FileFunc.nsh"

Var PlaKeep
Var PlaAttr
; 1 when a newer installer runs this uninstaller to replace PLA (final review I2)
Var PlaUpgrade
; the "Start with Windows" value, which Tauri's uninstaller deletes
Var PlaRunValue

; ${Locate} callback: a `.pla` folder marks a vault (vault.rs keeps its config there), and a junction
; or link may lead outside PLA's folder, where `RmDir /r` would delete the user's other files.
Function un.PlaCheckEntry
  ${If} $R7 == ".pla"
    StrCpy $PlaKeep 1
  ${Else}
    ${GetFileAttributes} "$R9" "REPARSE_POINT" $PlaAttr
    ${If} $PlaAttr == 1
      StrCpy $PlaKeep 1
    ${EndIf}
  ${EndIf}
  ${If} $PlaKeep == 1
    Push "StopLocate"
  ${Else}
    Push "go on"
  ${EndIf}
FunctionEnd

; Deletes `dir` unless something inside must stay: a vault, or a link out of it. PLA refuses a vault
; there (vault::inside_app_data), so this guards against one made before that rule or by hand.
; Anything unexpected keeps the folder (final review M1).
!macro PLA_DELETE_UNLESS_KEPT dir
  StrCpy $PlaKeep 0
  ${If} ${FileExists} "${dir}\*.*"
    ${GetFileAttributes} "${dir}" "REPARSE_POINT" $PlaAttr
    ${If} $PlaAttr == 1
      StrCpy $PlaKeep 1
    ${Else}
      ClearErrors
      ${Locate} "${dir}" "/L=FD" "un.PlaCheckEntry"
      ${If} ${Errors}
        StrCpy $PlaKeep 1
      ${EndIf}
    ${EndIf}
    ${If} $PlaKeep == 1
      DetailPrint "PLA: ${dir} is kept."
      MessageBox MB_OK|MB_ICONINFORMATION "$(plaFolderKept)$\n$\n${dir}" /SD IDOK
    ${Else}
      RmDir /r "${dir}"
    ${EndIf}
  ${EndIf}
!macroend

; What uninstalling removes besides the program. `webview` is WebView2's cache and storage, which
; PLA uses for nothing of the user's. Some of its files may still be held for a moment after PLA
; closes; a per-user uninstaller cannot schedule them for the next restart, so they stay.
!macro PLA_REMOVE_DATA appdata localappdata webview
  ${If} $UpdateMode <> 1
  ${AndIf} $PlaUpgrade <> 1
    RmDir /r "${webview}"
    ${If} $DeleteAppDataCheckboxState = 1
      !insertmacro PLA_DELETE_UNLESS_KEPT "${appdata}"
      !insertmacro PLA_DELETE_UNLESS_KEPT "${localappdata}"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The uninstaller takes its language from this value and asks with a dialog when it is missing;
  ; Tauri writes it only when the installer shows its own language selector, which PLA does not.
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $LANGUAGE
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Uninstalling from Windows' settings runs a copy of this file from %TEMP%; a newer installer
  ; replacing PLA runs it where it is installed. An upgrade keeps everything, whatever the box says.
  ${If} $EXEDIR == $INSTDIR
    StrCpy $PlaUpgrade 1
  ${EndIf}
  ReadRegStr $PlaRunValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  SetShellVarContext current
  !insertmacro PLA_REMOVE_DATA "$APPDATA\PLA" "$LOCALAPPDATA\PLA" "$LOCALAPPDATA\${BUNDLEID}"
  ; Tauri deleted the autostart value; an upgrade puts it back (the program stays in the same place)
  ${If} $PlaUpgrade == 1
  ${AndIf} $PlaRunValue != ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" $PlaRunValue
  ${EndIf}
!macroend
