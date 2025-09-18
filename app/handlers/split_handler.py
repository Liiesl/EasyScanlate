# app/handlers/split_handler.py

from PySide6.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QLabel, QMessageBox
from PySide6.QtCore import QObject, Qt, QRectF
from PySide6.QtGui import QPixmap, QPainter, QPen, QColor
from app.ui.components import ResizableImageLabel
import qtawesome as qta
import os

class SplitHandler(QObject):
    """
    Manages the UI and logic for splitting a single image into multiple images.
    """
    def __init__(self, main_window):
        super().__init__()
        self.main_window = main_window
        self.is_active = False
        self.selected_label = None
        self.split_points = [] # Will only ever contain 0 or 1 item

        self._setup_ui()

    def _setup_ui(self):
        """Creates the widget that appears during splitting mode."""
        self.split_widget = QWidget(self.main_window.scroll_area)
        self.split_widget.setObjectName("SplitWidget")
        self.split_widget.setStyleSheet("""
            #SplitWidget {
                background-color: rgba(30, 30, 30, 0.95);
                border-radius: 10px; border: 1px solid #555;
            }
            QPushButton {
                background-color: #007ACC; color: white; border: none;
                padding: 8px 12px; border-radius: 4px; font-weight: bold;
            }
            QPushButton:hover { background-color: #005C99; }
            QPushButton:disabled { background-color: #555; }
            #CancelButton { background-color: #C40C0C; }
            #CancelButton:hover { background-color: #8B0000; }
            QLabel { color: white; font-size: 13px; }
        """)

        layout = QVBoxLayout(self.split_widget)
        self.info_label = QLabel("Click on an image to place a split indicator.")
        self.info_label.setAlignment(Qt.AlignCenter)
        layout.addWidget(self.info_label)

        button_layout = QHBoxLayout()
        self.btn_confirm = QPushButton(qta.icon('fa5s.check', color='white'), " Confirm Split")
        self.btn_confirm.clicked.connect(self.confirm_split)
        button_layout.addWidget(self.btn_confirm)

        self.btn_clear = QPushButton(qta.icon('fa5s.undo', color='white'), " Clear Indicator")
        self.btn_clear.clicked.connect(self.clear_split_points)
        button_layout.addWidget(self.btn_clear)

        self.btn_cancel = QPushButton(qta.icon('fa5s.times', color='white'), " Cancel")
        self.btn_cancel.setObjectName("CancelButton")
        self.btn_cancel.clicked.connect(self.cancel_splitting_mode)
        button_layout.addWidget(self.btn_cancel)

        layout.addLayout(button_layout)
        self.split_widget.setFixedSize(380, 90)
        self.split_widget.hide()
        self._update_button_states()

    def start_splitting_mode(self):
        """Enters the image splitting mode."""
        if self.is_active: return
        if self.main_window.manual_ocr_handler.is_active:
             QMessageBox.warning(self.main_window, "Mode Conflict", "Cannot start splitting while in Manual OCR mode.")
             return

        self.is_active = True
        self.selected_label = None
        self.split_points = []
        
        self._update_widget_position()
        self.split_widget.show()
        self.split_widget.raise_()
        self._update_info_label()
        self._update_button_states()

        for widget in self._get_image_labels():
            widget.enable_splitting_selection(True)
            # Connect to the new signal that indicates a click anywhere
            widget.split_indicator_requested.connect(self._handle_indicator_placement)

    def _handle_indicator_placement(self, clicked_label, y_pos):
        """Moves the split indicator to the clicked position."""
        # If an indicator was on a different image, tell that image it's no longer selected.
        if self.selected_label and self.selected_label != clicked_label:
            self.selected_label.set_selected_for_splitting(False)

        # Update the state to the newly clicked label and position
        self.selected_label = clicked_label
        self.split_points = [y_pos]

        # Tell the new label it's selected (to show the overlay)
        self.selected_label.set_selected_for_splitting(True)
        # And draw the split line at the new position
        self.selected_label.draw_split_lines(self.split_points)

        self._update_info_label()
        self._update_button_states()

    def confirm_split(self):
        """Slices the image and redistributes OCR data."""
        if not self.selected_label or not self.split_points:
            QMessageBox.warning(self.main_window, "Input Error", "Please click on an image to place a split indicator.")
            return

        print("--- Starting Image Splitting Process ---")
        
        # 1. Prepare Data
        source_label = self.selected_label
        source_pixmap = source_label.original_pixmap
        source_filename = source_label.filename
        images_dir = os.path.join(self.main_window.model.temp_dir, 'images')
        basename, ext = os.path.splitext(source_filename)

        # 2. Generate unique filenames to avoid conflicts with existing split images
        def generate_unique_filename(base_name, extension, existing_files):
            """Generate a unique filename by checking against existing files."""
            counter = 1
            while True:
                candidate = f"{base_name}_split_{counter}{extension}"
                if candidate not in existing_files:
                    return candidate
                counter += 1

        # Get list of existing files to avoid naming conflicts
        existing_files = set()
        try:
            existing_files = set(os.listdir(images_dir))
        except OSError:
            pass
        
        # Also check current image paths in model
        for path in self.main_window.model.image_paths:
            existing_files.add(os.path.basename(path))

        # 3. Slice the Pixmap
        split_boundaries = [0] + self.split_points + [source_pixmap.height()]
        new_pixmaps = []
        for i in range(len(split_boundaries) - 1):
            y_start, y_end = split_boundaries[i], split_boundaries[i+1]
            rect = QRectF(0, y_start, source_pixmap.width(), y_end - y_start).toRect()
            new_pixmaps.append(source_pixmap.copy(rect))

        # 4. Save New Images and Generate New Data with unique filenames
        new_image_data = []
        for i, pixmap in enumerate(new_pixmaps):
            # Generate unique filename for each split part
            new_filename = generate_unique_filename(basename, ext, existing_files)
            existing_files.add(new_filename)  # Add to set to avoid duplicates in this batch
            
            new_filepath = os.path.join(images_dir, new_filename)
            if not pixmap.save(new_filepath):
                QMessageBox.critical(self.main_window, "Save Error", f"Failed to save split image to {new_filepath}.")
                self.cancel_splitting_mode()
                return
            new_image_data.append({
                'filename': new_filename, 
                'pixmap': pixmap, 
                'path': new_filepath,
                'y_start': split_boundaries[i],
                'y_end': split_boundaries[i+1]
            })
        print(f"Split image into {len(new_image_data)} new files: {[data['filename'] for data in new_image_data]}")

        # 5. Update OCR Data Model (FIXED LOGIC)
        print("Updating OCR data model...")
        
        # Process each OCR result that belongs to the source image
        for result in self.main_window.model.ocr_results:
            if result.get('filename') == source_filename:
                try:
                    # Get the Y coordinate of this OCR result
                    coords = result.get('coordinates', [])
                    if not coords:
                        print(f"Warning: OCR result has no coordinates, skipping: {result}")
                        continue
                    
                    # Find the minimum Y coordinate of the coordinates
                    box_y = min(p[1] for p in coords if len(p) >= 2)
                    
                    # Determine which split section this OCR result belongs to
                    assigned = False
                    for data in new_image_data:
                        if data['y_start'] <= box_y < data['y_end']:
                            # Update filename to the correct split part
                            result['filename'] = data['filename']
                            
                            # Adjust coordinates relative to the new image
                            y_offset = data['y_start']
                            if y_offset > 0:
                                result['coordinates'] = [[p[0], p[1] - y_offset] for p in coords]
                            
                            assigned = True
                            print(f"Assigned OCR result at Y={box_y} to {data['filename']} (offset: {y_offset})")
                            break
                    
                    if not assigned:
                        print(f"Warning: Could not assign OCR result at Y={box_y} to any split section")
                        
                except (TypeError, ValueError, IndexError) as e:
                    print(f"Warning: Skipping an OCR result for '{source_filename}' due to malformed data: {e}")
                    continue

        # 6. Clean up old file and model's image path list
        source_path_in_model = next((p for p in self.main_window.model.image_paths if os.path.basename(p) == source_filename), None)
        if source_path_in_model:
            index = self.main_window.model.image_paths.index(source_path_in_model)
            self.main_window.model.image_paths.pop(index)
            for i, data in enumerate(new_image_data):
                self.main_window.model.image_paths.insert(index + i, data['path'])
            try:
                os.remove(source_path_in_model)
                print(f"Deleted original file: {source_path_in_model}")
            except Exception as e:
                print(f"Warning: Could not delete old image file {source_path_in_model}. Error: {e}")

        # 7. Update UI
        print("Updating UI with new split images...")
        source_label_index = self._get_widget_index(source_label)
        if source_label_index == -1:
            QMessageBox.critical(self.main_window, "UI Error", "Could not find original image in layout.")
            self.cancel_splitting_mode()
            return

        self.main_window.scroll_layout.removeWidget(source_label)
        source_label.cleanup()
        source_label.deleteLater()

        for i, data in enumerate(new_image_data):
            new_label = ResizableImageLabel(data['pixmap'], data['filename'])
            new_label.textBoxDeleted.connect(self.main_window.delete_row)
            new_label.textBoxSelected.connect(self.main_window.handle_text_box_selected)
            new_label.manual_area_selected.connect(self.main_window.manual_ocr_handler.handle_area_selected)
            self.main_window.scroll_layout.insertWidget(source_label_index + i, new_label)

        # 8. Finalize
        self.main_window.model._sort_ocr_results()
        self.main_window.update_all_views()
        QMessageBox.information(self.main_window, "Split Successful", f"Image successfully split into {len(new_pixmaps)} parts.")
        self.cancel_splitting_mode()

    def cancel_splitting_mode(self):
        """Exits splitting mode and cleans up."""
        if not self.is_active: return
        
        try:
            if self.selected_label:
                self.selected_label.set_selected_for_splitting(False)
        except RuntimeError:
            print("Info: selected_label was already deleted when attempting to exit split mode.")
        
        for widget in self._get_image_labels():
            try:
                widget.split_indicator_requested.disconnect(self._handle_indicator_placement)
            except (TypeError, RuntimeError): pass
            widget.enable_splitting_selection(False)
        
        self.is_active = False
        self.selected_label = None
        self.split_points = []
        self.split_widget.hide()
        print("Exited splitting selection mode.")
    
    def clear_split_points(self):
        """Removes the split indicator and deselects the image."""
        if self.selected_label:
            self.selected_label.set_selected_for_splitting(False)
            self.selected_label = None
        self.split_points = []
        self._update_info_label()
        self._update_button_states()

    def _update_widget_position(self):
        """Positions the control widget at the top-center of the scroll area."""
        if not self.split_widget.isVisible(): return
        scroll_area_width = self.main_window.scroll_area.viewport().width()
        x = (scroll_area_width - self.split_widget.width()) / 2
        y = 10
        self.split_widget.move(int(x), int(y))

    def _update_button_states(self):
        has_indicator = self.selected_label is not None and len(self.split_points) > 0
        self.btn_confirm.setEnabled(has_indicator)
        self.btn_clear.setEnabled(has_indicator)

    def _update_info_label(self):
        if not self.selected_label:
            self.info_label.setText("Click on an image to place a split indicator.")
        else:
            num_pieces = len(self.split_points) + 1
            self.info_label.setText(f"<b>{self.selected_label.filename}</b> selected.<br>"
                                    f"Click to move the indicator. (1 split / {num_pieces} pieces)")

    def _get_image_labels(self):
        """Helper to get all ResizableImageLabel widgets from the scroll layout."""
        labels = []
        for i in range(self.main_window.scroll_layout.count()):
            widget = self.main_window.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                labels.append(widget)
        return labels

    def _get_widget_index(self, widget_to_find):
        """Helper to find the index of a widget in the scroll layout."""
        for i in range(self.main_window.scroll_layout.count()):
            if self.main_window.scroll_layout.itemAt(i).widget() == widget_to_find:
                return i
        return -1