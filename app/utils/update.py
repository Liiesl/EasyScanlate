# app/utils/update.py

import os
import sys
import json
import heapq
import shutil
import requests
import threading
from PySide6.QtCore import QObject, Signal, QStandardPaths, QUrl, QProcess, QCoreApplication, QSettings

# --- UPDATE CONSTANTS ---
GH_REPO = "Liiesl/EasyScanlate"

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
    return "v0.0.0" # Fallback version

class UpdateHandler(QObject):
    """Handles the backend logic for checking for and downloading updates."""
    # Signals to communicate with the UI (emitted from worker threads)
    update_check_finished = Signal(bool, dict)  # update_available, update_info
    download_progress = Signal(int, int)       # bytes_received, bytes_total
    download_finished = Signal(bool, str)     # success, path_to_update_dir
    status_changed = Signal(str)               # message for UI label
    error_occurred = Signal(str)               # error message

    def __init__(self, parent=None):
        super().__init__(parent)
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self.manifest_data = None
        self.update_path = []
        self.download_queue = []
        self.downloaded_files = []
        self.total_download_size = 0
        self.total_bytes_received = 0
        self.current_bytes_offset = 0
        self.latest_release_data = None # To store latest release info
        self.session = requests.Session() # Use a session for potential connection pooling

        self.app_version = get_app_version()
        # Create a specific directory for this update attempt
        self.update_temp_dir = os.path.join(QStandardPaths.writableLocation(QStandardPaths.AppLocalDataLocation), "update_package")

    def get_current_version(self):
        return self.app_version

    def check_for_updates(self):
        """Spawns a thread to fetch the latest release from the GitHub API."""
        self.status_changed.emit("Checking for updates...")
        self.latest_release_data = None # Reset previous check
        
        thread = threading.Thread(target=self._run_check_for_updates)
        thread.daemon = True
        thread.start()

    def _run_check_for_updates(self):
        """Fetches release info in a separate thread."""
        api_url = f"https://api.github.com/repos/{GH_REPO}/releases?per_page=1"
        try:
            response = self.session.get(api_url, timeout=15)
            response.raise_for_status() # Raise an exception for bad status codes (4xx or 5xx)
            releases = response.json()

            if not releases:
                self.status_changed.emit("No releases found on GitHub.")
                self.update_check_finished.emit(False, {})
                return

            latest_release = releases[0]
            latest_version = latest_release.get("tag_name")

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
            self.error_occurred.emit(f"Could not check for updates: {e}")
            self.update_check_finished.emit(False, {})
        except (json.JSONDecodeError, KeyError) as e:
            self.error_occurred.emit(f"Failed to parse GitHub API response: {e}")
            self.update_check_finished.emit(False, {})

    def download_manifest_and_start_update(self):
        """Spawns a thread to download the manifest and begin the update process."""
        if not self.latest_release_data:
            self.error_occurred.emit("Update check has not been run or found no new version.")
            self.download_finished.emit(False, "")
            return

        thread = threading.Thread(target=self._run_download_manifest)
        thread.daemon = True
        thread.start()

    def _run_download_manifest(self):
        """Downloads and processes the manifest in a thread."""
        manifest_asset = next((asset for asset in self.latest_release_data.get("assets", []) if asset["name"] == "manifest.json"), None)

        if not manifest_asset or "browser_download_url" not in manifest_asset:
            self.error_occurred.emit("A manifest.json file was not found in the latest release assets.")
            self.download_finished.emit(False, "")
            return

        self.status_changed.emit("Downloading update information...")
        manifest_url = manifest_asset["browser_download_url"]

        try:
            response = self.session.get(manifest_url, timeout=15)
            response.raise_for_status()
            
            self.manifest_data = response.content
            self.manifest = json.loads(self.manifest_data.decode())
            
            # Manifest is downloaded, now process it to find the update path
            self._process_manifest()

        except requests.exceptions.RequestException as e:
            self.error_occurred.emit(f"Could not download update manifest: {e}")
            self.download_finished.emit(False, "")
        except json.JSONDecodeError as e:
            self.error_occurred.emit(f"Failed to parse update manifest: {e}")
            self.download_finished.emit(False, "")

    def _process_manifest(self):
        """Calculates the best update path from the manifest and starts the download."""
        try:
            target_version = self.latest_release_data.get("tag_name")
            if not target_version:
                raise ValueError("Could not determine target version for update.")

            self.update_path = self._find_update_path(self.app_version, target_version)
            
            if not self.update_path:
                self.error_occurred.emit(f"Update to {target_version} found, but no viable update path from {self.app_version} exists.")
                self.download_finished.emit(False, "")
                return

            # Path found, proceed to download the actual packages
            self.start_update_download()

        except (ValueError, KeyError) as e:
            self.error_occurred.emit(f"Failed to process update manifest: {e}")
            self.download_finished.emit(False, "")

    def _find_update_path(self, start_version, end_version):
        """Calculates the most efficient (smallest total size) update path using Dijkstra's algorithm."""
        edges = []
        for to_v, packages in self.manifest['packages'].items():
            for pkg in packages:
                edges.append((pkg['from_version'], to_v, pkg['size'], pkg))
        
        distances = {v: float('inf') for v in self.manifest['versions']}
        previous_nodes = {v: None for v in self.manifest['versions']}
        
        if start_version not in distances:
            return []
            
        distances[start_version] = 0
        pq = [(0, start_version)]

        while pq:
            current_dist, current_v = heapq.heappop(pq)
            if current_dist > distances[current_v]:
                continue
            
            for from_v, to_v, size, pkg in edges:
                if from_v == current_v:
                    new_dist = current_dist + size
                    if new_dist < distances[to_v]:
                        distances[to_v] = new_dist
                        previous_nodes[to_v] = (current_v, pkg, to_v)
                        heapq.heappush(pq, (new_dist, to_v))

        path = []
        current = end_version
        while current != start_version:
            prev_info = previous_nodes.get(current)
            if prev_info is None:
                return []
            prev_v, pkg_info, to_v_tag = prev_info
            pkg_info_with_target = pkg_info.copy()
            pkg_info_with_target['download_from_tag'] = to_v_tag
            path.append(pkg_info_with_target)
            current = prev_v
            
        path.reverse()
        return path

    def start_update_download(self):
        """Prepares for and starts downloading the chain of update packages."""
        if not self.update_path:
            self.error_occurred.emit("No update path calculated.")
            return

        # Clean up old update directory and recreate it
        if os.path.exists(self.update_temp_dir):
            shutil.rmtree(self.update_temp_dir)
        os.makedirs(self.update_temp_dir, exist_ok=True)
        
        # Save the manifest to be used by the updater
        with open(os.path.join(self.update_temp_dir, "manifest.json"), "wb") as f:
            f.write(self.manifest_data)
            
        self.download_queue = self.update_path.copy()
        self.downloaded_files = []
        self.total_download_size = sum(pkg['size'] for pkg in self.update_path)
        self.total_bytes_received = 0
        self.current_bytes_offset = 0

        self._start_next_download()

    def _start_next_download(self):
        """Spawns a thread to download the next file in the queue."""
        if not self.download_queue:
            self.status_changed.emit("All updates downloaded. Ready to install.")
            self.settings.setValue("downloaded_update_dir", self.update_temp_dir)
            self.download_finished.emit(True, self.update_temp_dir)
            return

        package = self.download_queue.pop(0)
        
        thread = threading.Thread(target=self._run_download_file, args=(package,))
        thread.daemon = True
        thread.start()

    def _run_download_file(self, package):
        """Downloads a single package file in a thread, with progress."""
        file_name = package['file']
        tag = package['download_from_tag']
        
        self.status_changed.emit(f"Downloading {file_name}...")
        url = f"https://github.com/{GH_REPO}/releases/download/{tag}/{file_name}"
        file_path = os.path.join(self.update_temp_dir, file_name)
        
        try:
            # Use streaming to avoid loading the whole file into memory and to track progress
            with self.session.get(url, stream=True, timeout=30) as response:
                response.raise_for_status()
                with open(file_path, 'wb') as f:
                    for chunk in response.iter_content(chunk_size=8192):
                        f.write(chunk)
                        # Update progress
                        bytes_received = len(chunk)
                        self.total_bytes_received += bytes_received
                        self.download_progress.emit(self.total_bytes_received, self.total_download_size)
            
            # Download of this file is complete
            self.downloaded_files.append({"file": file_name, "path": file_path})
            self.current_bytes_offset += os.path.getsize(file_path)
            
            # Start the next download in the chain
            self._start_next_download()

        except requests.exceptions.RequestException as e:
            self.error_occurred.emit(f"Download failed for {file_name}: {e}")
            self._cleanup_after_failure()
            self.download_finished.emit(False, "")
        except IOError as e:
            self.error_occurred.emit(f"Could not write file {file_name}: {e}")
            self._cleanup_after_failure()
            self.download_finished.emit(False, "")

    def _cleanup_after_failure(self):
        """Removes the entire update temp directory on failure."""
        if os.path.exists(self.update_temp_dir):
            shutil.rmtree(self.update_temp_dir, ignore_errors=True)
        self.downloaded_files = []

    def check_for_existing_download(self):
        """Checks if an update package was downloaded in a previous session."""
        try:
            update_dir = self.settings.value("downloaded_update_dir", "")
            if update_dir and os.path.exists(os.path.join(update_dir, "manifest.json")):
                return update_dir
        except (json.JSONDecodeError, TypeError):
            pass
        return None

    def apply_update(self, update_dir):
        """Launches the external updater executable with the temp dir and install dir."""
        # Determine the installation directory of the main application
        install_dir = os.path.dirname(sys.executable) if getattr(sys, 'frozen', False) else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
        updater_exe = os.path.join(install_dir, "updater", "Updater.exe")
        
        if not os.path.exists(updater_exe):
            self.error_occurred.emit(f"Updater executable not found at:\n{updater_exe}")
            return
            
        if not os.path.isdir(update_dir):
             self.error_occurred.emit("The update data directory is missing or invalid.")
             return
        
        self.settings.remove("downloaded_update_dir")
        
        # Arguments for the updater: 1. temp data directory, 2. install directory
        args = [update_dir, install_dir]
        
        if QProcess.startDetached(updater_exe, args):
            QCoreApplication.quit()
        else:
            self.error_occurred.emit("Failed to launch the updater process.")