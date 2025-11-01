# updater.py
# A standalone, elevated-privilege application for applying updates.
# Expects 2 command-line arguments:
# 1. Path to the temp directory with manifest.json and update packages
# 2. Path to the application's installation directory

import sys
import os
import json
import zipfile
import shutil
import subprocess
import hashlib
import heapq

# Attempt to import bsdiff4. Must be compiled with the executable.
try:
    import bsdiff4 # type: ignore
except ImportError:
    print("CRITICAL: bsdiff4 module not found. Updater cannot proceed.")
    bsdiff4 = None

from PySide6.QtWidgets import QApplication, QDialog, QVBoxLayout, QLabel, QProgressBar, QMessageBox
from PySide6.QtCore import QThread, Signal, Qt, QTimer

# --- Configuration ---
APP_NAME = "MangaOCRTool"
MAIN_APP_EXE = "main.exe"
APPVERSION_FILE = "APPVERSION"

def get_sha256(file_path):
    """Calculates the SHA256 hash of a file to verify integrity."""
    sha256 = hashlib.sha256()
    with open(file_path, "rb") as f:
        for byte_block in iter(lambda: f.read(65536), b""):
            sha256.update(byte_block)
    return sha256.hexdigest()
    
def get_current_app_version(install_dir):
    """Reads the application version from the APPVERSION file in the install directory."""
    try:
        version_file = os.path.join(install_dir, APPVERSION_FILE)
        if os.path.exists(version_file):
            with open(version_file, 'r') as f:
                return f.read().strip()
    except Exception:
        pass # Fallback to v0.0.0 if not found
    return "v0.0.0"


class UpdateWorker(QThread):
    """Handles the file operations for the update in a background thread."""
    progress_update = Signal(str)
    progress_percent = Signal(int)
    finished = Signal(bool, str) # Success (bool), Message (str)

    def __init__(self, temp_dir, install_dir):
        super().__init__()
        self.temp_dir = temp_dir
        self.install_dir = install_dir
        self.manifest_path = os.path.join(self.temp_dir, "manifest.json")
        self.extract_path = os.path.join(self.temp_dir, "extracted")

    def run(self):
        """Main update logic: find path, extract, patch, delete, copy for each step."""
        try:
            # Pre-checks
            if bsdiff4 is None:
                raise ImportError("bsdiff4 is missing from the updater build.")
            if not os.path.exists(self.manifest_path):
                raise FileNotFoundError("manifest.json not found in temp directory.")
            if not os.path.isdir(self.install_dir):
                raise FileNotFoundError(f"Installation directory not found: {self.install_dir}")

            # 1. Load the manifest
            self.progress_update.emit("Reading update manifest...")
            with open(self.manifest_path, 'r') as f:
                self.manifest = json.load(f)
            
            # 2. Determine the update path from current to latest version
            current_version = get_current_app_version(self.install_dir)
            all_versions = sorted(self.manifest["versions"].keys(), reverse=True)
            if not all_versions:
                 raise ValueError("No versions found in manifest.")
            latest_version = all_versions[0]
            
            update_path = self._find_update_path(current_version, latest_version)
            if not update_path:
                raise Exception(f"Could not find an update path from {current_version} to {latest_version}.")
            
            # 3. Process each update package in the chain
            total_steps = len(update_path)
            for i, package_info in enumerate(update_path):
                step_progress_start = int((i / total_steps) * 100)
                step_progress_end = int(((i + 1) / total_steps) * 100)
                
                self.progress_update.emit(f"Step {i+1}/{total_steps}: Applying update to {package_info['download_from_tag']}...")
                self.progress_percent.emit(step_progress_start)
                
                self._apply_single_update(package_info, step_progress_start, step_progress_end)
            
            # 4. Finalize installation by writing the new version
            self.progress_update.emit("Finalizing installation...")
            try:
                appversion_path = os.path.join(self.install_dir, APPVERSION_FILE)
                with open(appversion_path, 'w') as f:
                    f.write(latest_version)
            except OSError as e:
                print(f"Warning: Could not write to {APPVERSION_FILE} file: {e}")

            # 5. Finish and Clean up
            self.progress_update.emit("Update complete. Cleaning up...")
            self.progress_percent.emit(100)
            
            try:
                shutil.rmtree(self.temp_dir, ignore_errors=True)
            except OSError as e:
                print(f"Non-critical error cleaning temp dir: {e}")
            
            self.progress_update.emit("Restarting application...")
            
            main_app_path = os.path.join(self.install_dir, MAIN_APP_EXE)
            if os.path.exists(main_app_path):
                subprocess.Popen([main_app_path], close_fds=True, shell=False)
            else:
                raise FileNotFoundError(f"Could not find main executable to relaunch: {main_app_path}")
            
            self.finished.emit(True, "Update successful!")

        except Exception as e:
            print(f"Update Error: {e}")
            self.finished.emit(False, f"An error occurred during update:\n{str(e)}")

    def _apply_single_update(self, package_info, progress_start, progress_end):
        """Applies a single update package from the chain."""
        zip_path = os.path.join(self.temp_dir, package_info['file'])
        from_version = package_info['from_version']
        to_version = package_info['download_from_tag']

        if not os.path.exists(zip_path):
            raise FileNotFoundError(f"Update package {package_info['file']} not found.")

        # Extract the specific update package
        if os.path.exists(self.extract_path):
            shutil.rmtree(self.extract_path)
        os.makedirs(self.extract_path, exist_ok=True)
        
        with zipfile.ZipFile(zip_path, 'r') as zip_ref:
            zip_ref.extractall(self.extract_path)

        # The package itself contains a mini-manifest with patch info
        pkg_manifest_path = os.path.join(self.extract_path, "package-manifest.json")
        if not os.path.exists(pkg_manifest_path):
            raise FileNotFoundError("package-manifest.json missing from the update zip.")
        
        with open(pkg_manifest_path, 'r') as f:
            pkg_manifest = json.load(f)

        # Apply Binary Patch if it exists for this step
        patch_info = pkg_manifest.get("patch")
        if patch_info:
            self._apply_patch(patch_info)
        
        # Manifests for comparison
        from_manifest = self.manifest['versions'][from_version]
        to_manifest = self.manifest['versions'][to_version]

        # Delete removed files
        files_to_remove = [f for f in from_manifest if f not in to_manifest]
        self._delete_old_files(files_to_remove)

        # Copy new and updated files
        self._copy_new_files()

    def _find_update_path(self, start_version, end_version):
        """(Re-implementation for updater) Calculates the update path from the manifest."""
        edges = []
        for to_v, packages in self.manifest['packages'].items():
            for pkg in packages:
                # Add the target version to the package info for the updater
                pkg_with_target = pkg.copy()
                pkg_with_target['download_from_tag'] = to_v
                edges.append((pkg['from_version'], to_v, pkg['size'], pkg_with_target))
        
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
                        previous_nodes[to_v] = (current_v, pkg)
                        heapq.heappush(pq, (new_dist, to_v))

        path = []
        current = end_version
        while current != start_version:
            prev_info = previous_nodes.get(current)
            if prev_info is None:
                return []
            prev_v, pkg_info = prev_info
            path.append(pkg_info)
            current = prev_v
            
        path.reverse()
        return path

    def _apply_patch(self, patch_info):
        """Applies bsdiff patch."""
        target_filename = patch_info["file"]
        patch_filename = patch_info["patch_file"]
        expected_old_hash = patch_info["old_sha256"]

        old_file_path = os.path.join(self.install_dir, target_filename)
        patch_file_path = os.path.join(self.extract_path, patch_filename)
        patched_file_dest_temp = os.path.join(self.install_dir, target_filename + ".new")

        if not os.path.exists(old_file_path):
            raise FileNotFoundError(f"Cannot patch. Old file not found: {target_filename}")
        if not os.path.exists(patch_file_path):
            raise FileNotFoundError(f"Patch file missing from update package: {patch_filename}")

        self.progress_update.emit("Verifying current version...")
        current_old_hash = get_sha256(old_file_path)
        if current_old_hash != expected_old_hash:
            raise Exception("Version mismatch. Cannot be patched safely.")

        self.progress_update.emit(f"Patching {target_filename}...")
        try:
            bsdiff4.file_patch(old_file_path, patched_file_dest_temp, patch_file_path)
        except Exception as e:
            if os.path.exists(patched_file_dest_temp): os.remove(patched_file_dest_temp)
            raise Exception(f"Failed to apply binary patch: {e}")

        os.replace(patched_file_dest_temp, old_file_path)
        os.remove(patch_file_path)

        unpatched_in_extract = os.path.join(self.extract_path, target_filename)
        if os.path.exists(unpatched_in_extract):
            os.remove(unpatched_in_extract)


    def _delete_old_files(self, files_to_remove):
        """Removes files that are no longer in the new version."""
        self.progress_update.emit(f"Cleaning up {len(files_to_remove)} old files...")
        for relative_path in files_to_remove:
            if ".." in relative_path: continue
            file_to_delete = os.path.join(self.install_dir, relative_path)
            try:
                if os.path.isfile(file_to_delete): os.remove(file_to_delete)
                elif os.path.isdir(file_to_delete): shutil.rmtree(file_to_delete)
            except OSError as e:
                print(f"Could not remove {file_to_delete}: {e}")

    def _copy_new_files(self):
        """Moves new and updated files to the install directory."""
        self.progress_update.emit("Copying new files...")
        for root, _, files in os.walk(self.extract_path):
            for file in files:
                # Skip the package manifest itself
                if file == "package-manifest.json":
                    continue
                src_path = os.path.join(root, file)
                rel_path = os.path.relpath(src_path, self.extract_path)
                dest_path = os.path.join(self.install_dir, rel_path)
                os.makedirs(os.path.dirname(dest_path), exist_ok=True)
                shutil.move(src_path, dest_path)

class UpdaterWindow(QDialog):
    def __init__(self, temp_dir, install_dir):
        super().__init__()
        self.setWindowTitle(f"{APP_NAME} Updater")
        self.setFixedSize(400, 150)
        self.setWindowFlags(Qt.Dialog | Qt.WindowTitleHint | Qt.CustomizeWindowHint)

        self.layout = QVBoxLayout(self)
        
        self.status_label = QLabel("Initializing update...")
        self.status_label.setAlignment(Qt.AlignCenter)
        self.status_label.setWordWrap(True)
        
        self.progress_bar = QProgressBar()
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        
        self.layout.addWidget(self.status_label)
        self.layout.addWidget(self.progress_bar)

        self.updating = True

        self.worker = UpdateWorker(temp_dir, install_dir)
        self.worker.progress_update.connect(self.status_label.setText)
        self.worker.progress_percent.connect(self.progress_bar.setValue)
        self.worker.finished.connect(self.on_update_finished)
        
        QTimer.singleShot(500, self.worker.start)

    def closeEvent(self, event):
        if self.updating:
            event.ignore()
        else:
            event.accept()

    def on_update_finished(self, success, message):
        self.updating = False
        self.progress_bar.setValue(100)
        
        if success:
            self.status_label.setText("Update complete. Launching application...")
            QTimer.singleShot(2000, self.close)
        else:
            self.status_label.setText("Update Failed.")
            QMessageBox.critical(self, "Update Failed", message + "\n\nPlease download the full installer from the website.")
            self.close()

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print("ERROR: Incorrect arguments.")
        print("Usage: Updater.exe <temp_data_dir> <install_dir>")
        # For testing purposes, show a message box.
        app_test = QApplication.instance() or QApplication(sys.argv)
        QMessageBox.critical(None, "Argument Error", "This updater was launched with incorrect arguments and cannot proceed.")
        sys.exit(1)
    
    temp_directory = sys.argv[1]
    install_directory = sys.argv[2]

    app = QApplication(sys.argv)
    
    window = UpdaterWindow(temp_directory, install_directory)
    window.show()
    sys.exit(app.exec())