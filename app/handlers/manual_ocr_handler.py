import traceback
import sys
import io
import math
import numpy as np
from PIL import Image

from PySide6.QtWidgets import QMessageBox, QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton
from PySide6.QtCore import QPoint, QBuffer
from app.utils.data_processing import group_and_merge_text
from app.ui.components import ResizableImageLabel
from assets import MANUALOCR_STYLES

class ManualOCRHandler:
    """Handles all logic for the Manual OCR feature."""
    def __init__(self, main_window):
        self.main_window = main_window
        self.is_active = False
        self.active_label = None
        self.selected_rect_scene = None

        self._setup_ui()

        # Connect signal to the main toggle button in MainWindow
        self.main_window.btn_manual_ocr.clicked.connect(self.toggle_mode)

    def _setup_ui(self):
        """Creates the overlay widget that appears after selecting an area."""
        self.overlay_widget = QWidget(self.main_window)
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
        """Activates or deactivates the manual OCR mode."""
        if checked:
            self.is_active = True
            self.main_window.btn_manual_ocr.setText("Cancel Manual OCR")
            self.main_window.btn_process.setEnabled(False)

            # Initialize EasyOCR reader if needed (shared instance)
            if self.main_window.reader is None:
                if not self.main_window._initialize_ocr_reader("Manual OCR"):
                    self.cancel_mode()
                    return

            if self.main_window.ocr_processor and self.main_window.ocr_processor.isRunning():
                print("Stopping ongoing standard OCR process to enter manual mode...")
                self.main_window.stop_ocr()

            self.main_window.btn_stop_ocr.setVisible(False)
            self._clear_selection_state()
            self._set_selection_enabled_on_all(True)
            QMessageBox.information(self.main_window, "Manual OCR Mode",
                                    "Click and drag on an image to select an area for OCR.")
        else:
            self.cancel_mode()

    def _clear_selection_state(self):
        """Hides the overlay and clears any graphical selection indicators."""
        self.overlay_widget.hide()
        self._clear_active_selection_graphics()
        self.active_label = None
        self.selected_rect_scene = None

    def cancel_mode(self):
        """Cancels the manual OCR mode and resets the UI."""
        print("Cancelling Manual OCR mode...")
        self.is_active = False
        if self.main_window.btn_manual_ocr.isChecked():
            self.main_window.btn_manual_ocr.setChecked(False)
        self.main_window.btn_manual_ocr.setText("Manual OCR")
        self.main_window.btn_process.setEnabled(bool(self.main_window.model.image_paths))
        self._clear_selection_state()
        self._set_selection_enabled_on_all(False)
        print("Manual OCR mode cancelled.")

    def reset_selection(self):
        """Clears the current selection to allow for a new one."""
        self._clear_selection_state()
        if self.is_active:
             self._set_selection_enabled_on_all(True)
             print("Selection reset. Ready for new selection.")
        else:
             print("Selection reset (mode was also exited).")

    def _set_selection_enabled_on_all(self, enabled):
        """Enables or disables the selection rubber band on all image labels."""
        for i in range(self.main_window.scroll_layout.count()):
            widget = self.main_window.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_manual_selection_enabled(enabled)

    def _clear_active_selection_graphics(self):
        """Tells the active label to remove its selection rectangle."""
        if self.active_label:
             self.active_label.clear_active_selection()
        else:
            # Fallback if active label is not set for some reason
            for i in range(self.main_window.scroll_layout.count()):
                widget = self.main_window.scroll_layout.itemAt(i).widget()
                if isinstance(widget, ResizableImageLabel):
                    widget.clear_active_selection()

    def handle_area_selected(self, rect_scene, label_widget):
        """Callback for when a user finishes drawing a selection on an image."""
        if not self.is_active:
            print("DEBUG: handle_area_selected called but manual_ocr_mode is False. Ignoring.")
            label_widget.clear_active_selection()
            return

        print(f"Handling completed manual selection from {label_widget.filename}")
        self.selected_rect_scene = rect_scene
        self.active_label = label_widget
        self._set_selection_enabled_on_all(False)  # Disable starting new ones

        try:
            # Position and show the control overlay
            label_pos_in_viewport = label_widget.mapTo(self.main_window.scroll_area.viewport(), QPoint(0, 0))
            global_pos = self.main_window.scroll_area.viewport().mapToGlobal(label_pos_in_viewport)
            main_window_pos = self.main_window.mapFromGlobal(global_pos)
            overlay = self.overlay_widget
            overlay_x = main_window_pos.x() + (label_widget.width() - overlay.width()) // 2
            overlay_y = main_window_pos.y() + label_widget.height() + 5
            overlay_x = max(0, min(overlay_x, self.main_window.width() - overlay.width()))
            overlay_y = max(0, min(overlay_y, self.main_window.height() - overlay.height()))
            overlay.move(overlay_x, overlay_y)
            overlay.show()
            overlay.raise_()
            print(f"Manual OCR overlay shown for selection on {label_widget.filename}")
        except Exception as e:
            print(f"Error positioning/showing manual OCR overlay: {e}")
            traceback.print_exc(file=sys.stdout)
            self.reset_selection()

    def process_selected_area(self):
        """Crops the selected area, runs OCR, and adds the results."""
        if not self.selected_rect_scene or not self.active_label or not self.main_window.reader:
            QMessageBox.warning(self.main_window, "Error", "No area selected, active label lost, or OCR reader not ready.")
            self.reset_selection()
            return

        print(f"Processing manual OCR for selection on {self.active_label.filename}")
        self.overlay_widget.hide()

        try:
            # 1. Get the Crop
            crop_rect = self.selected_rect_scene.toRect()
            if crop_rect.width() <= 0 or crop_rect.height() <= 0:
                QMessageBox.warning(self.main_window, "Error", "Invalid selection area.")
                self.reset_selection(); return

            pixmap = self.active_label.original_pixmap
            bounded_crop_rect = crop_rect.intersected(pixmap.rect())
            if bounded_crop_rect.width() <= 0 or bounded_crop_rect.height() <= 0:
                 QMessageBox.warning(self.main_window, "Error", "Selection area is outside image bounds.")
                 self.reset_selection(); return

            cropped_pixmap = pixmap.copy(bounded_crop_rect)
            buffer = QBuffer()
            buffer.open(QBuffer.ReadWrite); cropped_pixmap.save(buffer, "PNG")
            pil_image = Image.open(io.BytesIO(buffer.data())).convert('L')
            img_np = np.array(pil_image)

            # 2. Run OCR on the Cropped Area
            print(f"Running manual OCR on cropped area: {bounded_crop_rect}")
            settings = self.main_window.settings
            batch_size = int(settings.value("ocr_batch_size", 1))
            decoder = settings.value("ocr_decoder", "beamsearch")
            adjust_contrast = float(settings.value("ocr_adjust_contrast", 0.5))

            raw_results_relative = self.main_window.reader.readtext(
                img_np, batch_size=batch_size, adjust_contrast=adjust_contrast,
                decoder=decoder, detail=1
            )
            print(f"Manual OCR raw results (relative coords): {raw_results_relative}")

            if not raw_results_relative:
                 QMessageBox.information(self.main_window, "Info", "No text found in the selected area.")
                 self.reset_selection(); return

            # 3. Pre-filter RAW results BEFORE Merging
            temp_results_for_merge = []
            self.main_window._load_filter_settings()
            min_h, max_h = self.main_window.min_text_height, self.main_window.max_text_height
            min_conf = self.main_window.min_confidence
            print(f"Pre-filtering {len(raw_results_relative)} raw manual results (MinH={min_h}, MaxH={max_h}, MinConf={min_conf})...")

            for (coord_rel, text, confidence) in raw_results_relative:
                raw_height = 0
                if coord_rel:
                     try:
                         y_coords_rel = [p[1] for p in coord_rel]
                         raw_height = max(y_coords_rel) - min(y_coords_rel) if y_coords_rel else 0
                     except (ValueError, IndexError, TypeError) as coord_err:
                          print(f"Warning: Error calculating raw height for coords {coord_rel}. Error: {coord_err}")
                          raw_height = 0

                if (min_h <= raw_height <= max_h and confidence >= min_conf):
                    temp_results_for_merge.append({
                        'coordinates': coord_rel, 'text': text, 'confidence': confidence,
                        'filename': "manual_crop", # Placeholder
                    })
                else:
                    exclusion_reasons = []
                    if not (min_h <= raw_height <= max_h):
                         exclusion_reasons.append(f"height {raw_height:.1f}px (bounds: {min_h}-{max_h})")
                    if confidence < min_conf:
                        exclusion_reasons.append(f"low confidence ({confidence:.2f} < {min_conf})")
                    if exclusion_reasons:
                         print(f"Excluded RAW manual block ({', '.join(exclusion_reasons)}): '{text[:50]}...'")

            if not temp_results_for_merge:
                QMessageBox.information(self.main_window, "Info", "No text found in the selected area passed the initial filters.")
                self.reset_selection(); return

            # 4. Merge the PRE-FILTERED Results
            merged_results_relative = group_and_merge_text(
                temp_results_for_merge,
                distance_threshold=self.main_window.distance_threshold
            )
            print(f"Internal merge of pre-filtered results produced {len(merged_results_relative)} final block(s).")

            # 5. Process Final MERGED Blocks
            filename_actual = self.active_label.filename
            any_change_made = False
            offset_x, offset_y = bounded_crop_rect.left(), bounded_crop_rect.top()

            for merged_result in merged_results_relative:
                coords_relative = merged_result['coordinates']
                if not coords_relative: continue

                coords_absolute = [[int(p[0] + offset_x), int(p[1] + offset_y)] for p in coords_relative]
                try:
                    new_row_number = self._calculate_row_number(coords_absolute, filename_actual)
                except Exception as e:
                     print(f"Error calculating row number for manual block '{merged_result['text'][:20]}...': {e}. Skipping.")
                     continue

                final_result = {
                    'coordinates': coords_absolute, 'text': merged_result['text'],
                    'confidence': merged_result['confidence'], 'filename': filename_actual,
                    'is_manual': True, 'row_number': new_row_number
                }
                self.main_window.model.ocr_results.append(final_result)
                any_change_made = True
                print(f"Added final MERGED manual block: Row {new_row_number}, Text: '{merged_result['text'][:20]}...'")

            # 6. Sort Results & Update UI
            if any_change_made:
                 self.main_window.model._sort_ocr_results()
                 self.main_window.update_all_views(affected_filenames=[filename_actual])
                 QMessageBox.information(self.main_window, "Success", f"Added {len(merged_results_relative)} text block(s) from manual selection.")

            # 7. Reset state for new selection
            self.reset_selection()

        except Exception as e:
            print(f"Error during manual OCR processing: {e}")
            traceback.print_exc(file=sys.stdout)
            QMessageBox.critical(self.main_window, "Manual OCR Error", f"An unexpected error occurred: {str(e)}")
            self.reset_selection()

    def _calculate_row_number(self, coordinates, filename):
        """Calculates a new fractional row number for manually added text."""
        if not coordinates: return 0.0
        try:
            sort_key_y = min(p[1] for p in coordinates)
        except (ValueError, TypeError, IndexError) as e:
            print(f"Error calculating sort key Y: {e}"); return float('inf')

        preceding_result = None
        for res in self.main_window.model.ocr_results:
            if res.get('is_deleted', False): continue
            res_filename, res_coords = res.get('filename', ''), res.get('coordinates')
            res_row_number_raw = res.get('row_number')
            if res_row_number_raw is None or res_coords is None: continue

            try: res_sort_key_y = min(p[1] for p in res_coords)
            except: continue

            if res_filename < filename or (res_filename == filename and res_sort_key_y < sort_key_y):
                preceding_result = res
            else:
                break

        base_row_number = 0
        if preceding_result:
            try:
                base_row_number = math.floor(float(preceding_result.get('row_number', 0.0)))
            except (ValueError, TypeError): pass

        max_sub_index_for_base = 0
        for res in self.main_window.model.ocr_results:
             current_row_num_raw = res.get('row_number')
             if current_row_num_raw is None: continue
             try:
                 current_row_num_float = float(current_row_num_raw)
                 if math.floor(current_row_num_float) == base_row_number:
                      epsilon = 1e-9
                      sub_index = int((current_row_num_float - base_row_number) * 10 + epsilon)
                      if sub_index > 0:
                           max_sub_index_for_base = max(max_sub_index_for_base, sub_index)
             except (ValueError, TypeError): pass

        new_sub_index = max_sub_index_for_base + 1
        return float(base_row_number) + (float(new_sub_index) / 10.0)