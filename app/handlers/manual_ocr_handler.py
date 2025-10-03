# app/handlers/manual_ocr_handler.py

import traceback
import sys
import io
import numpy as np
from PIL import Image

from PySide6.QtWidgets import QMessageBox, QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton
from PySide6.QtCore import QBuffer, Signal, QObject
from app.ui.components import ResizableImageLabel
from assets import MANUALOCR_STYLES
from app.core.ocr_processor import OCRProcessor

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
        # --- NEW: Add state for the async processor ---
        self.ocr_thread = None
        self.crop_offset = None

        self._setup_ui()

    def _setup_ui(self):
        """Creates the overlay widget, parented to the scroll_area."""
        self.overlay_widget = QWidget(self.scroll_area)
        self.overlay_widget.setObjectName("ManualOCROverlay")
        self.overlay_widget.setStyleSheet(MANUALOCR_STYLES)
        overlay_layout = QVBoxLayout(self.overlay_widget)
        overlay_layout.setContentsMargins(5, 5, 5, 5)
        self.status_label = QLabel("Selected Area:") # Give it a name to update text
        overlay_layout.addWidget(self.status_label)
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
            self.scroll_area.cancel_active_modes(exclude_handler=self)
            self.is_active = True
            
            if not self.scroll_area.main_window.reader:
                print("ManualOCRHandler: Reader not found, requesting initialization...")
                self.reader_initialization_requested.emit()

            if not self.scroll_area.main_window.reader:
                print("ManualOCRHandler: Reader initialization failed.")
                self.cancel_mode() 
                return

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

        if self.ocr_thread and self.ocr_thread.isRunning():
            self.ocr_thread.stop_requested = True
        
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
        # Re-enable buttons
        self.btn_ocr_manual_area.setEnabled(True)
        self.btn_reset_manual_selection.setEnabled(True)
        self.status_label.setText("Selected Area:")


    def _clear_selection_state(self):
        """Hides the overlay and clears any graphical selection indicators."""
        self.overlay_widget.hide()
        if self.active_label:
            self.active_label.clear_selection_visuals()
        self.active_label = None
        self.selected_rect_scene = None
        self.crop_offset = None
        self.ocr_thread = None

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

    # ... (handle_area_selected and _update_widget_position are unchanged) ...
    def handle_area_selected(self, rect_scene, label_widget):
        """Callback for when a user finishes drawing a selection on an image."""
        if not self.is_active: return
        label_widget.clear_rubber_band()
        
        print(f"Handling completed manual selection from {label_widget.filename}")
        self.selected_rect_scene = rect_scene
        self.active_label = label_widget
        self._set_selection_enabled_on_all(False)
        
        label_widget.draw_selections([rect_scene])

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

    # --- REWRITTEN: This method now uses OCRProcessor ---
    def process_selected_area(self):
        """Crops the selected area, runs OCR using the OCRProcessor thread, and adds results."""
        main_window = self.scroll_area.main_window
        if not self.selected_rect_scene or not self.active_label or not main_window.reader:
            QMessageBox.warning(self.scroll_area, "Error", "Missing selection, image, or OCR reader.")
            self.reset_selection()
            return
        
        if self.ocr_thread and self.ocr_thread.isRunning():
            QMessageBox.warning(self.scroll_area, "Busy", "Already processing an area.")
            return

        print(f"Processing manual OCR for selection on {self.active_label.filename}")
        self.btn_ocr_manual_area.setEnabled(False)
        self.btn_reset_manual_selection.setEnabled(False)
        self.status_label.setText("Processing OCR...")

        try:
            crop_rect = self.selected_rect_scene.toRect()
            pixmap = self.active_label.original_pixmap
            bounded_crop_rect = crop_rect.intersected(pixmap.rect())
            if bounded_crop_rect.width() <= 1 or bounded_crop_rect.height() <= 1:
                 QMessageBox.warning(self.scroll_area, "Error", "Selection area is invalid or outside image bounds.")
                 self.reset_selection(); return

            self.crop_offset = (bounded_crop_rect.left(), bounded_crop_rect.top())
            
            cropped_pixmap = pixmap.copy(bounded_crop_rect)
            buffer = QBuffer(); buffer.open(QBuffer.ReadWrite); cropped_pixmap.save(buffer, "PNG")
            pil_image = Image.open(io.BytesIO(buffer.data()))

            settings = main_window.settings
            ocr_settings = {
                "min_text_height": int(settings.value("min_text_height", 40)),
                "max_text_height": int(settings.value("max_text_height", 100)),
                "min_confidence": float(settings.value("min_confidence", 0.2)),
                "distance_threshold": int(settings.value("distance_threshold", 100)),
                "batch_size": int(settings.value("ocr_batch_size", 8)),
                "decoder": settings.value("ocr_decoder", "beamsearch"),
                "adjust_contrast": float(settings.value("ocr_adjust_contrast", 0.5)),
                "resize_threshold": int(settings.value("ocr_resize_threshold", 1024)),
                "auto_context_fill": False
            }

            # --- MODIFIED: Use the ** operator to unpack the settings dictionary ---
            self.ocr_thread = OCRProcessor(
                reader=main_window.reader,
                image_data=pil_image,
                **ocr_settings
            )
            
            self.ocr_thread.ocr_finished.connect(self._handle_manual_ocr_results)
            self.ocr_thread.error_occurred.connect(self._handle_manual_ocr_error)
            self.ocr_thread.start()

        except Exception as e:
            print(f"Error preparing manual OCR processing: {e}")
            traceback.print_exc(file=sys.stdout)
            QMessageBox.critical(self.scroll_area, "Manual OCR Error", f"An unexpected error occurred: {str(e)}")
            self.reset_selection()

    # --- NEW: Slot to handle results from the processor thread ---
    def _handle_manual_ocr_results(self, processed_results):
        """
        Receives results, calculates a floating-point row number based on vertical
        position, and adds the new results to the model.
        """
        if not processed_results:
            QMessageBox.information(self.scroll_area, "Info", "No text was found in the selected area after filtering.")
            self.reset_selection()
            return

        try:
            # Sort new results vertically to process them in order
            processed_results.sort(key=lambda r: min(p[1] for p in r.get('coordinates', [[0, float('inf')]])))
        except (ValueError, TypeError, IndexError) as e:
            print(f"Warning: Could not sort manual OCR results: {e}. Using processor order.")

        filename_actual = self.active_label.filename
        offset_x, offset_y = self.crop_offset
        
        # Determine the absolute Y position of the new selection's top edge
        # We use the first result in the sorted list as the reference point
        if processed_results and 'coordinates' in processed_results[0]:
            new_selection_top_y = offset_y + min(p[1] for p in processed_results[0]['coordinates'])
        else: # Fallback if coordinates are somehow missing
            new_selection_top_y = offset_y

        # --- Find the row number of the text box immediately preceding the new selection ---
        anchor_row_number = 0.0
        # Get all existing results for this specific image
        image_results = [res for res in self.model.ocr_results if res.get('filename') == filename_actual]

        for res in image_results:
            # Check if the existing box is vertically above the new selection
            res_top_y = min(p[1] for p in res.get('coordinates', [[0,0]]))
            if res_top_y < new_selection_top_y:
                # Of all boxes above, find the one with the largest row number
                current_row = float(res.get('row_number', 0))
                if current_row > anchor_row_number:
                    anchor_row_number = current_row

        final_results_for_model = []
        
        # Get a set of all existing row numbers for quick collision checks
        all_existing_rows = {float(res.get('row_number', 0)) for res in self.model.ocr_results}
        
        increment = 0.1
        for res in processed_results:
            new_row_number = anchor_row_number + increment
            
            # Ensure the new row number is unique, incrementing if it collides
            while new_row_number in all_existing_rows:
                increment += 0.01 # Use smaller increments to find a free spot
                new_row_number = anchor_row_number + increment
            
            all_existing_rows.add(new_row_number) # Add to the set for the next loop

            # Convert the relative coordinates from the crop to absolute coordinates on the image
            coords_abs = [[int(p[0] + offset_x), int(p[1] + offset_y)] for p in res['coordinates']]
            
            final_results_for_model.append({
                'row_number': round(new_row_number, 4), # Round to prevent long floats
                'coordinates': coords_abs,
                'text': res['text'],
                'confidence': res['confidence'],
                'filename': filename_actual,
                'is_manual': True,
                'translations': {}
            })
            
            increment += 0.1 # Prepare for the next potential result in the same selection

        # --- IMPORTANT: We DO NOT touch self.model.next_global_row_number here ---

        if final_results_for_model:
            self.model.add_new_ocr_results(final_results_for_model)
            QMessageBox.information(self.scroll_area, "Success", f"Added {len(final_results_for_model)} new text block(s).")
        
        self.reset_selection()

    def _handle_manual_ocr_error(self, error_message):
        """Handles a critical error from the OCR processor thread."""
        QMessageBox.critical(self.scroll_area, "Manual OCR Error", f"An unexpected error occurred during processing:\n{error_message}")
        self.reset_selection()

    # ... (rest of the file is unchanged) ...```