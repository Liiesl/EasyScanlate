# app/services/manual_ocr_service.py

import io
import math
import os
import traceback
from PIL import Image

from PySide6.QtCore import QObject, Signal, QBuffer, QRectF
from PySide6.QtGui import QPixmap

from app.core.ocr_processor import OCRProcessor


class ManualOCRService(QObject):
    """
    Pure service for manual OCR on a selected image area.
    No QtWidgets, no layout mutation, no dialogs.
    """

    ocr_finished = Signal()
    error_occurred = Signal(str)

    def __init__(self, model, get_reader, get_settings, parent=None):
        super().__init__(parent)
        self.model = model
        self.get_reader = get_reader
        self.get_settings = get_settings
        self.ocr_thread = None
        self._current_filename = None
        self._crop_offset = None

    def start_ocr(self, filename: str, rect: QRectF) -> tuple[bool, str]:
        """
        Validates reader, crops rect from the image, starts OCRProcessor thread.
        Returns: (started, message)
        """
        reader = self.get_reader() if self.get_reader else None
        if not reader:
            return False, "OCR reader not initialized."

        if not rect or rect.width() <= 1 or rect.height() <= 1:
            return False, "Invalid selection area."

        images_dir = os.path.join(self.model.temp_dir, 'images')
        image_path = os.path.join(images_dir, filename)
        pixmap = QPixmap(image_path)
        if pixmap.isNull():
            return False, f"Could not load image: {filename}"

        crop_rect = rect.toRect()
        bounded_crop_rect = crop_rect.intersected(pixmap.rect())
        if bounded_crop_rect.width() <= 1 or bounded_crop_rect.height() <= 1:
            return False, "Selection area is invalid or outside image bounds."

        self._crop_offset = (bounded_crop_rect.left(), bounded_crop_rect.top())
        self._current_filename = filename

        cropped_pixmap = pixmap.copy(bounded_crop_rect)
        buffer = QBuffer()
        buffer.open(QBuffer.ReadWrite)
        cropped_pixmap.save(buffer, "PNG")
        pil_image = Image.open(io.BytesIO(buffer.data()))

        settings = self.get_settings() if self.get_settings else None
        ocr_settings = {
            "min_text_height": int(settings.value("min_text_height", 40)) if settings else 40,
            "max_text_height": int(settings.value("max_text_height", 100)) if settings else 100,
            "min_confidence": float(settings.value("min_confidence", 0.2)) if settings else 0.2,
            "distance_threshold": int(settings.value("distance_threshold", 100)) if settings else 100,
            "adjust_contrast": float(settings.value("ocr_adjust_contrast", 0.5)) if settings else 0.5,
            "resize_threshold": int(settings.value("ocr_resize_threshold", 1024)) if settings else 1024,
            "auto_context_fill": False,
        }

        self.ocr_thread = OCRProcessor(
            reader=reader,
            image_data=pil_image,
            **ocr_settings
        )
        self.ocr_thread.ocr_finished.connect(self._handle_results)
        self.ocr_thread.error_occurred.connect(self._handle_error)
        self.ocr_thread.start()
        return True, ""

    def stop(self):
        if self.ocr_thread and self.ocr_thread.isRunning():
            self.ocr_thread.stop_requested = True

    def _handle_results(self, processed_results):
        if not processed_results:
            self.ocr_finished.emit()
            return

        try:
            processed_results.sort(
                key=lambda r: min(p[1] for p in r.get('coordinates', [[0, float('inf')]]))
            )
        except (ValueError, TypeError, IndexError) as e:
            print(f"Warning: Could not sort manual OCR results: {e}")

        filename_actual = self._current_filename
        offset_x, offset_y = self._crop_offset

        if processed_results and 'coordinates' in processed_results[0]:
            new_selection_top_y = offset_y + min(p[1] for p in processed_results[0]['coordinates'])
        else:
            new_selection_top_y = offset_y

        anchor_row_number = 0.0
        image_results = [res for res in self.model.ocr_results if res.get('filename') == filename_actual]

        for res in image_results:
            res_top_y = min(p[1] for p in res.get('coordinates', [[0, 0]]))
            if res_top_y < new_selection_top_y:
                current_row = float(res.get('row_number', 0))
                if current_row > anchor_row_number:
                    anchor_row_number = current_row

        final_results = []
        all_existing_rows = {float(res.get('row_number', 0)) for res in self.model.ocr_results}
        increment = 0.1

        for res in processed_results:
            new_row_number = anchor_row_number + increment
            while new_row_number in all_existing_rows:
                increment += 0.01
                new_row_number = anchor_row_number + increment
            all_existing_rows.add(new_row_number)

            coords_abs = [[int(p[0] + offset_x), int(p[1] + offset_y)] for p in res['coordinates']]
            final_results.append({
                'row_number': round(new_row_number, 4),
                'coordinates': coords_abs,
                'text': res['text'],
                'confidence': res['confidence'],
                'filename': filename_actual,
                'is_manual': True,
                'translations': {},
            })
            increment += 0.1

        if final_results:
            self.model.add_new_ocr_results(final_results)

        self.ocr_finished.emit()

    def _handle_error(self, error_message):
        self.error_occurred.emit(error_message)
