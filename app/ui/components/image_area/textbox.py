
from PySide6.QtWidgets import QGraphicsTextItem, QGraphicsItem, QGraphicsRectItem
from PySide6.QtCore import Qt, Signal, QRectF, QPointF, QObject
from PySide6.QtGui import QPainter, QFont, QBrush, QColor, QPen, QPainterPath, QLinearGradient

from app.ui.components.image_area.textbox_frame import SelectionFrameItem

# --- Signal class remains the same ---
class TextBoxSignals(QObject):
    rowDeleted = Signal(object)
    selectedChanged = Signal(bool, object)

class TextBoxItem(QGraphicsRectItem):
    def __init__(self, rect, row_number, text="", original_rect=None, initial_style=None):
        self.padding = 10
        bubble_rect = rect.adjusted(-self.padding, -self.padding, self.padding, self.padding)
        super().__init__(QRectF(0, 0, bubble_rect.width(), bubble_rect.height()))
        self.setPos(bubble_rect.x(), bubble_rect.y())
        self.setTransformOriginPoint(self.rect().center()) # For rotation

        self.signals = TextBoxSignals()
        self.setFlag(QGraphicsItem.ItemIsMovable, True)
        self.setFlag(QGraphicsItem.ItemIsSelectable, True)
        self.setFlag(QGraphicsItem.ItemSendsGeometryChanges, True)
        self.row_number = row_number
        self.original_rect = original_rect

        # --- Style and State Variables ---
        self._bubble_type, self.corner_radius, self._auto_font_size = 1, 50, True
        self._fill_type, self._bg_color, self._bg_gradient = 'solid', QColor(255, 255, 255), None
        self._border_width, self._border_color = 1, QColor(0, 0, 0)
        self._text_color_type, self._text_color, self._text_gradient = 'solid', QColor(0, 0, 0), None
        self._font, self._alignment = QFont("Arial", 12), Qt.AlignCenter
        self._original_pen = QPen(self._border_color, self._border_width)

        self.min_width, self.min_height = 50, 30
        
        # --- Create Selection Frame ---
        self.selection_frame = SelectionFrameItem(self)
        self.selection_frame.hide()

        # --- Text Item ---
        self.text_item = QGraphicsTextItem(text, self)
        
        if initial_style: self.apply_styles(initial_style)
        else: self.apply_styles({}) # Apply defaults

        self.setCursor(Qt.SizeAllCursor)

    def request_delete(self):
        """Emits the rowDeleted signal."""
        self.signals.rowDeleted.emit(self.row_number)

    def itemChange(self, change, value):
        if change == QGraphicsItem.ItemSelectedHasChanged:
            selected = bool(value)
            self.selection_frame.setVisible(selected)
            self.signals.selectedChanged.emit(selected, self.row_number)
        
        elif change == QGraphicsItem.ItemPositionChange and self.scene():
            # This logic is complex with rotation. For now, we'll keep it simple.
            # A truly robust solution would need to account for the rotated bounding box.
            scene_rect = self.scene().sceneRect()
            # Approximate the bounding rect in scene coords
            item_scene_rect = self.sceneBoundingRect()
            # Calculate future rect
            current_pos = self.pos()
            future_rect_pos = value
            future_rect = item_scene_rect.translated(future_rect_pos - current_pos)

            new_pos = QPointF(value)
            if future_rect.left() < scene_rect.left(): new_pos.setX(value.x() - (future_rect.left() - scene_rect.left()))
            elif future_rect.right() > scene_rect.right(): new_pos.setX(value.x() - (future_rect.right() - scene_rect.right()))
            if future_rect.top() < scene_rect.top(): new_pos.setY(value.y() - (future_rect.top() - scene_rect.top()))
            elif future_rect.bottom() > scene_rect.bottom(): new_pos.setY(value.y() - (future_rect.bottom() - scene_rect.bottom()))
            
            if new_pos != value and self.original_rect:
                 self.original_rect = self.rect().translated(new_pos)
            return new_pos

        elif change == QGraphicsItem.ItemScenePositionHasChanged:
            if self.original_rect is not None:
                self.original_rect = self.sceneBoundingRect()

        return super().itemChange(change, value)

    def apply_styles(self, style_dict):
        style_dict = self._ensure_style_defaults(style_dict)
        self._bubble_type = style_dict.get('bubble_type', 1)
        self.corner_radius = style_dict.get('corner_radius', 50)
        self._fill_type = style_dict.get('fill_type', 'solid')
        self._bg_color = QColor(style_dict.get('bg_color', '#ffffffff'))
        if self._fill_type == 'linear_gradient':
            gradient_data = style_dict.get('bg_gradient', {})
            self._bg_gradient = {
                'color1': QColor(gradient_data.get('color1', '#ffffffff')),
                'color2': QColor(gradient_data.get('color2', '#ffcccccc')),
                'direction': gradient_data.get('direction', 0),
                'midpoint': float(gradient_data.get('midpoint', 50))
            }
            self.setBrush(QBrush(Qt.NoBrush))
        else:
            self._bg_gradient = None
            self.setBrush(QBrush(self._bg_color))

        self._border_color = QColor(style_dict.get('border_color', '#ff000000'))
        self._border_width = style_dict.get('border_width', 1)
        new_pen = QPen(self._border_color, self._border_width)
        new_pen.setStyle(Qt.NoPen if self._border_width == 0 else Qt.SolidLine)
        self._original_pen = new_pen
        self.setPen(self._original_pen)

        self._text_color_type = style_dict.get('text_color_type', 'solid')
        self._text_color = QColor(style_dict.get('text_color', '#ff000000'))
        if self._text_color_type == 'linear_gradient':
            gradient_data = style_dict.get('text_gradient', {})
            self._text_gradient = {
                'color1': QColor(gradient_data.get('color1', '#ffffffff')),
                'color2': QColor(gradient_data.get('color2', '#ffcccccc')),
                'direction': gradient_data.get('direction', 0),
                'midpoint': float(gradient_data.get('midpoint', 50))
            }
            self.text_item.setVisible(False)
            self.text_item.setDefaultTextColor(Qt.transparent)
        else:
            self._text_gradient = None
            self.text_item.setDefaultTextColor(self._text_color)
            self.text_item.setVisible(True)

        font_family = style_dict.get('font_family', "Arial")
        font_size = style_dict.get('font_size', 12)
        font_bold = style_dict.get('font_bold', False)
        font_italic = style_dict.get('font_italic', False)
        self._auto_font_size = style_dict.get('auto_font_size', True)
        self._font = QFont()
        if font_family != "Default (System Font)": self._font.setFamily(font_family)
        self._font.setPointSize(font_size); self._font.setBold(font_bold); self._font.setItalic(font_italic)
        self.text_item.setFont(self._font)
        alignment_index = style_dict.get('text_alignment', 1)
        if alignment_index == 0: self._alignment = Qt.AlignLeft | Qt.AlignVCenter
        elif alignment_index == 1: self._alignment = Qt.AlignCenter
        elif alignment_index == 2: self._alignment = Qt.AlignRight | Qt.AlignVCenter
        else: self._alignment = Qt.AlignCenter
        doc_option = self.text_item.document().defaultTextOption()
        doc_option.setAlignment(self._alignment)
        self.text_item.document().setDefaultTextOption(doc_option)
        
        self.prepareGeometryChange()
        if self.rect().isValid(): self.setRect(self.rect())
        self.update()

    def _ensure_style_defaults(self, style_dict):
        style = style_dict.copy() if style_dict else {}
        if 'bg_gradient' not in style: style['bg_gradient'] = {}
        if 'midpoint' not in style['bg_gradient']: style['bg_gradient']['midpoint'] = 50
        if 'text_gradient' not in style: style['text_gradient'] = {}
        if 'midpoint' not in style['text_gradient']: style['text_gradient']['midpoint'] = 50
        return style

    def setRect(self, rect):
        new_width = max(rect.width(), self.min_width)
        new_height = max(rect.height(), self.min_height)
        local_rect = QRectF(0, 0, new_width, new_height)
        super().setRect(local_rect)
        self.setTransformOriginPoint(local_rect.center()) # Update rotation origin
        padding = self.padding
        text_rect_width = max(0, new_width - 2 * padding)
        self.text_item.setTextWidth(text_rect_width)
        self.adjust_font_size()
        if hasattr(self, 'selection_frame'):
            self.selection_frame.prepareGeometryChange() # Inform frame to update

    def adjust_font_size(self):
        padding = self.padding
        available_width = self.rect().width() - 2 * padding
        available_height = self.rect().height() - 2 * padding
        if available_width <= 0 or available_height <= 0:
            if self.text_item: self.text_item.setPlainText("")
            return
        if not self.text_item: return
        text = self.text_item.toPlainText()
        if not text:
            self.text_item.setPos(padding, padding)
            return
        font = self.text_item.font()
        if self._auto_font_size:
            min_font_size = 6; max_font_size = 72
            low, high = min_font_size, max_font_size
            optimal_size = low
            temp_text = QGraphicsTextItem()
            temp_text.setPlainText(text)
            temp_text.setTextWidth(available_width)
            temp_text.document().setDefaultTextOption(self.text_item.document().defaultTextOption())
            while low <= high:
                mid = (low + high) // 2
                if mid < min_font_size: break
                current_font = QFont(font); current_font.setPointSize(mid)
                temp_text.setFont(current_font)
                text_rect = temp_text.boundingRect()
                doc_height = temp_text.document().size().height()
                if doc_height <= available_height and text_rect.width() <= available_width:
                    optimal_size = mid; low = mid + 1
                else:
                    high = mid - 1
            del temp_text
            font.setPointSize(optimal_size)
            self.text_item.setFont(font)
            self._font = QFont(font)
        text_height = self.text_item.boundingRect().height()
        doc_height = self.text_item.document().size().height()
        effective_text_height = max(text_height, doc_height)
        vertical_offset = padding
        if self._alignment & Qt.AlignVCenter:
             vertical_offset = (self.rect().height() - effective_text_height) / 2
        elif self._alignment & Qt.AlignBottom:
             vertical_offset = self.rect().height() - effective_text_height - padding
        vertical_offset = max(padding, vertical_offset)
        self.text_item.setPos(padding, vertical_offset)

    def paint(self, painter, option, widget):
        painter.setRenderHint(QPainter.Antialiasing)
        rect = self.rect()
        path = QPainterPath()
        if self._bubble_type == 0: path.addRect(rect)
        elif self._bubble_type == 1: radius = min(rect.width()/2, rect.height()/2, self.corner_radius); path.addRoundedRect(rect, radius, radius)
        elif self._bubble_type == 2: path.addEllipse(rect)
        elif self._bubble_type == 3:
            radius = min(rect.width()/2, rect.height()/2, self.corner_radius); path.addRoundedRect(rect, radius, radius)
            tail_width = max(10, min(30, rect.width()*0.2)); tail_height = max(10, min(25, rect.height()*0.15))
            path.moveTo(rect.center().x() - tail_width/2, rect.bottom())
            control_y = rect.bottom() + tail_height*0.6
            path.cubicTo(rect.center().x() - tail_width*0.3, control_y, rect.center().x() + tail_width*0.3, control_y, rect.center().x() + tail_width/2, rect.bottom())
            path.closeSubpath()
        else: radius = min(rect.width()/2, rect.height()/2, self.corner_radius); path.addRoundedRect(rect, radius, radius)
        if self._fill_type == 'linear_gradient' and self._bg_gradient:
            gradient = QLinearGradient()
            direction = self._bg_gradient['direction']
            if direction == 0: gradient.setStart(rect.topLeft()); gradient.setFinalStop(rect.topRight())
            elif direction == 1: gradient.setStart(rect.topLeft()); gradient.setFinalStop(rect.bottomLeft())
            elif direction == 2: gradient.setStart(rect.topLeft()); gradient.setFinalStop(rect.bottomRight())
            elif direction == 3: gradient.setStart(rect.bottomLeft()); gradient.setFinalStop(rect.topRight())
            color1 = self._bg_gradient['color1']; color2 = self._bg_gradient['color2']
            midpoint_float = max(0.0, min(1.0, self._bg_gradient['midpoint'] / 100.0))
            gradient.setColorAt(0.0, color1); gradient.setColorAt(midpoint_float, color1); gradient.setColorAt(1.0, color2)
            painter.fillPath(path, gradient)
        elif self._fill_type == 'solid':
            painter.fillPath(path, self.brush())
        if self.pen().style() != Qt.NoPen:
            painter.strokePath(path, self.pen())
        if self._text_color_type == 'linear_gradient' and self._text_gradient:
            text = self.text_item.toPlainText()
            if text:
                text_pos = self.text_item.pos(); text_width = self.text_item.textWidth()
                text_height = self.text_item.document().size().height()
                text_rect = QRectF(text_pos.x(), text_pos.y(), text_width, text_height)
                if text_rect.isValid():
                    gradient = QLinearGradient()
                    direction = self._text_gradient['direction']
                    if direction == 0: gradient.setStart(text_rect.topLeft()); gradient.setFinalStop(text_rect.topRight())
                    elif direction == 1: gradient.setStart(text_rect.topLeft()); gradient.setFinalStop(text_rect.bottomLeft())
                    elif direction == 2: gradient.setStart(text_rect.topLeft()); gradient.setFinalStop(text_rect.bottomRight())
                    elif direction == 3: gradient.setStart(text_rect.bottomLeft()); gradient.setFinalStop(text_rect.topRight())
                    color1 = self._text_gradient['color1']; color2 = self._text_gradient['color2']
                    midpoint_float = max(0.0, min(1.0, self._text_gradient['midpoint'] / 100.0))
                    gradient.setColorAt(0.0, color1); gradient.setColorAt(midpoint_float, color1); gradient.setColorAt(1.0, color2)
                    painter.setFont(self._font); painter.setPen(QPen(gradient, 0))
                    draw_flags = int(self._alignment | Qt.TextWordWrap)
                    painter.drawText(text_rect, draw_flags, text)

    def cleanup(self):
        if hasattr(self, 'signals'):
            try:
                self.signals.rowDeleted.disconnect()
                self.signals.selectedChanged.disconnect()
            except (TypeError, RuntimeError): pass
        for child in reversed(self.childItems()):
            child.setParentItem(None)
            if child.scene(): child.scene().removeItem(child)
        self.text_item = None
        self.selection_frame = None
        if self.scene(): self.scene().removeItem(self)