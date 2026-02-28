# app/ui/widgets/menus.py

from PySide6.QtWidgets import QWidget, QVBoxLayout, QPushButton, QProgressBar, QLabel, QHBoxLayout, QSizePolicy
from PySide6.QtCore import Qt, QPoint, QPropertyAnimation, QEasingCurve
from PySide6.QtGui import QIcon

from assets import MENUS_STYLES

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
        self.setWindowFlags(Qt.Popup)
        self.setAttribute(Qt.WA_StyledBackground, True)
        self.setAttribute(Qt.WA_DeleteOnClose)

        self.layout = QVBoxLayout(self)
        self.layout.setContentsMargins(5, 5, 5, 5)
        self.layout.setSpacing(1)

        # Apply QSS styles
        self.setStyleSheet(MENUS_STYLES)

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
                      Options: 'bottom left', 'bottom right', 'top left', 'top right', 'right'.
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
        elif position == 'right':
            menu_pos = QPoint(button_top_right.x(), button_top_right.y())
        else: # Default to bottom left
            menu_pos = button_bottom_left

        self.move(menu_pos)
        self.show()

class ToggleWithProgress(QPushButton):
    """
    A custom button that integrates a progress bar. 
    It expands to show the progress bar when active (Stop state) 
    and collapses to a normal button when idle (Start state).
    """
    def __init__(self, start_text="Process OCR", stop_text="Stop OCR", 
                 start_icon=None, stop_icon=None, parent=None):
        super().__init__(parent)
        self.setCheckable(True)
        self.setSizePolicy(QSizePolicy.Minimum, QSizePolicy.Fixed)
        self.setMinimumHeight(40) # Match existing buttons

        self._start_text = start_text
        self._stop_text = stop_text
        self._start_icon = start_icon
        self._stop_icon = stop_icon
        
        # Main Layout
        self.layout = QHBoxLayout(self)
        self.layout.setContentsMargins(15, 0, 15, 0)
        self.layout.setSpacing(10)
        
        # Icon Label 
        self.icon_label = QLabel()
        self.text_label = QLabel(self._start_text)
        
        if self._start_icon:
            self.icon_label.setPixmap(self._start_icon.pixmap(20, 20))
            
        # Progress Container (Hidden by default)
        self.progress_container = QWidget()
        self.progress_layout = QHBoxLayout(self.progress_container)
        self.progress_layout.setContentsMargins(0, 0, 0, 0)
        self.progress_layout.setSpacing(10)
        
        self.progress_bar = QProgressBar()
        self.progress_bar.setTextVisible(False)
        self.progress_bar.setFixedHeight(4)
        
        self.percent_label = QLabel("0%")
        
        self.progress_layout.addWidget(self.progress_bar)
        self.progress_layout.addWidget(self.percent_label)
        
        # Add to main layout
        self.layout.addWidget(self.icon_label)
        self.layout.addWidget(self.text_label)
        self.layout.addWidget(self.progress_container)
        
        # Initial State
        self.progress_container.setVisible(False)
        self.transition_to_idle()

    def transition_to_active(self):
        """ Switches to 'Stop' state, showing progress bar. """
        self.setChecked(True)
        if self._stop_icon:
            self.icon_label.setPixmap(self._stop_icon.pixmap(20, 20))
        self.text_label.setText(self._stop_text)
        self.progress_container.setVisible(True)
        self.progress_bar.setValue(0)
        self.percent_label.setText("0%")
        self.update() # Force redraw

    def transition_to_idle(self):
        """ Switches to 'Start' state, hiding progress bar. """
        self.setChecked(False)
        if self._start_icon:
            self.icon_label.setPixmap(self._start_icon.pixmap(20, 20))
        self.text_label.setText(self._start_text)
        self.progress_container.setVisible(False)
        self.update()

    def set_progress(self, value, total):
        """ Updates the progress bar and percentage label. """
        if total > 0:
            percent = int((value / total) * 100)
            self.progress_bar.setMaximum(total)
            self.progress_bar.setValue(value)
            self.percent_label.setText(f"{percent}%")
        else:
            self.progress_bar.setValue(0)
            self.percent_label.setText("0%")

    def setValue(self, value):
        """ Compatibility method for QProgressBar interface. """
        self.progress_bar.setValue(value)
        total = self.progress_bar.maximum()
        if total > 0:
             percent = int((value / total) * 100)
             self.percent_label.setText(f"{percent}%")

    def setMaximum(self, value):
        self.progress_bar.setMaximum(value)