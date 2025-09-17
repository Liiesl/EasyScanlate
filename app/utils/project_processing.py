import os
import zipfile
import json
import tempfile
import re
from shutil import copyfile, rmtree
from app.ui.dialogs.project_dialog import NewProjectDialog, ImportWFWFDialog
from PySide6.QtWidgets import QMessageBox, QFileDialog, QDialog, QApplication
from PySide6.QtCore import QDateTime, QDir, Qt

def new_project(self):
    dialog = NewProjectDialog(self)
    if dialog.exec_() == QDialog.Accepted:
        source_path, project_path, language = dialog.get_paths()
        
        if not source_path or not project_path:
            QMessageBox.warning(self, "Error", "Please select both source and project location")
            return
            
        try:
            # Create new .mmtl file
            with zipfile.ZipFile(project_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
                # Create meta.json
                meta = {
                    'created': QDateTime.currentDateTime().toString(Qt.ISODate),
                    'source': source_path,
                    'original_language': language,
                    'version': '1.0'
                }
                zipf.writestr('meta.json', json.dumps(meta, indent=2))
                
                # Add images
                images_dir = 'images/'
                if os.path.isfile(source_path):
                    # If it's a single file, just add it
                    zipf.write(source_path, os.path.join(images_dir, os.path.basename(source_path)))
                elif os.path.isdir(source_path):
                    # --- START OF MODIFIED SECTION ---
                    # If it's a directory, correct the filenames to ensure sequential order
                    filename_map = correct_filenames(source_path)
                    
                    # Add images to the zip with their new, corrected names
                    for original_name, new_name in filename_map.items():
                        if new_name.lower().endswith(('png', 'jpg', 'jpeg')):
                            # Source path uses the original filename
                            src_path = os.path.join(source_path, original_name)
                            # Destination path inside the zip uses the new, standardized filename
                            dst_path_in_zip = os.path.join(images_dir, new_name)
                            zipf.write(src_path, dst_path_in_zip)
                    # --- END OF MODIFIED SECTION ---
                
                # Create empty OCR results
                zipf.writestr('master.json', json.dumps([]))  # Empty list
            
            launch_project(self, project_path)
            
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Failed to create project: {str(e)}")

def open_project(self):
    file, _ = QFileDialog.getOpenFileName(self, "Open Project", "", "Manga Translation Files (*.mmtl)")
    if file:
        launch_project(self, file)  # Use the universal launch_project function

def import_from_wfwf(self):
    dialog = ImportWFWFDialog(self)
    if dialog.exec_() == QDialog.Accepted:
        temp_dir = dialog.get_temp_dir()
        if not temp_dir or not os.path.exists(temp_dir):
            QMessageBox.warning(self, "Error", "No downloaded images found.")
            return
        
        project_path, _ = QFileDialog.getSaveFileName(
            self, "Save Project", QDir.homePath(), "Manga Translation Files (*.mmtl)"
        )
        
        if not project_path:
            rmtree(temp_dir)
            return
        
        try:
            # Run number correction on downloaded files
            filename_map = correct_filenames(temp_dir)
            
            # Create a temporary directory for corrected filenames
            corrected_dir = tempfile.mkdtemp()
            
            # Copy files with corrected names
            for old_name, new_name in filename_map.items():
                if old_name.lower().endswith(('png', 'jpg', 'jpeg')):
                    src = os.path.join(temp_dir, old_name)
                    dst = os.path.join(corrected_dir, new_name)
                    copyfile(src, dst)
            
            with zipfile.ZipFile(project_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
                meta = {
                    'created': QDateTime.currentDateTime().toString(Qt.ISODate),
                    'source': dialog.get_url(),
                    'version': '1.0'
                }
                zipf.writestr('meta.json', json.dumps(meta, indent=2))
                
                images_dir = 'images/'
                for img in os.listdir(corrected_dir):
                    if img.lower().endswith(('png', 'jpg', 'jpeg')):
                        zipf.write(os.path.join(corrected_dir, img), os.path.join(images_dir, img))
                
                zipf.writestr('master.json', json.dumps([]))
            
            launch_project(self, project_path)
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Failed to create project: {str(e)}")
        finally:
            rmtree(temp_dir)
            if 'corrected_dir' in locals() and os.path.exists(corrected_dir):
                rmtree(corrected_dir)

def launch_project(self, mmtl_path):
    """
    Universal function to launch a project from an mmtl file.
    Works for both Home and MenuBar classes.
    """
    # Check which class is calling this function
    if hasattr(self, 'launch_main_app'):
        # Called from Home class
        self.launch_main_app(mmtl_path)
    elif hasattr(self, 'main_window'):
        # Called from MenuBar class
        from app.ui.window.main_window import LoadingDialog, ProjectLoaderThread
        
        # Show loading dialog
        loading_dialog = LoadingDialog(self)
        loading_dialog.show()
        
        # Create and start loader thread - store as instance variable
        self.loader_thread = ProjectLoaderThread(mmtl_path)
        
        def handle_project_loaded(mmtl_path, temp_dir):
            try:
                from app.ui.window import MainWindow
                
                # Update recent projects if we're in the MenuBar class
                if hasattr(self, 'main_window'):
                    # Close current window and open new one
                    main_app = QApplication.instance()
                    old_window = self.main_window
                    
                    # Create main window
                    new_window = MainWindow()
                    new_window.process_mmtl(mmtl_path, temp_dir)
                    new_window.show()
                    
                    # Close the old window
                    old_window.close()
                
                loading_dialog.close()
            except Exception as e:
                QMessageBox.critical(self, "Error", f"Failed to launch project: {str(e)}")
                rmtree(temp_dir, ignore_errors=True)
            
            # Clean up the thread
            self.loader_thread.deleteLater()
        
        def handle_project_error(error_msg):
            loading_dialog.close()
            QMessageBox.critical(self, "Error", f"Failed to open project:\n{error_msg}")
            
            # Clean up the thread
            self.loader_thread.deleteLater()
        
        self.loader_thread.finished.connect(handle_project_loaded)
        self.loader_thread.error.connect(handle_project_error)
        self.loader_thread.start()
    else:
        QMessageBox.critical(self, "Error", "Cannot launch project: Unknown context")

def correct_filenames(directory):
    """
    Renames files by extracting the last number found in the filename,
    sorting numerically, and standardizing to a 'temp_XXXX.ext' format.
    This ensures correct sequential order regardless of the original naming scheme.
    Returns a dict mapping original filenames to corrected ones.
    """
    files = os.listdir(directory)
    numbered_files = []

    # 1. First pass: Collect all files that have numbers.
    for filename in files:
        # Find all sequences of digits in the filename.
        numbers = re.findall(r'\d+', filename)
        if numbers:
            # The page number is assumed to be the last one found.
            num = int(numbers[-1])
            _, ext = os.path.splitext(filename)
            numbered_files.append({'num': num, 'original': filename, 'ext': ext})

    # If no processable files were found, return a map of all files to themselves.
    if not numbered_files:
        return {f: f for f in files}

    # 2. Sort the collected files numerically based on the extracted number.
    numbered_files.sort(key=lambda x: x['num'])

    # 3. Build the final mapping.
    filename_map = {}
    
    # 4. Add the renamed numbered files to the map, using a new 1-based index.
    for i, file_info in enumerate(numbered_files, 1):
        # Create a new name like 'temp_0001.jpg', 'temp_0002.jpg', etc.
        new_name = f"temp_{str(i).zfill(4)}{file_info['ext']}"
        filename_map[file_info['original']] = new_name

    # 5. Add any files that were NOT numbered back into the map, with their original names.
    # This ensures files like "cover.jpg" or "notes.txt" are not lost.
    numbered_originals = {f['original'] for f in numbered_files}
    for f in files:
        if f not in numbered_originals:
            filename_map[f] = f
            
    return filename_map