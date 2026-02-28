# --- START OF FILE textbox_frame.py ---

from PySide6.QtWidgets import QGraphicsTextItem, QGraphicsItem, QGraphicsRectItem, QToolTip
from PySide6.QtCore import Qt, Signal, QRectF, QPointF, QObject, QLineF
from PySide6.QtGui import QPainter, QFont, QBrush, QColor, QPen, QPainterPath, QLinearGradient, QTransform, QPolygonF

# --- NEW: Custom Item for Selection and Resize Handles ---
class SelectionFrameItem(QGraphicsItem):
    """
    A custom QGraphicsItem that provides a selection, resize, and rotation frame.
    It draws an outline, resize handles, a rotation handle, and a delete button.
    It also supports free transform (perspective distortion) when dragging handles with Ctrl key.
    This item is intended to be a child of the item it frames (e.g., TextBoxItem).
    """

    def __init__(self, parent_item=None):
        super().__init__()
        self.parent_item = parent_item
        self.setZValue(10000)
        self.setAcceptHoverEvents(True)

        # --- Configuration ---
        self.handle_size = 10
        self.outline_color = QColor(0, 120, 215)  # Standard blue selection color
        self.handle_fill_color = QColor(255, 255, 255)
        self.delete_btn_size = 32
        self.edit_btn_size = 32
        self.rotate_btn_size = 20
        self.control_offset = 15  # How far above the top edge the controls are
        self.toolbar_offset = 8  # How far below the bottom edge the toolbar is
        
        # Toolbar styling
        self.toolbar_bg_color = QColor(45, 45, 45)  # Dark background
        self.toolbar_hover_color = QColor(70, 70, 70)  # Hover state
        self.edit_btn_color = QColor(45, 45, 45)  # Same as toolbar background
        self.edit_btn_hover = QColor(100, 100, 100)  # Hover state
        self.delete_btn_color = QColor(45, 45, 45)  # Same as toolbar background
        self.delete_btn_hover = QColor(100, 100, 100)  # Hover state

        # --- State ---
        self.active_handle = None
        self.is_dragging = False
        self.drag_start_pos = None
        self.drag_start_rect = None
        self.drag_start_angle = 0
        self.drag_start_center = QPointF()
        self.is_free_transform = False  # True if Ctrl is held during a resize drag
        self.initial_scene_quad = None  # Stores the item's corner positions for free transform
        self._hover_on_edit = False
        self._hover_on_delete = False
        self._current_hover_handle = None  # Tracks what we are currently hovering over for tooltips

        # --- Caches for hit testing ---
        self._handle_rects = {}
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

    def _get_toolbar_local_center(self, scale):
        """Calculates the fixed bottom-center position relative to the screen, mapped to local coordinates."""
        visual_toolbar_offset = self.toolbar_offset / scale
        visual_edit_size = self.edit_btn_size / scale

        parent_rect = self.parent_item.rect()
        
        # Fallback to local coordinate logic if the item is not yet attached to a scene
        if not self.scene():
            return QPointF(parent_rect.center().x(), parent_rect.bottom() + visual_toolbar_offset + visual_edit_size / 2)

        # Map the 4 corners of the textbox to scene coordinates to find the true visual bounding box (AABB)
        scene_poly = self.parent_item.mapToScene(parent_rect)
        scene_bounds = scene_poly.boundingRect()

        # The toolbar should sit at the absolute bottom of this visual bounding box
        toolbar_scene_x = scene_bounds.center().x()
        toolbar_scene_y = scene_bounds.bottom() + visual_toolbar_offset + visual_edit_size / 2

        # Map this scene coordinate back to local coordinates for drawing
        return self.parent_item.mapFromScene(QPointF(toolbar_scene_x, toolbar_scene_y))

    def boundingRect(self):
        """The bounding rect must include the parent, handles, and control buttons."""
        visual_handle_rects, visual_rotate_handle_rect, visual_edit_btn_poly, visual_delete_btn_poly, visual_toolbar_poly = self._get_visual_rects()
        rect = self.parent_item.rect()
        for handle_rect in visual_handle_rects.values():
            rect = rect.united(handle_rect)
            
        rect = rect.united(visual_rotate_handle_rect)
        
        # Include toolbar area (polygons correctly map counter-rotated bounding rects)
        rect = rect.united(visual_toolbar_poly.boundingRect())
        
        return rect

    def paint(self, painter, option, widget=None):
        self._update_geometry()  # Ensure positions are fresh for painting
        painter.setRenderHint(QPainter.Antialiasing)

        scale = self._get_view_scale()
        visual_handle_size = self.handle_size / scale
        visual_hs_half = visual_handle_size / 2.0
        visual_delete_size = self.delete_btn_size / scale
        visual_edit_size = self.edit_btn_size / scale
        visual_rotate_size = self.rotate_btn_size / scale
        visual_control_offset = self.control_offset / scale
        
        # 1. Draw main outline
        outline_pen = QPen(self.outline_color, 1.5 / scale, Qt.SolidLine)
        painter.setPen(outline_pen)
        painter.setBrush(Qt.NoBrush)
        painter.drawRect(self.parent_item.rect())

        # 2. Draw 8 resize handles (unfilled rectangles)
        handle_pen = QPen(self.outline_color, 1 / scale)
        painter.setPen(handle_pen)
        painter.setBrush(self.handle_fill_color)
        for name, rect in self._handle_rects.items():
            visual_rect = QRectF(
                rect.center().x() - visual_hs_half,
                rect.center().y() - visual_hs_half,
                visual_handle_size,
                visual_handle_size
            )
            painter.drawRect(visual_rect)

        # 3. Draw center '+' indicator
        center = self.parent_item.rect().center()
        cross_size = 5 / scale
        painter.setPen(outline_pen)
        painter.drawLine(QPointF(center.x() - cross_size, center.y()), QPointF(center.x() + cross_size, center.y()))
        painter.drawLine(QPointF(center.x(), center.y() - cross_size), QPointF(center.x(), center.y() + cross_size))

        # 4. Draw rotate handle and its connecting line
        top_handle_center = self._handle_rects['t'].center()
        rotate_btn_center_y = top_handle_center.y() - visual_control_offset - visual_rotate_size / 2
        rotate_btn_center = QPointF(top_handle_center.x(), rotate_btn_center_y)
        rotate_handle_rect = QRectF(0, 0, visual_rotate_size, visual_rotate_size)
        rotate_handle_rect.moveCenter(rotate_btn_center)

        painter.drawLine(top_handle_center, rotate_handle_rect.center())

        # Draw rotate handle
        painter.setPen(QPen(self.outline_color, 1 / scale)); painter.setBrush(self.handle_fill_color)
        painter.drawRect(rotate_handle_rect)
        # Draw rotate icon (circular arrow)
        icon_rect = rotate_handle_rect.adjusted(4 / scale, 4 / scale, -4 / scale, -4 / scale)
        path = QPainterPath()
        path.moveTo(icon_rect.right(), icon_rect.center().y())
        path.arcTo(icon_rect, 0, 270)
        p1 = path.currentPosition(); arrow_size = 3 / scale
        path.moveTo(p1); path.lineTo(p1.x() - arrow_size, p1.y() - arrow_size)
        path.moveTo(p1); path.lineTo(p1.x() + arrow_size, p1.y() - arrow_size)
        painter.setBrush(Qt.NoBrush); painter.setPen(QPen(self.outline_color, 1.5 / scale))
        painter.drawPath(path)

        # 5. Draw toolbar (edit and delete buttons fixed below the textbox visual bounding box)
        local_toolbar_center = self._get_toolbar_local_center(scale)
        
        # --- FIXED POSITION AND COUNTER-ROTATION LOGIC ---
        # Draw the toolbar entirely upright by translating to its calculated fixed scene pos 
        # and reversing the parent's rotation.
        painter.save()
        painter.translate(local_toolbar_center)
        painter.rotate(-self.parent_item.rotation())
        
        # Calculate button positions relative to center of the upright toolbar
        button_spacing = 0 / scale
        edit_btn_center_x = - visual_edit_size / 2 - button_spacing / 2
        delete_btn_center_x = visual_delete_size / 2 + button_spacing / 2
        
        edit_btn_rect = QRectF(0, 0, visual_edit_size, visual_edit_size)
        edit_btn_rect.moveCenter(QPointF(edit_btn_center_x, 0))
        
        delete_btn_rect = QRectF(0, 0, visual_delete_size, visual_delete_size)
        delete_btn_rect.moveCenter(QPointF(delete_btn_center_x, 0))

        # Draw toolbar background (dark rounded rectangle)
        toolbar_padding = 0 / scale
        toolbar_width = visual_edit_size + visual_delete_size + button_spacing + toolbar_padding * 2
        toolbar_height = max(visual_edit_size, visual_delete_size) + toolbar_padding * 2
        toolbar_rect = QRectF(
            - toolbar_width / 2,
            - toolbar_height / 2,
            toolbar_width,
            toolbar_height
        )
        
        toolbar_path = QPainterPath()
        radius = 6 / scale
        toolbar_path.addRoundedRect(toolbar_rect, radius, radius)
        painter.setPen(Qt.NoPen)
        painter.setBrush(QBrush(self.toolbar_bg_color))
        painter.drawPath(toolbar_path)

        # --- Draw Edit Button (Actual Pencil Icon) ---
        edit_btn_color = self.edit_btn_hover if self._hover_on_edit else self.edit_btn_color
        painter.setPen(Qt.NoPen)
        painter.setBrush(QBrush(edit_btn_color))
        painter.drawRoundedRect(edit_btn_rect, 4 / scale, 4 / scale)
        
        painter.save()
        painter.translate(edit_btn_rect.center())
        painter.rotate(45)  # Tilt pencil so it points bottom-left
        icon_pen = QPen(Qt.white, 1.5 / scale, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
        painter.setPen(icon_pen)
        painter.setBrush(Qt.NoBrush)
        
        # Pencil body
        painter.drawRect(QRectF(-3 / scale, -7 / scale, 6 / scale, 10 / scale))
        # Eraser separation line
        painter.drawLine(QPointF(-3 / scale, -3 / scale), QPointF(3 / scale, -3 / scale))
        
        # Pencil tip shape
        tip_path = QPainterPath()
        tip_path.moveTo(-3 / scale, 3 / scale)
        tip_path.lineTo(0, 8 / scale)
        tip_path.lineTo(3 / scale, 3 / scale)
        painter.drawPath(tip_path)
        
        # Pencil lead (filled tip)
        painter.setBrush(Qt.white)
        painter.setPen(Qt.NoPen)
        lead_path = QPainterPath()
        lead_path.moveTo(-1 / scale, 6.3 / scale)
        lead_path.lineTo(0, 8 / scale)
        lead_path.lineTo(1 / scale, 6.3 / scale)
        lead_path.closeSubpath()
        painter.drawPath(lead_path)
        painter.restore()

        # --- Draw Delete Button (Trash Bin Icon) ---
        delete_btn_color = self.delete_btn_hover if self._hover_on_delete else self.delete_btn_color
        painter.setPen(Qt.NoPen)
        painter.setBrush(QBrush(delete_btn_color))
        painter.drawRoundedRect(delete_btn_rect, 4 / scale, 4 / scale)
        
        painter.save()
        painter.translate(delete_btn_rect.center())
        icon_pen = QPen(Qt.white, 1.5 / scale, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
        painter.setPen(icon_pen)
        painter.setBrush(Qt.NoBrush)
        
        # Lid handle
        painter.drawRect(QRectF(-2 / scale, -7 / scale, 4 / scale, 2 / scale))
        # Lid top
        painter.drawLine(QPointF(-7 / scale, -5 / scale), QPointF(7 / scale, -5 / scale))
        
        # Bin body
        bin_path = QPainterPath()
        bin_path.moveTo(-5.5 / scale, -3.5 / scale)
        bin_path.lineTo(5.5 / scale, -3.5 / scale)
        bin_path.lineTo(4 / scale, 6 / scale)
        bin_path.lineTo(-4 / scale, 6 / scale)
        bin_path.closeSubpath()
        painter.drawPath(bin_path)
        
        # Vertical ribs inside the trash bin
        painter.drawLine(QPointF(-2 / scale, -1 / scale), QPointF(-1.5 / scale, 4 / scale))
        painter.drawLine(QPointF(2 / scale, -1 / scale), QPointF(1.5 / scale, 4 / scale))
        painter.drawLine(QPointF(0, -1 / scale), QPointF(0, 4 / scale))
        painter.restore()

        # Restore from counter-rotation
        painter.restore()

    def _get_view_scale(self):
        """Returns the scale factor from the graphics view, or 1.0 if no view."""
        if not self.scene():
            return 1.0
        views = self.scene().views()
        if not views:
            return 1.0
        return views[0].transform().m11()

    def _get_visual_rects(self):
        """Returns scaled versions of handle/control rects/polygons for hit testing and cursor display."""
        scale = self._get_view_scale()

        visual_handle_size = self.handle_size / scale
        visual_hs_half = visual_handle_size / 2.0
        visual_delete_size = self.delete_btn_size / scale
        visual_edit_size = self.edit_btn_size / scale
        visual_rotate_size = self.rotate_btn_size / scale
        visual_control_offset = self.control_offset / scale

        parent_rect = self.parent_item.rect()
        visual_handle_rects = {
            'tl': QRectF(parent_rect.left() - visual_hs_half, parent_rect.top() - visual_hs_half, visual_handle_size, visual_handle_size),
            'tr': QRectF(parent_rect.right() - visual_hs_half, parent_rect.top() - visual_hs_half, visual_handle_size, visual_handle_size),
            'bl': QRectF(parent_rect.left() - visual_hs_half, parent_rect.bottom() - visual_hs_half, visual_handle_size, visual_handle_size),
            'br': QRectF(parent_rect.right() - visual_hs_half, parent_rect.bottom() - visual_hs_half, visual_handle_size, visual_handle_size),
            't': QRectF(parent_rect.center().x() - visual_hs_half, parent_rect.top() - visual_hs_half, visual_handle_size, visual_handle_size),
            'b': QRectF(parent_rect.center().x() - visual_hs_half, parent_rect.bottom() - visual_hs_half, visual_handle_size, visual_handle_size),
            'l': QRectF(parent_rect.left() - visual_hs_half, parent_rect.center().y() - visual_hs_half, visual_handle_size, visual_handle_size),
            'r': QRectF(parent_rect.right() - visual_hs_half, parent_rect.center().y() - visual_hs_half, visual_handle_size, visual_handle_size),
        }

        top_handle_center = visual_handle_rects['t'].center()
        rotate_btn_center_y = top_handle_center.y() - visual_control_offset - visual_rotate_size / 2
        rotate_btn_center = QPointF(top_handle_center.x(), rotate_btn_center_y)
        visual_rotate_handle_rect = QRectF(0, 0, visual_rotate_size, visual_rotate_size)
        visual_rotate_handle_rect.moveCenter(rotate_btn_center)

        # Calculate toolbar component polygons incorporating fixed position and unrotated transformation
        local_toolbar_center = self._get_toolbar_local_center(scale)
        
        button_spacing = 0 / scale
        edit_btn_center_x = - visual_edit_size / 2 - button_spacing / 2
        delete_btn_center_x = visual_delete_size / 2 + button_spacing / 2
        
        unrotated_edit_btn_rect = QRectF(0, 0, visual_edit_size, visual_edit_size)
        unrotated_edit_btn_rect.moveCenter(QPointF(edit_btn_center_x, 0))
        
        unrotated_delete_btn_rect = QRectF(0, 0, visual_delete_size, visual_delete_size)
        unrotated_delete_btn_rect.moveCenter(QPointF(delete_btn_center_x, 0))

        toolbar_padding = 0 / scale
        toolbar_width = visual_edit_size + visual_delete_size + button_spacing + toolbar_padding * 2
        toolbar_height = max(visual_edit_size, visual_delete_size) + toolbar_padding * 2
        unrotated_toolbar_rect = QRectF(-toolbar_width / 2, -toolbar_height / 2, toolbar_width, toolbar_height)

        # Transform to position and un-rotate the rects so hitboxes accurately reflect their visual location
        t = QTransform()
        t.translate(local_toolbar_center.x(), local_toolbar_center.y())
        t.rotate(-self.parent_item.rotation())

        visual_edit_btn_poly = t.map(QPolygonF(unrotated_edit_btn_rect))
        visual_delete_btn_poly = t.map(QPolygonF(unrotated_delete_btn_rect))
        visual_toolbar_poly = t.map(QPolygonF(unrotated_toolbar_rect))

        return visual_handle_rects, visual_rotate_handle_rect, visual_edit_btn_poly, visual_delete_btn_poly, visual_toolbar_poly

    def _get_handle_at(self, pos):
        visual_handle_rects, visual_rotate_handle_rect, visual_edit_btn_poly, visual_delete_btn_poly, visual_toolbar_poly = self._get_visual_rects()
        
        if visual_delete_btn_poly.containsPoint(pos, Qt.OddEvenFill): return 'delete'
        if visual_edit_btn_poly.containsPoint(pos, Qt.OddEvenFill): return 'edit'
        if visual_rotate_handle_rect.contains(pos): return 'rotate'
        for name, rect in visual_handle_rects.items():
            if rect.contains(pos): return name
        return None

    def hoverMoveEvent(self, event):
        handle = self._get_handle_at(event.pos())
        
        # --- MODIFIED: Use native ToolTip property! Only process when state CHANGES. ---
        if handle != self._current_hover_handle:
            self._current_hover_handle = handle
            
            # Update internal state for UI coloring
            self._hover_on_edit = (handle == 'edit')
            self._hover_on_delete = (handle == 'delete')
            
            # Let Qt handle the debounce timer inherently via setToolTip!
            if handle == 'edit':
                self.setToolTip("Edit Text")
            elif handle == 'delete':
                self.setToolTip("Delete Textbox")
            elif handle == 'rotate':
                self.setToolTip("Rotate")
            else:
                self.setToolTip("") # Clears the native tooltip
            
            # Cursor Handling
            cursor = self.parent_item.cursor()
            if handle:
                if handle == 'rotate' and self.is_dragging:
                    cursor = Qt.ClosedHandCursor
                else:
                    cursors = {
                        'tl': Qt.SizeFDiagCursor, 'br': Qt.SizeFDiagCursor, 
                        'tr': Qt.SizeBDiagCursor, 'bl': Qt.SizeBDiagCursor, 
                        't': Qt.SizeVerCursor, 'b': Qt.SizeVerCursor,
                        'l': Qt.SizeHorCursor, 'r': Qt.SizeHorCursor, 
                        'delete': Qt.PointingHandCursor, 'edit': Qt.PointingHandCursor, 
                        'rotate': Qt.OpenHandCursor
                    }
                    cursor = cursors.get(handle, cursor)
            self.setCursor(cursor)
            
            # ONLY call update() when the visual state actually changes to avoid repaints interrupting native tooltips.
            self.update() 
        # ------------------------------------------------------------------------------

        super().hoverMoveEvent(event)

    def hoverLeaveEvent(self, event):
        # Reset state safely when the mouse leaves the frame entirely
        if self._current_hover_handle is not None:
            self._hover_on_edit = False
            self._hover_on_delete = False
            self._current_hover_handle = None
            self.setToolTip("")
            self.update()
            
        super().hoverLeaveEvent(event)

    def mousePressEvent(self, event):
        self.setToolTip("") # Hide tooltip once we initiate an interaction

        self.active_handle = self._get_handle_at(event.pos())
        if not self.active_handle:
            event.ignore()
            return

        if self.active_handle == 'edit':
            self.parent_item.enable_editing()
            event.accept()
            return

        self.is_dragging = True
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

            if handle == 'tl': new_scene_quad_pts[0] += delta
            elif handle == 'tr': new_scene_quad_pts[1] += delta
            elif handle == 'br': new_scene_quad_pts[2] += delta
            elif handle == 'bl': new_scene_quad_pts[3] += delta
            elif handle == 't':
                new_scene_quad_pts[0] += delta; new_scene_quad_pts[1] += delta
            elif handle == 'b':
                new_scene_quad_pts[2] += delta; new_scene_quad_pts[3] += delta
            elif handle == 'l':
                new_scene_quad_pts[0] += delta; new_scene_quad_pts[3] += delta
            elif handle == 'r':
                new_scene_quad_pts[1] += delta; new_scene_quad_pts[2] += delta
            
            parent_rect = self.parent_item.rect()
            source_poly = QPolygonF([parent_rect.topLeft(), parent_rect.topRight(), parent_rect.bottomRight(), parent_rect.bottomLeft()])
            target_poly = QPolygonF(new_scene_quad_pts)
            
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
        self.is_dragging = False
        self.drag_start_pos = None
        self.drag_start_rect = None
        self.drag_start_angle = 0
        self.drag_start_center = None
        self.is_free_transform = False
        self.initial_scene_quad = None
        
        # After completing action, evaluate what the mouse is hovering over
        handle = self._get_handle_at(event.pos())
        self._hover_on_edit = (handle == 'edit')
        self._hover_on_delete = (handle == 'delete')
        
        if handle != self._current_hover_handle:
            self._current_hover_handle = handle
            self.setToolTip("") # Force reset ToolTip on drop
        
        self.update()
        self.setCursor(self.parent_item.cursor())
        event.accept()