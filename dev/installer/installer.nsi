; installer.nsi
; NSIS Script for MangaOCRTool Application
; =============================================

!include "MUI2.nsh"

; --- General Information ---
!define APP_NAME "MangaOCRTool"
!define APP_VERSION "1.0"
!define APP_PUBLISHER "Your Name"
!define APP_EXE "main.exe"
!define APP_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
!define STARTMENU_FOLDER "$SMPROGRAMS\${APP_NAME}" ; Define for Start Menu folder

!define /date INSTALL_DATE_YYYYMMDD "%Y%m%d" ; Defines the current date for the registry

Name "${APP_NAME}"
OutFile "${APP_NAME}-Installer.exe"
InstallDir "$PROGRAMFILES\${APP_NAME}"
RequestExecutionLevel admin

; --- Modern UI Settings ---
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "This setup will guide you through the installation of MangaOCRTool.$\r$\n$\r$\nClick Next to continue."

; --- Page Macros ---
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS ; NEW - Components page for shortcut selection
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; --- Descriptions for Sections ---
LangString DESC_SecApp ${LANG_ENGLISH} "Install the main application files."
LangString DESC_SecShortcuts ${LANG_ENGLISH} "Create shortcuts in the Start Menu and on the Desktop."
LangString DESC_SecFileAssoc ${LANG_ENGLISH} "Associate .mmtl files with this application."
LangString DESC_SecAppPath ${LANG_ENGLISH} "Register the application path for easier command-line access."


; --- Installer Sections ---

; Section 1: Main Application (Now includes detailed registry keys)
Section "Install Application" SecApp
  SectionIn RO ; Required section
  AddSize 1096
  
  SetOutPath $INSTDIR
  
  ; Check if the torch directory already exists.
  IfFileExists "$INSTDIR\torch\*.*" an_existing_torch
    ; If it doesn't exist, install everything including torch.
    File /r "..\..\build\main.dist\*"
    Goto post_install_files
  an_existing_torch:
    ; If it exists, install all files except for the torch directory.
    File /r /x torch "..\..\build\main.dist\*"
  post_install_files:

  ; --- Uninstaller and Enhanced Registry Keys ---
  WriteUninstaller "$INSTDIR\uninstall.exe"
  
  ; Write to the 64-bit view of the registry if on a 64-bit system
  SetRegView 64 

  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "Publisher" "${APP_PUBLISHER}"
  
  ; --- NEW - Detailed "Apps & features" keys ---
  WriteRegDWORD HKLM "${APP_UNINSTALL_KEY}" "EstimatedSize" 1122304 ; Size in KB (1096 MB * 1024)
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/your-repo/MangaOCRTool"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "HelpLink" "https://github.com/your-repo/MangaOCRTool/issues"
  WriteRegStr HKLM "${APP_UNINSTALL_KEY}" "InstallDate" "${INSTALL_DATE_YYYYMMDD}"
  WriteRegDWORD HKLM "${APP_UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${APP_UNINSTALL_KEY}" "NoRepair" 1
  
  !insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "$(DESC_SecApp)"
  !insertmacro MUI_FUNCTION_DESCRIPTION_END
SectionEnd

; --- NEW Section Group for Shortcuts ---
SectionGroup "Shortcuts" SecShortcuts
  AddSize 1
  
  Section "Start Menu Shortcut" SecStartMenu
    CreateDirectory "${STARTMENU_FOLDER}"
    CreateShortCut "${STARTMENU_FOLDER}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
    CreateShortCut "${STARTMENU_FOLDER}\Uninstall ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"
  SectionEnd
  
  Section "Desktop Shortcut" SecDesktop
    CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  SectionEnd
  
  !insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecShortcuts} "$(DESC_SecShortcuts)"
  !insertmacro MUI_FUNCTION_DESCRIPTION_END
SectionGroupEnd

; Section 3: File Association (No changes needed here, but kept for context)
Section "Register File Association" SecFileAssoc
  AddSize 0
  SetRegView 64
  SetOutPath $INSTDIR
  
  DetailPrint "Registering .mmtl file association..."
  WriteRegStr HKCR ".mmtl" "" "MangaOCRTool.MMTLFile"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile" "" "Manhwa/hua/ga Machine Translation"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
  
  !insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecFileAssoc} "$(DESC_SecFileAssoc)"
  !insertmacro MUI_FUNCTION_DESCRIPTION_END
SectionEnd

; Section 4: Register App Path
Section "Register App Path" SecAppPath
  AddSize 0
  SetRegView 64

  DetailPrint "Registering application path..."
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${APP_EXE}" "" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${APP_EXE}" "Path" "$INSTDIR"
  
  !insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecAppPath} "$(DESC_SecAppPath)"
  !insertmacro MUI_FUNCTION_DESCRIPTION_END
SectionEnd


; --- Updated Uninstaller Section ---
Section "Uninstall"
  ; --- Cleanup from Shortcuts ---
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "${STARTMENU_FOLDER}\${APP_NAME}.lnk"
  Delete "${STARTMENU_FOLDER}\Uninstall ${APP_NAME}.lnk"
  RMDir "${STARTMENU_FOLDER}"

  ; --- Cleanup from Main Application ---
  ; Check if the torch directory exists. If it does, ask the user what to do.
  IfFileExists "$INSTDIR\torch\*.*" 0 NoTorchFound
    ; Prompt the user to decide whether to keep the torch libraries.
    MessageBox MB_YESNO|MB_ICONQUESTION "Do you want to completely remove MangaOCRTool, including the large PyTorch libraries (over 4GB)?$\r$\n$\r$\nClicking 'No' will preserve these libraries to speed up future installations." IDYES CompleteRemove
    
    ; --- User clicked NO: Preserve the torch directory ---
    DetailPrint "Preserving torch directory and removing other application files..."
    ; Use $PLUGINSDIR as a safe temporary location. It's cleaned up automatically.
    Rename "$INSTDIR\torch" "$PLUGINSDIR\torch_bak"
    RMDir /r "$INSTDIR"
    CreateDirectory "$INSTDIR"
    Rename "$PLUGINSDIR\torch_bak" "$INSTDIR\torch"
    Goto CleanupRegistry ; Skip to the registry cleanup part

  CompleteRemove:
    ; --- User clicked YES: Perform a full and complete uninstallation ---
    DetailPrint "Performing complete removal..."
    RMDir /r "$INSTDIR"
    Goto CleanupRegistry ; Continue to registry cleanup

  NoTorchFound:
    ; --- The torch directory was not found, so just remove everything normally ---
    RMDir /r "$INSTDIR"

  CleanupRegistry:
    ; --- Cleanup from File Association and Registry (for all scenarios) ---
    SetRegView 64
    DetailPrint "Removing .mmtl file association..."
    DeleteRegKey HKCR ".mmtl"
    DeleteRegKey HKCR "MangaOCRTool.MMTLFile"
    
    DetailPrint "Removing application path..."
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\${APP_EXE}"
    
    DetailPrint "Removing uninstaller information..."
    DeleteRegKey HKLM "${APP_UNINSTALL_KEY}"
SectionEnd