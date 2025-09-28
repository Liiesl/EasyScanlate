# app/handlers/manual_ocr_handler.py

import traceback
import sys
import io
import math
import numpy as np
from PIL import Image

from PySide6.QtWidgets import QMessageBox, QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton
from PySide6.QtCore import QBuffer, Signal, QObject
from app.utils.data_processing import group_and_merge_text
from app.ui.components import ResizableImageLabel
from assets import MANUALOCR_STYLES

class ManualOCRHandler(QObject):
    """Handles all logic for the Manual OCR feature, independent of MainWindow."""
    reader_initialization_requested = Signal()
    
    def __init__(self, scroll_area, model):
        super().__init__()
        self.scroll_area = scroll_area
        self.model = model
        self.is_active = False
        self.active_label = None
        self.selected_rect_scene = None

        self._setup_ui()

    def _setup_ui(self):
        """Creates the overlay widget, parented to the scroll_area."""
        self.overlay_widget = QWidget(self.scroll_area)
        self.overlay_widget.setObjectName("ManualOCROverlay")
        self.overlay_widget.setStyleSheet(MANUALOCR_STYLES)
        overlay_layout = QVBoxLayout(self.overlay_widget)
        overlay_layout.setContentsMargins(5, 5, 5, 5)
        overlay_layout.addWidget(QLabel("Selected Area:"))
        overlay_buttons = QHBoxLayout()
        
        self.btn_ocr_manual_area = QPushButton("OCR This Part")
        self.btn_ocr_manual_area.clicked.connect(self.process_selected_area)
        overlay_buttons.addWidget(self.btn_ocr_manual_area)
        
        self.btn_reset_manual_selection = QPushButton("Reset Selection")
        self.btn_reset_manual_selection.setObjectName("ResetButton")
        self.btn_reset_manual_selection.clicked.connect(self.reset_selection)
        overlay_buttons.addWidget(self.btn_reset_manual_selection)
        
        self.btn_cancel_manual_ocr = QPushButton("Cancel Manual OCR")
        self.btn_cancel_manual_ocr.setObjectName("CancelButton")
        self.btn_cancel_manual_ocr.clicked.connect(self.cancel_mode)
        overlay_buttons.addWidget(self.btn_cancel_manual_ocr)
        
        overlay_layout.addLayout(overlay_buttons)
        self.overlay_widget.setFixedSize(350, 80)
        self.overlay_widget.hide()

    def toggle_mode(self, checked):
        """Public method called by MainWindow to activate or deactivate the mode."""
        if checked:
            # Deactivate other conflicting modes before starting
            self.scroll_area.cancel_active_modes(exclude_handler=self)
            self.is_active = True
            
            # Check for the reader. If it doesn't exist, request initialization.
            if not self.scroll_area.main_window.reader:
                print("ManualOCRHandler: Reader not found, requesting initialization...")
                self.reader_initialization_requested.emit()

            # Check again. The connected slot in MainWindow should have run by now.
            if not self.scroll_area.main_window.reader:
                # If it's still not there, initialization must have failed.
                # MainWindow's _initialize_ocr_reader already shows a QMessageBox.
                print("ManualOCRHandler: Reader initialization failed.")
                self.cancel_mode() # Instantly cancel if reader isn't ready
                return

            # If we get here, the reader exists. Proceed with activation.
            print("ManualOCRHandler: Reader is ready. Activating mode.")
            self._clear_selection_state()
            self._set_selection_enabled_on_all(True)
            QMessageBox.information(self.scroll_area, "Manual OCR Mode",
                                    "Click and drag on an image to select an area for OCR.")
        else:
            if self.is_active:
                self.cancel_mode()

    def cancel_mode(self):
        """Cancels the manual OCR mode and resets the UI."""
        if not self.is_active: return
        print("Cancelling Manual OCR mode...")
        self.is_active = False
        
        # Tell MainWindow to update its button state
        main_window_button = self.scroll_area.main_window.btn_manual_ocr
        if main_window_button.isChecked():
            main_window_button.setChecked(False)

        self._clear_selection_state()
        self._set_selection_enabled_on_all(False)
        print("Manual OCR mode cancelled.")

    def reset_selection(self):
        """Clears the current selection to allow for a new one."""
        self._clear_selection_state()
        if self.is_active:
             self._set_selection_enabled_on_all(True)
             print("Selection reset. Ready for new selection.")

    def _clear_selection_state(self):
        """Hides the overlay and clears any graphical selection indicators."""
        self.overlay_widget.hide()
        if self.active_label:
            self.active_label.clear_selection_visuals()
        self.active_label = None
        self.selected_rect_scene = None

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

    def handle_area_selected(self, rect_scene, label_widget):
        """Callback for when a user finishes drawing a selection on an image."""
        if not self.is_active: return

        # --- BUG FIX: Hide the rubber band immediately after selection ---
        label_widget.clear_rubber_band()
        
        print(f"Handling completed manual selection from {label_widget.filename}")
        self.selected_rect_scene = rect_scene
        self.active_label = label_widget
        self._set_selection_enabled_on_all(False)  # Disable starting new ones
        
        # Draw a visual representation of the selection
        label_widget.draw_selections([rect_scene])

        # Position and show the control overlay
        self._update_widget_position()
        self.overlay_widget.show()
        self.overlay_widget.raise_()

    def _update_widget_position(self):
        """Positions the overlay widget at the top-center of the visible scroll area."""
        if not self.is_active: return
        viewport = self.scroll_area.viewport()
        overlay = self.overlay_widget
        overlay_x = (viewport.width() - overlay.width()) // 2
        overlay_y = 10 
        overlay.move(overlay_x, overlay_y)

    def process_selected_area(self):
        """Crops the selected area, runs OCR, and adds the results to the model."""
        main_window = self.scroll_area.main_window
        if not self.selected_rect_scene or not self.active_label or not main_window.reader:
            QMessageBox.warning(self.scroll_area, "Error", "Missing selection, image, or OCR reader.")
            self.reset_selection()
            return

        print(f"Processing manual OCR for selection on {self.active_label.filename}")
        self.overlay_widget.hide()

        try:
            crop_rect = self.selected_rect_scene.toRect()
            pixmap = self.active_label.original_pixmap
            bounded_crop_rect = crop_rect.intersected(pixmap.rect())
            if bounded_crop_rect.width() <= 1 or bounded_crop_rect.height() <= 1:
                 QMessageBox.warning(self.scroll_area, "Error", "Selection area is invalid or outside image bounds.")
                 self.reset_selection(); return

            cropped_pixmap = pixmap.copy(bounded_crop_rect)
            buffer = QBuffer(); buffer.open(QBuffer.ReadWrite); cropped_pixmap.save(buffer, "PNG")
            pil_image = Image.open(io.BytesIO(buffer.data())).convert('L')
            img_np = np.array(pil_image)

            settings = main_window.settings
            raw_results_relative = main_window.reader.readtext(
                img_np, batch_size=int(settings.value("ocr_batch_size", 1)),
                adjust_contrast=float(settings.value("ocr_adjust_contrast", 0.5)),
                decoder=settings.value("ocr_decoder", "beamsearch"), detail=1
            )

            if not raw_results_relative:
                 QMessageBox.information(self.scroll_area, "Info", "No text found in the selected area.")
                 self.reset_selection(); return
            
            # Use filter settings directly from the main_window instance
            filtered_results = self._filter_and_merge_results(raw_results_relative, main_window)

            if not filtered_results:
                QMessageBox.information(self.scroll_area, "Info", "No text passed the configured filters.")
                self.reset_selection(); return

            final_results_for_model = []
            filename_actual = self.active_label.filename
            offset_x, offset_y = bounded_crop_rect.left(), bounded_crop_rect.top()

            for res in filtered_results:
                coords_abs = [[int(p[0] + offset_x), int(p[1] + offset_y)] for p in res['coordinates']]
                final_results_for_model.append({
                    'coordinates': coords_abs, 'text': res['text'],
                    'confidence': res['confidence'], 'filename': filename_actual,
                    'is_manual': True
                })

            # Add all new results to the model in a single operation
            self.model.add_new_ocr_results(final_results_for_model)
            QMessageBox.information(self.scroll_area, "Success", f"Added {len(final_results_for_model)} text block(s).")
            self.reset_selection()

        except Exception as e:
            print(f"Error during manual OCR processing: {e}")
            traceback.print_exc(file=sys.stdout)
            QMessageBox.critical(self.scroll_area, "Manual OCR Error", f"An unexpected error occurred: {str(e)}")
            self.reset_selection()

    def _filter_and_merge_results(self, raw_results, main_window_ref):
        """Helper to apply filters and merge raw OCR text blocks."""
        temp_results = []
        min_h, max_h = main_window_ref.min_text_height, main_window_ref.max_text_height
        min_conf = main_window_ref.min_confidence

        for (coord, text, conf) in raw_results:
            y_coords = [p[1] for p in coord]
            height = max(y_coords) - min(y_coords) if y_coords else 0
            if (min_h <= height <= max_h and conf >= min_conf):
                temp_results.append({'coordinates': coord, 'text': text, 'confidence': conf})
        
        if not temp_results: return []

        return group_and_merge_text(
            temp_results, distance_threshold=main_window_ref.distance_threshold
        )