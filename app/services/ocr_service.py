# app/services/ocr_service.py

import traceback
from PySide6.QtCore import QObject, Signal

from app.core.rapid_ocr_engine import RapidOCREngine


class OCRService(QObject):
    """
    Owns the lifecycle of the RapidOCR reader.
    No QtWidgets; only QtCore Signals.
    """

    reader_ready = Signal(bool)  # True if initialization succeeded

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self._model = model
        self._reader = None

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------
    def initialize(self, context="OCR") -> bool:
        """
        Lazy-initializes the RapidOCR reader.
        Returns True if ready (or already initialized), False on failure.
        """
        if self._reader is not None:
            print("OCRService: RapidOCR reader already initialized.")
            return True

        try:
            language = getattr(self._model, 'original_language', 'Korean')
            print(f"OCRService: Initializing RapidOCR reader for {context} (language: {language})")
            self._reader = RapidOCREngine(language=language)
            print("OCRService: RapidOCR reader initialized successfully.")
            self.reader_ready.emit(True)
            return True
        except Exception as e:
            print(f"OCRService: Failed to initialize OCR reader for {context}: {e}")
            traceback.print_exc()
            self._reader = None
            self.reader_ready.emit(False)
            return False

    def reset(self):
        """Discards the current reader so the next initialize() call recreates it."""
        if self._reader is not None:
            print("OCRService: Resetting reader.")
            self._reader = None

    @property
    def reader(self):
        """Returns the current RapidOCREngine instance, or None."""
        return self._reader
