# app/handlers/context_fill_handler.py

import traceback
import sys
import io
import cv2
import numpy as np
from PIL import Image
import os

from PySide6.QtWidgets import QMessageBox, QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton
from PySide6.QtGui import QImage, QPixmap, QPainterPath
from PySide6.QtCore import QBuffer
from app.ui.components import ResizableImageLabel
from assets import MANUALOCR_STYLES

class ContextFillHandler:
    """Handles the Context Fill (Inpainting) feature, independent of MainWindow."""
    def __init__(self, scroll_area, model):
        self.scroll_area = scroll_area
        self.model = model
        self.is_active = False
        self.active_label = None
        self.selection_paths = []

        self._setup_ui()

    def _setup_ui(self):
        """Creates the overlay widget, parented to the scroll_area."""
        self.overlay_widget = QWidget(self.scroll_area)
        self.overlay_widget.setObjectName("ContextFillOverlay")
        self.overlay_widget.setStyleSheet(MANUALOCR_STYLES)
        overlay_layout = QVBoxLayout(self.overlay_widget)
        overlay_layout.setContentsMargins(5, 5, 5, 5)
        overlay_layout.addWidget(QLabel("Context Fill Controls"))
        overlay_buttons = QHBoxLayout()
        
        self.btn_process_fill = QPushButton("Fill Selected Areas")
        self.btn_process_fill.clicked.connect(self.process_inpainting)
        self.btn_process_fill.setEnabled(False)
        overlay_buttons.addWidget(self.btn_process_fill)

        self.btn_reset_selection = QPushButton("Reset All Selections")
        self.btn_reset_selection.setObjectName("ResetButton")
        self.btn_reset_selection.clicked.connect(self.reset_selection)
        self.btn_reset_selection.setEnabled(False)
        overlay_buttons.addWidget(self.btn_reset_selection)

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
        self.scroll_area.cancel_active_modes(exclude_handler=self)
        self.is_active = True
        
        self._clear_selection_state()
        self._set_selection_enabled_on_all(True)
        
        self._update_widget_position()
        self.overlay_widget.show()
        self.overlay_widget.raise_()
        
        QMessageBox.information(self.scroll_area, "Context Fill Mode",
                                "Click and drag on an image to select an area to inpaint. "
                                "You can make multiple selections on the same image.")

    def cancel_mode(self):
        """Cancels the context fill mode and resets the UI."""
        if not self.is_active: return
        print("Cancelling Context Fill mode...")
        self.is_active = False
        self.overlay_widget.hide()
        self._clear_selection_state()
        self._set_selection_enabled_on_all(False)
        print("Context Fill mode cancelled.")

    def reset_selection(self):
        """Clears all selections to allow for a new session."""
        self._clear_selection_state()
        if self.is_active:
            self._set_selection_enabled_on_all(True)
            self.btn_process_fill.setEnabled(False)
            self.btn_reset_selection.setEnabled(False)
            print("All selections reset.")

    def _clear_selection_state(self):
        """Hides the overlay and clears any graphical selection indicators."""
        if self.active_label:
            self.active_label.clear_selection_visuals()
        self.active_label = None
        self.selection_paths.clear()

    def _set_selection_enabled_on_all(self, enabled):
        """
        Enables or disables selection on all labels and manages signal connections.
        """
        layout = self.scroll_area.widget().layout()
        if not layout: return

        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_manual_selection_enabled(enabled)
                # --- LOGIC MOVED HERE ---
                if enabled:
                    # Connect this handler's slot to the label's signal
                    widget.manual_area_selected.connect(self.handle_area_selected)
                else:
                    # Disconnect to prevent calls when inactive
                    try:
                        widget.manual_area_selected.disconnect(self.handle_area_selected)
                    except (TypeError, RuntimeError):
                        pass # Signal was not connected, which is fine.

    def handle_area_selected(self, new_rect_scene, label_widget):
        """Callback to handle a new selection, merging it into a unified shape."""
        if not self.is_active: return

        if not self.active_label:
            self.active_label = label_widget
            self._set_selection_enabled_on_all(False)
            self.active_label.set_manual_selection_enabled(True)
        elif self.active_label is not label_widget:
            QMessageBox.warning(self.scroll_area, "Selection Error", 
                                "You can only make selections on one image at a time. "
                                "Reset selections to switch to a different image.")
            label_widget.clear_rubber_band()
            return
            
        label_widget.clear_rubber_band()

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
        self.active_label.draw_selections(self.selection_paths)

        self.btn_process_fill.setEnabled(True)
        self.btn_reset_selection.setEnabled(True)

    def _update_widget_position(self):
        """Positions the overlay widget at the top-center of the visible scroll area."""
        if not self.is_active: return
        viewport = self.scroll_area.viewport()
        overlay = self.overlay_widget
        overlay_x = (viewport.width() - overlay.width()) // 2
        overlay_y = 10 
        overlay.move(overlay_x, overlay_y)
        overlay.raise_()

    def process_inpainting(self):
        """Crops all selected areas, runs inpainting, and updates the image."""
        if not self.selection_paths or not self.active_label:
            QMessageBox.warning(self.scroll_area, "Error", "No area selected.")
            self.reset_selection()
            return

        print(f"Processing inpainting for {self.active_label.filename}")

        try:
            pixmap = self.active_label.original_pixmap
            buffer = QBuffer(); buffer.open(QBuffer.ReadWrite); pixmap.save(buffer, "PNG")
            pil_img = Image.open(io.BytesIO(buffer.data())).convert('RGB')
            image_np = np.array(pil_img)
            image_cv = cv2.cvtColor(image_np, cv2.COLOR_RGB2BGR)
            
            mask = np.zeros(image_cv.shape[:2], dtype=np.uint8)
            for path in self.selection_paths:
                polygon = path.toFillPolygon().toPolygon()
                points = np.array([[p.x(), p.y()] for p in polygon], dtype=np.int32)
                if points.size > 0:
                    cv2.fillPoly(mask, [points], 255)

            inpainted_image_cv = cv2.inpaint(image_cv, mask, 3, cv2.INPAINT_TELEA)

            inpainted_image_np = cv2.cvtColor(inpainted_image_cv, cv2.COLOR_BGR2RGB)
            h, w, ch = inpainted_image_np.shape
            q_image = QImage(inpainted_image_np.data, w, h, ch * w, QImage.Format_RGB888)
            new_pixmap = QPixmap.fromImage(q_image)

            self.active_label.update_pixmap(new_pixmap)
            
            full_path = next((p for p in self.model.image_paths if os.path.basename(p) == self.active_label.filename), None)
            
            if full_path:
                if not new_pixmap.save(full_path):
                     raise IOError(f"Failed to save inpainted image to {full_path}")
                print(f"Successfully saved inpainted image to {full_path}")
            else:
                QMessageBox.critical(self.scroll_area, "File Error", "Could not find file path. Changes will not be saved.")

            QMessageBox.information(self.scroll_area, "Success", "Context fill applied successfully.")
            self.reset_selection()

        except Exception as e:
            print(f"Error during inpainting: {e}")
            traceback.print_exc(file=sys.stdout)
            QMessageBox.critical(self.scroll_area, "Inpainting Error", f"An unexpected error occurred: {str(e)}")
            self.reset_selection()