import os
import ast
import json
from PySide6.QtWidgets import QMessageBox, QFileDialog
from PySide6.QtCore import QRectF
from app.ui.components import ResizableImageLabel
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
    """Export OCR results to either a JSON file (Master) or a Markdown file (For-Translate)."""
    if not self.model.ocr_results:
        QMessageBox.warning(self, "Error", "No OCR results to export.")
        return

    # Ask user to choose export type
    msg = QMessageBox()
    msg.setIcon(QMessageBox.Question)
    msg.setWindowTitle("Export OCR")
    msg.setText("Choose export type:")
    master_btn = msg.addButton("Master (JSON)", QMessageBox.AcceptRole)
    translate_btn = msg.addButton("For-Translate (Markdown)", QMessageBox.AcceptRole)
    msg.addButton(QMessageBox.Cancel)
    msg.exec_()

    if msg.clickedButton() == master_btn:
        export_type = 'master'
    elif msg.clickedButton() == translate_btn:
        export_type = 'for-translate'
    else:
        return  # User cancelled

    # Generate content based on export type
    if export_type == 'master':
        try:
            file_path, _ = QFileDialog.getSaveFileName(
                self, "Export OCR Results", "", "JSON Files (*.json)"
            )
            if not file_path:
                return
            with open(file_path, 'w', encoding='utf-8') as f:
                json.dump(self.ocr_results, f, ensure_ascii=False, indent=4)
            QMessageBox.information(self, "Success", "OCR results exported successfully in JSON format.")
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Failed to export: {str(e)}")

    elif export_type == 'for-translate':
        content = "<!-- type: for-translate -->\n\n"
        grouped_results = {}
        extensions = set()

        # Group results by filename while preserving row numbers
        for result in self.ocr_results:
            filename = result['filename']
            ext = os.path.splitext(filename)[1].lstrip('.').lower()
            extensions.add(ext)
            text = result['text']
            row_number = result['row_number']  # This will now always have a value
            
            if filename not in grouped_results:
                grouped_results[filename] = []
            grouped_results[filename].append((text, row_number))

        # Add type and extension header
        if len(extensions) == 1:
            content += f"<!-- ext: {list(extensions)[0]} -->\n\n"

        # Write grouped results with filename headers
        for idx, (filename, texts) in enumerate(grouped_results.items()):
            if idx > 0:
                content += "\n\n"
            content += f"<!-- file: {filename} -->\n\n"
            
            # Sort texts by row number to maintain order
            sorted_texts = sorted(texts, key=lambda x: x[1])
            
            for text, row_number in sorted_texts:
                lines = text.split('\n')
                for line in lines:
                    content += f"{line.strip()}\n"
                    content += f"-/{row_number}\\-\n"  # This will now always have a valid number


        # Get save path and write to file
        file_path, _ = QFileDialog.getSaveFileName(
            self, "Export OCR Results", "", "Markdown Files (*.md)"
        )
        if not file_path:
            return
        try:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            QMessageBox.information(self, "Success", "OCR results exported successfully in Markdown format.")
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Failed to export: {str(e)}")

def import_translation_file(self):
    """Import translation from an exported file (JSON for Master, Markdown for For-Translate)."""
    file_path, _ = QFileDialog.getOpenFileName(
        self, "Import Translation", "", "Files (*.json *.md)"
    )
    if not file_path:
        return

    try:
        with open(file_path, 'r', encoding='utf-8') as file:
            content = file.read()

        if file_path.endswith('.json'):
            # Parse JSON for Master format
            new_ocr_results = json.loads(content)
            if not isinstance(new_ocr_results, list):
                raise ValueError("Invalid JSON format: Expected a list of OCR results.")

            # Check if we have existing OCR results
            if self.ocr_results:
                reply = QMessageBox.question(
                    self,
                    "Replace OCR Results?",
                    "This will overwrite existing OCR data. Continue?",
                    QMessageBox.Yes | QMessageBox.No,
                    QMessageBox.No
                )
                if reply != QMessageBox.Yes:
                    return

            self.ocr_results = new_ocr_results
            self.update_results_table()
            QMessageBox.information(self, "Success", "Master file imported successfully!\n"
                                f"Loaded {len(new_ocr_results)} OCR entries.")

        elif file_path.endswith('.md'):
            # Parse Markdown for For-Translate format
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
                        except ValueError:
                            row_number = 0  # Default to 0 if parsing fails
                        row_numbers.append(row_number)
                        # Save current entry
                        file_texts[current_file].append(('\n'.join(current_entry), row_numbers))
                        current_entry = []
                        row_numbers = []
                elif current_file is not None:
                    # Skip empty lines between entries
                    if line or current_entry:
                        current_entry.append(line)

            # Add the last entry if buffer isn't empty
            if current_file is not None and current_entry:
                file_texts[current_file].append(('\n'.join(current_entry), row_numbers))

            # Rebuild translations in original OCR order
            translation_index = {k: 0 for k in file_texts.keys()}
            for result in self.ocr_results:
                filename = result['filename']
                if filename in file_texts and translation_index[filename] < len(file_texts[filename]):
                    translated_text, row_numbers = file_texts[filename][translation_index[filename]]
                    result['text'] = translated_text
                    result['row_number'] = row_numbers[0]  # Update row number
                    translation_index[filename] += 1
                else:
                    print(f"Warning: No translation found for entry in '{filename}'")

            self.update_results_table()
            QMessageBox.information(self, "Success", "Translation imported and updated successfully!")

        else:
            raise ValueError("Unsupported file format. Please provide a JSON or Markdown file.")

    except Exception as e:
        QMessageBox.critical(
            self, "Error",
            f"Failed to import translation:\n{str(e)}"
        )

def export_rendered_images(self):
    """Export images with applied translations directly from QGraphicsView scenes."""
    if not self.model.image_paths:
        QMessageBox.warning(self, "Warning", "No images available for export.")
        return

    # Ask user for save location, defaulting to the project's directory.
    project_directory = os.path.dirname(self.model.mmtl_path) if self.model.mmtl_path else ""
    default_filename = f"{self.model.project_name}.zip"
    default_path = os.path.join(project_directory, default_filename)
    
    export_path, _ = QFileDialog.getSaveFileName(
        self,
        "Export Rendered Images",
        default_path,
        "ZIP Files (*.zip)"
    )

    if not export_path:
        return # User cancelled
        
    # Suspend updates during export
    for i in range(self.scroll_layout.count()):
        widget = self.scroll_layout.itemAt(i).widget()
        if isinstance(widget, ResizableImageLabel):
            widget.setUpdatesEnabled(False)

    import tempfile, shutil
    from PySide6.QtGui import QImage, QPainter
    from PySide6.QtCore import Qt

    temp_dir = tempfile.mkdtemp()
    translated_images = []

    try:
        for i in range(self.scroll_layout.count()):
            widget = self.scroll_layout.itemAt(i).widget()
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
            QMessageBox.information(self, "Success", f"Exported to:\n{saved_path}")
        else:
            QMessageBox.critical(self, "Error", "Export failed")
    except Exception as e:
        QMessageBox.critical(self, "Render Error", f"Failed to render image: {str(e)}")
        import traceback
        traceback.print_exc()
    finally:
        for i in range(self.scroll_layout.count()):
            widget = self.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.setUpdatesEnabled(True)
        shutil.rmtree(temp_dir, ignore_errors=True)