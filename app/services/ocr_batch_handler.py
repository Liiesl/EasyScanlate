# app/services/ocr_batch_handler.py

import os
import gc
from PySide6.QtCore import QObject, Signal, Slot
from app.core.ocr_processor import OCRProcessor
from app.core.inpaint_processor import InpaintProcessor
from app.core.project_model import ProjectModel


class BatchOCRHandler(QObject):
    """
    Manages the entire batch OCR process for multiple images.
    This object lives in the main thread but orchestrates worker QThreads.
    No QtWidgets imports.
    """

    batch_finished = Signal(int)
    error_occurred = Signal(str)
    processing_stopped = Signal()

    # De-QtWidget'd progress signals
    progress_changed = Signal(int)
    status_message_changed = Signal(str)

    def __init__(self, image_paths, reader, settings, starting_row_number, model: ProjectModel):
        super().__init__()
        self.image_paths = image_paths
        self.reader = reader
        self.settings = settings
        self.starting_row_number = starting_row_number
        self.model = model

        self.current_image_index = 0
        self.next_global_row_number = self.starting_row_number
        self._is_stopped = False
        self.ocr_thread = None
        self.inpaint_thread = None
        self._waiting_for_inpaint = False
        self._auto_context_fill = settings.get("auto_context_fill", False)

    def start_processing(self):
        """Starts the batch process."""
        print("Batch Handler: Starting processing...")
        self._is_stopped = False
        self.progress_changed.emit(0)
        self.status_message_changed.emit("Starting batch OCR...")
        self._process_next_image()

    def stop(self):
        """Requests the batch process to stop."""
        print("Batch Handler: Stop requested by user.")
        self._is_stopped = True
        if self.ocr_thread and self.ocr_thread.isRunning():
            self.ocr_thread.stop_requested = True
        if self.inpaint_thread and self.inpaint_thread.isRunning():
            self.inpaint_thread.stop_requested = True

    def _process_next_image(self):
        """Processes a single image or finishes the batch if all are done."""
        if self._is_stopped:
            print("Batch Handler: Process was stopped, not starting next image.")
            self.processing_stopped.emit()
            return

        if self.current_image_index >= len(self.image_paths):
            print("Batch Handler: All images processed.")
            self._finish_batch()
            return

        if not self.reader:
            self.error_occurred.emit("OCR Reader not available. Cannot process next image.")
            return

        image_path = self.image_paths[self.current_image_index]
        filename = os.path.basename(image_path)
        total = len(self.image_paths)
        current = self.current_image_index + 1
        print(f"Batch Handler: Creating thread for image {current}/{total}: {filename}")
        self.status_message_changed.emit(f"Processing image {current}/{total}: {filename}")

        # Filter out keys that OCRProcessor does not know about
        ocr_settings = {
            k: v for k, v in self.settings.items()
            if k in {
                "min_text_height", "max_text_height", "min_confidence",
                "distance_threshold", "adjust_contrast", "resize_threshold",
            }
        }

        self.ocr_thread = OCRProcessor(
            image_path=image_path,
            reader=self.reader,
            **ocr_settings
        )

        self.ocr_thread.ocr_progress.connect(self._handle_image_progress)
        self.ocr_thread.ocr_finished.connect(self._handle_image_results)
        self.ocr_thread.error_occurred.connect(self._handle_image_error)
        self.ocr_thread.finished.connect(self._on_ocr_thread_finished)

        self.ocr_thread.start()

    @Slot()
    def _on_ocr_thread_finished(self):
        """
        Called when the OCR QThread.run() has returned.
        If an inpaint thread was launched, we wait for it.
        """
        print(f"Batch Handler: OCR thread for image {self.current_image_index + 1} finished.")
        self.ocr_thread = None
        gc.collect()

        if self._waiting_for_inpaint:
            print("Batch Handler: Waiting for inpaint to finish before next image.")
            return

        self.current_image_index += 1
        self._process_next_image()

    def _handle_image_progress(self, progress):
        """Calculates and updates the overall batch progress."""
        total_images = len(self.image_paths)
        if total_images == 0:
            return
        per_image_contribution = 80.0 / total_images
        current_image_progress = progress / 100.0
        progress_base = 20 + (self.current_image_index * per_image_contribution)
        overall_progress = progress_base + (current_image_progress * per_image_contribution)
        self.progress_changed.emit(int(overall_progress))

    def _handle_image_results(self, processed_results):
        """Receives results from a single image and updates the model."""
        if self._is_stopped:
            print("Batch Handler: Ignoring results from finished image due to stop request.")
            return

        current_image_path = self.image_paths[self.current_image_index]
        filename = os.path.basename(current_image_path)

        newly_numbered_results = []
        all_coordinates = []

        if processed_results:
            try:
                processed_results.sort(key=lambda r: min(p[1] for p in r.get("coordinates", [[0, float("inf")]])))
            except (ValueError, TypeError, IndexError) as e:
                print(f"Warning: Could not sort processed results for {filename}: {e}. Using processor order.")

            for result in processed_results:
                result["filename"] = filename
                result["row_number"] = self.next_global_row_number
                result["is_manual"] = False
                result["translations"] = {}
                if self._auto_context_fill:
                    result["custom_style"] = {"bg_color": "#00000000"}
                    all_coordinates.append(result["coordinates"])
                newly_numbered_results.append(result)
                self.next_global_row_number += 1

        if newly_numbered_results:
            self.model.add_new_ocr_results(newly_numbered_results)
            print(f"Batch Handler: Added {len(newly_numbered_results)} blocks from {filename} to model.")

        # Launch background inpainting if requested
        if self._auto_context_fill and all_coordinates:
            print(f"Batch Handler: Launching auto-inpaint for {len(all_coordinates)} regions in {filename}.")
            self._waiting_for_inpaint = True
            self.inpaint_thread = InpaintProcessor(
                model=self.model,
                filename=filename,
                bounding_boxes=all_coordinates,
            )
            self.inpaint_thread.finished.connect(self._on_inpaint_finished)
            self.inpaint_thread.start()

    def _on_inpaint_finished(self, success, msg):
        """Called when the InpaintProcessor finishes."""
        print(f"Batch Handler: Inpaint finished: {msg}")
        self.inpaint_thread = None
        self._waiting_for_inpaint = False
        gc.collect()

        if self._is_stopped:
            self.processing_stopped.emit()
            return

        self.current_image_index += 1
        self._process_next_image()

    def _handle_image_error(self, message):
        """Handles an error from a worker thread."""
        print(f"Batch Handler: An error occurred: {message}")
        self._is_stopped = True
        self.error_occurred.emit(message)

    def _finish_batch(self):
        """Cleans up and signals that the entire batch is complete."""
        print("Batch Handler: Finishing run.")
        self.progress_changed.emit(100)
        self.status_message_changed.emit("Batch complete")
        self.batch_finished.emit(self.next_global_row_number)
        gc.collect()
