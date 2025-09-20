; NSIS Script for MangaOCRTool Application
; =============================================

!include "MUI2.nsh"

; --- Application Information ---
!define APP_NAME "MangaOCRTool"
!define APP_PUBLISHER "YourCompany"
!define APP_EXE "main.exe"
!define APP_VERSION "0.0.1" ; Use a consistent versioning scheme

; --- Registry & Path Definitions ---
!define REG_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
!define REG_APP_KEY "Software\${APP_PUBLISHER}\${APP_NAME}" ; For update detection signal
!define STARTMENU_FOLDER "$SMPROGRAMS\${APP_NAME}"

; --- Date for Registry ---
!define /date INSTALL_DATE_YYYYMMDD "%Y%m%d"

; --- Installer General Settings ---
Name "${APP_NAME}"
OutFile "${APP_NAME}-Installer.exe"
InstallDir "$PROGRAMFILES\${APP_NAME}"
RequestExecutionLevel admin
SetCompressor lzma

; --- Modern UI 2 Settings ---
!define MUI_ABORTWARNING
!define MUI_ICON "..\..\assets\app_icon.ico"
!define MUI_UNICON "..\..\assets\app_icon.ico"
!define MUI_WELCOMEPAGE_TEXT "This setup will guide you through the installation of MangaOCRTool.$\r$\n$\r$\nClick Next to continue."

; --- Installer Pages ---
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

; --- Uninstaller Pages ---
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; --- Language ---
!insertmacro MUI_LANGUAGE "English"

; --- Component Page Descriptions (Pre-defined text) ---
LangString DESC_SecApp ${LANG_ENGLISH} "Install the main application files."
LangString DESC_SecShortcuts ${LANG_ENGLISH} "Create shortcuts in the Start Menu and on the Desktop."
LangString DESC_SecFileAssoc ${LANG_ENGLISH} "Associate .mmtl files with this application."
LangString DESC_SecAppPath ${LANG_ENGLISH} "Register the application path for easier command-line access."

; --- Installer Logic ---

; This function runs before the installer UI is shown.
; It checks for a previous installation and handles the silent uninstall for updates.
Function .onInit
  SetRegView 64
  
  ; Check if a previous version is installed
  ReadRegStr $R0 HKLM "${REG_UNINSTALL_KEY}" "UninstallString"
  IfFileExists $R0 0 NoPreviousVersion

  ; Previous version found. Signal to the uninstaller that an update is in progress.
  WriteRegStr HKLM "${REG_APP_KEY}" "UpdateInProgress" "1"
  
  ; Silently run the old uninstaller and wait for it to complete.
  ; The old uninstaller will see the "UpdateInProgress" flag and preserve the 'torch' directory.
  ExecWait '"$R0" /S _?=$INSTDIR'
  
  ; Clean up the signal flag after the old uninstaller has finished.
  DeleteRegValue HKLM "${REG_APP_KEY}" "UpdateInProgress"

NoPreviousVersion:
FunctionEnd


; --- Installer Sections ---

Section "Main Application" SecApp
  SectionIn RO
  ; This size should reflect the application WITHOUT torch (e.g., from 'build-output.7z')
  ; If your app is 300MB without torch, this is approx 307200 KB.
  AddSize 309200

  SetOutPath $INSTDIR
  
  DetailPrint "Installing application files (excluding PyTorch)..."
  ; Package everything EXCEPT the torch directory, which is handled separately.
  File /r /x torch "..\..\build\main.dist\*"

  ; --- Create Uninstaller and Write Registry Keys ---
  WriteUninstaller "$INSTDIR\uninstall.exe"
  SetRegView 64
  
  WriteRegStr HKLM "${REG_UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "${REG_UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${REG_UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${REG_UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKLM "${REG_UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKLM "${REG_UNINSTALL_KEY}" "Publisher" "${APP_PUBLISHER}"
  WriteRegDWORD HKLM "${REG_UNINSTALL_KEY}" "EstimatedSize" 309200 ; Update to match AddSize
  WriteRegStr HKLM "${REG_UNINSTALL_KEY}" "InstallDate" "${INSTALL_DATE_YYYYMMDD}"
  WriteRegDWORD HKLM "${REG_UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${REG_UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

SectionGroup "Shortcuts" SecShortcuts  
  Section "Start Menu Shortcut" SecStartMenu
    CreateDirectory "${STARTMENU_FOLDER}"
    CreateShortCut "${STARTMENU_FOLDER}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
    CreateShortCut "${STARTMENU_FOLDER}\Uninstall ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"
  SectionEnd
  
  Section "Desktop Shortcut" SecDesktop
    CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
  SectionEnd
SectionGroupEnd

Section "Register File Association" SecFileAssoc
  SetRegView 64
  
  DetailPrint "Registering .mmtl file association..."
  WriteRegStr HKCR ".mmtl" "" "MangaOCRTool.MMTLFile"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile" "" "Manga OCR Tool Project"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
SectionEnd

Section "Add Application to Path" SecAppPath
  SetRegView 64

  DetailPrint "Registering application path..."
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${APP_EXE}" "" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${APP_EXE}" "Path" "$INSTDIR"
SectionEnd

; =========================================================================
; --- Descriptions for Components Page (This is the corrected block) ---
; This block must be OUTSIDE all Section/Function blocks.
; =========================================================================
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "$(DESC_SecApp)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecShortcuts} "$(DESC_SecShortcuts)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecFileAssoc} "$(DESC_SecFileAssoc)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAppPath} "$(DESC_SecAppPath)"
!insertmacro MUI_FUNCTION_DESCRIPTION_END


; --- Uninstaller Logic ---

Section "Uninstall"
  SetRegView 64
  
  ; Check if this is a silent uninstall triggered by an update.
  ReadRegStr $R0 HKLM "${REG_APP_KEY}" "UpdateInProgress"
  StrCmp $R0 "1" HandleUpdateUninstall HandleManualUninstall

HandleUpdateUninstall:
  ; UPDATE MODE: Silently uninstall everything EXCEPT the 'torch' directory.
  DetailPrint "Update detected. Preserving PyTorch libraries..."
  
  ; To safely remove everything else, we temporarily move 'torch'.
  Rename "$INSTDIR\torch" "$PLUGINSDIR\torch_bak"
  
  ; Remove all other application files and folders.
  RMDir /r "$INSTDIR"
  
  ; Re-create the installation directory and move 'torch' back.
  CreateDirectory "$INSTDIR"
  Rename "$PLUGINSDIR\torch_bak" "$INSTDIR\torch"
  
  Goto CleanupRegistry ; Skip to final registry cleanup.

HandleManualUninstall:
  ; MANUAL MODE: Ask the user what to do with the 'torch' directory.
  IfFileExists "$INSTDIR\torch\*.*" 0 NoTorchFound
  
    ; Prompt the user.
    MessageBox MB_YESNO|MB_ICONQUESTION \
      "Do you want to completely remove MangaOCRTool, including the large PyTorch libraries (over 4GB)?$\r$\n$\r$\nClicking 'No' will preserve these libraries to speed up future installations." \
      IDYES CompleteRemove IDNO PreserveTorch
    
    Goto CleanupRegistry ; Should not be reached, but as a fallback.

  PreserveTorch:
    DetailPrint "Preserving torch directory and removing other files..."
    Rename "$INSTDIR\torch" "$PLUGINSDIR\torch_bak"
    RMDir /r "$INSTDIR"
    CreateDirectory "$INSTDIR"
    Rename "$PLUGINSDIR\torch_bak" "$INSTDIR\torch"
    Goto CleanupRegistry

  CompleteRemove:
    DetailPrint "Performing complete removal of all files..."
    RMDir /r "$INSTDIR"
    Goto CleanupRegistry

  NoTorchFound:
    ; The 'torch' directory doesn't exist, so just remove everything.
    DetailPrint "Removing application directory..."
    RMDir /r "$INSTDIR"

CleanupRegistry:
  ; --- Final cleanup for ALL uninstall scenarios ---
  
  ; --- Cleanup Shortcuts ---
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "${STARTMENU_FOLDER}\${APP_NAME}.lnk"
  Delete "${STARTMENU_FOLDER}\Uninstall ${APP_NAME}.lnk"
  RMDir "${STARTMENU_FOLDER}"

  ; --- Cleanup Registry ---
  SetRegView 64
  DetailPrint "Removing registry keys..."
  DeleteRegKey HKCR ".mmtl"
  DeleteRegKey HKCR "MangaOCRTool.MMTLFile"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${APP_EXE}"
  DeleteRegKey HKLM "${REG_UNINSTALL_KEY}"
  ; Also remove the app's own registry key, if it exists.
  DeleteRegKey /ifempty HKLM "${REG_APP_KEY}"
SectionEnd