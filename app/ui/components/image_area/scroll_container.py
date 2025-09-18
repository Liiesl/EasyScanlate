from PySide6.QtWidgets import QScrollArea, QWidget, QHBoxLayout, QPushButton
from PySide6.QtCore import Signal
import qtawesome as qta
from assets import IV_BUTTON_STYLES
    
class CustomScrollArea(QScrollArea):
    """
    A custom QScrollArea that features a self-contained, floating overlay with
    buttons for scrolling and saving.
    """
    # This signal is emitted when the "Save" button in the overlay is clicked.
    # It passes the button widget itself, which the main window uses to
    # position the save menu correctly.
    save_requested = Signal(QWidget)
    # This signal is emitted when the "Actions" menu button is clicked,
    # passing the button to allow for correct menu positioning.
    action_menu_requested = Signal(QWidget)
    resized = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.overlay_widget = None
        self._init_overlay()

    def _init_overlay(self):
        """
        Creates and configures the overlay widget and its buttons. This logic
        is now entirely encapsulated within the CustomScrollArea class.
        """
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

        # Action Menu Button (newly added)
        btn_action_menu = QPushButton(qta.icon('fa5s.bars', color='white'), "")
        btn_action_menu.setFixedSize(50, 50)
        btn_action_menu.clicked.connect(lambda: self.action_menu_requested.emit(btn_action_menu))
        btn_action_menu.setStyleSheet(IV_BUTTON_STYLES)
        layout.addWidget(btn_action_menu)

        # Save Menu Button
        btn_save_menu = QPushButton(qta.icon('fa5s.save', color='white'), "Save")
        btn_save_menu.setFixedSize(120, 50)
        # Connect the click to emit our custom signal, passing the button instance
        btn_save_menu.clicked.connect(lambda: self.save_requested.emit(btn_save_menu))
        btn_save_menu.setStyleSheet(IV_BUTTON_STYLES)
        layout.addWidget(btn_save_menu)

        # Scroll to Bottom Button
        btn_scroll_bottom = QPushButton(qta.icon('fa5s.arrow-down', color='white'), "")
        btn_scroll_bottom.setFixedSize(50, 50)
        btn_scroll_bottom.clicked.connect(lambda: self.verticalScrollBar().setValue(self.verticalScrollBar().maximum()))
        btn_scroll_bottom.setStyleSheet(IV_BUTTON_STYLES)
        layout.addWidget(btn_scroll_bottom)

    def resizeEvent(self, event):
        """
        When the scroll area is resized, reposition the overlay to keep it
        at the bottom-center of the viewport.
        """
        super().resizeEvent(event)
        self.update_overlay_position()
        self.resized.emit()

    def update_overlay_position(self):
        """
        Calculates the correct position for the overlay widget within the
        scroll area's viewport and moves it there.
        """
        if self.overlay_widget:
            # Increased width to accommodate the new button
            # 50(up) + 50(actions) + 120(save) + 50(down) + spacing + margins = ~295px. 320px gives good padding.
            overlay_width = 320
            overlay_height = 60
            
            # Use viewport size for positioning relative to the visible area
            viewport_width = self.viewport().width()
            viewport_height = self.viewport().height()

            # Center horizontally in the viewport
            x = (viewport_width - overlay_width) // 2
            # Place near the bottom of the viewport
            y = viewport_height - overlay_height - 10 

            self.overlay_widget.setGeometry(x, y, overlay_width, overlay_height)
            self.overlay_widget.raise_()