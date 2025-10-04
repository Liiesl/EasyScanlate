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

from PySide6.QtWidgets import QApplication, QDialog, QVBoxLayout, QLabel, QProgressBar, QMessageBox
from PySide6.QtCore import QThread, Signal, Qt

# --- Configuration ---
# These are used for display and relaunching the app
APP_NAME = "MangaOCRTool"
MAIN_APP_EXE = "main.exe"
EXCLUDE_FROM_DELETION = ("torch",) # Directories to never delete from

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

    def run(self):
        """Main update logic: extract, delete, copy."""
        try:
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
            
            files_to_remove = manifest.get("_removed_files", [])
            
            # 2. Delete removed files
            if files_to_remove:
                self.progress_update.emit(f"Removing {len(files_to_remove)} old files...")
                for i, relative_path in enumerate(files_to_remove):
                    file_to_delete = os.path.join(self.install_dir, relative_path)
                    try:
                        if os.path.exists(file_to_delete):
                            os.remove(file_to_delete)
                    except OSError as e:
                        print(f"Could not remove {file_to_delete}: {e}") # Log error but continue
                    self.progress_percent.emit(int((i + 1) / len(files_to_remove) * 50))

            # 3. Extract the update package
            self.progress_update.emit("Extracting new files...")
            extract_path = os.path.join(self.temp_dir, "extracted")
            with zipfile.ZipFile(self.zip_path, 'r') as zip_ref:
                zip_ref.extractall(extract_path)

            # 4. Copy new and updated files to the installation directory
            self.progress_update.emit("Copying new files...")
            new_files = os.listdir(extract_path)
            total_files = len(new_files)
            for i, filename in enumerate(new_files):
                src_path = os.path.join(extract_path, filename)
                dest_path = os.path.join(self.install_dir, filename)
                
                # Ensure parent directory exists
                os.makedirs(os.path.dirname(dest_path), exist_ok=True)
                
                # Use move for efficiency; it's a temp directory anyway
                shutil.move(src_path, dest_path)
                self.progress_percent.emit(50 + int((i + 1) / total_files * 50))

            self.progress_update.emit("Update complete. Cleaning up...")
            
            # 5. Clean up the temp directory
            shutil.rmtree(self.temp_dir, ignore_errors=True)
            
            self.progress_update.emit("Restarting application...")
            
            # 6. Relaunch main application
            main_app_path = os.path.join(self.install_dir, MAIN_APP_EXE)
            if os.path.exists(main_app_path):
                subprocess.Popen([main_app_path])
            
            self.finished.emit(True, "Update successful!")

        except Exception as e:
            self.finished.emit(False, f"An error occurred during update: {e}")


class UpdaterWindow(QDialog):
    def __init__(self, temp_dir, install_dir):
        super().__init__()
        self.setWindowTitle(f"{APP_NAME} Updater")
        self.setFixedSize(400, 150)
        self.setWindowFlags(self.windowFlags() & ~Qt.WindowContextHelpButtonHint)

        self.layout = QVBoxLayout(self)
        self.status_label = QLabel("Applying update...")
        self.status_label.setAlignment(Qt.AlignCenter)
        self.progress_bar = QProgressBar()
        
        self.layout.addWidget(self.status_label)
        self.layout.addWidget(self.progress_bar)

        self.worker = UpdateWorker(temp_dir, install_dir)
        self.worker.progress_update.connect(self.status_label.setText)
        self.worker.progress_percent.connect(self.progress_bar.setValue)
        self.worker.finished.connect(self.on_update_finished)
        self.worker.start()

    def on_update_finished(self, success, message):
        """Called when the update process is complete."""
        if success:
            # The worker handles relaunching, so we just close.
            QMessageBox.information(self, "Update Complete", message)
        else:
            QMessageBox.critical(self, "Update Failed", message)
        
        self.close()

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print("CRITICAL ERROR: This updater requires two arguments:")
        print("1. The temporary directory containing update files.")
        print("2. The target installation directory.")
        sys.exit(1)

    temp_directory = sys.argv[1]
    install_directory = sys.argv[2]
        
    app = QApplication(sys.argv)
    window = UpdaterWindow(temp_directory, install_directory)
    window.show()
    sys.exit(app.exec())