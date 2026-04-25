# scroll_container.py

from PySide6.QtWidgets import QScrollArea, QWidget, QVBoxLayout, QPushButton
from PySide6.QtCore import Signal, QPoint
import qtawesome as qta
from app.handlers.stitch_handler import StitchHandler
from app.handlers.split_handler import SplitHandler
from app.handlers.context_fill_handler import ContextFillHandler
from app.handlers.manual_ocr_handler import ManualOCRHandler
# --- MODIFIED: Import the generic Menu class ---
from app.ui.widgets.menus import Menu
from app.ui.components.image_area.label import ResizableImageLabel
from assets.styles import SCROLL_OVERLAY_STYLES
    
class CustomScrollArea(QScrollArea):
    """
    A custom QScrollArea that now owns and manages all action handlers,
    making them independent of the main window.
    """
    resized = Signal()

    def __init__(self, model, selection_manager, on_initialize_reader, on_save_project, on_export_manhwa,
                 get_display_text, on_text_edited, on_delete_row, get_reader, get_settings, on_manual_ocr_cancelled,
                 parent=None):
        """ The scroll area instantiates its own action handlers, passing only
            the necessary components (self and the model). """
        super().__init__(parent)
        self.model = model
        self.selection_manager = selection_manager
        self.on_initialize_reader = on_initialize_reader
        self.on_save_project = on_save_project
        self.on_export_manhwa = on_export_manhwa
        self.get_display_text = get_display_text
        self.on_text_edited = on_text_edited
        self.on_delete_row = on_delete_row
        self.get_reader = get_reader
        self.get_settings = get_settings
        self.on_manual_ocr_cancelled = on_manual_ocr_cancelled
        self.overlay_widget = None
        self._text_is_visible = True
        self._inpainting_is_visible = True

        # Instantiate all handlers, breaking the MainWindow dependency
        self.manual_ocr_handler = ManualOCRHandler(self, self.model)
        self.manual_ocr_handler.reader_initialization_requested.connect(self.on_initialize_reader)
        self.stitch_handler = StitchHandler(self, self.model)
        self.split_handler = SplitHandler(self, self.model)
        self.context_fill_handler = ContextFillHandler(self, self.model)

        self.action_handlers = [
            self.manual_ocr_handler, self.stitch_handler,
            self.split_handler, self.context_fill_handler
        ]

        self._init_overlay()
        self.resized.connect(self.update_handler_ui_positions)
        self.verticalScrollBar().valueChanged.connect(self.update_handler_ui_positions)

    def create_image_label(self, pixmap, filename):
        """Factory to create a ResizableImageLabel wired with all necessary callbacks."""
        label = ResizableImageLabel(
            pixmap, filename,
            self.selection_manager,
            self.model,
            self.get_display_text,
            self.on_text_edited,
            self.on_delete_row,
            self
        )
        label.textBoxDeleted.connect(self.on_delete_row)
        label.inpaintRecordDeleted.connect(self.model.remove_inpaint_record)
        label.manual_area_selected.connect(self.manual_ocr_handler.handle_area_selected)
        label.manual_area_selected.connect(self.context_fill_handler.handle_area_selected)
        return label

    def _init_overlay(self):
        """ Creates and configures the overlay widget and its buttons. """
        self.overlay_widget = QWidget(self)
        self.overlay_widget.setObjectName("ScrollButtonOverlay")
        self.overlay_widget.setStyleSheet(SCROLL_OVERLAY_STYLES)

        layout = QVBoxLayout(self.overlay_widget)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(8)

        # Scroll to Top Button
        btn_scroll_top = QPushButton(qta.icon('fa5s.arrow-up', color='white'), "")
        btn_scroll_top.setObjectName("ScrollArrowButton")
        btn_scroll_top.setFixedSize(40, 40)
        btn_scroll_top.clicked.connect(lambda: self.verticalScrollBar().setValue(0))
        layout.addWidget(btn_scroll_top)

        # Save Menu Button
        btn_save_menu = QPushButton(qta.icon('fa5s.save', color='white'), "Save")
        btn_save_menu.setObjectName("ScrollSaveButton")
        btn_save_menu.setFixedSize(100, 40)
        btn_save_menu.clicked.connect(self._show_save_menu)
        layout.addWidget(btn_save_menu)

        # Scroll to Bottom Button
        btn_scroll_bottom = QPushButton(qta.icon('fa5s.arrow-down', color='white'), "")
        btn_scroll_bottom.setObjectName("ScrollArrowButton")
        btn_scroll_bottom.setFixedSize(40, 40)
        btn_scroll_bottom.clicked.connect(lambda: self.verticalScrollBar().setValue(self.verticalScrollBar().maximum()))
        layout.addWidget(btn_scroll_bottom)
    
    def _show_save_menu(self):
        """Creates, populates, and shows the Save menu."""
        trigger_button = self.sender()
        if not isinstance(trigger_button, QWidget):
            return

        menu = Menu(self)

        btn_save_project = QPushButton(qta.icon('fa5s.save', color='white'), " Save Project (.mmtl)")
        btn_save_project.clicked.connect(self.on_save_project)
        menu.addButton(btn_save_project)

        btn_save_images = QPushButton(qta.icon('fa5s.images', color='white'), " Save Rendered Images")
        btn_save_images.clicked.connect(self.on_export_manhwa)
        menu.addButton(btn_save_images)

        menu.set_position_and_show(trigger_button, 'right')

    def cancel_active_modes(self, exclude_handler=None):
        """Deactivates any currently running action handler mode."""
        if self.context_fill_handler.is_edit_mode_active and self.context_fill_handler is not exclude_handler:
            self.context_fill_handler._disable_edit_mode()
        for handler in self.action_handlers:
            if handler is not exclude_handler and handler.is_active:
                if hasattr(handler, 'cancel_mode'):
                    handler.cancel_mode()
                elif hasattr(handler, 'cancel_stitching_mode'):
                    handler.cancel_stitching_mode()
                elif hasattr(handler, 'cancel_splitting_mode'):
                    handler.cancel_splitting_mode()

    def toggle_text_visibility(self):
        """ Toggles the visibility of all text boxes in all image labels. """
        self._text_is_visible = not self._text_is_visible
        layout = self.widget().layout()
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_text_visibility(self._text_is_visible)

    def toggle_inpainting_visibility(self):
        """ Toggles whether the inpainting patches are applied to the images. """
        self._inpainting_is_visible = not self._inpainting_is_visible
        layout = self.widget().layout()
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_inpaints_applied(self._inpainting_is_visible)

    def update_handler_ui_positions(self):
        """ Updates the position of any active handler UI overlays. """
        for handler in self.action_handlers:
            if handler.is_active and hasattr(handler, '_update_widget_position'):
                handler._update_widget_position()

    def resizeEvent(self, event):
        """ Repositions the overlay on resize. """
        super().resizeEvent(event)
        self.update_overlay_position()
        self.resized.emit()

    def update_overlay_position(self):
        """ Calculates and sets the correct position for the overlay widget. """
        if self.overlay_widget:
            overlay_width = 140
            overlay_height = 186
            viewport_width = self.viewport().width()
            viewport_height = self.viewport().height()
            x = 10
            y = viewport_height - overlay_height - 10
            self.overlay_widget.setGeometry(x, y, overlay_width, overlay_height)
            self.overlay_widget.raise_()