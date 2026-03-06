# app/ui/widgets/handler_overlay.py

from PySide6.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton
from PySide6.QtCore import QObject, Signal, Qt
import qtawesome as qta

from assets import HANDLER_OVERLAY_STYLES

class HandlerOverlay(QWidget):
    """
    Base class for handler UI overlays. Provides common functionality for
    positioning, button creation, and lifecycle management.
    """
    confirmed = Signal()
    cancelled = Signal()
    reset_clicked = Signal()

    def __init__(self, parent, object_name: str, title: str = "", fixed_size: tuple = None):
        super().__init__(parent)
        self.scroll_area = parent
        self._is_visible = False
        self._fixed_size = fixed_size or (350, 80)

        self.setObjectName(object_name)
        self.setFixedSize(*self._fixed_size)
        self.setAttribute(Qt.WA_StyledBackground, True)
        self.hide()

        self._main_layout = QVBoxLayout(self)
        self._main_layout.setContentsMargins(5, 5, 5, 5)
        self._main_layout.setSpacing(5)

        if title:
            self._title_label = QLabel(title)
            self._title_label.setAlignment(Qt.AlignCenter)
            self._main_layout.addWidget(self._title_label)

        self._button_layout = QHBoxLayout()
        self._button_layout.setSpacing(5)
        self._main_layout.addLayout(self._button_layout)

        self._confirm_button = None
        self._cancel_button = None
        self._reset_button = None

        self.setStyleSheet(HANDLER_OVERLAY_STYLES)

    def add_widget(self, widget):
        """Add a custom widget to the overlay."""
        self._main_layout.insertWidget(self._main_layout.count() - 1, widget)

    def create_confirm_button(self, text: str, icon: str = None, icon_color: str = 'white'):
        """Create a standard confirm button."""
        if icon:
            self._confirm_button = QPushButton(qta.icon(icon, color=icon_color), f" {text}")
        else:
            self._confirm_button = QPushButton(text)
        self._confirm_button.clicked.connect(self._on_confirm)
        self._button_layout.addWidget(self._confirm_button)
        return self._confirm_button

    def create_cancel_button(self, text: str = "Cancel", icon: str = None, icon_color: str = 'white'):
        """Create a standard cancel button."""
        if icon:
            self._cancel_button = QPushButton(qta.icon(icon, color=icon_color), f" {text}")
        else:
            self._cancel_button = QPushButton(text)
        self._cancel_button.setObjectName("CancelButton")
        self._cancel_button.clicked.connect(self._on_cancel)
        self._button_layout.addWidget(self._cancel_button)
        return self._cancel_button

    def create_reset_button(self, text: str = "Reset", icon: str = None, icon_color: str = 'white'):
        """Create a standard reset button."""
        if icon:
            self._reset_button = QPushButton(qta.icon(icon, color=icon_color), f" {text}")
        else:
            self._reset_button = QPushButton(text)
        self._reset_button.setObjectName("ResetButton")
        self._reset_button.clicked.connect(self._on_reset)
        self._button_layout.addWidget(self._reset_button)
        return self._reset_button

    def _on_confirm(self):
        self.confirmed.emit()

    def _on_cancel(self):
        self.cancelled.emit()

    def _on_reset(self):
        self.reset_clicked.emit()

    def show_overlay(self):
        """Show the overlay and position it."""
        self._update_position()
        self.show()
        self.raise_()
        self._is_visible = True

    def hide_overlay(self):
        """Hide the overlay."""
        self.hide()
        self._is_visible = False

    def _update_widget_position(self):
        """Position the overlay at the top-center of the scroll area viewport."""
        viewport = self.scroll_area.viewport()
        if not viewport or not self.isVisible():
            return
        overlay_x = (viewport.width() - self.width()) // 2
        overlay_y = 10
        self.move(overlay_x, overlay_y)

    def _update_position(self):
        """Alias for _update_widget_position for backward compatibility."""
        self._update_widget_position()

    def set_confirm_enabled(self, enabled: bool):
        """Enable or disable the confirm button."""
        if self._confirm_button:
            self._confirm_button.setEnabled(enabled)

    def set_reset_enabled(self, enabled: bool):
        """Enable or disable the reset button."""
        if self._reset_button:
            self._reset_button.setEnabled(enabled)


class HandlerController(QObject):
    """
    Base class for handlers that need both QObject (for signals) and a UI overlay.
    Provides multiple inheritance for handlers.
    """
    def __init__(self, scroll_area, model, overlay_object_name: str, title: str = "", fixed_size: tuple = None):
        super().__init__()
        self.scroll_area = scroll_area
        self.model = model
        self.is_active = False
        self.overlay = HandlerOverlay(scroll_area, overlay_object_name, title, fixed_size)
