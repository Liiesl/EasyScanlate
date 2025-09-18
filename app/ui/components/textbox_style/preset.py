from PySide6.QtWidgets import QPushButton, QMenu
from PySide6.QtGui import QPainter, QPen, QColor, QBrush, QFont, QLinearGradient
from PySide6.QtCore import Qt, Signal
from assets import DEFAULT_GRADIENT

class PresetButton(QPushButton):
    """A button that displays a preview of a style preset."""
    overwrite_requested = Signal(int)
    delete_requested = Signal(int)

    def __init__(self, index, parent=None):
        super().__init__(parent)
        self.index = index
        self._style = None
        self.setFixedSize(48, 48)
        self.setToolTip("Click to apply preset.\nRight-click for more options.")
        self.setContextMenuPolicy(Qt.CustomContextMenu)
        self.customContextMenuRequested.connect(self._show_context_menu)

    def set_style(self, style_dict):
        """Sets the style for this preset and triggers a repaint."""
        self._style = style_dict
        self.update()

    def _create_gradient_brush(self, rect, gradient_style):
        """Creates a QLinearGradient brush from a style dict."""
        direction = gradient_style.get('direction', 0)
        if direction == 0:  # Horizontal
            gradient = QLinearGradient(rect.topLeft(), rect.topRight())
        elif direction == 1:  # Vertical
            gradient = QLinearGradient(rect.topLeft(), rect.bottomLeft())
        elif direction == 2:  # Diagonal TL-BR
            gradient = QLinearGradient(rect.topLeft(), rect.bottomRight())
        else:  # Diagonal BL-TR
            gradient = QLinearGradient(rect.bottomLeft(), rect.topRight())

        color1 = QColor(gradient_style.get('color1', '#ffffffff'))
        color2 = QColor(gradient_style.get('color2', '#ff000000'))
        
        # NOTE: A simple 2-stop gradient is used for the preview.
        # The actual midpoint is saved but not visually rendered here.
        gradient.setColorAt(0, color1)
        gradient.setColorAt(1, color2)
        return QBrush(gradient)

    def paintEvent(self, event):
        """Paints the button with a style preview or as an empty slot."""
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        rect = self.rect().adjusted(2, 2, -2, -2) # Inset for border

        if self._style is None:
            # Draw empty, dashed slot
            pen = QPen(QColor("#555555"), 1, Qt.DashLine)
            painter.setPen(pen)
            painter.setBrush(Qt.NoBrush)
            painter.drawRect(self.rect().adjusted(0, 0, -1, -1))
            return

        # --- Create a full style by merging with a default baseline ---
        full_style = DEFAULT_GRADIENT.copy() # Start with a minimal base
        for key, value in self._style.items():
            if isinstance(value, dict) and key in full_style:
                full_style[key].update(value)
            else:
                full_style[key] = value

        # --- Draw Background ---
        border_width = full_style.get('border_width', 1)
        pen = QPen(QColor(full_style.get('border_color', '#ff000000')), border_width)
        pen.setJoinStyle(Qt.MiterJoin)
        painter.setPen(pen)

        fill_type = full_style.get('fill_type', 'solid')
        if fill_type == 'linear_gradient':
            bg_gradient = full_style.get('bg_gradient', {})
            brush = self._create_gradient_brush(rect, bg_gradient)
        else:
            brush = QBrush(QColor(full_style.get('bg_color', '#ffffffff')))
        painter.setBrush(brush)
        painter.drawRect(rect)

        # --- Draw Text ("Aa") ---
        font = QFont(
            full_style.get('font_family', 'Arial'),
            14 # Fixed size for preview
        )
        font.setBold(full_style.get('font_bold', False))
        font.setItalic(full_style.get('font_italic', False))
        font.setStyleName(full_style.get('font_style', 'Regular'))
        painter.setFont(font)
        
        text_color_type = full_style.get('text_color_type', 'solid')
        if text_color_type == 'linear_gradient':
            text_gradient = full_style.get('text_gradient', {})
            # For text, we set the pen to use a gradient brush
            gradient_brush = self._create_gradient_brush(rect, text_gradient)
            text_pen = QPen()
            text_pen.setBrush(gradient_brush)
            painter.setPen(text_pen)
        else:
            painter.setPen(QColor(full_style.get('text_color', '#ff000000')))

        painter.drawText(self.rect(), Qt.AlignCenter, "Aa")

    def _show_context_menu(self, pos):
        """Shows a context menu for overwriting or deleting a preset."""
        if self._style is None:
            return # No menu for empty slots

        menu = QMenu(self)
        overwrite_action = menu.addAction("Overwrite with current style")
        delete_action = menu.addAction("Delete preset")
        
        action = menu.exec_(self.mapToGlobal(pos))
        
        if action == overwrite_action:
            self.overwrite_requested.emit(self.index)
        elif action == delete_action:
            self.delete_requested.emit(self.index)