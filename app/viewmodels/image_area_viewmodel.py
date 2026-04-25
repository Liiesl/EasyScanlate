# app/viewmodels/image_area_viewmodel.py

import os
from PySide6.QtCore import Signal, QRectF
from PySide6.QtGui import QPainterPath
from app.viewmodels.base_viewmodel import BaseViewModel
from app.services.stitch_service import StitchService
from app.services.split_service import SplitService
from app.services.inpaint_service import InpaintService
from app.services.manual_ocr_service import ManualOCRService


class ImageAreaViewModel(BaseViewModel):
    """
    Manages image-list state, visibility toggles, selection, and action modes
    for the image area. Owns pure services for stitch/split/inpaint/manual OCR.
    """

    # --- Image list / visibility signals ---
    images_changed = Signal(list)                # list[str]
    selected_image_changed = Signal(str)         # filename
    text_visible_changed = Signal(bool)
    inpaints_visible_changed = Signal(bool)

    # --- Action mode signals ---
    active_action_mode_changed = Signal(str, str, bool)  # mode, message, can_confirm
    can_confirm_action_changed = Signal(bool)
    can_reset_action_changed = Signal(bool)
    action_mode_message_changed = Signal(str)
    action_mode_cancelled = Signal(str)

    # --- Mode-specific visual signals ---
    selected_images_for_stitch_changed = Signal(list)
    split_point_changed = Signal(str, int)
    inpaint_selection_paths_changed = Signal(str, list)
    inpaint_edit_mode_active_changed = Signal(bool)
    selected_inpaint_record_id_changed = Signal(str)
    manual_ocr_rect_changed = Signal(str, object)
    manual_ocr_processing_changed = Signal(bool)

    # Temporary error signal (Phase 7 TODO: move error dialog ownership to AppViewModel)
    error_occurred = Signal(str, str)
    # Emitted when manual OCR needs the reader but it isn't initialized yet.
    reader_initialization_requested = Signal()

    def __init__(self, model, get_reader=None, get_settings=None, parent=None):
        super().__init__(parent)
        self._model = model
        self._get_reader = get_reader
        self._get_settings = get_settings

        self._images = []
        self._selected_image = ""
        self._text_visible = True
        self._inpaints_visible = True

        # Action mode state
        self._active_action_mode = None
        self._action_mode_message = ""
        self._can_confirm_action = False
        self._can_reset_action = False

        # Stitch state
        self._selected_images_for_stitch = []

        # Split state
        self._split_filename = ""
        self._split_y = 0

        # Inpaint state
        self._inpaint_active_filename = ""
        self._inpaint_selection_paths = []
        self._inpaint_edit_mode_active = False
        self._selected_inpaint_record_id = None

        # Manual OCR state
        self._manual_ocr_filename = ""
        self._manual_ocr_rect = None
        self._manual_ocr_processing = False

        # Services
        self._stitch_service = StitchService(model, self)
        self._split_service = SplitService(model, self)
        self._inpaint_service = InpaintService(model, self)
        self._manual_ocr_service = ManualOCRService(model, get_reader, get_settings, self)

        self._manual_ocr_service.ocr_finished.connect(self._on_manual_ocr_finished)
        self._manual_ocr_service.error_occurred.connect(self._on_manual_ocr_error)

        self._model.image_list_changed.connect(self._on_image_list_changed)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------
    def _on_image_list_changed(self):
        self.images = [os.path.basename(p) for p in self._model.image_paths]

    def _update_action_mode_message(self, message: str):
        if self._action_mode_message != message:
            self._action_mode_message = message
            self.action_mode_message_changed.emit(message)

    def _set_can_confirm(self, value: bool):
        if self._can_confirm_action != value:
            self._can_confirm_action = value
            self.can_confirm_action_changed.emit(value)

    def _set_can_reset(self, value: bool):
        if self._can_reset_action != value:
            self._can_reset_action = value
            self.can_reset_action_changed.emit(value)

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

    @property
    def active_action_mode(self):
        return self._active_action_mode or ""

    @property
    def action_mode_message(self):
        return self._action_mode_message

    @property
    def can_confirm_action(self):
        return self._can_confirm_action

    @property
    def can_reset_action(self):
        return self._can_reset_action

    @property
    def selected_images_for_stitch(self):
        return self._selected_images_for_stitch

    @property
    def split_filename(self):
        return self._split_filename

    @property
    def split_y(self):
        return self._split_y

    @property
    def inpaint_active_filename(self):
        return self._inpaint_active_filename

    @property
    def inpaint_edit_mode_active(self):
        return self._inpaint_edit_mode_active

    @property
    def selected_inpaint_record_id(self):
        return self._selected_inpaint_record_id or ""

    @property
    def manual_ocr_processing(self):
        return self._manual_ocr_processing

    # ------------------------------------------------------------------
    # Commands
    # ------------------------------------------------------------------
    def select_image(self, filename):
        self.selected_image = filename

    def toggle_text_visibility(self):
        self.text_visible = not self._text_visible

    def toggle_inpaint_visibility(self):
        self.inpaints_visible = not self._inpaints_visible

    # --- Action Mode Management ---

    def start_action_mode(self, mode: str):
        """Activates an action mode, cancelling any previous mode."""
        self.cancel_action_mode()
        self._active_action_mode = mode

        if mode == "stitch":
            self._start_stitch_mode()
        elif mode == "split":
            self._start_split_mode()
        elif mode == "inpaint":
            self._start_inpaint_mode()
        elif mode == "manual_ocr":
            self._start_manual_ocr_mode()

        # Mode-specific start may have cancelled itself (e.g. manual OCR reader init failed).
        if self._active_action_mode != mode:
            return

        self.active_action_mode_changed.emit(
            mode, self._action_mode_message, self._can_confirm_action
        )
        self.can_confirm_action_changed.emit(self._can_confirm_action)
        self.can_reset_action_changed.emit(self._can_reset_action)
        self.action_mode_message_changed.emit(self._action_mode_message)

    def cancel_action_mode(self):
        """Cancels the current action mode and clears all transient state."""
        mode = self._active_action_mode
        if not mode and not self._inpaint_edit_mode_active:
            return

        self._active_action_mode = None
        self._action_mode_message = ""
        old_can_confirm = self._can_confirm_action
        old_can_reset = self._can_reset_action
        self._can_confirm_action = False
        self._can_reset_action = False

        # Clear stitch state
        if self._selected_images_for_stitch:
            self._selected_images_for_stitch = []
            self.selected_images_for_stitch_changed.emit([])

        # Clear split state
        if self._split_filename or self._split_y:
            self._split_filename = ""
            self._split_y = 0
            self.split_point_changed.emit("", 0)

        # Clear inpaint fill state
        if self._inpaint_active_filename or self._inpaint_selection_paths:
            self._inpaint_active_filename = ""
            self._inpaint_selection_paths = []
            self.inpaint_selection_paths_changed.emit("", [])

        # Clear manual OCR state
        if self._manual_ocr_filename or self._manual_ocr_rect or self._manual_ocr_processing:
            self._manual_ocr_filename = ""
            self._manual_ocr_rect = None
            self._manual_ocr_processing = False
            self.manual_ocr_rect_changed.emit("", None)
            self.manual_ocr_processing_changed.emit(False)
            self._manual_ocr_service.stop()

        if old_can_confirm != self._can_confirm_action:
            self.can_confirm_action_changed.emit(False)
        if old_can_reset != self._can_reset_action:
            self.can_reset_action_changed.emit(False)
        # Notify the view that the mode is cleared so it hides the overlay.
        self.active_action_mode_changed.emit("", "", False)
        if mode:
            self.action_mode_cancelled.emit(mode)

    def confirm_action_mode(self):
        """Confirms the current action mode."""
        mode = self._active_action_mode
        if mode == "stitch":
            self._confirm_stitch()
        elif mode == "split":
            self._confirm_split()
        elif mode == "inpaint":
            self._confirm_inpaint()
        elif mode == "manual_ocr":
            self._confirm_manual_ocr()

    def reset_action_mode(self):
        """Resets the current mode's selection state but stays in the mode."""
        mode = self._active_action_mode
        if mode == "stitch":
            self._selected_images_for_stitch = []
            self.selected_images_for_stitch_changed.emit([])
            self._update_stitch_state()
        elif mode == "split":
            self._split_filename = ""
            self._split_y = 0
            self.split_point_changed.emit("", 0)
            self._update_split_state()
        elif mode == "inpaint":
            self._inpaint_active_filename = ""
            self._inpaint_selection_paths = []
            self.inpaint_selection_paths_changed.emit("", [])
            self._update_inpaint_state()
        elif mode == "manual_ocr":
            self._manual_ocr_filename = ""
            self._manual_ocr_rect = None
            self.manual_ocr_rect_changed.emit("", None)
            self._update_manual_ocr_state()

        self.can_confirm_action_changed.emit(self._can_confirm_action)
        self.can_reset_action_changed.emit(self._can_reset_action)
        self.action_mode_message_changed.emit(self._action_mode_message)

    # --- Stitch ---

    def _start_stitch_mode(self):
        self._selected_images_for_stitch = []
        self._update_stitch_state()

    def _update_stitch_state(self):
        num = len(self._selected_images_for_stitch)
        if num == 0:
            self._update_action_mode_message("Click on images to select them for stitching.")
        elif num == 1:
            self._update_action_mode_message(
                f"<b>{self._selected_images_for_stitch[0]}</b> selected.<br>Select at least one more image."
            )
        else:
            self._update_action_mode_message(
                f"<b>{num}</b> images selected.<br>Click to reorder. (Top to Bottom)"
            )
        self._set_can_confirm(num >= 2)
        self._set_can_reset(num > 0)

    def toggle_image_selected_for_stitch(self, filename: str, selected: bool):
        if self._active_action_mode != "stitch":
            return
        temp = set(self._selected_images_for_stitch)
        if selected:
            temp.add(filename)
        else:
            temp.discard(filename)
        self._selected_images_for_stitch = [f for f in self._images if f in temp]
        self.selected_images_for_stitch_changed.emit(self._selected_images_for_stitch)
        self._update_stitch_state()

    def _confirm_stitch(self):
        if len(self._selected_images_for_stitch) < 2:
            self.error_occurred.emit(
                "Selection Error", "Please select at least two images to stitch."
            )
            return
        success, msg = self._stitch_service.stitch_images(self._selected_images_for_stitch)
        if success:
            self.cancel_action_mode()
        else:
            self.error_occurred.emit("Stitch Error", msg)

    # --- Split ---

    def _start_split_mode(self):
        self._split_filename = ""
        self._split_y = 0
        self._update_split_state()

    def _update_split_state(self):
        has_point = bool(self._split_filename) and self._split_y > 0
        if not self._split_filename:
            self._update_action_mode_message("Click on an image to place a split indicator.")
        else:
            self._update_action_mode_message(
                f"<b>{self._split_filename}</b> selected.<br>Click to move the indicator. (1 split / 2 pieces)"
            )
        self._set_can_confirm(has_point)
        self._set_can_reset(has_point)

    def set_split_point(self, filename: str, y: int):
        if self._active_action_mode != "split":
            return
        self._split_filename = filename
        self._split_y = max(0, y)
        self.split_point_changed.emit(filename, self._split_y)
        self._update_split_state()

    def _confirm_split(self):
        if not self._split_filename or self._split_y <= 0:
            self.error_occurred.emit("Input Error", "Please place a split indicator.")
            return
        success, msg, _ = self._split_service.split_image(self._split_filename, self._split_y)
        if success:
            self.cancel_action_mode()
        else:
            self.error_occurred.emit("Split Error", msg)

    # --- Inpaint ---

    def _start_inpaint_mode(self):
        self._inpaint_active_filename = ""
        self._inpaint_selection_paths = []
        self._update_inpaint_state()

    def _update_inpaint_state(self):
        has_selection = bool(self._inpaint_active_filename) and len(self._inpaint_selection_paths) > 0
        self._set_can_confirm(has_selection)
        self._set_can_reset(has_selection)
        if not self._inpaint_active_filename:
            self._update_action_mode_message(
                "Click and drag on an image to select an area to inpaint."
            )
        else:
            self._update_action_mode_message(
                f"Area selected on <b>{self._inpaint_active_filename}</b>. Ready to fill."
            )

    def toggle_inpaint_edit_mode(self):
        if self._inpaint_edit_mode_active:
            self._inpaint_edit_mode_active = False
            self._selected_inpaint_record_id = None
            self.inpaint_edit_mode_active_changed.emit(False)
            self.selected_inpaint_record_id_changed.emit("")
        else:
            if self._active_action_mode:
                self.cancel_action_mode()
            self._inpaint_edit_mode_active = True
            self.inpaint_edit_mode_active_changed.emit(True)

    def handle_area_selected(self, filename: str, rect: QRectF):
        if self._active_action_mode != "inpaint":
            return
        if not self._inpaint_active_filename:
            self._inpaint_active_filename = filename
        elif self._inpaint_active_filename != filename:
            self.error_occurred.emit(
                "Selection Error",
                "You can only make selections on one image at a time. Reset selections to switch.",
            )
            return

        new_path = QPainterPath()
        new_path.addRect(rect)
        remaining = []
        for existing in self._inpaint_selection_paths:
            if existing.intersects(new_path):
                new_path = new_path.united(existing)
            else:
                remaining.append(existing)
        remaining.append(new_path)
        self._inpaint_selection_paths = remaining
        self.inpaint_selection_paths_changed.emit(filename, self._inpaint_selection_paths)
        self._update_inpaint_state()

    def select_inpaint_record(self, record_id: str):
        self._selected_inpaint_record_id = record_id
        self.selected_inpaint_record_id_changed.emit(record_id or "")

    def _confirm_inpaint(self):
        if not self._inpaint_active_filename or not self._inpaint_selection_paths:
            self.error_occurred.emit("Error", "No area selected.")
            return
        success, msg = self._inpaint_service.process_inpaint(
            self._inpaint_active_filename, self._inpaint_selection_paths
        )
        if success:
            self.cancel_action_mode()
        else:
            self.error_occurred.emit("Inpainting Error", msg)

    def delete_selected_inpaint(self):
        if not self._selected_inpaint_record_id:
            return
        success, msg = self._inpaint_service.delete_inpaint_record(self._selected_inpaint_record_id)
        if success:
            self._selected_inpaint_record_id = None
            self.selected_inpaint_record_id_changed.emit("")
        else:
            self.error_occurred.emit("Error", msg)

    def perform_auto_inpainting(self, filename: str, bounding_boxes: list):
        self._inpaint_service.perform_auto_inpainting(filename, bounding_boxes)

    # --- Manual OCR ---

    def _start_manual_ocr_mode(self):
        reader = self._get_reader() if self._get_reader else None
        if not reader:
            print("ManualOCR: Reader not found, requesting initialization...")
            self.reader_initialization_requested.emit()
            reader = self._get_reader() if self._get_reader else None
            if not reader:
                print("ManualOCR: Reader initialization failed.")
                self.cancel_action_mode()
                return
        self._manual_ocr_filename = ""
        self._manual_ocr_rect = None
        self._manual_ocr_processing = False
        self._update_manual_ocr_state()

    def _update_manual_ocr_state(self):
        if not self._manual_ocr_filename:
            self._update_action_mode_message("Draw a box on an image to begin.")
        elif self._manual_ocr_processing:
            self._update_action_mode_message("Processing OCR...")
        else:
            self._update_action_mode_message("Area selected. Ready to OCR.")
        self._set_can_confirm(bool(self._manual_ocr_filename) and not self._manual_ocr_processing)
        self._set_can_reset(bool(self._manual_ocr_filename) and not self._manual_ocr_processing)

    def handle_manual_area_selected(self, filename: str, rect: QRectF):
        if self._active_action_mode != "manual_ocr":
            return
        self._manual_ocr_filename = filename
        self._manual_ocr_rect = rect
        self.manual_ocr_rect_changed.emit(filename, rect)
        self._update_manual_ocr_state()

    def _confirm_manual_ocr(self):
        if not self._manual_ocr_filename or not self._manual_ocr_rect:
            self.error_occurred.emit("Error", "Missing selection or image.")
            return
        if self._manual_ocr_processing:
            return
        success, msg = self._manual_ocr_service.start_ocr(
            self._manual_ocr_filename, self._manual_ocr_rect
        )
        if success:
            self._manual_ocr_processing = True
            self.manual_ocr_processing_changed.emit(True)
            self._update_manual_ocr_state()
        else:
            self.error_occurred.emit("Manual OCR Error", msg)

    def _on_manual_ocr_finished(self):
        self._manual_ocr_processing = False
        self.manual_ocr_processing_changed.emit(False)
        self.cancel_action_mode()

    def _on_manual_ocr_error(self, message: str):
        self._manual_ocr_processing = False
        self.manual_ocr_processing_changed.emit(False)
        self.error_occurred.emit("Manual OCR Error", message)
        self.reset_action_mode()
