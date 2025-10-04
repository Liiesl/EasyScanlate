# updater.py
# A standalone application for checking and applying updates.

import sys
import os
import subprocess
import urllib.request
import json
import py7zr
import winreg # For reading the installed version from the registry
from packaging.version import parse as parse_version

from PySide6.QtWidgets import QApplication, QDialog, QVBoxLayout, QLabel, QProgressBar, QMessageBox
from PySide6.QtCore import QThread, Signal, Qt

# --- Configuration ---
# These MUST match your main application and NSIS script
GH_OWNER = "Liiesl"
GH_REPO = "ManhwaOCR"
APP_PUBLISHER = "YourCompany"
APP_NAME = "MangaOCRTool"
MAIN_APP_EXE = "main.exe"
INSTALLER_ASSET_NAME = "MangaOCRTool-Installer.exe"
DEPS_ASSET_NAME = "dependency-dll.7z"

# This registry key is written by the NSIS installer
REG_UNINSTALL_KEY = f"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{APP_NAME}"


class UpdateWorker(QThread):
    """Handles the update logic in a background thread."""
    progress_update = Signal(str)
    progress_percent = Signal(int)
    finished = Signal(bool, str) # Success (bool), Message (str)

    def get_installed_version(self):
        """Reads the application's version from the Windows Registry."""
        try:
            # NSIS installer writes to HKEY_LOCAL_MACHINE (HKLM)
            key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, REG_UNINSTALL_KEY, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY)
            version, _ = winreg.QueryValueEx(key, "DisplayVersion")
            winreg.CloseKey(key)
            return str(version)
        except FileNotFoundError:
            self.progress_update.emit("Registry key not found. Is the app installed?")
            return "0.0.0"
        except Exception as e:
            self.progress_update.emit(f"Error reading registry: {e}")
            return "0.0.0"

    def run(self):
        """Main update logic."""
        self.progress_update.emit("Checking for updates...")
        
        # 1. Get installed version from registry
        installed_version_str = self.get_installed_version()
        if installed_version_str == "0.0.0":
            self.finished.emit(False, "Could not determine installed version. Update cannot proceed.")
            return
            
        self.progress_update.emit(f"Current version: {installed_version_str}")
        installed_version = parse_version(installed_version_str)

        # 2. Get latest version from GitHub
        try:
            api_url = f"https://api.github.com/repos/{GH_OWNER}/{GH_REPO}/releases/latest"
            with urllib.request.urlopen(api_url) as response:
                if response.status != 200:
                    raise Exception(f"GitHub API Error: Status {response.status}")
                release_data = json.loads(response.read().decode())
            
            latest_version_str = release_data.get("tag_name", "v0.0.0").lstrip('v')
            self.progress_update.emit(f"Latest version available: {latest_version_str}")
            latest_version = parse_version(latest_version_str)
            
            assets = release_data.get("assets", [])
            installer_url = next((asset["browser_download_url"] for asset in assets if asset["name"] == INSTALLER_ASSET_NAME), None)
            deps_url = next((asset["browser_download_url"] for asset in assets if asset["name"] == DEPS_ASSET_NAME), None)

            if not installer_url or not deps_url:
                self.finished.emit(False, "Required update files not found in the latest release.")
                return

        except Exception as e:
            self.finished.emit(False, f"Failed to check for updates: {e}")
            return

        # 3. Compare versions
        if latest_version <= installed_version:
            self.finished.emit(True, "You are already using the latest version!")
            return

        self.progress_update.emit("New version found. Starting download...")
        
        # 4. Download files
        try:
            # Determine install location from registry
            key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, REG_UNINSTALL_KEY, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY)
            install_location, _ = winreg.QueryValueEx(key, "InstallLocation")
            winreg.CloseKey(key)

            temp_dir = os.path.join(os.environ["TEMP"], APP_NAME + "_Update")
            os.makedirs(temp_dir, exist_ok=True)
            
            installer_path = os.path.join(temp_dir, INSTALLER_ASSET_NAME)
            deps_path = os.path.join(temp_dir, DEPS_ASSET_NAME)
            
            self.download_file(installer_url, installer_path)
            self.download_file(deps_url, deps_path)

        except Exception as e:
            self.finished.emit(False, f"Download failed: {e}")
            return
            
        # 5. Run the silent update
        self.progress_update.emit("Download complete. Applying update...")
        self.progress_percent.emit(0)
        try:
            # The /S flag makes the NSIS installer run silently.
            # _?=$INSTDIR tells the uninstaller where the app is located,
            # ensuring it correctly preserves the 'torch' directory.
            command = [installer_path, '/S', f'_?={install_location}']
            # We use Popen and wait to ensure the installer finishes before we proceed.
            process = subprocess.Popen(command, shell=True)
            process.wait()

            # 6. Extract dependencies
            self.progress_update.emit("Extracting additional dependencies...")
            with py7zr.SevenZipFile(deps_path, mode='r') as z:
                z.extractall(path=install_location)
            
            self.progress_update.emit("Update complete. Restarting application...")
            
            # 7. Relaunch main application
            main_app_path = os.path.join(install_location, MAIN_APP_EXE)
            subprocess.Popen([main_app_path])
            
            self.finished.emit(True, "Update successful!")

        except Exception as e:
            self.finished.emit(False, f"An error occurred during update: {e}")

    def download_file(self, url, path):
        """Downloads a file and updates the progress bar."""
        self.progress_update.emit(f"Downloading {os.path.basename(path)}...")
        with urllib.request.urlopen(url) as response:
            total_size = int(response.getheader('Content-Length', 0))
            bytes_downloaded = 0
            chunk_size = 8192
            with open(path, 'wb') as f:
                while chunk := response.read(chunk_size):
                    f.write(chunk)
                    bytes_downloaded += len(chunk)
                    if total_size > 0:
                        percent = int((bytes_downloaded / total_size) * 100)
                        self.progress_percent.emit(percent)
        self.progress_percent.emit(100)


class UpdaterWindow(QDialog):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(f"{APP_NAME} Updater")
        self.setFixedSize(400, 150)
        self.setWindowFlags(self.windowFlags() & ~Qt.WindowContextHelpButtonHint)

        self.layout = QVBoxLayout(self)
        self.status_label = QLabel("Initializing updater...")
        self.status_label.setAlignment(Qt.AlignCenter)
        self.progress_bar = QProgressBar()
        
        self.layout.addWidget(self.status_label)
        self.layout.addWidget(self.progress_bar)

        self.worker = UpdateWorker()
        self.worker.progress_update.connect(self.status_label.setText)
        self.worker.progress_percent.connect(self.progress_bar.setValue)
        self.worker.finished.connect(self.on_update_finished)
        self.worker.start()

    def on_update_finished(self, success, message):
        """Called when the update process is complete."""
        if "already using the latest version" in message:
            QMessageBox.information(self, "No Updates", message)
        elif success:
            # The app will be restarted by the worker, so we just close.
            pass
        else:
            QMessageBox.critical(self, "Update Failed", message)
        
        self.close()

if __name__ == '__main__':
    # Nuitka will bundle PySide6, so this error handling is for script-based runs.
    try:
        from PySide6.QtWidgets import QApplication
    except ImportError:
        print("CRITICAL ERROR: PySide6 is required to run the updater.")
        sys.exit(1)
        
    app = QApplication(sys.argv)
    window = UpdaterWindow()
    window.show()
    sys.exit(app.exec())