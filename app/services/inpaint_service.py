# app/services/inpaint_service.py

from PySide6.QtCore import QObject


class InpaintService(QObject):
    """
    Thin delegate for inpaint record management.
    The heavy lifting (cv2.inpaint) now lives in app.core.inpaint_processor.
    """

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self.model = model

    def delete_inpaint_record(self, record_id: str) -> tuple[bool, str]:
        """Delegates to model.remove_inpaint_record."""
        return self.model.remove_inpaint_record(record_id)
