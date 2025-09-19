; installer.nsi
; NSIS Script for MangaOCRTool Application
; =============================================

!include "MUI2.nsh"

; --- General Information ---
Name "MangaOCRTool"
OutFile "MangaOCRTool-Installer.exe"
InstallDir "$PROGRAMFILES\MangaOCRTool"
RequestExecutionLevel admin

; --- Modern UI Settings ---
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "This setup will guide you through the installation of MangaOCRTool.$\r$\n$\r$\nClick Next to continue."

; --- Page Macros ---
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; --- Installer Sections ---

; Section 1: Main Application (Always installed)
Section "Install Application" SecApp
  ; Set the required size for this section in Megabytes (MB).
  ; This should be the size of your application files WITHOUT the torch libs.
  AddSize 1096
  
  SetOutPath $INSTDIR
  ; Copy all built files into the installation directory.
  File /r "build\main.dist\*"

  ; --- Uninstaller and Registry Keys ---
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool" "DisplayName" "MangaOCRTool"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool" "DisplayIcon" "$INSTDIR\main.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool" "DisplayVersion" "1.0"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool" "Publisher" "Your Name"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool" "InstallLocation" "$INSTDIR"
SectionEnd

; Section 2: File Association (Always installed)
Section "Register File Association" SecFileAssoc
  ; This section only adds registry keys, so its size is negligible.
  AddSize 0
  
  SetOutPath $INSTDIR
  !define APP_EXE "main.exe"

  DetailPrint "Registering .mmtl file association..."
  WriteRegStr HKCR ".mmtl" "" "MangaOCRTool.MMTLFile"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile" "" "Manhwa/hua/ga Machine Translation"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
SectionEnd

; --- Uninstaller Section ---
Section "Uninstall"
  ; Remove the entire installation directory
  RMDir /r "$INSTDIR"

  ; Remove file association and uninstaller keys from the registry 
  DetailPrint "Removing .mmtl file association..."
  DeleteRegKey HKCR ".mmtl"
  DeleteRegKey HKCR "MangaOCRTool.MMTLFile"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool"
SectionEnd