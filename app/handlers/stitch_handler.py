# app/stitch_handler.py

from PySide6.QtWidgets import QWidget, QHBoxLayout, QPushButton, QMessageBox
from PySide6.QtCore import QObject, Qt
from PySide6.QtGui import QPixmap, QPainter
from app.ui.components import ResizableImageLabel
import qtawesome as qta
import os

class StitchHandler(QObject):
    """
    Manages the UI and logic for selecting images to be stitched together.
    """
    def __init__(self, main_window):
        super().__init__()
        self.main_window = main_window
        self.is_active = False
        self.selected_images = []
        
        self._setup_ui()

    def _setup_ui(self):
        """Creates the confirmation/cancellation widget that appears during stitching mode."""
        self.stitch_widget = QWidget(self.main_window.scroll_area)
        self.stitch_widget.setObjectName("StitchWidget")
        self.stitch_widget.setStyleSheet("""
            #StitchWidget {
                background-color: rgba(30, 30, 30, 0.9);
                border-radius: 10px;
                border: 1px solid #555;
            }
            QPushButton {
                background-color: #007ACC;
                color: white;
                border: none;
                padding: 8px 16px;
                border-radius: 4px;
                font-weight: bold;
            }
            QPushButton:hover { background-color: #005C99; }
            QPushButton:disabled { background-color: #555; }
            #CancelButton { background-color: #C40C0C; }
            #CancelButton:hover { background-color: #8B0000; }
        """)
        
        layout = QHBoxLayout(self.stitch_widget)
        
        self.btn_confirm = QPushButton(qta.icon('fa5s.check', color='white'), " Confirm Stitch")
        self.btn_confirm.clicked.connect(self.confirm_stitch)
        self.btn_confirm.setEnabled(False)
        layout.addWidget(self.btn_confirm)
        
        self.btn_cancel = QPushButton(qta.icon('fa5s.times', color='white'), " Cancel")
        self.btn_cancel.setObjectName("CancelButton")
        self.btn_cancel.clicked.connect(self.cancel_stitching_mode)
        layout.addWidget(self.btn_cancel)
        
        self.stitch_widget.setFixedSize(320, 60)
        self.stitch_widget.hide()

    def start_stitching_mode(self):
        """Enters the image selection mode for stitching."""
        if self.is_active:
            return
        if self.main_window.manual_ocr_handler.is_active:
             QMessageBox.warning(self.main_window, "Mode Conflict", "Cannot start stitching while in Manual OCR mode.")
             return

        self.is_active = True
        self.selected_images.clear()
        self.btn_confirm.setEnabled(False)
        
        print("Entering stitching selection mode.")
        
        self._update_widget_position()
        self.stitch_widget.show()
        self.stitch_widget.raise_()

        for i in range(self.main_window.scroll_layout.count()):
            widget = self.main_window.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.enable_stitching_selection(True)
                widget.stitching_selection_changed.connect(self._handle_image_selection)

    def _handle_image_selection(self, image_label, is_selected):
        """Updates the list of selected images based on user interaction."""
        temp_selection = set(self.selected_images)
        if is_selected:
            temp_selection.add(image_label)
        else:
            if image_label in temp_selection:
                temp_selection.remove(image_label)

        # To maintain the visual order of images, we rebuild the list
        ordered_selection = []
        for i in range(self.main_window.scroll_layout.count()):
            widget = self.main_window.scroll_layout.itemAt(i).widget()
            if widget in temp_selection:
                ordered_selection.append(widget)
        self.selected_images = ordered_selection
        
        print(f"Selected images (in order): {[img.filename for img in self.selected_images]}")
        self.btn_confirm.setEnabled(len(self.selected_images) >= 2)

    def confirm_stitch(self):
        """
        Combines the selected images into a single image, updates the data model
        (OCR results and coordinates), and refreshes the UI.
        """
        if len(self.selected_images) < 2:
            QMessageBox.warning(self.main_window, "Selection Error", "Please select at least two images to stitch.")
            return

        print("--- Starting Image Stitching Process ---")
        
        # --- 1. Prepare Data and Image Objects ---
        labels_to_stitch = self.selected_images
        first_label = labels_to_stitch[0]
        
        # The new combined image will inherit the filename of the first image
        new_filename = first_label.filename
        # --- FIX: Path must point to the 'images' subdirectory in the temp folder ---
        images_dir = os.path.join(self.main_window.model.temp_dir, 'images')
        new_filepath = os.path.join(images_dir, new_filename)
        print(f"New combined image will be saved as: {new_filepath}")

        # Get the original, full-resolution QPixmap from each label
        pixmaps = [label.original_pixmap for label in labels_to_stitch]
        
        # --- 2. Stitch Images using QPainter ---
        if not pixmaps:
            QMessageBox.critical(self.main_window, "Stitch Error", "Could not retrieve image data for stitching.")
            self.cancel_stitching_mode()
            return

        total_width = pixmaps[0].width()
        total_height = sum(p.height() for p in pixmaps)
        
        combined_pixmap = QPixmap(total_width, total_height)
        combined_pixmap.fill(Qt.transparent) # Use a transparent background

        painter = QPainter(combined_pixmap)
        current_y = 0
        for pixmap in pixmaps:
            painter.drawPixmap(0, current_y, pixmap)
            current_y += pixmap.height()
        painter.end()

        # Save the new combined image, overwriting the first original image in the temp folder
        if not combined_pixmap.save(new_filepath):
            QMessageBox.critical(self.main_window, "Save Error", f"Failed to save the stitched image to {new_filepath}.")
            self.cancel_stitching_mode()
            return
        print("Stitched image saved successfully.")

        # --- 3. Update OCR Results Data Model ---
        print("Updating OCR data model...")
        height_offset = 0
        for i, label in enumerate(labels_to_stitch):
            current_filename = label.filename
            
            # For all images after the first one, add the height of previous images
            if i > 0:
                height_offset += labels_to_stitch[i-1].original_pixmap.height()
            
            print(f"Processing results for '{current_filename}' with Y-offset: {height_offset}")

            # Iterate through all OCR results to find matches
            for result in self.main_window.model.ocr_results:
                if result.get('filename') == current_filename:
                    # Update filename to the new combined filename
                    result['filename'] = new_filename
                    
                    # Update bounding box coordinates if an offset is needed
                    if height_offset > 0:
                        bbox = result.get('bbox', [])
                        if bbox:
                            result['bbox'] = [[p[0], p[1] + height_offset] for p in bbox]
                        coords = result.get('coordinates', [])
                        if coords:
                             result['coordinates'] = [[p[0], p[1] + height_offset] for p in coords]

        # --- 4. Clean up old files and main window's image list ---
        filenames_to_remove = [label.filename for label in labels_to_stitch[1:]]
        print(f"Removing old files and references for: {filenames_to_remove}")
        for filename in filenames_to_remove:
            # --- FIX: Path must point to the 'images' subdirectory ---
            full_path_to_remove = os.path.join(images_dir, filename)
            
            # Find the full path in image_paths as it was originally stored
            original_full_path = next((p for p in self.main_window.model.image_paths if os.path.basename(p) == filename), None)
            if original_full_path and original_full_path in self.main_window.model.image_paths:
                self.main_window.model.image_paths.remove(original_full_path)

            try:
                if os.path.exists(full_path_to_remove):
                    os.remove(full_path_to_remove)
            except Exception as e:
                print(f"Warning: Could not delete old image file {full_path_to_remove}. Error: {e}")

        # --- 5. Update the UI ---
        print("Updating UI with new stitched image...")
        # Find the position of the first image to insert the new one
        first_label_index = -1
        for i in range(self.main_window.scroll_layout.count()):
             if self.main_window.scroll_layout.itemAt(i).widget() == first_label:
                 first_label_index = i
                 break
        
        if first_label_index == -1:
            QMessageBox.critical(self.main_window, "UI Error", "Could not find the original image position in the layout.")
            self.cancel_stitching_mode()
            return

        # Remove all old image labels from the layout
        for label in labels_to_stitch:
            self.main_window.scroll_layout.removeWidget(label)
            label.cleanup()
            label.deleteLater()
            
        # Create and insert the new combined image label
        new_label = ResizableImageLabel(combined_pixmap, new_filename)
        # Re-connect signals, just as in process_mmtl
        new_label.textBoxDeleted.connect(self.main_window.delete_row)
        new_label.textBoxSelected.connect(self.main_window.handle_text_box_selected)
        new_label.manual_area_selected.connect(self.main_window.manual_ocr_handler.handle_area_selected)
        self.main_window.scroll_layout.insertWidget(first_label_index, new_label)

        # --- 6. Finalize and Clean Up ---
        # Sort results to ensure correct order after filename changes
        self.main_window.model._sort_ocr_results()
        
        # Refresh all views to show the updated results on the new image
        self.main_window.update_all_views()
        
        QMessageBox.information(self.main_window, "Stitch Successful", 
                                f"{len(labels_to_stitch)} images have been successfully stitched into one.")
        
        self.cancel_stitching_mode()

    def cancel_stitching_mode(self):
        """Exits the stitching mode and cleans up the UI."""
        if not self.is_active:
            return
        
        for i in range(self.main_window.scroll_layout.count()):
            widget = self.main_window.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                try:
                    widget.stitching_selection_changed.disconnect(self._handle_image_selection)
                except (TypeError, RuntimeError):
                    pass # Signal was not connected or already disconnected
                widget.enable_stitching_selection(False)
        
        self.is_active = False
        self.selected_images.clear()
        self.stitch_widget.hide()
        print("Exited stitching selection mode.")

    def _update_widget_position(self):
        """Positions the control widget at the top-center of the scroll area."""
        if not self.stitch_widget.isVisible():
            return
        scroll_area_width = self.main_window.scroll_area.viewport().width()
        x = (scroll_area_width - self.stitch_widget.width()) / 2
        y = 10 # A small margin from the top
        self.stitch_widget.move(int(x), int(y))