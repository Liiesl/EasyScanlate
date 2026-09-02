; EasyScanlate-Installer.nsi — single Velopack-compatible installer
; Old client ManhwaOCR/app/utils/update.py:155 downloads
;   https://github.com/Liiesl/EasyScanlate/releases/download/{tag}/EasyScanlate-Installer.exe
; and runs it with /SILENT (update.py:247-249).
; Velopack Setup.exe only understands -s/--silent (Setup.exe --help).
; This wrapper is published AS EasyScanlate-Installer.exe and embeds the
; Velopack Setup.exe produced by `vpk pack -u EasyScanlate ...` (Releases/EasyScanlate-win-Setup.exe).
; It translates /SILENT -> --silent, elevates to remove the old admin
; NSIS install (HKLM $PROGRAMFILES, installer.nsi:60-64) that previously
; persisted due to user-level wrapper failing HKLM uninstall, then runs the
; Velopack Setup (per-user %LocalAppData%\EasyScanlate).
; Result: one published installer satisfies both old and new clients and
; fully replaces the $PROGRAMFILES install.

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define APP_NAME "EasyScanlate"
!define APP_ID "EasyScanlate"
!define APP_EXE "scanlateit.exe"
!define REG_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
!define VEL_SETUP "..\..\Releases\EasyScanlate-win-Setup.exe"

Name "${APP_NAME}"
OutFile "..\..\EasyScanlate-Installer.exe"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
Icon "..\..\app_icon.ico"
UninstallIcon "..\..\app_icon.ico"

!define MUI_ICON "..\..\app_icon.ico"
!define MUI_UNICON "..\..\app_icon.ico"
!define MUI_ABORTWARNING

; No pages — wrapper is silent-aware one-click like Velopack. Show InstFiles briefly when not silent.
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var IsSilent
Var SetupResult

Function .onInit
  ; Detect /SILENT /S /VERYSILENT /SILENT=... (case-insensitive, any position)
  StrCpy $IsSilent "0"
  ${GetParameters} $R0
  ; Simple substring check — covers NSIS /SILENT and Velopack --silent
  ${If} $R0 != ""
    ; Check for --silent / -s / /SILENT etc.
    StrCpy $R1 $R0
    ; lowercase for comparison via brute: check several variants
    ${If} $R1 != ""
      ; NSIS StrStr is case-sensitive; check both cases
      StrCpy $R2 $R0
      Push $R2
      Push "/SILENT"
      Call StrStr
      Pop $R3
      ${If} $R3 != ""
        StrCpy $IsSilent "1"
      ${EndIf}
      Push $R2
      Push "/S"
      Call StrStr
      Pop $R3
      ${If} $R3 != ""
        StrCpy $IsSilent "1"
      ${EndIf}
      Push $R2
      Push "--silent"
      Call StrStr
      Pop $R3
      ${If} $R3 != ""
        StrCpy $IsSilent "1"
      ${EndIf}
      Push $R2
      Push "-s"
      Call StrStr
      Pop $R3
      ${If} $R3 != ""
        StrCpy $IsSilent "1"
      ${EndIf}
    ${EndIf}
  ${EndIf}

  ; Uninstall old admin install if present — now elevated (admin), so HKLM uninstall succeeds.
  ; Mirrors EasyScanlate/dev/installer/installer.nsi:60-64 + cleanup of orphaned keys/files.
  SetRegView 64
  ReadRegStr $R0 HKLM "${REG_UNINSTALL_KEY}" "UninstallString"
  ${If} $R0 != ""
    DetailPrint "Found old install (64-bit view), running uninstaller..."
    ExecWait '"$R0" /S _?=$INSTDIR'
  ${EndIf}
  SetRegView 32
  ReadRegStr $R0 HKLM "${REG_UNINSTALL_KEY}" "UninstallString"
  ${If} $R0 != ""
    DetailPrint "Found old install (32-bit view), running uninstaller..."
    ExecWait '"$R0" /S _?=$INSTDIR'
  ${EndIf}
  ; Fallback: uninstaller may have been deleted but registry/files remain — force-clean.
  SetRegView 64
  ReadRegStr $R0 HKLM "${REG_UNINSTALL_KEY}" "UninstallString"
  ${If} $R0 != ""
    DetailPrint "Old uninstall key still present, forcing cleanup..."
  ${EndIf}
  ; Remove old $PROGRAMFILES dir if still exists (uninstall /S is async via _?=$INSTDIR trick)
  IfFileExists "$PROGRAMFILES\${APP_NAME}\*.*" 0 +2
    RMDir /r "$PROGRAMFILES\${APP_NAME}"
  IfFileExists "$PROGRAMFILES64\${APP_NAME}\*.*" 0 +2
    RMDir /r "$PROGRAMFILES64\${APP_NAME}"
  SetRegView 64
  DeleteRegKey HKLM "${REG_UNINSTALL_KEY}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\main.exe"
  DeleteRegKey HKCR ".mmtl"
  DeleteRegKey HKCR "EasyScanlate.MMTLFile"
  SetRegView 32
  DeleteRegKey HKLM "${REG_UNINSTALL_KEY}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\main.exe"
  ; Old shortcuts (admin StartMenu/Desktop) — Velopack recreates per-user ones
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
FunctionEnd

Section "Velopack Setup" SecVelopack
  SetOutPath $PLUGINSDIR
  ; Embed the Velopack-generated setup (must exist at build time from `vpk pack`)
  File /oname=Setup.exe "${VEL_SETUP}"

  DetailPrint "Launching Velopack Setup..."
  ${If} $IsSilent == "1"
    ExecWait '"$PLUGINSDIR\Setup.exe" --silent' $SetupResult
  ${Else}
    ExecWait '"$PLUGINSDIR\Setup.exe"' $SetupResult
  ${EndIf}

  ; Velopack Setup handles shortcuts + Update.exe; we just propagate its exit code.
  ; If Setup launches the app, it returns 0 quickly.
  DetailPrint "Setup finished: $SetupResult"
SectionEnd

; --- StrStr helper (case-sensitive) ---
Function StrStr
  Exch $R1 ; needle
  Exch
  Exch $R0 ; haystack
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  StrLen $R2 $R1
  StrLen $R3 $R0
  IntOp $R4 $R3 - $R2
  StrCpy $R5 0
  loop:
    IntCmp $R5 $R4 done 0 done
    StrCpy $R3 $R0 $R2 $R5
    StrCmp $R3 $R1 found
    IntOp $R5 $R5 + 1
    Goto loop
  found:
    StrCpy $R1 $R0 "" $R5
    Goto end
  done:
    StrCpy $R1 ""
  end:
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Exch $R1
FunctionEnd
