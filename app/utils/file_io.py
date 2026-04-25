import os
import ast
import json
from PySide6.QtWidgets import QMessageBox, QFileDialog, QInputDialog
from PySide6.QtCore import QRectF, QDir
from app.ui.components.image_area.label import ResizableImageLabel
from app.core.translations import generate_for_translate_content, import_translation_file_content
import zipfile

def export_translated_images_to_zip(image_paths_with_names, output_path):
    """Export translated images into a ZIP file."""
    try:
        # Create a ZIP file and add images
        with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
            for image_path, image_name in image_paths_with_names:
                zipf.write(image_path, image_name)
        
        return output_path, True  # Return the path and success status
    except Exception as e:
        print(f"Error exporting images: {e}")
        return None, False

def export_ocr_results(self):
    """Export OCR results to either a JSON file (Master) or an XML file (For-Translate)."""
    if not self.model.ocr_results:
        QMessageBox.warning(self, "Error", "No OCR results to export.")
        return

    try:
        from app.ui.dialogs.import_export_dialog import ExportDialog
        from PySide6.QtWidgets import QDialog
        
        # Get available profiles
        available_profiles = list(self.model.profiles.keys())
        if not available_profiles:
            available_profiles = ["Original"]
        
        # Get project info for default path
        project_directory = os.path.dirname(self.model.mmtl_path) if self.model.mmtl_path else None
        project_name = self.model.project_name if hasattr(self.model, 'project_name') else None
        
        # Show export dialog
        dialog = ExportDialog(self, available_profiles, project_name, project_directory)
        if dialog.exec_() != QDialog.Accepted:
            return
        
        config = dialog.get_export_config()
        
        try:
            file_path = config['output_path']
            
            if config['format'] == 'master':
                # Export as Master JSON
                indent = 4 if config['pretty_print'] else None
                with open(file_path, 'w', encoding='utf-8') as f:
                    json.dump(self.model.ocr_results, f, ensure_ascii=False, indent=indent)
                QMessageBox.information(self, "Success", "OCR results exported successfully in JSON format.")
            
            elif config['format'] == 'for-translate':
                # Export as For-Translate XML/TXT
                profile_name = config['profile_name']
                content = generate_for_translate_content(self.model.ocr_results, profile_name)
                
                with open(file_path, 'w', encoding='utf-8') as f:
                    f.write(content)
                
                format_str = config['file_format'].upper()
                QMessageBox.information(self, "Success", 
                                       f"OCR results exported successfully in {format_str} format.\nProfile: {profile_name}")
        
        except Exception as e:
            import traceback
            from app.ui.dialogs.error_dialog import ErrorDialog
            ErrorDialog.critical(self, "Export Error", f"Failed to export OCR results:\n{str(e)}", traceback.format_exc())
    except Exception as e:
        import traceback
        from app.ui.dialogs.error_dialog import ErrorDialog
        ErrorDialog.critical(
            self, "Dialog Error",
            f"Failed to open export dialog:\n{str(e)}",
            traceback.format_exc()
        )

def import_master_file(self, file_path=None, skip_confirmation=False):
    """Import Master (JSON) file and replace entire OCR results.
    
    Args:
        file_path: Optional file path. If None, opens file dialog.
        skip_confirmation: If True, skip the confirmation dialog (used when called from ImportDialog).
    
    Returns:
        List of OCR results if successful, None otherwise.
    """
    if file_path is None:
        file_path, _ = QFileDialog.getOpenFileName(
            self, "Import Master File", "", "JSON Files (*.json);;All Files (*.*)"
        )
        if not file_path:
            return None

    try:
        with open(file_path, 'r', encoding='utf-8') as file:
            content = file.read()
        
        new_ocr_results = json.loads(content)
        if not isinstance(new_ocr_results, list):
            raise ValueError("Invalid JSON format: Expected a list of OCR results.")

        # Check if we have existing OCR results (only if not skipping confirmation)
        if not skip_confirmation and self.model.ocr_results:
            reply = QMessageBox.question(
                self,
                "Replace OCR Results?",
                "This will overwrite existing OCR data. Continue?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No
            )
            if reply != QMessageBox.Yes:
                return None

        return new_ocr_results

    except Exception as e:
        import traceback
        from app.ui.dialogs.error_dialog import ErrorDialog
        ErrorDialog.critical(
            self, "Import Error",
            f"Failed to import master file:\n{str(e)}",
            traceback.format_exc()
        )
        return None

def import_translation_file_content_only(file_path):
    """
    Parse translation file (XML/TXT/MD) and return translation data dictionary.
    Returns: {filename: {row_number_str: translated_text}} or None on error.
    Handles both new XML format and old Markdown format for backward compatibility.
    """
    try:
        with open(file_path, 'r', encoding='utf-8') as file:
            content = file.read()
        
        # Detect file type
        if file_path.endswith('.xml') or file_path.endswith('.txt'):
            # New XML format - use the translation system parser
            return import_translation_file_content(content)
        
        elif file_path.endswith('.md'):
            # Old Markdown format - parse for backward compatibility
            if '<!-- type: for-translate -->' not in content:
                raise ValueError("Unsupported MD format - missing type comment.")

            translations = {}
            current_file = None
            file_texts = {}
            current_entry = []  # Buffer for current text entry
            row_numbers = []

            # Parse filename groups and entries
            for line in content.split('\n'):
                line = line.strip()
                if line.startswith('<!-- file:') and line.endswith('-->'):
                    # Save previous file's entries
                    if current_file is not None and current_entry:
                        file_texts[current_file].append(('\n'.join(current_entry), row_numbers))
                        current_entry = []
                        row_numbers = []
                    # Extract filename
                    current_file = line[10:-3].strip()
                    file_texts[current_file] = []
                elif line.startswith('-/') and line.endswith('\\-'):
                    # Extract row number from marker (e.g., "-/3\\-")
                    if current_entry:
                        row_number_str = line[2:-2].strip()
                        try:
                            row_number = int(row_number_str)
                            row_numbers.append(row_number)
                            # Save current entry with accumulated row_numbers
                            file_texts[current_file].append(('\n'.join(current_entry), list(row_numbers)))
                            current_entry = []
                            row_numbers = []
                        except ValueError:
                            # Skip invalid row numbers
                            pass
                elif current_file is not None:
                    # Skip empty lines between entries
                    if line or current_entry:
                        current_entry.append(line)

            # Add the last entry if buffer isn't empty
            if current_file is not None and current_entry:
                file_texts[current_file].append(('\n'.join(current_entry), row_numbers))

            # Convert to dictionary format: {filename: {row_number_str: translated_text}}
            # If an entry has multiple row_numbers, use the first one as the key
            for filename, entries in file_texts.items():
                translations[filename] = {}
                for translated_text, row_nums in entries:
                    if row_nums:
                        # Use the first row_number as the key (typically there's only one)
                        row_number_str = str(row_nums[0])
                        translations[filename][row_number_str] = translated_text

            return translations
        
        else:
            raise ValueError("Unsupported file format. Please provide XML, TXT, or MD file.")
    
    except Exception as e:
        raise ValueError(f"Failed to parse translation file: {str(e)}")

def import_translation_file(self):
    """
    Unified import function that handles both Master and Translation files.
    Opens file explorer first, then shows confirmation (master) or dialog (translation).
    Returns True if successful, False otherwise.
    """
    try:
        # Open file explorer first
        file_path, _ = QFileDialog.getOpenFileName(
            self, "Import File", QDir.homePath(),
            "All Supported Files (*.json *.xml *.txt *.md);;JSON Files (*.json);;XML Files (*.xml);;Text Files (*.txt);;Markdown Files (*.md);;All Files (*.*)"
        )
        
        if not file_path:
            return False
    except Exception as e:
        import traceback
        from app.ui.dialogs.error_dialog import ErrorDialog
        ErrorDialog.critical(
            self, "File Dialog Error",
            f"Failed to open file selection dialog:\n{str(e)}",
            traceback.format_exc()
        )
        return False
    
    # Determine file type based on extension
    is_master = file_path.endswith('.json')
    
    if is_master:
        # Master file - show confirmation dialog directly
        new_ocr_results = import_master_file(self, file_path, skip_confirmation=False)
        if new_ocr_results is not None:
            try:
                # Always replace existing OCR results
                self.model.ocr_results = new_ocr_results
                
                # Reload profiles from the OCR results
                loaded_profiles = set(["Original"])
                max_row_num = -1
                
                for res in self.model.ocr_results:
                    if 'row_number' in res:
                        try:
                            max_row_num = max(max_row_num, int(float(res['row_number'])))
                        except (ValueError, TypeError):
                            pass
                    if 'translations' in res and isinstance(res['translations'], dict):
                        for profile_name in res['translations']:
                            loaded_profiles.add(profile_name)
                
                # Update next_global_row_number
                if max_row_num >= 0:
                    self.model.next_global_row_number = max_row_num + 1
                
                # Replace profiles
                self.model.profiles = {name: {} for name in loaded_profiles}
                
                # Check if current active profile still exists, if not default to "Original"
                if self.model.active_profile_name not in self.model.profiles:
                    print(f"Warning: Active profile '{self.model.active_profile_name}' not found in imported data. Defaulting to 'Original'.")
                    self.model.active_profile_name = "Original"
                
                # Emit signals to update UI reactively via ViewModels
                self.model.profiles_updated.emit()
                self.model.model_updated.emit([])
                self.model.image_list_changed.emit()
                
                QMessageBox.information(self, "Success", 
                                      f"Master file imported successfully!\n"
                                      f"Loaded {len(new_ocr_results)} OCR entries.\n"
                                      f"Profiles: {', '.join(sorted(self.model.profiles.keys()))}")
                return True
            except Exception as e:
                import traceback
                from app.ui.dialogs.error_dialog import ErrorDialog
                ErrorDialog.critical(
                    self, "Import Processing Error",
                    f"Failed to process imported master file:\n{str(e)}",
                    traceback.format_exc()
                )
                return False
        return False
    else:
        # Translation file - show import dialog for profile selection
        try:
            from app.ui.dialogs.import_export_dialog import ImportDialog
            from PySide6.QtWidgets import QDialog
            
            # Get available profiles
            available_profiles = list(self.model.profiles.keys())
            
            # Show import dialog with file pre-selected
            dialog = ImportDialog(self, available_profiles)
            dialog.file_path = file_path
            dialog.file_path_edit.setText(file_path)
            
            # Set default profile name to filename (without extension)
            filename = os.path.splitext(os.path.basename(file_path))[0]
            if filename in available_profiles:
                # If filename matches an existing profile, select it
                dialog.profile_combo.setCurrentText(filename)
            else:
                # If filename doesn't match, keep "Create New Profile" and prefill the name
                dialog.profile_combo.setCurrentIndex(0)  # "<Create New Profile>"
                dialog.new_profile_edit.setText(filename)
            
            if dialog.exec_() != QDialog.Accepted:
                return False
            
            config = dialog.get_import_config()
            file_path = config['file_path']
            
            try:
                # Translation file (XML/TXT/MD) - apply to a profile
                translation_data = import_translation_file_content_only(file_path)
                if translation_data:
                    # Get profile name from config
                    profile_name = config['profile_name']
                    
                    if not profile_name:
                        QMessageBox.warning(self, "Invalid Profile", "Please specify a valid profile name.")
                        return False
                    
                    # Set flag to prevent textChanged events from deleting translations during profile switch
                    # add_profile switches to the new profile and emits signals that clear highlighters
                    if hasattr(self, 'results_widget') and self.results_widget:
                        self.results_widget._is_updating_views = True
                    
                    # Apply translation to profile
                    self.model.add_profile(profile_name, translation_data)
                    
                    QMessageBox.information(self, "Success", 
                                          f"Translation successfully applied to profile:\n'{profile_name}'")
                    return True
                return False
            
            except ValueError as e:
                # ValueError from import_translation_file_content_only - show as warning
                QMessageBox.warning(self, "Import Error", f"Failed to parse translation file:\n{str(e)}")
                return False
            except Exception as e:
                import traceback
                from app.ui.dialogs.error_dialog import ErrorDialog
                ErrorDialog.critical(
                    self, "Import Error", 
                    f"Failed to import and apply translation file:\n{str(e)}",
                    traceback.format_exc()
                )
                return False
        except Exception as e:
            import traceback
            from app.ui.dialogs.error_dialog import ErrorDialog
            ErrorDialog.critical(
                self, "Dialog Error",
                f"Failed to open import dialog:\n{str(e)}",
                traceback.format_exc()
            )
            return False

def export_rendered_images(main_window, scroll_layout):
    """Export images with applied translations directly from QGraphicsView scenes."""
    if not main_window.model.image_paths:
        QMessageBox.warning(main_window, "Warning", "No images available for export.")
        return

    # Ask user for save location, defaulting to the project's directory.
    project_directory = os.path.dirname(main_window.model.mmtl_path) if main_window.model.mmtl_path else ""
    default_filename = f"{main_window.model.project_name}.zip"
    default_path = os.path.join(project_directory, default_filename)
    
    export_path, _ = QFileDialog.getSaveFileName(
        main_window,
        "Export Rendered Images",
        default_path,
        "ZIP Files (*.zip)"
    )

    if not export_path:
        return # User cancelled
        
    # Suspend updates during export
    for i in range(scroll_layout.count()):
        widget = scroll_layout.itemAt(i).widget()
        if isinstance(widget, ResizableImageLabel):
            widget.setUpdatesEnabled(False)

    import tempfile, shutil
    from PySide6.QtGui import QImage, QPainter
    from PySide6.QtCore import Qt

    temp_dir = tempfile.mkdtemp()
    translated_images = []

    try:
        for i in range(scroll_layout.count()):
            widget = scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                scene = widget.scene()
                
                # --- CORRECTED LINE ---
                # The check for scene.isActive() has been removed.
                if not scene:
                    print(f"Skipping a null scene for {widget.filename}")
                    continue  # Skip only if the scene object doesn't exist at all
                
                # The scene is valid, proceed with rendering...
                img_size = widget.original_pixmap.size()
                image = QImage(img_size, QImage.Format_ARGB32)
                image.fill(Qt.transparent)
                
                painter = QPainter()
                try:
                    if painter.begin(image):
                        scene.render(painter, 
                                QRectF(image.rect()),
                                QRectF(scene.sceneRect()),
                                Qt.KeepAspectRatio)
                    else:
                        print(f"Failed to initialize painter for {widget.filename}")
                        continue
                finally:
                    painter.end()  # Ensure painter is always released

                # Save rendered image
                temp_path = os.path.join(temp_dir, widget.filename)
                image.save(temp_path)
                translated_images.append((temp_path, widget.filename))

        # Package images into ZIP
        saved_path, success = export_translated_images_to_zip(translated_images, export_path)

        if success:
            # Success message - keep QMessageBox.information for non-error cases
            QMessageBox.information(main_window, "Success", f"Exported to:\n{saved_path}")
        else:
            from app.ui.dialogs.error_dialog import ErrorDialog
            ErrorDialog.critical(main_window, "Export Error", "Failed to export rendered images.", None)
    except Exception as e:
        import traceback
        from app.ui.dialogs.error_dialog import ErrorDialog
        ErrorDialog.critical(
            main_window, "Render Error", 
            f"Failed to render images for export:\n{str(e)}",
            traceback.format_exc()
        )
    finally:
        for i in range(scroll_layout.count()):
            widget = scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.setUpdatesEnabled(True)
        shutil.rmtree(temp_dir, ignore_errors=True)