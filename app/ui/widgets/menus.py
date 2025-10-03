# app/ui/widgets/menus.py

from PySide6.QtWidgets import QWidget, QVBoxLayout, QPushButton
from PySide6.QtCore import Qt, QPoint
from PySide6.QtGui import QIcon
from assets import MENU_STYLES

class ToggleButton(QPushButton):
    """
    A custom QPushButton that acts as a toggle switch with 'on' and 'off' states.
    It can have different text and icons for each state.
    """
    def __init__(self, off_text: str, on_text: str, off_icon: QIcon = None, on_icon: QIcon = None, parent=None):
        """
        Initializes the toggle button.

        Args:
            off_text: The text to display when the button is in the 'off' state.
            on_text: The text to display when the button is in the 'on' state.
            off_icon: The icon for the 'off' state.
            on_icon: The icon for the 'on' state.
            parent: The parent widget.
        """
        super().__init__(off_text, parent)
        self.setCheckable(True)

        self._off_text = off_text
        self._on_text = on_text
        self._off_icon = off_icon
        self._on_icon = on_icon or off_icon # Use off_icon if on_icon is not provided

        self.toggled.connect(self._update_state)
        # Set initial state
        self._update_state(self.isChecked())

    def _update_state(self, checked: bool):
        """Internal slot to update the text, icon, and 'state' property for QSS styling."""
        if checked:
            self.setText(self._on_text)
            self.setIcon(self._on_icon)
            self.setProperty("state", "on")
        else:
            self.setText(self._off_text)
            self.setIcon(self._off_icon)
            self.setProperty("state", "off")
        
        # Force a style re-evaluation
        self.style().unpolish(self)
        self.style().polish(self)

    def setState(self, is_on: bool):
        """Programmatically sets the button's toggled state."""
        self.setChecked(is_on)

class Menu(QWidget):
    """
    A generic, constructor-based popup menu.
    This widget can be instantiated, populated with buttons, and then positioned
    and shown dynamically.
    """
    def __init__(self, parent=None):
        """
        Initializes the menu as a frameless, popup-style widget.
        """
        super().__init__(parent)
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.Popup)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setAttribute(Qt.WA_DeleteOnClose)

        self.setStyleSheet(MENU_STYLES)

        self.layout = QVBoxLayout(self)
        self.layout.setContentsMargins(5, 5, 5, 5)
        self.layout.setSpacing(1)

    def addButton(self, button: QPushButton, close_on_click: bool = True):
        """
        Adds a QPushButton to the menu's layout.
        
        Args:
            button: The QPushButton instance to add.
            close_on_click: If True, the menu will automatically close when the
                            button is clicked.
        """
        if not isinstance(button, QPushButton):
            raise TypeError("Only QPushButton instances can be added to the menu.")
        
        if close_on_click:
            button.clicked.connect(self.close)
            
        self.layout.addWidget(button)

    def set_position_and_show(self, trigger_button: QWidget, position: str):
        """
        Calculates the menu's position relative to a triggering widget and shows it.

        Args:
            trigger_button: The widget (e.g., a QPushButton) that the menu should
                            appear next to.
            position: A string indicating where the menu should be placed.
                      Options: 'bottom left', 'bottom right', 'top left', 'top right'.
        """
        self.setFixedSize(self.sizeHint())
        menu_size = self.sizeHint()
        
        # Map button coordinates to the global screen space
        button_top_left = trigger_button.mapToGlobal(trigger_button.rect().topLeft())
        button_top_right = trigger_button.mapToGlobal(trigger_button.rect().topRight())
        button_bottom_left = trigger_button.mapToGlobal(trigger_button.rect().bottomLeft())
        button_bottom_right = trigger_button.mapToGlobal(trigger_button.rect().bottomRight())

        # Determine the top-left position of the menu
        menu_pos = QPoint()
        if position == 'bottom left':
            menu_pos = button_bottom_left
        elif position == 'bottom right':
            menu_pos = QPoint(button_bottom_right.x() - menu_size.width(), button_bottom_right.y())
        elif position == 'top left':
            menu_pos = QPoint(button_top_left.x(), button_top_left.y() - menu_size.height())
        elif position == 'top right':
            menu_pos = QPoint(button_top_right.x() - menu_size.width(), button_top_right.y() - menu_size.height())
        else: # Default to bottom left
            menu_pos = button_bottom_left

        self.move(menu_pos)
        self.show()