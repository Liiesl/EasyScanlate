; NSIS Script for MangaOCRTool Online Installer
; =============================================

!include "MUI2.nsh"
!include "nsisunz.nsh" ; Required for unzipping

; --- General Information ---
Name "MangaOCRTool"
OutFile "MangaOCRTool-Installer.exe"
InstallDir "$PROGRAMFILES\MangaOCRTool"
RequestExecutionLevel admin ; Request admin rights for installing in Program Files

; --- GitHub Release URL for PyTorch ---
; !!! IMPORTANT !!!
; You MUST replace this URL with the actual URL of the pytorch-libs.zip file
; from your own GitHub repository's releases page.
!define PYTORCH_URL "https://github.com/YourUser/YourRepo/releases/download/v1.0.0/pytorch-libs.zip"

; --- Modern UI Settings ---
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; --- Installer Section: Main Application ---
; This section installs the main application files, excluding the large PyTorch libraries.
Section "Install Application"
  SetOutPath $INSTDIR

  ; Recursively add all files from the Nuitka build output,
  ; but EXCLUDE the PyTorch directories, as they will be downloaded.
  ; The path "build\main.dist\*" assumes the script is run from the repo root.
  File /r /x torch /x torchvision /x torchaudio "build\main.dist\*"
SectionEnd

; --- Installer Section: Download and Install PyTorch ---
; This section downloads and extracts the PyTorch libraries.
Section "Download and Install PyTorch"
  SetOutPath $INSTDIR
  DetailPrint "Downloading PyTorch libraries... (This may take several minutes)"
  
  ; Download the zip file from the URL defined above
  NSISdl::download "${PYTORCH_URL}" "$PLUGINSDIR\pytorch-libs.zip"
  Pop $R0 ; Get the return value from the download (e.g., "success")
  StrCmp $R0 "success" dl_ok dl_failed

dl_ok:
  DetailPrint "Download complete. Extracting files..."
  ; Unzip the downloaded file into the installation directory
  nsisunz::UnzipToLog "$PLUGINSDIR\pytorch-libs.zip" "$INSTDIR"
  Pop $R0 ; Get the return value from the unzip
  StrCmp $R0 "success" extract_ok extract_failed
  Goto extract_end

dl_failed:
  MessageBox MB_OK|MB_ICONSTOP "Failed to download PyTorch libraries. Please check your internet connection and try again."
  Abort "Download Failed"

extract_failed:
  MessageBox MB_OK|MB_ICONSTOP "Failed to extract PyTorch libraries."
  Abort "Extraction Failed"

extract_ok:
  DetailPrint "PyTorch libraries installed successfully."

extract_end:
  ; Clean up the downloaded zip file
  Delete "$PLUGINSDIR\pytorch-libs.zip"
SectionEnd

; --- Uninstaller Section ---
Section "Uninstall"
  ; Remove the entire installation directory
  RMDir /r "$INSTDIR"

  ; Remove uninstaller keys from the registry (optional but good practice)
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\MangaOCRTool"
SectionEnd