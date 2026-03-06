# app/utils/update.py

import os
import sys
import json
import shutil
import requests
import threading
from PySide6.QtCore import QObject, Signal, QStandardPaths, QProcess, QCoreApplication, QSettings

# --- UPDATE CONSTANTS ---
GH_REPO = "Liiesl/EasyScanlate"
INSTALLER_NAME = "EasyScanlate-Installer.exe"


def get_app_version():
    """Reads the application version from the APPVERSION file."""
    try:
        # Determine the base path, whether running as a script or a frozen exe
        base_path = os.path.dirname(sys.executable) if getattr(sys, 'frozen', False) else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
        version_file = os.path.join(base_path, "APPVERSION")
        if os.path.exists(version_file):
            with open(version_file, 'r') as f:
                return f.read().strip()
    except Exception:
        print("Failed to read APPVERSION file. Using fallback.")
    return "v0.0.0"  # Fallback version


class UpdateHandler(QObject):
    """Handles the backend logic for checking for and downloading updates."""
    # Signals to communicate with the UI (emitted from worker threads)
    update_check_finished = Signal(bool, dict)  # update_available, update_info
    download_progress = Signal(int, int)        # bytes_received, bytes_total
    download_finished = Signal(bool, str)       # success, path_to_installer
    download_cancelled = Signal()               # emitted when download was cancelled
    status_changed = Signal(str)                # message for UI label
    error_occurred = Signal(str)                # error message

    def __init__(self, parent=None):
        super().__init__(parent)
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self.latest_release_data = None
        self.session = requests.Session()
        self._abort_check_flag = False
        self._cancel_download_flag = threading.Event()
        self._download_thread = None
        self._download_in_progress = False
        self.installer_path = None

        self.app_version = get_app_version()
        # Create a specific directory for this update attempt
        self.update_temp_dir = os.path.join(QStandardPaths.writableLocation(QStandardPaths.AppLocalDataLocation), "update_package")

    def get_current_version(self):
        return self.app_version

    def is_download_in_progress(self):
        """Returns True if a download is currently in progress."""
        return self._download_in_progress

    def check_for_updates(self):
        """Spawns a thread to fetch the latest release from the GitHub API."""
        self.status_changed.emit("Checking for updates...")
        self.latest_release_data = None
        self._abort_check_flag = False

        thread = threading.Thread(target=self._run_check_for_updates)
        thread.daemon = True
        thread.start()

    def _run_check_for_updates(self):
        """Fetches release info in a separate thread."""
        api_url = f"https://api.github.com/repos/{GH_REPO}/releases?per_page=1"
        try:
            response = self.session.get(api_url, timeout=15)
            if self._abort_check_flag:
                return

            response.raise_for_status()
            if self._abort_check_flag:
                return

            releases = response.json()
            if self._abort_check_flag:
                return

            if not releases:
                self.status_changed.emit("No releases found on GitHub.")
                self.update_check_finished.emit(False, {})
                return

            latest_release = releases[0]
            latest_version = latest_release.get("tag_name")
            if self._abort_check_flag:
                return

            if not latest_version:
                self.error_occurred.emit("Latest release found, but it has no version tag.")
                self.update_check_finished.emit(False, {})
                return

            if latest_version > self.app_version:
                self.status_changed.emit(f"Update available: {latest_version}")
                self.latest_release_data = latest_release
                update_info = {"to_version": latest_version}
                self.update_check_finished.emit(True, update_info)
            else:
                self.status_changed.emit("You are using the latest version.")
                self.update_check_finished.emit(False, {})

        except requests.exceptions.RequestException as e:
            if self._abort_check_flag:
                return
            self.error_occurred.emit(f"Could not check for updates: {e}")
            self.update_check_finished.emit(False, {})
        except (json.JSONDecodeError, KeyError) as e:
            if self._abort_check_flag:
                return
            self.error_occurred.emit(f"Failed to parse GitHub API response: {e}")
            self.update_check_finished.emit(False, {})

    def start_update_download(self):
        """Starts downloading the installer for the latest release."""
        if not self.latest_release_data:
            self.error_occurred.emit("Update check has not been run or found no new version.")
            self.download_finished.emit(False, "")
            return

        if self._download_in_progress:
            self.error_occurred.emit("Download already in progress.")
            return

        # Reset cancellation flag
        self._cancel_download_flag.clear()
        self._download_in_progress = True

        self._download_thread = threading.Thread(target=self._run_download_installer)
        self._download_thread.daemon = True
        self._download_thread.start()

    def _run_download_installer(self):
        """Downloads the installer executable with progress tracking."""
        try:
            latest_version = self.latest_release_data.get("tag_name")
            if not latest_version:
                raise ValueError("Could not determine target version for update.")

            # Clean up old update directory and recreate it
            if os.path.exists(self.update_temp_dir):
                shutil.rmtree(self.update_temp_dir)
            os.makedirs(self.update_temp_dir, exist_ok=True)

            # Construct download URL
            url = f"https://github.com/{GH_REPO}/releases/download/{latest_version}/{INSTALLER_NAME}"
            self.installer_path = os.path.join(self.update_temp_dir, INSTALLER_NAME)

            self.status_changed.emit(f"Downloading {INSTALLER_NAME}...")

            # Download with progress tracking
            with self.session.get(url, stream=True, timeout=60) as response:
                response.raise_for_status()

                # Get total size if available
                total_size = int(response.headers.get('content-length', 0))
                bytes_received = 0

                with open(self.installer_path, 'wb') as f:
                    for chunk in response.iter_content(chunk_size=8192):
                        # Check for cancellation
                        if self._cancel_download_flag.is_set():
                            raise InterruptedError("Download cancelled by user")

                        if chunk:
                            f.write(chunk)
                            bytes_received += len(chunk)
                            self.download_progress.emit(bytes_received, total_size)

            # Download complete
            if self._cancel_download_flag.is_set():
                raise InterruptedError("Download cancelled by user")

            self.status_changed.emit("Download complete. Ready to install.")
            self.settings.setValue("downloaded_installer_path", self.installer_path)
            self._download_in_progress = False
            self.download_finished.emit(True, self.installer_path)

        except InterruptedError:
            # Clean up partial download
            self._cleanup_after_failure()
            self.status_changed.emit("Download cancelled.")
            self._download_in_progress = False
            self.download_cancelled.emit()

        except requests.exceptions.RequestException as e:
            self.error_occurred.emit(f"Download failed: {e}")
            self._cleanup_after_failure()
            self._download_in_progress = False
            self.download_finished.emit(False, "")

        except (IOError, ValueError) as e:
            self.error_occurred.emit(f"Download error: {e}")
            self._cleanup_after_failure()
            self._download_in_progress = False
            self.download_finished.emit(False, "")

    def cancel_download(self):
        """Signals the download thread to cancel."""
        if self._download_in_progress:
            self._cancel_download_flag.set()
            self.status_changed.emit("Cancelling download...")

    def _cleanup_after_failure(self):
        """Removes the entire update temp directory on failure."""
        if os.path.exists(self.update_temp_dir):
            shutil.rmtree(self.update_temp_dir, ignore_errors=True)
        self.installer_path = None

    def abort_check(self):
        """Flags the running update check thread to stop processing."""
        self._abort_check_flag = True

    def check_for_existing_download(self):
        """Checks if an installer was downloaded in a previous session."""
        try:
            installer_path = self.settings.value("downloaded_installer_path", "")
            if installer_path and os.path.exists(installer_path):
                self.installer_path = installer_path
                return installer_path
        except (TypeError, AttributeError):
            pass
        return None

    def apply_update(self, installer_path=None):
        """Launches the downloaded installer."""
        if installer_path is None:
            installer_path = self.installer_path

        if not installer_path or not os.path.exists(installer_path):
            self.error_occurred.emit("Installer not found. Please download again.")
            return

        self.settings.remove("downloaded_installer_path")

        # Launch the installer
        # Common installer flags: /SILENT or /VERYSILENT for Inno Setup
        args = ["/SILENT"]

        if QProcess.startDetached(installer_path, args):
            QCoreApplication.quit()
        else:
            self.error_occurred.emit("Failed to launch the installer.")
