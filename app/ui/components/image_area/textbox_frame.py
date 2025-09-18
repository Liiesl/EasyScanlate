# --- START OF FILE image_area_widgets.py ---

from PySide6.QtWidgets import QGraphicsTextItem, QGraphicsItem, QGraphicsRectItem
from PySide6.QtCore import Qt, Signal, QRectF, QPointF, QObject,QLineF
from PySide6.QtGui import QPainter, QFont, QBrush, QColor, QPen, QPainterPath, QLinearGradient, QTransform, QPolygonF

# --- NEW: Custom Item for Selection and Resize Handles ---
class SelectionFrameItem(QGraphicsItem):
    """
    A custom QGraphicsItem that provides a selection, resize, and rotation frame.
    It draws an outline, resize handles, a rotation handle, and a delete button.
    It also supports free transform (perspective distortion) when dragging handles with Ctrl key.
    This item is intended to be a child of the item it frames (e.g., TextBoxItem).
    """

    def __init__(self, parent):
        super().__init__(parent)
        self.parent_item = parent
        self.setZValue(10)  # Ensure it's drawn on top of the parent
        self.setAcceptHoverEvents(True)

        # --- Configuration ---
        self.handle_size = 10
        self.outline_color = QColor(0, 120, 215)  # Standard blue selection color
        self.handle_fill_color = QColor(255, 255, 255)
        self.delete_btn_size = 20
        self.rotate_btn_size = 20
        self.control_offset = 15  # How far above the top edge the controls are

        # --- State ---
        self.active_handle = None
        self.drag_start_pos = None
        self.drag_start_rect = None
        self.drag_start_angle = 0
        self.drag_start_center = QPointF()
        self.is_free_transform = False  # True if Ctrl is held during a resize drag
        self.initial_scene_quad = None  # Stores the item's corner positions for free transform

        # --- Caches for hit testing ---
        self._handle_rects = {}
        self._delete_btn_rect = QRectF()
        self._rotate_handle_rect = QRectF()

    def _update_geometry(self):
        """Recalculates positions of all frame elements based on parent's rect."""
        parent_rect = self.parent_item.rect()
        hs = self.handle_size
        hs_half = hs / 2.0

        self._handle_rects = {
            'tl': QRectF(parent_rect.left() - hs_half, parent_rect.top() - hs_half, hs, hs),
            'tr': QRectF(parent_rect.right() - hs_half, parent_rect.top() - hs_half, hs, hs),
            'bl': QRectF(parent_rect.left() - hs_half, parent_rect.bottom() - hs_half, hs, hs),
            'br': QRectF(parent_rect.right() - hs_half, parent_rect.bottom() - hs_half, hs, hs),
            't': QRectF(parent_rect.center().x() - hs_half, parent_rect.top() - hs_half, hs, hs),
            'b': QRectF(parent_rect.center().x() - hs_half, parent_rect.bottom() - hs_half, hs, hs),
            'l': QRectF(parent_rect.left() - hs_half, parent_rect.center().y() - hs_half, hs, hs),
            'r': QRectF(parent_rect.right() - hs_half, parent_rect.center().y() - hs_half, hs, hs),
        }

        # Calculate rotation handle position
        top_handle_center = self._handle_rects['t'].center()
        rotate_btn_center_y = top_handle_center.y() - self.control_offset - self.rotate_btn_size / 2
        rotate_btn_center = QPointF(top_handle_center.x(), rotate_btn_center_y)
        self._rotate_handle_rect = QRectF(0, 0, self.rotate_btn_size, self.rotate_btn_size)
        self._rotate_handle_rect.moveCenter(rotate_btn_center)

        # Calculate delete button position, shifted right from the rotate handle
        delete_btn_center_x = self._rotate_handle_rect.center().x() + self._rotate_handle_rect.width() / 2 + self.control_offset
        delete_btn_center = QPointF(delete_btn_center_x, rotate_btn_center.y())
        self._delete_btn_rect = QRectF(0, 0, self.delete_btn_size, self.delete_btn_size)
        self._delete_btn_rect.moveCenter(delete_btn_center)

    def boundingRect(self):
        """The bounding rect must include the parent, handles, and control buttons."""
        self._update_geometry()
        rect = self.parent_item.rect()
        for handle_rect in self._handle_rects.values():
            rect = rect.united(handle_rect)
        rect = rect.united(self._delete_btn_rect)
        rect = rect.united(self._rotate_handle_rect)
        return rect.adjusted(-2, -2, 2, 2)  # Add a small margin for pen widths

    def paint(self, painter, option, widget=None):
        self._update_geometry()  # Ensure positions are fresh for painting
        painter.setRenderHint(QPainter.Antialiasing)
        
        # 1. Draw main outline
        outline_pen = QPen(self.outline_color, 1.5, Qt.SolidLine)
        painter.setPen(outline_pen)
        painter.setBrush(Qt.NoBrush)
        painter.drawRect(self.parent_item.rect())

        # 2. Draw 8 resize handles (unfilled rectangles)
        handle_pen = QPen(self.outline_color, 1)
        painter.setPen(handle_pen)
        painter.setBrush(self.handle_fill_color)
        for handle_rect in self._handle_rects.values():
            painter.drawRect(handle_rect)

        # 3. Draw center '+' indicator
        center = self.parent_item.rect().center()
        cross_size = 5
        painter.setPen(outline_pen)
        painter.drawLine(QPointF(center.x() - cross_size, center.y()), QPointF(center.x() + cross_size, center.y()))
        painter.drawLine(QPointF(center.x(), center.y() - cross_size), QPointF(center.x(), center.y() + cross_size))

        # 4. Draw control buttons and their connecting lines
        top_handle_center = self._handle_rects['t'].center()
        painter.drawLine(top_handle_center, self._rotate_handle_rect.center())
        painter.drawLine(self._rotate_handle_rect.center(), self._delete_btn_rect.center())

        # Draw rotate handle
        painter.setPen(QPen(self.outline_color, 1)); painter.setBrush(self.handle_fill_color)
        painter.drawRect(self._rotate_handle_rect)
        # Draw rotate icon (circular arrow)
        icon_rect = self._rotate_handle_rect.adjusted(4, 4, -4, -4)
        path = QPainterPath()
        path.moveTo(icon_rect.right(), icon_rect.center().y())
        path.arcTo(icon_rect, 0, 270)
        p1 = path.currentPosition(); arrow_size = 3
        path.moveTo(p1); path.lineTo(p1.x() - arrow_size, p1.y() - arrow_size)
        path.moveTo(p1); path.lineTo(p1.x() + arrow_size, p1.y() - arrow_size)
        painter.setBrush(Qt.NoBrush); painter.setPen(QPen(self.outline_color, 1.5))
        painter.drawPath(path)

        # Draw delete button
        painter.setBrush(QBrush(Qt.red)); painter.setPen(QPen(Qt.black, 1))
        painter.drawEllipse(self._delete_btn_rect)
        painter.setPen(QPen(Qt.white, 2))
        margin = 6
        x_rect = self._delete_btn_rect.adjusted(margin, margin, -margin, -margin)
        painter.drawLine(x_rect.topLeft(), x_rect.bottomRight())
        painter.drawLine(x_rect.topRight(), x_rect.bottomLeft())

    def _get_handle_at(self, pos):
        if self._delete_btn_rect.contains(pos): return 'delete'
        if self._rotate_handle_rect.contains(pos): return 'rotate'
        for name, rect in self._handle_rects.items():
            if rect.contains(pos): return name
        return None

    def hoverMoveEvent(self, event):
        handle = self._get_handle_at(event.pos())
        cursor = self.parent_item.cursor()
        if handle:
            cursors = {'tl': Qt.SizeFDiagCursor, 'br': Qt.SizeFDiagCursor, 'tr': Qt.SizeBDiagCursor, 
                         'bl': Qt.SizeBDiagCursor, 't': Qt.SizeVerCursor, 'b': Qt.SizeVerCursor,
                         'l': Qt.SizeHorCursor, 'r': Qt.SizeHorCursor, 'delete': Qt.PointingHandCursor,
                         'rotate': Qt.CrossCursor}
            cursor = cursors.get(handle)
        self.setCursor(cursor)
        super().hoverMoveEvent(event)

    def mousePressEvent(self, event):
        self.active_handle = self._get_handle_at(event.pos())
        if not self.active_handle:
            event.ignore()
            return

        # --- FIX: Check for free transform mode (Ctrl key pressed on a CORNER resize handle) ---
        self.is_free_transform = (event.modifiers() & Qt.ControlModifier) and \
                                 self.active_handle in ['tl', 'tr', 'bl', 'br']

        if self.active_handle == 'delete':
            self.parent_item.request_delete()
        elif self.active_handle == 'rotate':
            self.drag_start_pos = event.scenePos()
            self.drag_start_angle = self.parent_item.rotation()
            self.drag_start_center = self.parent_item.mapToScene(self.parent_item.transformOriginPoint())
        elif self.is_free_transform:
            # --- FREE TRANSFORM START ---
            p_rect = self.parent_item.rect()
            # Store the current visual corners of the item in scene coordinates
            self.initial_scene_quad = q = [
                self.parent_item.mapToScene(p_rect.topLeft()),
                self.parent_item.mapToScene(p_rect.topRight()),
                self.parent_item.mapToScene(p_rect.bottomRight()),
                self.parent_item.mapToScene(p_rect.bottomLeft())
            ]
            
            # To make the handle follow the cursor, we must calculate the delta
            # relative to the handle's logical anchor point, not the arbitrary mouse click point.
            # By setting drag_start_pos to the anchor, the delta in mouseMoveEvent
            # becomes (current_mouse - anchor). When this delta is added to the initial
            # corner positions, it effectively makes the anchor point follow the cursor.
            handle = self.active_handle
            anchor_point = QPointF()
            if   handle == 'tl': anchor_point = q[0]
            elif handle == 'tr': anchor_point = q[1]
            elif handle == 'br': anchor_point = q[2]
            elif handle == 'bl': anchor_point = q[3]
            self.drag_start_pos = anchor_point

        else:  # --- REGULAR RESIZE START ---
            self.drag_start_pos = event.scenePos()
            self.drag_start_rect = self.parent_item.sceneBoundingRect()
        
        event.accept()

    def mouseMoveEvent(self, event):
        if self.is_free_transform and self.active_handle:
            # --- FREE TRANSFORM LOGIC ---
            delta = event.scenePos() - self.drag_start_pos
            new_scene_quad_pts = list(self.initial_scene_quad)
            handle = self.active_handle

            # Move the corner(s) corresponding to the dragged handle.
            # Quad points order: 0:tl, 1:tr, 2:br, 3:bl
            if handle == 'tl': new_scene_quad_pts[0] += delta
            elif handle == 'tr': new_scene_quad_pts[1] += delta
            elif handle == 'br': new_scene_quad_pts[2] += delta
            elif handle == 'bl': new_scene_quad_pts[3] += delta
            # --- The code for side handles below is now unreachable, ---
            # --- but is kept for completeness/future reference.      ---
            elif handle == 't':
                new_scene_quad_pts[0] += delta; new_scene_quad_pts[1] += delta
            elif handle == 'b':
                new_scene_quad_pts[2] += delta; new_scene_quad_pts[3] += delta
            elif handle == 'l':
                new_scene_quad_pts[0] += delta; new_scene_quad_pts[3] += delta
            elif handle == 'r':
                new_scene_quad_pts[1] += delta; new_scene_quad_pts[2] += delta
            
            # Source quad is the item's local, untransformed rectangle
            parent_rect = self.parent_item.rect()
            source_poly = QPolygonF([parent_rect.topLeft(), parent_rect.topRight(), parent_rect.bottomRight(), parent_rect.bottomLeft()])
            
            # Target quad is the new set of corners in scene coordinates
            target_poly = QPolygonF(new_scene_quad_pts)
            
            # To avoid compounding transforms, reset pos/rotation and control geometry with a single QTransform.
            self.parent_item.prepareGeometryChange()
            self.parent_item.setPos(0, 0)
            self.parent_item.setRotation(0)

            transform = QTransform()
            ok = QTransform.quadToQuad(source_poly, target_poly, transform)
            
            if ok:
                self.parent_item.setTransform(transform)
            event.accept()
        
        elif self.active_handle == 'rotate':
            start_line = QLineF(self.drag_start_center, self.drag_start_pos)
            current_line = QLineF(self.drag_start_center, event.scenePos())
            angle_delta = start_line.angleTo(current_line)
            self.parent_item.setRotation(self.drag_start_angle - angle_delta)
            event.accept()

        elif self.active_handle and self.active_handle != 'delete':
            # --- REGULAR RESIZE LOGIC ---
            # Reset any free-transform to ensure a predictable rectangular result.
            self.parent_item.setTransform(QTransform())
            
            delta = event.scenePos() - self.drag_start_pos
            new_rect = QRectF(self.drag_start_rect)

            if self.active_handle == 'tl': new_rect.setTopLeft(new_rect.topLeft() + delta)
            elif self.active_handle == 'tr': new_rect.setTopRight(new_rect.topRight() + delta)
            elif self.active_handle == 'bl': new_rect.setBottomLeft(new_rect.bottomLeft() + delta)
            elif self.active_handle == 'br': new_rect.setBottomRight(new_rect.bottomRight() + delta)
            elif self.active_handle == 't': new_rect.setTop(new_rect.top() + delta.y())
            elif self.active_handle == 'b': new_rect.setBottom(new_rect.bottom() + delta.y())
            elif self.active_handle == 'l': new_rect.setLeft(new_rect.left() + delta.x())
            elif self.active_handle == 'r': new_rect.setRight(new_rect.right() + delta.x())

            min_w, min_h = self.parent_item.min_width, self.parent_item.min_height
            if new_rect.width() < min_w:
                if self.active_handle in ['tl', 'bl', 'l']: new_rect.setLeft(new_rect.right() - min_w)
                else: new_rect.setWidth(min_w)
            if new_rect.height() < min_h:
                if self.active_handle in ['tl', 'tr', 't']: new_rect.setTop(new_rect.bottom() - min_h)
                else: new_rect.setHeight(min_h)
            
            self.parent_item.prepareGeometryChange()
            self.parent_item.setPos(new_rect.topLeft())
            self.parent_item.setRect(QRectF(0, 0, new_rect.width(), new_rect.height()))
            event.accept()
        else:
            event.ignore()

    def mouseReleaseEvent(self, event):
        self.active_handle = None
        self.drag_start_pos = None
        self.drag_start_rect = None
        self.drag_start_angle = 0
        self.drag_start_center = None
        self.is_free_transform = False
        self.initial_scene_quad = None
        self.setCursor(self.parent_item.cursor())
        event.accept()
