@echo off
setlocal enabledelayedexpansion

:: =============================================================================
:: Script to automate the update and tagging process for EasyScanlate
:: =============================================================================

echo  =======================================
echo  EasyScanlate Update and Release Script
echo  =======================================
echo.

:: --- Configuration ---
set "STABLE_BRANCH=stable"
set "MAIN_BRANCH=main"
set "INSTALLER_FILE=dev/installer/installer.nsi"

:: ============================
:: 1. Check Git Tag
:: ============================
echo --- Step 1: Checking Git Tags ---
git fetch --tags
for /f %%i in ('git describe --tags --abbrev^=0') do set LATEST_TAG=%%i

if defined LATEST_TAG (
    echo The newest tag is: %LATEST_TAG%
    set /p USE_LATEST="Do you want to use this tag? (y/n): "
    if /i "!USE_LATEST!"=="y" (
        set "CHOSEN_TAG=%LATEST_TAG%"
        set "IS_NEW_TAG=false"
    ) else (
        set /p CHOSEN_TAG="Enter the new tag (e.g., v1.2.3): "
        set "IS_NEW_TAG=true"
    )
) else (
    echo No existing tags found.
    set /p CHOSEN_TAG="Please enter the initial tag for your release (e.g., v0.1.0): "
    set "IS_NEW_TAG=true"
)
echo.

:: ============================
:: 2. Check Git Branch
:: ============================
echo --- Step 2: Verifying Git Branch ---
for /f %%i in ('git rev-parse --abbrev-ref HEAD') do set CURRENT_BRANCH=%%i

if /i not "!CURRENT_BRANCH!"=="%STABLE_BRANCH%" (
    echo You are on branch '!CURRENT_BRANCH!'. Switching to '%STABLE_BRANCH%'...
    git checkout %STABLE_BRANCH%
    if !errorlevel! neq 0 (
        echo ERROR: Failed to switch to branch '%STABLE_BRANCH%'.
        goto :eof
    )
) else (
    echo You are already on the '%STABLE_BRANCH%' branch.
)
echo.

:: ============================
:: 3. Merge from Main Branch
:: ============================
echo --- Step 3: Merging from Main Branch ---
set /p MERGE_MAIN="Do you want to merge from '%MAIN_BRANCH%' branch? (y/n): "
if /i "!MERGE_MAIN!"=="y" (
    echo Merging changes from '%MAIN_BRANCH%' into '%STABLE_BRANCH%'...
    git merge %MAIN_BRANCH%
    if !errorlevel! neq 0 (
        echo ERROR: Merge failed. Please resolve conflicts manually.
        goto :eof
    )
    echo Merge successful.
) else (
    echo Skipping merge from '%MAIN_BRANCH%'.
)
echo.

:: ============================
:: 4. Update Installer Version
:: ============================
echo --- Step 4: Updating Installer Version ---
set "VERSION_WITHOUT_V=%CHOSEN_TAG:v=%"
set "TEMP_FILE=%INSTALLER_FILE%.tmp"

echo Updating '%INSTALLER_FILE%' to version %VERSION_WITHOUT_V%...

(for /f "tokens=* delims=" %%a in (%INSTALLER_FILE%) do (
    set "line=%%a"
    echo !line! | findstr /b /c:"!define APP_VERSION" >nul
    if !errorlevel! equ 0 (
        echo !define APP_VERSION "!VERSION_WITHOUT_V!"
    ) else (
        echo !line!
    )
)) > "%TEMP_FILE%"

move /y "%TEMP_FILE%" "%INSTALLER_FILE%"
echo Installer version updated successfully.
echo.

:: Commit the version change
git add "%INSTALLER_FILE%"
git commit -m "chore: Update installer version to %CHOSEN_TAG%"
echo.

:: ============================
:: 5. Create and Push Git Tag
:: ============================
echo --- Step 5: Handling Git Tags ---
if %IS_NEW_TAG%==true (
    echo Creating and pushing new tag: %CHOSEN_TAG%
    git tag %CHOSEN_TAG%
    if !errorlevel! neq 0 (
        echo ERROR: Failed to create new tag. It might already exist locally.
        goto :eof
    )
    git push origin %CHOSEN_TAG%
    if !errorlevel! neq 0 (
        echo ERROR: Failed to push new tag to remote.
        goto :eof
    )
) else (
    echo Re-tagging with existing tag: %CHOSEN_TAG%
    
    echo 1. Deleting local tag...
    git tag -d %CHOSEN_TAG%
    if !errorlevel! neq 0 (
        echo WARNING: Could not delete local tag. It may not exist.
    )
    
    echo 2. Pushing deletion to remote...
    git push origin :refs/tags/%CHOSEN_TAG%
    if !errorlevel! neq 0 (
        echo WARNING: Could not delete remote tag. It may not exist on the remote.
    )
    
    echo 3. Creating new local tag...
    git tag %CHOSEN_TAG%
    if !errorlevel! neq 0 (
        echo ERROR: Failed to re-create local tag.
        goto :eof
    )
    
    echo 4. Pushing new tag to remote...
    git push origin %CHOSEN_TAG%
    if !errorlevel! neq 0 (
        echo ERROR: Failed to push updated tag to remote.
        goto :eof
    )
)
echo.
echo ======================================================
echo Done! The workflow has been successfully prepared.
echo ======================================================

endlocal