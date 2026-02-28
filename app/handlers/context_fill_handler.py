# app/handlers/context_fill_handler.py

import traceback
import sys
import io, os
import cv2
import numpy as np
from PIL import Image
import uuid

from PySide6.QtWidgets import QMessageBox, QLabel
from PySide6.QtGui import QImage, QPixmap, QPainterPath, QPolygonF, QPainter
from PySide6.QtCore import QBuffer, QRectF, QPointF, QObject
from app.ui.components.image_area.label import ResizableImageLabel
from app.ui.dialogs.error_dialog import ErrorDialog
from app.ui.widgets.handler_overlay import HandlerOverlay
from assets.styles import HANDLER_OVERLAY_STYLES


class ContextFillHandler(QObject):
    """Handles the Context Fill (Inpainting) feature, independent of MainWindow."""
    def __init__(self, scroll_area, model):
        super().__init__()
        self.scroll_area = scroll_area
        self.model = model
        self.is_active = False
        self.is_edit_mode_active = False
        self.active_label = None
        self.selection_paths = []

        self._setup_ui()

    def _setup_ui(self):
        """Creates the overlay widget using HandlerOverlay base."""
        self.overlay_widget = HandlerOverlay(
            self.scroll_area,
            "ContextFillOverlay",
            "Context Fill Controls",
            (380, 80)
        )
        self.overlay_widget.setStyleSheet(HANDLER_OVERLAY_STYLES)

        self.btn_process_fill = self.overlay_widget.create_confirm_button("Fill Selected Areas")
        self.btn_process_fill.clicked.connect(self.process_inpainting)
        self.btn_process_fill.setEnabled(False)

        self.btn_reset_selection = self.overlay_widget.create_reset_button("Reset All Selections")
        self.btn_reset_selection.clicked.connect(self.reset_selection)
        self.btn_reset_selection.setEnabled(False)

        self.btn_cancel_fill = self.overlay_widget.create_cancel_button("Exit Context Fill")
        self.btn_cancel_fill.clicked.connect(self.cancel_mode)

    def _update_widget_position(self):
        """Positions the overlay widget at the top-center of the visible scroll area."""
        if not self.is_active: return
        self.overlay_widget._update_widget_position()

    def start_mode(self):
        """Activates the context fill mode."""
        if self.is_active: return
        self.scroll_area.cancel_active_modes(exclude_handler=self)
        self.is_active = True
        
        self._clear_selection_state()
        self._set_selection_enabled_on_all(True)
        
        self.overlay_widget.show_overlay()
        
        # Information message - keep QMessageBox.information for non-error cases
        QMessageBox.information(self.scroll_area, "Context Fill Mode",
                                "Click and drag on an image to select an area to inpaint. "
                                "You can make multiple selections on the same image.")

    def toggle_edit_mode(self):
        if self.is_edit_mode_active:
            self._disable_edit_mode()
        else:
            self._enable_edit_mode()

    def _enable_edit_mode(self):
        if self.is_edit_mode_active: return
        self.scroll_area.cancel_active_modes(exclude_handler=self)
        self.is_edit_mode_active = True
        print("Enabling Context Fill Edit Mode.")

        layout = self.scroll_area.widget().layout()
        if not layout: return

        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_text_visibility(False)
                widget.set_inpaint_edit_mode(True)
        
        # Information message - keep QMessageBox.information for non-error cases
        QMessageBox.information(self.scroll_area, "Edit Context Fill Mode",
                                "Inpaint areas are highlighted. Click on a highlight to select it, then press Delete or Backspace to remove it.")

    def _disable_edit_mode(self):
        if not self.is_edit_mode_active: return
        self.is_edit_mode_active = False
        print("Disabling Context Fill Edit Mode.")

        layout = self.scroll_area.widget().layout()
        if not layout: return

        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_text_visibility(self.scroll_area._text_is_visible)
                widget.set_inpaint_edit_mode(False)

    def cancel_mode(self):
        """Cancels the context fill mode and resets the UI."""
        if self.is_edit_mode_active:
            self._disable_edit_mode()
        if not self.is_active: return
        print("Cancelling Context Fill mode...")
        self.is_active = False
        self.overlay_widget.hide_overlay()
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
        """Enables or disables selection on all labels and manages signal connections."""
        layout = self.scroll_area.widget().layout()
        if not layout: return

        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_manual_selection_enabled(enabled)
                if enabled:
                    widget.manual_area_selected.connect(self.handle_area_selected)
                else:
                    try:
                        widget.manual_area_selected.disconnect(self.handle_area_selected)
                    except (TypeError, RuntimeError):
                        pass

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
        for existing_path in self.selection_paths:
            if existing_path.intersects(new_path):
                new_path = new_path.united(existing_path)
            else:
                remaining_paths.append(existing_path)
        
        remaining_paths.append(new_path)
        self.selection_paths = remaining_paths
        self.active_label.draw_selections(self.selection_paths)

        self.btn_process_fill.setEnabled(True)
        self.btn_reset_selection.setEnabled(True)

    def process_inpainting(self):
        """
        Performs inpainting non-destructively by saving only the patched area
        and its metadata to the project model.
        """
        if not self.selection_paths or not self.active_label:
            QMessageBox.warning(self.scroll_area, "Error", "No area selected.")
            self.reset_selection()
            return

        print(f"Processing non-destructive inpainting for {self.active_label.filename}")
        self._perform_inpainting_logic(self.active_label, self.selection_paths, show_dialogs=True)
        self.reset_selection()

    def perform_auto_inpainting(self, target_label, bounding_boxes):
        """
        Performs inpainting based on a list of bounding boxes from OCR.
        This is a non-interactive, programmatic version of process_inpainting.
        """
        if not target_label or not bounding_boxes:
            return

        paths = []
        for box in bounding_boxes:
            path = QPainterPath()
            try:
                poly = QPolygonF([QPointF(p[0], p[1]) for p in box])
                path.addPolygon(poly)
                paths.append(path)
            except (TypeError, IndexError):
                continue

        if not paths:
            return

        self._perform_inpainting_logic(target_label, paths, show_dialogs=False)
        
    def _perform_inpainting_logic(self, target_label, paths, show_dialogs=True):
        """
        Shared logic for inpainting. Now uses a proximity check to decide which
        existing patches to temporarily merge before the operation.
        """
        try:
            # 1. Prepare a temporary base pixmap
            temp_base_pixmap = target_label.original_pixmap.copy()

            # 2. Combine new selection paths and define proximity area
            new_selection_path = QPainterPath()
            for path in paths:
                new_selection_path = new_selection_path.united(path)

            # Define a proximity margin in pixels. This is how far around the
            # new selection we will look for existing patches.
            PROXIMITY_MARGIN = 20 

            # Get the bounding box of the new selection and expand it
            selection_bounds = new_selection_path.boundingRect()
            proximity_rect = selection_bounds.adjusted(
                -PROXIMITY_MARGIN, -PROXIMITY_MARGIN,
                PROXIMITY_MARGIN, PROXIMITY_MARGIN
            )

            # 3. Detect and merge patches within the proximity area
            all_records = self.model.get_inpaint_records_for_image(target_label.filename)
            proximal_records = []
            for record in all_records:
                coords = record.get('coordinates', [])
                if len(coords) == 4:
                    existing_rect = QRectF(coords[0], coords[1], coords[2], coords[3])
                    # --- MODIFIED: Check for intersection with the expanded proximity rectangle ---
                    if proximity_rect.intersects(existing_rect):
                        proximal_records.append(record)
            
            if proximal_records:
                print(f"Found {len(proximal_records)} proximal patches. Merging them into the base for this operation.")
                painter = QPainter(temp_base_pixmap)
                for record in proximal_records:
                    patch_pixmap = self.model.get_inpaint_patch_pixmap(record["patch_filename"])
                    if patch_pixmap:
                        coords = record['coordinates']
                        target_point = QPointF(coords[0], coords[1])
                        painter.drawPixmap(target_point, patch_pixmap)
                painter.end()

            # 4. Convert the (potentially merged) pixmap to an OpenCV image
            buffer = QBuffer()
            buffer.open(QBuffer.ReadWrite)
            temp_base_pixmap.save(buffer, "PNG")
            pil_img = Image.open(io.BytesIO(buffer.data())).convert('RGB')
            image_np = np.array(pil_img)
            image_cv = cv2.cvtColor(image_np, cv2.COLOR_RGB2BGR)
            
            # 5. Create the mask from the new selection paths
            mask = np.zeros(image_cv.shape[:2], dtype=np.uint8)
            for path in paths:
                polygon = path.toFillPolygon().toPolygon()
                points = np.array([[p.x(), p.y()] for p in polygon], dtype=np.int32)
                if points.size > 0:
                    cv2.fillPoly(mask, [points], 255)

            # 6. Perform the inpainting
            inpainted_image_cv = cv2.inpaint(image_cv, mask, 3, cv2.INPAINT_TELEA)

            # 7. Extract the newly inpainted area (the patch)
            bounding_rect = new_selection_path.boundingRect().toRect()
            x, y, w, h = bounding_rect.x(), bounding_rect.y(), bounding_rect.width(), bounding_rect.height()
            if w <= 0 or h <= 0: return

            patch_cv = inpainted_image_cv[y:y+h, x:x+w]
            patch_rgb = cv2.cvtColor(patch_cv, cv2.COLOR_BGR2RGB)
            
            patch_h, patch_w, ch = patch_rgb.shape
            q_image = QImage(patch_rgb.data, patch_w, patch_h, ch * patch_w, QImage.Format_RGB888)
            patch_pixmap = QPixmap.fromImage(q_image)

            # 8. Save the new patch and its record to the model
            patch_filename = f"{os.path.splitext(target_label.filename)[0]}_{uuid.uuid4().hex[:8]}.png"
            record = {
                "id": str(uuid.uuid4()),
                "patch_filename": patch_filename,
                "target_image": target_label.filename,
                "coordinates": [x, y, w, h]
            }
            
            success, error_msg = self.model.add_inpaint_record(record, patch_pixmap)

            if success:
                if show_dialogs:
                    # Success message - keep QMessageBox.information for non-error cases
                    QMessageBox.information(self.scroll_area, "Success", "Context fill applied successfully.")
            else:
                raise Exception(error_msg)

        except Exception as e:
            print(f"Error during inpainting: {e}")
            traceback.print_exc(file=sys.stdout)
            if show_dialogs:
                exc_type, exc_value, exc_traceback = sys.exc_info()
                traceback_text = ''.join(traceback.format_exception(exc_type, exc_value, exc_traceback))
                ErrorDialog.critical(self.scroll_area, "Inpainting Error", f"An unexpected error occurred: {str(e)}", traceback_text)