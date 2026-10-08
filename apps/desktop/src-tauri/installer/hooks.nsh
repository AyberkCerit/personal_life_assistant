; PLA's additions to Tauri's NSIS installer (tauri.conf.json > bundle.windows.nsis.installerHooks).
;
; NFR-SEC-010: uninstalling never deletes the vault and asks before deleting PLA's data. Tauri's
; own "delete app data" box (its text is ours, see Turkish.nsh / English.nsh) only knows the
; identifier folders; PLA keeps its data in %APPDATA%\PLA (settings, task and assistant databases)
; and %LOCALAPPDATA%\PLA (models), so the box removes those too.

!include "FileFunc.nsh"

Var PlaVaultFound

; ${Locate} callback: a `.pla` folder marks a vault (vault.rs keeps its config there).
Function un.PlaFoundVault
  StrCpy $PlaVaultFound 1
  Push "StopLocate"
FunctionEnd

; Deletes `dir` unless a vault lives inside it. PLA refuses such a vault (vault::inside_app_data),
; so this only guards against one made before that rule or by hand.
!macro PLA_DELETE_UNLESS_VAULT dir
  StrCpy $PlaVaultFound 0
  ${If} ${FileExists} "${dir}\*.*"
    ${Locate} "${dir}" "/L=D /M=.pla" "un.PlaFoundVault"
    ${If} $PlaVaultFound = 1
      DetailPrint "PLA: ${dir} holds a vault, so it is kept."
    ${Else}
      RmDir /r "${dir}"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The uninstaller takes its language from this value and asks with a dialog when it is missing;
  ; Tauri writes it only when the installer shows its own language selector, which PLA does not.
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $LANGUAGE
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; an update uninstalls the old version first: everything stays then
  ${If} $UpdateMode <> 1
    SetShellVarContext current
    ; WebView2's cache and storage, which PLA does not use for anything of the user's
    ; /REBOOTOK: WebView2 processes may still hold files for a moment after PLA was closed
    RmDir /r /REBOOTOK "$LOCALAPPDATA\${BUNDLEID}"
    ${If} $DeleteAppDataCheckboxState = 1
      !insertmacro PLA_DELETE_UNLESS_VAULT "$APPDATA\PLA"
      !insertmacro PLA_DELETE_UNLESS_VAULT "$LOCALAPPDATA\PLA"
    ${EndIf}
  ${EndIf}
!macroend
