# app/handlers/context_fill_handler.py

import traceback
import sys
import io
import cv2
import numpy as np
from PIL import Image
import os

from PySide6.QtWidgets import QMessageBox, QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton
from PySide6.QtCore import QPoint, QBuffer
from PySide6.QtGui import QImage, QPixmap, QPainterPath
from app.ui.components import ResizableImageLabel
from assets import MANUALOCR_STYLES # Re-using styles for consistency

class ContextFillHandler:
    """Handles all logic for the Context Fill (Inpainting) feature."""
    def __init__(self, main_window):
        self.main_window = main_window
        self.is_active = False
        self.active_label = None
        # --- MODIFICATION: Store multiple selection shapes (paths) instead of rects ---
        self.selection_paths = []

        self._setup_ui()

    def _setup_ui(self):
        """Creates the overlay widget that appears after selecting an area."""
        # MODIFICATION: Parent is now the scroll_area for stable positioning
        self.overlay_widget = QWidget(self.main_window.scroll_area)
        self.overlay_widget.setObjectName("ContextFillOverlay")
        self.overlay_widget.setStyleSheet(MANUALOCR_STYLES) # Re-use style
        overlay_layout = QVBoxLayout(self.overlay_widget)
        overlay_layout.setContentsMargins(5, 5, 5, 5)
        # MODIFICATION: Changed label to reflect persistent nature
        overlay_layout.addWidget(QLabel("Context Fill Controls"))
        overlay_buttons = QHBoxLayout()
        
        self.btn_process_fill = QPushButton("Fill Selected Areas")
        self.btn_process_fill.clicked.connect(self.process_inpainting)
        # MODIFICATION: Disabled by default
        self.btn_process_fill.setEnabled(False)
        overlay_buttons.addWidget(self.btn_process_fill)

        self.btn_reset_selection = QPushButton("Reset All Selections")
        self.btn_reset_selection.setObjectName("ResetButton")
        self.btn_reset_selection.clicked.connect(self.reset_selection)
        # MODIFICATION: Disabled by default
        self.btn_reset_selection.setEnabled(False)
        overlay_buttons.addWidget(self.btn_reset_selection)

        # MODIFICATION: Changed button text for clarity
        self.btn_cancel_fill = QPushButton("Exit Context Fill")
        self.btn_cancel_fill.setObjectName("CancelButton")
        self.btn_cancel_fill.clicked.connect(self.cancel_mode)
        overlay_buttons.addWidget(self.btn_cancel_fill)
        
        overlay_layout.addLayout(overlay_buttons)
        self.overlay_widget.setFixedSize(380, 80)
        self.overlay_widget.hide()

    def start_mode(self):
        """Activates the context fill mode."""
        if self.is_active: return
        self.is_active = True
        
        # Deactivate other conflicting modes
        if self.main_window.manual_ocr_handler.is_active:
            self.main_window.manual_ocr_handler.cancel_mode()
        if self.main_window.stitch_handler.is_active:
            self.main_window.stitch_handler.cancel_stitching_mode()
        if self.main_window.split_handler.is_active:
            self.main_window.split_handler.cancel_splitting_mode()

        self._clear_selection_state()
        self._set_selection_enabled_on_all(True)
        
        # MODIFICATION: Show overlay immediately upon starting the mode
        self._update_widget_position()
        self.overlay_widget.show()
        self.overlay_widget.raise_()
        
        QMessageBox.information(self.main_window, "Context Fill Mode",
                                "Click and drag on an image to select an area to inpaint. "
                                "You can make multiple selections on the same image.")

    def cancel_mode(self):
        """Cancels the context fill mode and resets the UI."""
        if not self.is_active: return
        print("Cancelling Context Fill mode...")
        self.is_active = False
        # MODIFICATION: Explicitly hide overlay on cancel
        self.overlay_widget.hide()
        self._clear_selection_state()
        self._set_selection_enabled_on_all(False)
        print("Context Fill mode cancelled.")

    def reset_selection(self):
        """Clears all selections to allow for a new session."""
        self._clear_selection_state()
        if self.is_active:
            # Re-enable selection on all images for a fresh start
            self._set_selection_enabled_on_all(True)
            self.btn_process_fill.setEnabled(False)
            self.btn_reset_selection.setEnabled(False)
            print("All selections reset. Ready for new selection on any image.")

    def _clear_selection_state(self):
        """Hides the overlay and clears any graphical selection indicators."""
        if self.active_label:
            self.active_label.clear_selection_visuals()
        self.active_label = None
        # --- MODIFICATION: Clear paths list ---
        self.selection_paths.clear()

    def _set_selection_enabled_on_all(self, enabled):
        """Enables or disables the selection rubber band on all image labels."""
        for i in range(self.main_window.scroll_layout.count()):
            widget = self.main_window.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_manual_selection_enabled(enabled)

    def handle_area_selected(self, new_rect_scene, label_widget):
        """Callback to handle a new selection, merging it into a unified shape."""
        if not self.is_active: return

        # This is the first selection of a session. Lock in the active image.
        if not self.active_label:
            self.active_label = label_widget
            # Disable selection on all other images to enforce the single-image rule
            self._set_selection_enabled_on_all(False)
            self.active_label.set_manual_selection_enabled(True)
        # Ignore selections on other images during an active session
        elif self.active_label is not label_widget:
            QMessageBox.warning(self.main_window, "Selection Error", 
                                "You can only make selections on one image at a time. "
                                "Please 'Reset All Selections' to switch to a different image.")
            label_widget.clear_rubber_band()
            return
            
        label_widget.clear_rubber_band() # The selection is handled, hide the drawing tool

        # --- MODIFICATION: Merging Logic using QPainterPath for complex shapes ---
        new_path = QPainterPath()
        new_path.addRect(new_rect_scene)

        remaining_paths = []
        # Iterate through existing paths to find intersections
        for existing_path in self.selection_paths:
            if existing_path.intersects(new_path):
                # If they intersect, merge them into a single unified path
                new_path = new_path.united(existing_path)
            else:
                # If they don't intersect, keep the existing path as is
                remaining_paths.append(existing_path)
        
        # Add the newly created or merged path to the list
        remaining_paths.append(new_path)
        self.selection_paths = remaining_paths
        
        # Update the visuals on the active image with the new set of paths
        self.active_label.draw_selections(self.selection_paths)

        # Enable buttons now that at least one selection exists
        self.btn_process_fill.setEnabled(True)
        self.btn_reset_selection.setEnabled(True)

    def _update_widget_position(self):
        """Positions the overlay widget at the top-center of the visible scroll area."""
        if not self.is_active: return
        try:
            scroll_area = self.main_window.scroll_area
            viewport = scroll_area.viewport()
            overlay = self.overlay_widget
            overlay_x = (viewport.width() - overlay.width()) // 2
            overlay_y = 10 
            overlay.move(overlay_x, overlay_y)
            overlay.raise_()
        except Exception as e:
            print(f"Error positioning context fill overlay: {e}")
            traceback.print_exc(file=sys.stdout)

    def process_inpainting(self):
        """Crops all selected areas, runs inpainting, and updates the image."""
        if not self.selection_paths or not self.active_label:
            QMessageBox.warning(self.main_window, "Error", "No area selected or active image lost.")
            self.reset_selection()
            return

        print(f"Processing inpainting for {len(self.selection_paths)} selections on {self.active_label.filename}")

        try:
            # 1. Get Image
            pixmap = self.active_label.original_pixmap

            # 2. Convert QPixmap to OpenCV format (BGR)
            buffer = QBuffer(); buffer.open(QBuffer.ReadWrite); pixmap.save(buffer, "PNG")
            pil_img = Image.open(io.BytesIO(buffer.data())).convert('RGB')
            image_np = np.array(pil_img)
            image_cv = cv2.cvtColor(image_np, cv2.COLOR_RGB2BGR)
            
            # --- MODIFICATION: Create a mask from complex QPainterPath shapes ---
            mask = np.zeros(image_cv.shape[:2], dtype=np.uint8)
            for path in self.selection_paths:
                # Convert the QPainterPath to a QPolygon (integer points)
                polygon = path.toFillPolygon().toPolygon()
                
                # Convert the QPolygon to a NumPy array of points for OpenCV
                points = np.array([[p.x(), p.y()] for p in polygon], dtype=np.int32)
                
                # Draw the filled polygon onto the mask
                if points.size > 0:
                    cv2.fillPoly(mask, [points], 255)

            # 4. Perform Inpainting
            print("Running cv2.inpaint...")
            inpainted_image_cv = cv2.inpaint(image_cv, mask, 3, cv2.INPAINT_TELEA)

            # 5. Convert back to QPixmap
            inpainted_image_np = cv2.cvtColor(inpainted_image_cv, cv2.COLOR_BGR2RGB)
            h, w, ch = inpainted_image_np.shape
            q_image = QImage(inpainted_image_np.data, w, h, ch * w, QImage.Format_RGB888)
            new_pixmap = QPixmap.fromImage(q_image)

            # 6. Update UI and save the file persistently
            self.active_label.update_pixmap(new_pixmap)
            
            full_path = next((p for p in self.main_window.model.image_paths if os.path.basename(p) == self.active_label.filename), None)
            
            if full_path:
                if not new_pixmap.save(full_path):
                     raise IOError(f"Failed to save inpainted image to {full_path}")
                print(f"Successfully saved inpainted image to {full_path}")
            else:
                QMessageBox.critical(self.main_window, "File Error", 
                                     f"Could not find the full file path for {self.active_label.filename}. Changes will not be saved.")

            QMessageBox.information(self.main_window, "Success", "Context fill applied successfully.")
            # Start a new session after successful processing
            self.reset_selection()

        except Exception as e:
            print(f"Error during inpainting processing: {e}")
            traceback.print_exc(file=sys.stdout)
            QMessageBox.critical(self.main_window, "Inpainting Error", f"An unexpected error occurred: {str(e)}")
            self.reset_selection()