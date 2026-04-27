# app/viewmodels/batch_ocr_viewmodel.py

import gc
from PySide6.QtCore import Signal
from app.viewmodels.base_viewmodel import BaseViewModel
from app.services.ocr_batch_handler import BatchOCRHandler


class BatchOCRViewModel(BaseViewModel):
    """
    Owns batch-OCR UI state and orchestrates the BatchOCRHandler.
    No QtWidgets imports.
    """

    # --- State signals ---
    is_running_changed = Signal(bool)
    progress_changed = Signal(int)
    status_message_changed = Signal(str)
    can_run_ocr_changed = Signal(bool)

    # --- Forwarded handler signals ---
    batch_finished = Signal(int)          # next_global_row_number
    error_occurred = Signal(str)
    processing_stopped = Signal()
    auto_inpaint_requested = Signal(str, list)

    def __init__(self, model, ocr_service, get_settings, parent=None):
        super().__init__(parent)
        self._model = model
        self._ocr_service = ocr_service
        self._get_settings = get_settings

        self._handler = None
        self._is_running = False
        self._progress = 0
        self._status_message = ""
        self._can_run_ocr = bool(self._model.image_paths)

        self._model.image_list_changed.connect(self._on_image_list_changed)

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------
    @property
    def is_running(self):
        return self._is_running

    @is_running.setter
    def is_running(self, value):
        if self._is_running != value:
            self._is_running = value
            self.is_running_changed.emit(value)
            self._update_can_run_ocr()

    @property
    def progress(self):
        return self._progress

    @progress.setter
    def progress(self, value):
        value = max(0, min(int(value), 100))
        if self._progress != value:
            self._progress = value
            self.progress_changed.emit(value)

    @property
    def status_message(self):
        return self._status_message

    @status_message.setter
    def status_message(self, value):
        if self._status_message != value:
            self._status_message = value
            self.status_message_changed.emit(value)

    @property
    def can_run_ocr(self):
        return self._can_run_ocr

    @can_run_ocr.setter
    def can_run_ocr(self, value):
        if self._can_run_ocr != value:
            self._can_run_ocr = value
            self.can_run_ocr_changed.emit(value)

    # ------------------------------------------------------------------
    # Public commands
    # ------------------------------------------------------------------
    @property
    def has_existing_standard_results(self) -> bool:
        return any(not res.get('is_manual', False) for res in self._model.ocr_results)

    def start_ocr(self) -> bool:
        """Starts a batch OCR run. Returns True if started, False if blocked."""
        if not self._model.image_paths:
            self.error_occurred.emit("No images loaded to process.")
            return False
        if self._is_running:
            self.error_occurred.emit("OCR is already running.")
            return False

        if not self._ocr_service.initialize("Standard OCR"):
            return False

        reader = self._ocr_service.reader
        if not reader:
            return False

        settings = self._get_settings() if self._get_settings else None
        ocr_settings = {
            "min_text_height": int(settings.value("min_text_height", 40)) if settings else 40,
            "max_text_height": int(settings.value("max_text_height", 100)) if settings else 100,
            "min_confidence": float(settings.value("min_confidence", 0.2)) if settings else 0.2,
            "distance_threshold": int(settings.value("distance_threshold", 100)) if settings else 100,
            "adjust_contrast": float(settings.value("ocr_adjust_contrast", 0.5)) if settings else 0.5,
            "resize_threshold": int(settings.value("ocr_resize_threshold", 1024)) if settings else 1024,
            "auto_context_fill": settings.value("auto_context_fill", "false").lower() == "true" if settings else False,
        }

        self._model.clear_standard_results()

        self._handler = BatchOCRHandler(
            image_paths=self._model.image_paths,
            reader=reader,
            settings=ocr_settings,
            starting_row_number=self._model.next_global_row_number,
            model=self._model,
        )

        self._handler.progress_changed.connect(self._on_progress_changed)
        self._handler.status_message_changed.connect(self._on_status_message_changed)
        self._handler.batch_finished.connect(self._on_batch_finished)
        self._handler.error_occurred.connect(self._on_batch_error)
        self._handler.processing_stopped.connect(self._on_batch_stopped)
        self._handler.auto_inpaint_requested.connect(self.auto_inpaint_requested)

        self.is_running = True
        self.progress = 0
        self.status_message = "Starting batch OCR..."
        self._handler.start_processing()
        return True

    def stop_ocr(self):
        """Requests the current batch to stop."""
        if self._handler:
            self._handler.stop()

    # ------------------------------------------------------------------
    # Internal slots
    # ------------------------------------------------------------------
    def _on_progress_changed(self, value):
        self.progress = value

    def _on_status_message_changed(self, msg):
        self.status_message = msg

    def _on_batch_finished(self, next_row_number):
        print("BatchOCRViewModel: Batch finished.")
        self._model.next_global_row_number = next_row_number
        self._cleanup()
        self.batch_finished.emit(next_row_number)

    def _on_batch_error(self, message):
        print(f"BatchOCRViewModel: Batch error: {message}")
        self._cleanup()
        self.error_occurred.emit(message)

    def _on_batch_stopped(self):
        print("BatchOCRViewModel: Batch stopped by user.")
        self._cleanup()
        self.processing_stopped.emit()

    def _cleanup(self):
        if self._handler:
            self._handler.deleteLater()
            self._handler = None
        gc.collect()
        self.is_running = False
        self.progress = 0
        self.status_message = ""

    def _on_image_list_changed(self):
        self._update_can_run_ocr()

    def _update_can_run_ocr(self):
        self.can_run_ocr = bool(self._model.image_paths) and not self._is_running
