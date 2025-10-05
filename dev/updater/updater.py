# updater.py
# A standalone, elevated-privilege application for applying updates.
# Expects 2 command-line arguments:
# 1. Path to the temp directory with manifest.json and update-package.zip
# 2. Path to the application's installation directory

import sys
import os
import json
import zipfile
import shutil
import subprocess
import hashlib

# Attempt to import bsdiff4. Must be compiled with the executable.
try:
    import bsdiff4 # type: ignore
except ImportError:
    print("CRITICAL: bsdiff4 module not found. Updater cannot proceed.")
    # In a GUI app, we might not see this print, but the worker will fail.
    bsdiff4 = None

from PySide6.QtWidgets import QApplication, QDialog, QVBoxLayout, QLabel, QProgressBar, QMessageBox
from PySide6.QtCore import QThread, Signal, Qt, QTimer

# --- Configuration ---
APP_NAME = "MangaOCRTool"
MAIN_APP_EXE = "main.exe"

def get_sha256(file_path):
    """Calculates the SHA256 hash of a file to verify integrity."""
    sha256 = hashlib.sha256()
    # Read in 64k chunks
    with open(file_path, "rb") as f:
        for byte_block in iter(lambda: f.read(65536), b""):
            sha256.update(byte_block)
    return sha256.hexdigest()

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
        self.zip_path = os.path.join(self.temp_dir, "update-package.zip")
        self.extract_path = os.path.join(self.temp_dir, "extracted")

    def run(self):
        """Main update logic: extract, patch, delete, copy."""
        try:
            # Pre-checks
            if bsdiff4 is None:
                raise ImportError("bsdiff4 is missing from the updater build.")
            if not os.path.exists(self.manifest_path):
                raise FileNotFoundError("manifest.json not found in temp directory.")
            if not os.path.exists(self.zip_path):
                raise FileNotFoundError("update-package.zip not found in temp directory.")
            if not os.path.isdir(self.install_dir):
                raise FileNotFoundError(f"Installation directory not found: {self.install_dir}")

            # 1. Load the manifest
            self.progress_update.emit("Reading update manifest...")
            with open(self.manifest_path, 'r') as f:
                manifest = json.load(f)

            # 2. Extract the update package (do this early to get patch files)
            self.progress_update.emit("Extracting update files...")
            self.progress_percent.emit(10)
            
            # Clean extract path if it exists from a previous failed run
            if os.path.exists(self.extract_path):
                shutil.rmtree(self.extract_path)
            os.makedirs(self.extract_path, exist_ok=True)

            with zipfile.ZipFile(self.zip_path, 'r') as zip_ref:
                zip_ref.extractall(self.extract_path)

            # 3. Apply Binary Patch (if applicable)
            patch_info = manifest.get("patch")
            if patch_info:
                self.progress_update.emit("Applying differential patch...")
                self.progress_percent.emit(20)
                self._apply_patch(patch_info, manifest)
            else:
                self.progress_update.emit("No binary patch required.")

            # 4. Delete removed files
            files_to_remove = manifest.get("_removed_files", [])
            if files_to_remove:
                self.progress_update.emit(f"Cleaning up {len(files_to_remove)} old files...")
                self._delete_old_files(files_to_remove)

            # 5. Copy new and updated files to the installation directory
            self.progress_update.emit("Installing new files...")
            self.progress_percent.emit(50)
            self._copy_new_files()

            # 6. Finish and Clean up
            self.progress_update.emit("Update complete. Cleaning up...")
            self.progress_percent.emit(100)
            
            # Clean up the temp directory passed as arg 1
            try:
                shutil.rmtree(self.temp_dir, ignore_errors=True)
            except OSError as e:
                print(f"Non-critical error cleaning temp dir: {e}")
            
            self.progress_update.emit("Restarting application...")
            
            # Relaunch main application
            main_app_path = os.path.join(self.install_dir, MAIN_APP_EXE)
            if os.path.exists(main_app_path):
                # Use Popen to launch and detach
                subprocess.Popen([main_app_path], close_fds=True, shell=False)
            else:
                raise FileNotFoundError(f"Could not find main executable to relaunch: {main_app_path}")
            
            self.finished.emit(True, "Update successful!")

        except Exception as e:
            print(f"Update Error: {e}") # Log to stdout for debugging if needed
            self.finished.emit(False, f"An error occurred during update:\n{str(e)}")

    def _apply_patch(self, patch_info, manifest):
        """Applies bsdiff patch directly into the installation directory."""
        target_filename = patch_info["file"]  # e.g., "main.exe"
        patch_filename = patch_info["patch_file"]  # e.g., "main.exe.patch"
        expected_old_hash = patch_info["old_sha256"]

        old_file_path = os.path.join(self.install_dir, target_filename)
        patch_file_path = os.path.join(self.extract_path, patch_filename)
        # Create the patched file with a .new extension for a safe, atomic replace
        patched_file_dest_temp = os.path.join(self.install_dir, target_filename + ".new")

        # Validation
        if not os.path.exists(old_file_path):
            raise FileNotFoundError(f"Cannot patch. Old file not found: {target_filename}")
        if not os.path.exists(patch_file_path):
            raise FileNotFoundError(f"Patch file missing from update package: {patch_filename}")

        self.progress_update.emit("Verifying current version integrity...")
        current_old_hash = get_sha256(old_file_path)
        if current_old_hash != expected_old_hash:
            raise Exception(
                f"Version mismatch. The installation cannot be patched safely.\n"
                f"Expected hash: {expected_old_hash[:8]}...\n"
                f"Found hash: {current_old_hash[:8]}..."
            )

        self.progress_update.emit("Patching binary in installation directory...")
        try:
            bsdiff4.file_patch(old_file_path, patched_file_dest_temp, patch_file_path)
        except Exception as e:
            # If patching fails, clean up the temporary .new file
            if os.path.exists(patched_file_dest_temp):
                os.remove(patched_file_dest_temp)
            raise Exception(f"Failed to apply binary patch: {e}")

        # Optional: Verify the *new* file against the manifest's file list
        expected_new_hash = manifest.get("files", {}).get(target_filename)
        if expected_new_hash:
             self.progress_update.emit("Verifying patched file...")
             if get_sha256(patched_file_dest_temp) != expected_new_hash:
                 # Clean up the failed patched file before raising the error
                 os.remove(patched_file_dest_temp)
                 raise Exception("Patched file integrity check failed.")

        # Atomically replace the old file with the new, patched one
        try:
            # os.replace is atomic and will overwrite the destination
            os.replace(patched_file_dest_temp, old_file_path)
        except OSError as e:
            # If the replace fails, clean up the .new file
            if os.path.exists(patched_file_dest_temp):
                os.remove(patched_file_dest_temp)
            raise Exception(f"Could not replace the application executable: {e}")

        # Remove the patch file so it isn't copied later
        os.remove(patch_file_path)

        # Also remove the (likely unpatched) original from the extracted package
        # to prevent _copy_new_files from overwriting our newly patched version.
        unpatched_in_extract_path = os.path.join(self.extract_path, target_filename)
        if os.path.exists(unpatched_in_extract_path):
            os.remove(unpatched_in_extract_path)


    def _delete_old_files(self, files_to_remove):
        """Removes files listed in the manifest."""
        count = len(files_to_remove)
        for i, relative_path in enumerate(files_to_remove):
            # Basic path traversal protection
            if ".." in relative_path or relative_path.startswith("/"):
                continue

            file_to_delete = os.path.join(self.install_dir, relative_path)
            try:
                if os.path.exists(file_to_delete) and os.path.isfile(file_to_delete):
                    os.remove(file_to_delete)
            except OSError as e:
                print(f"Could not remove {file_to_delete}: {e}")
            
            # Update progress between 30-50%
            current_progress = 30 + int((i / count) * 20)
            self.progress_percent.emit(current_progress)

    def _copy_new_files(self):
        """Moves files from extracted temp folder to install directory."""
        # Walk through the extracted directory
        files_to_copy = []
        for root, dirs, files in os.walk(self.extract_path):
            for file in files:
                # Path inside extracted folder
                src_path = os.path.join(root, file)
                # Relative path from extracted root
                rel_path = os.path.relpath(src_path, self.extract_path)
                files_to_copy.append((src_path, rel_path))

        total_files = len(files_to_copy)
        for i, (src_path, rel_path) in enumerate(files_to_copy):
            # Destination in install directory
            dest_path = os.path.join(self.install_dir, rel_path)

            # Ensure parent directory exists
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)

            # Use move for efficiency from temp to target; overwrite if exists
            shutil.move(src_path, dest_path)

            # Update progress between 50-95%
            current_progress = 50 + int(((i + 1) / total_files) * 45)
            self.progress_percent.emit(current_progress)
            # Optional: emit filename being copied for detailed feedback
            # self.progress_update.emit(f"Installing {rel_path}...")


class UpdaterWindow(QDialog):
    def __init__(self, temp_dir, install_dir):
        super().__init__()
        self.setWindowTitle(f"{APP_NAME} Updater")
        self.setFixedSize(400, 150)
        # Hide Help button and Minimize/Maximize, keep Close
        self.setWindowFlags(Qt.Dialog | Qt.WindowTitleHint | Qt.CustomizeWindowHint)

        self.layout = QVBoxLayout(self)
        
        # Title/Icon area could be added here
        
        self.status_label = QLabel("Initializing update...")
        self.status_label.setAlignment(Qt.AlignCenter)
        self.status_label.setWordWrap(True)
        
        self.progress_bar = QProgressBar()
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        
        self.layout.addWidget(self.status_label)
        self.layout.addWidget(self.progress_bar)

        # Prevent closing the window while updating
        self.updating = True

        self.worker = UpdateWorker(temp_dir, install_dir)
        self.worker.progress_update.connect(self.status_label.setText)
        self.worker.progress_percent.connect(self.progress_bar.setValue)
        self.worker.finished.connect(self.on_update_finished)
        
        # Give UI a moment to render before starting heavy IO
        QTimer.singleShot(500, self.worker.start)

    def closeEvent(self, event):
        """Prevent user from closing the window during update."""
        if self.updating:
            event.ignore()
        else:
            event.accept()

    def on_update_finished(self, success, message):
        """Called when the update process is complete."""
        self.updating = False
        self.progress_bar.setValue(100)
        
        if success:
            self.status_label.setText("Update complete. Launching application...")
            # Short delay so user sees "Complete" before window vanishes
            QTimer.singleShot(2000, self.close)
        else:
            self.status_label.setText("Update Failed.")
            QMessageBox.critical(self, "Update Failed", message + "\n\nPlease download the full installer from the website.")
            self.close()

if __name__ == '__main__':
    # Ensure we are running with admin privileges if installed in Program Files
    # (Skipping ctypes admin check here assuming Main App elevated it correctly)

    if len(sys.argv) != 3:
        # This is meant to be run programmatically, not by a user.
        # Provide mock args for testing if run directly without args
        # temp_directory = "C:\\path\\to\\temp\\update_data"
        # install_directory = "C:\\path\\to\\install\\dir"
        print("ERROR: Incorrect arguments.")
        print("Usage: Updater.exe <temp_data_dir> <install_dir>")
        sys.exit(1)
    else:
        temp_directory = sys.argv[1]
        install_directory = sys.argv[2]

    app = QApplication(sys.argv)
    
    window = UpdaterWindow(temp_directory, install_directory)
    window.show()
    sys.exit(app.exec())