; installer.nsi
; NSIS Script for MangaOCRTool Online Installer
; =============================================

!include "MUI2.nsh"
!include "LogicLib.nsh" ; Required for advanced logic (If/Else)
!include /CHARSET=CP1252 zipdll.nsh   ; Required for unzipping with ZipDLL

; --- General Information ---
Name "MangaOCRTool"
OutFile "MangaOCRTool-Installer.exe"
InstallDir "$PROGRAMFILES\MangaOCRTool"
RequestExecutionLevel admin

; --- GitHub Release URL Placeholder ---
!define TORCHLIB_URL "https://github.com/Liiesl/ManhwaOCR/releases/download/${LATEST_TAG}/pytorch-libs.zip"

; --- Path to check for existing installation ---
; We check for a specific, crucial file. This is more reliable than checking for a directory.
!define CHECK_FILE "$INSTDIR\torch\lib\torch.dll" 

; --- Modern UI Settings ---
!define MUI_ABORTWARNING

; --- Page Macros (Welcome page is configured in .onInit) ---
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; --- Installer Sections ---
; Sections must be defined BEFORE functions that reference them.

; Section 1: Main Application (Always installed)
Section "Install Application" SecApp
  ; Set the required size for this section in Megabytes (MB)
  AddSize 900
  
  SetOutPath $INSTDIR
  File /r /x "torch" "build\main.dist\*"

  ; --- Uninstaller and Registry Keys ---
  ; Write the uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Write registry keys for Add/Remove Programs
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

  ; Create the registry key for the file extension
  WriteRegStr HKCR ".mmtl" "" "MangaOCRTool.MMTLFile"

  ; Create the key for the file type identifier and set its description
  WriteRegStr HKCR "MangaOCRTool.MMTLFile" "" "Manhwa/hua/ga Machine Translation"

  ; Set the default icon for .mmtl files to be the application's icon
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"

  ; Define the command to execute when a .mmtl file is opened
  WriteRegStr HKCR "MangaOCRTool.MMTLFile\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
SectionEnd

; Section 3: PyTorch Libraries (Conditionally installed)
Section "Download and Install PyTorch Libs" SecPyTorch
  ; Set the required size for this section in Megabytes (MB). 4GB = 4096MB
  AddSize 4096

  SetOutPath $INSTDIR
  DetailPrint "Downloading PyTorch libraries... (This may take several minutes)"
  
  NSISdl::download "${TORCHLIB_URL}" "$PLUGINSDIR\torch-lib.zip"
  Pop $R0
  StrCmp $R0 "success" dl_ok dl_failed

dl_ok:
  DetailPrint "Download complete. Extracting files..."
  ZipDLL::extractall "$PLUGINSDIR\torch-lib.zip" "$INSTDIR"
  Pop $R0
  StrCmp $R0 "success" extract_ok extract_failed
  Goto extract_end

dl_failed:
  MessageBox MB_OK|MB_ICONSTOP "Failed to download PyTorch libraries. The application will now be uninstalled."
  
  ; Execute the uninstaller silently and wait for it to finish
  ExecWait '"$INSTDIR\uninstall.exe" /S _?=$INSTDIR'
  
  Abort "Download Failed - Application has been uninstalled."

extract_failed:
  MessageBox MB_OK|MB_ICONSTOP "Failed to extract PyTorch libraries. Error: $R0"
  Abort "Extraction Failed"

extract_ok:
  DetailPrint "PyTorch libraries installed successfully."

extract_end:
  ; Clean up the downloaded zip file
  Delete "$PLUGINSDIR\torch-lib.zip"
SectionEnd

; --- Logic to run before the installer UI loads ---
; This function is now placed AFTER the sections are defined.
Function .onInit
  ; This function checks if the PyTorch libs seem to be installed already.
  ; If they are, it will automatically uncheck the "Download PyTorch" section.
  
  ; Read the proposed installation directory
  StrCpy $INSTDIR "$PROGRAMFILES\MangaOCRTool"
  
  ; Check if the key torch library file exists
  IfFileExists "${CHECK_FILE}" PathFound PathNotFound

PathFound:
  ; The file exists. This is likely an update.
  ; We will deselect the download section so it is skipped by default.
  ; The user can still manually check it if they want to force a re-download.
  SectionSetFlags ${SecPyTorch} 0 ; SF_SELECTED = 1. We set flags to 0 to unselect.
  Goto End

PathNotFound:
  ; The file does not exist. This is a fresh installation.
  ; Ensure the download section is selected and read-only.
  SectionSetFlags ${SecPyTorch} ${SF_SELECTED}|${SF_RO}
  
  ; **Add the warning message to the Welcome page**
  !define MUI_WELCOMEPAGE_TEXT "This setup will guide you through the installation of MangaOCRTool.$\r$\n$\r$\n\
    **Important:** An active internet connection is required for the initial installation to download necessary PyTorch libraries (approx. 4 GB).$\r$\n$\r$\n\
    Please ensure you are connected to the internet before proceeding.$\r$\n$\r$\n\
    Click Next to continue."
  
End:
FunctionEnd


; --- Uninstaller Section ---
Section "Uninstall"
  ; Remove the entire installation directory
  RMDir /r "$INSTDIR"

  ;  Remove file association from the registry 
  DetailPrint "Removing .mmtl file association..."
  DeleteRegKey HKCR ".mmtl"
  DeleteRegKey HKCR "MangaOCRTool.MMTLFile"

  ; Remove uninstaller keys from the registry
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool"
SectionEnd