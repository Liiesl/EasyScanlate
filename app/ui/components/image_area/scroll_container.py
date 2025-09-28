# scroll_container.py

from PySide6.QtWidgets import QScrollArea, QWidget, QHBoxLayout, QPushButton
from PySide6.QtCore import Signal, QPoint
import qtawesome as qta
from assets import IV_BUTTON_STYLES
from app.handlers import StitchHandler, SplitHandler, ContextFillHandler, ManualOCRHandler
from app.ui.widgets.menus import ActionMenu
from app.ui.components import ResizableImageLabel
    
class CustomScrollArea(QScrollArea):
    """
    A custom QScrollArea that now owns and manages all action handlers,
    making them independent of the main window.
    """
    save_requested = Signal(QWidget)
    resized = Signal()

    def __init__(self, main_window, parent=None):
        """ The scroll area instantiates its own action handlers, passing only
            the necessary components (self and the model). """
        super().__init__(parent or main_window)
        self.main_window = main_window
        self.model = main_window.model
        self.overlay_widget = None
        self._text_is_visible = True
        
        # Instantiate all handlers, breaking the MainWindow dependency
        self.manual_ocr_handler = ManualOCRHandler(self, self.model)
        self.manual_ocr_handler.reader_initialization_requested.connect(self.main_window._initialize_ocr_reader)
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

    def _init_overlay(self):
        """ Creates and configures the overlay widget and its buttons. """
        self.overlay_widget = QWidget(self)
        self.overlay_widget.setObjectName("ScrollButtonOverlay")
        self.overlay_widget.setStyleSheet("#ScrollButtonOverlay { background-color: transparent; }")

        layout = QHBoxLayout(self.overlay_widget)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(1)

        # Scroll to Top Button
        btn_scroll_top = QPushButton(qta.icon('fa5s.arrow-up', color='white'), "")
        btn_scroll_top.setFixedSize(50, 50)
        btn_scroll_top.clicked.connect(lambda: self.verticalScrollBar().setValue(0))
        btn_scroll_top.setStyleSheet(IV_BUTTON_STYLES)
        layout.addWidget(btn_scroll_top)

        # Action Menu Button
        btn_action_menu = QPushButton(qta.icon('fa5s.bars', color='white'), "")
        btn_action_menu.setFixedSize(50, 50)
        btn_action_menu.clicked.connect(self._show_action_menu)
        btn_action_menu.setStyleSheet(IV_BUTTON_STYLES)
        layout.addWidget(btn_action_menu)

        # Save Menu Button
        btn_save_menu = QPushButton(qta.icon('fa5s.save', color='white'), "Save")
        btn_save_menu.setFixedSize(120, 50)
        btn_save_menu.clicked.connect(lambda: self.save_requested.emit(btn_save_menu))
        btn_save_menu.setStyleSheet(IV_BUTTON_STYLES)
        layout.addWidget(btn_save_menu)

        # Scroll to Bottom Button
        btn_scroll_bottom = QPushButton(qta.icon('fa5s.arrow-down', color='white'), "")
        btn_scroll_bottom.setFixedSize(50, 50)
        btn_scroll_bottom.clicked.connect(lambda: self.verticalScrollBar().setValue(self.verticalScrollBar().maximum()))
        btn_scroll_bottom.setStyleSheet(IV_BUTTON_STYLES)
        layout.addWidget(btn_scroll_bottom)

    def _show_action_menu(self):
        """ Creates, positions, and shows the ActionMenu. """
        button = self.sender()
        # ActionMenu now only needs a reference to this scroll_area
        menu = ActionMenu(self)
        menu_size = menu.sizeHint()
        button_top_left = button.mapToGlobal(button.rect().topLeft())
        menu_pos = QPoint(button_top_left.x(), button_top_left.y() - menu_size.height())
        menu.move(menu_pos)
        menu.show()
    
    def cancel_active_modes(self, exclude_handler=None):
        """Deactivates any currently running action handler mode."""
        for handler in self.action_handlers:
            if handler is not exclude_handler and handler.is_active:
                # Assuming all handlers have a consistent cancellation method name
                if hasattr(handler, 'cancel_mode'):
                    handler.cancel_mode()
                elif hasattr(handler, 'cancel_stitching_mode'):
                    handler.cancel_stitching_mode()
                elif hasattr(handler, 'cancel_splitting_mode'):
                    handler.cancel_splitting_mode()

    def toggle_text_visibility(self):
        """ Toggles the visibility of all text boxes in all image labels. """
        self._text_is_visible = not self._text_is_visible
        for i in range(self.main_window.scroll_layout.count()):
            widget = self.main_window.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_text_visibility(self._text_is_visible)

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
            overlay_width = 320
            overlay_height = 60
            viewport_width = self.viewport().width()
            viewport_height = self.viewport().height()
            x = (viewport_width - overlay_width) // 2
            y = viewport_height - overlay_height - 10 
            self.overlay_widget.setGeometry(x, y, overlay_width, overlay_height)
            self.overlay_widget.raise_()