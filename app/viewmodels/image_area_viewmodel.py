# app/viewmodels/image_area_viewmodel.py

from PySide6.QtCore import Signal
from app.viewmodels.base_viewmodel import BaseViewModel

class ImageAreaViewModel(BaseViewModel):
    """
    Manages image-list state, visibility toggles, and selection for the image area.
    Does NOT own action handlers (Stitch/Split/etc.) – those remain in CustomScrollArea
    for now and will be refactored in Phase 2c.
    """

    # --- Signals ---
    images_changed = Signal(list)                # list[str]
    selected_image_changed = Signal(str)         # filename
    text_visible_changed = Signal(bool)
    inpaints_visible_changed = Signal(bool)

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self._model = model

        self._images = []
        self._selected_image = ""
        self._text_visible = True
        self._inpaints_visible = True

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------
    @property
    def images(self):
        return self._images

    @images.setter
    def images(self, value):
        if self._images != value:
            self._images = value
            self.images_changed.emit(value)

    @property
    def selected_image(self):
        return self._selected_image

    @selected_image.setter
    def selected_image(self, value):
        if self._selected_image != value:
            self._selected_image = value
            self.selected_image_changed.emit(value)

    @property
    def text_visible(self):
        return self._text_visible

    @text_visible.setter
    def text_visible(self, value):
        if self._text_visible != value:
            self._text_visible = value
            self.text_visible_changed.emit(value)

    @property
    def inpaints_visible(self):
        return self._inpaints_visible

    @inpaints_visible.setter
    def inpaints_visible(self, value):
        if self._inpaints_visible != value:
            self._inpaints_visible = value
            self.inpaints_visible_changed.emit(value)

    # ------------------------------------------------------------------
    # Commands
    # ------------------------------------------------------------------
    def select_image(self, filename):
        """Called by Views when an image label is selected."""
        self.selected_image = filename

    def toggle_text_visibility(self):
        """Flips text box visibility across all image labels."""
        self.text_visible = not self._text_visible

    def toggle_inpaint_visibility(self):
        """Flips inpaint patch visibility across all image labels."""
        self.inpaints_visible = not self._inpaints_visible
