# app/ui/components/image_area/label.py

from PySide6.QtWidgets import QGraphicsScene, QSizePolicy, QGraphicsRectItem, QGraphicsView, QRubberBand, QGraphicsLineItem, QGraphicsEllipseItem, QGraphicsPathItem
from PySide6.QtCore import Qt, Signal, QRectF, QPoint, QRect, QSize, QTimer
from PySide6.QtGui import QPainter, QColor, QPen, QBrush, QPixmap, QPainterPath
from app.ui.components.image_area.textbox import TextBoxItem

class ResizableImageLabel(QGraphicsView):
    # Signals
    textBoxDeleted = Signal(object)
    textBoxSelected = Signal(object, object, bool)
    manual_area_selected = Signal(QRectF, object)
    stitching_selection_changed = Signal(object, bool)
    # NEW: Signal to report a click's location during split mode
    split_indicator_requested = Signal(object, int)


    # --- MODIFICATION START ---
    def __init__(self, pixmap, filename, main_window):
        """
        The constructor now accepts a reference to the main_window to avoid
        fragile parent traversal.
        """
        super().__init__()
        self.main_window = main_window # Store the reference
        # --- MODIFICATION END ---
        self.setScene(QGraphicsScene())
        self.setRenderHints(QPainter.Antialiasing | QPainter.SmoothPixmapTransform)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setAlignment(Qt.AlignTop | Qt.AlignLeft)
        self.original_pixmap = pixmap
        self.current_pixmap = pixmap
        self.filename = filename
        self.pixmap_item = self.scene().addPixmap(self.current_pixmap)
        self.scene().setSceneRect(0, 0, self.original_pixmap.width(), self.original_pixmap.height())
        self.setInteractive(True)
        self.text_boxes = []
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        self.original_text_entries = {}
        # --- NEW: Store persistent selection visuals ---
        self.selection_visuals = []

        self._is_manual_select_active = False
        self._is_selection_active = False
        self._rubber_band = None
        self._rubber_band_origin = QPoint()

        self.setCursor(Qt.ArrowCursor)

        self._is_stitching_mode_active = False
        self._is_selected_for_stitching = False

        self.selection_overlay = QGraphicsRectItem()
        self.selection_overlay.setBrush(QColor(70, 130, 180, 100)) # SteelBlue, semi-transparent
        self.selection_overlay.setPen(QPen(Qt.NoPen))
        self.selection_overlay.setZValue(1000)
        self.selection_overlay.hide()
        self.scene().addItem(self.selection_overlay)
        
        # Splitting state management
        self._is_split_selection_active = False
        self._is_selected_for_splitting = False
        self.split_visuals = [] # List of dicts: [{'line': item, 'handle': item}]
        self._is_dragging_split_line = False
        self._dragged_item = None # The specific visual dict being dragged

    def apply_translation(self, main_window, text_entries_by_row, default_style):
        """
        Applies text and styles to the image.
        It is now profile-aware by using main_window.get_display_text().
        """
        processed_default_style = self._ensure_gradient_defaults_for_ril(default_style)
        current_entries = {rn: entry for rn, entry in text_entries_by_row.items()
                           if not entry.get('is_deleted', False)}
        existing_boxes = {tb.row_number: tb for tb in self.text_boxes}
        rows_to_remove_from_list = []

        # Update or remove existing text boxes
        for row_number, text_box in list(existing_boxes.items()):
            if row_number not in current_entries:
                text_box.cleanup()
                rows_to_remove_from_list.append(row_number)
            else:
                entry = current_entries[row_number]
                display_text = main_window.get_display_text(entry)
                combined_style = self._combine_styles(processed_default_style, entry.get('custom_style', {}))
                
                text_box.text_item.setPlainText(display_text)
                text_box.apply_styles(combined_style)

        self.text_boxes = [tb for tb in self.text_boxes if tb.row_number not in rows_to_remove_from_list]

        # Add new text boxes
        existing_rows_after_removal = {tb.row_number for tb in self.text_boxes}
        for row_number, entry in current_entries.items():
            if row_number not in existing_rows_after_removal:
                coords = entry.get('coordinates') or entry.get('bbox')
                if not coords: continue
                try:
                    x = min(p[0] for p in coords); y = min(p[1] for p in coords)
                    width = max(p[0] for p in coords) - x; height = max(p[1] for p in coords) - y
                    if width <= 0 or height <= 0: continue
                except Exception as e:
                    print(f"Error processing coords for new row {row_number}: {coords} -> {e}")
                    continue
                
                display_text = main_window.get_display_text(entry)
                combined_style = self._combine_styles(processed_default_style, entry.get('custom_style', {}))
                
                text_box = TextBoxItem (QRectF(x, y, width, height),
                                         row_number,
                                         display_text,
                                         initial_style=combined_style)

                text_box.signals.rowDeleted.connect(self.handle_text_box_deleted)
                text_box.signals.selectedChanged.connect(self.on_text_box_selected)
                self.scene().addItem(text_box)
                self.text_boxes.append(text_box)

        QTimer.singleShot(0, self.update_view_transform)
    
    # --- MODIFICATION: This method now handles complex QPainterPath objects ---
    def draw_selections(self, paths):
        """Draws multiple persistent selection shapes (paths) on the scene."""
        self.clear_selection_visuals()
        selection_brush = QBrush(QColor(255, 80, 80, 120)) # Semi-transparent red
        selection_pen = QPen(QColor(255, 0, 0), 1)
        for path in paths:
            # Use QGraphicsPathItem to render the complex shape
            item = QGraphicsPathItem(path)
            item.setBrush(selection_brush)
            item.setPen(selection_pen)
            item.setZValue(1100) # Ensure it's above selection overlay
            self.scene().addItem(item)
            self.selection_visuals.append(item)

    def clear_selection_visuals(self):
        """Removes all persistent selection rectangles from the scene."""
        for item in self.selection_visuals:
            self.scene().removeItem(item)
        self.selection_visuals.clear()
        
    def set_text_visibility(self, visible):
        """Sets the visibility of all text boxes on this label."""
        for text_box in self.text_boxes:
            text_box.setVisible(visible)

    def enable_stitching_selection(self, enabled):
        """Activates or deactivates the click-to-select mode for stitching."""
        self._is_stitching_mode_active = enabled
        if enabled:
            self.setCursor(Qt.PointingHandCursor)
        else:
            self._set_selected_for_stitching(False)
            if not self._is_split_selection_active:
                self.setCursor(Qt.ArrowCursor)

    def _set_selected_for_stitching(self, selected):
        """Internal helper to manage the selection state and visuals."""
        if self._is_selected_for_stitching == selected: return
        self._is_selected_for_stitching = selected
        if self._is_selected_for_stitching:
            self.selection_overlay.setRect(self.scene().sceneRect())
            self.selection_overlay.show()
        else:
            self.selection_overlay.hide()
        self.stitching_selection_changed.emit(self, self._is_selected_for_stitching)

    def enable_splitting_selection(self, enabled):
        """Activates/deactivates split mode for this label."""
        self._is_split_selection_active = enabled
        if enabled:
            self.setCursor(Qt.PointingHandCursor)
        else:
            self.set_selected_for_splitting(False) # Clean up visuals
            if not self._is_stitching_mode_active:
                self.setCursor(Qt.ArrowCursor)

    def set_selected_for_splitting(self, selected):
        """Sets the visual state when selected/deselected as the split target."""
        if self._is_selected_for_splitting == selected: return
        self._is_selected_for_splitting = selected
        
        if self._is_selected_for_splitting:
            # Use a different color overlay to distinguish from stitching
            self.selection_overlay.setBrush(QColor(220, 20, 60, 100)) # Crimson, semi-transparent
            self.selection_overlay.setRect(self.scene().sceneRect())
            self.selection_overlay.show()
            self.setCursor(Qt.CrossCursor) # Cursor to indicate adding points
        else:
            self.selection_overlay.hide()
            self.draw_split_lines([]) # Clear visual lines on deselect
            if self._is_split_selection_active:
                self.setCursor(Qt.PointingHandCursor)
        
    def draw_split_lines(self, y_coords):
        """Draws or clears draggable horizontal lines at given Y coordinates."""
        for visual in self.split_visuals:
            self.scene().removeItem(visual['line'])
            self.scene().removeItem(visual['handle'])
        self.split_visuals.clear()
        
        line_pen = QPen(QColor(0, 120, 215), 3, Qt.SolidLine) # Blue, solid
        handle_pen = QPen(QColor("white"), 1)
        handle_brush = QBrush(QColor(0, 120, 215))
        handle_size = 16
        width = self.original_pixmap.width()
        z_value = 1500

        for y in y_coords:
            line = self.scene().addLine(0, y, width, y, line_pen)
            line.setZValue(z_value)
            
            handle = QGraphicsEllipseItem(QRectF(-handle_size / 2, y - handle_size / 2, handle_size, handle_size))
            handle.setPen(handle_pen)
            handle.setBrush(handle_brush)
            handle.setZValue(z_value + 1)
            handle.setCursor(Qt.SizeVerCursor)
            self.scene().addItem(handle)
            
            self.split_visuals.append({'line': line, 'handle': handle})

    # --- MODIFICATION START ---
    def set_manual_selection_enabled(self, enabled):
        """
        Enables or disables manual selection mode (for OCR or Context Fill) and
        dynamically connects the selection signal to the correct active handler
        using the stored main_window reference.
        """
        self._is_manual_select_active = enabled
        if enabled:
            # Use the direct reference to the main window to find the active handler.
            if self.main_window.manual_ocr_handler.is_active:
                self.manual_area_selected.connect(self.main_window.manual_ocr_handler.handle_area_selected)
            elif self.main_window.context_fill_handler.is_active:
                self.manual_area_selected.connect(self.main_window.context_fill_handler.handle_area_selected)

            if not self._is_selection_active:
                self.setCursor(Qt.CrossCursor)
        else:
            # Disconnect all slots from the signal to prevent dangling connections.
            try:
                self.manual_area_selected.disconnect()
            except (TypeError, RuntimeError):
                pass  # It's okay if it was already disconnected.
            
            if not self._is_stitching_mode_active and not self._is_split_selection_active:
                self.setCursor(Qt.ArrowCursor)
    # --- MODIFICATION END ---

    def mousePressEvent(self, event):
        if event.button() != Qt.LeftButton:
            super().mousePressEvent(event)
            return

        # Check for split line drag first if this label is selected for splitting
        if self._is_selected_for_splitting:
            pos_in_scene = self.mapToScene(event.pos())
            item_under_cursor = self.scene().itemAt(pos_in_scene, self.transform())
            
            for visual in self.split_visuals:
                if item_under_cursor is visual['handle']:
                    self._is_dragging_split_line = True
                    self._dragged_item = visual
                    self.setCursor(Qt.SizeVerCursor)
                    event.accept()
                    return

        # Mode 1: Stitching
        if self._is_stitching_mode_active:
            self._set_selected_for_stitching(not self._is_selected_for_stitching)
            event.accept(); return

        # Mode 2: Splitting - A click simply reports its position to the handler
        if self._is_split_selection_active:
            pos_in_scene = self.mapToScene(event.pos())
            self.split_indicator_requested.emit(self, int(pos_in_scene.y()))
            event.accept(); return

        # Mode 3: Manual OCR Selection
        if self._is_manual_select_active and not self._is_selection_active:
            self._rubber_band_origin = event.pos()
            if not self._rubber_band:
                self._rubber_band = QRubberBand(QRubberBand.Rectangle, self)
            self._rubber_band.setGeometry(QRect(self._rubber_band_origin, QSize()))
            self._rubber_band.show()
            event.accept(); return
        
        super().mousePressEvent(event)

    def mouseReleaseEvent(self, event):
        # Finalize split line dragging
        if self._is_dragging_split_line and event.button() == Qt.LeftButton:
            self._is_dragging_split_line = False
            self._dragged_item = None
            if self._is_selected_for_splitting:
                self.setCursor(Qt.CrossCursor)
            event.accept()
            return

        if self._is_manual_select_active and self._rubber_band and event.button() == Qt.LeftButton and not self._rubber_band_origin.isNull():
            final_rect_viewport = self._rubber_band.geometry()
            self._rubber_band_origin = QPoint()
            if final_rect_viewport.width() > 4 and final_rect_viewport.height() > 4:
                rect_scene = self.mapToScene(final_rect_viewport).boundingRect()
                # --- MODIFICATION: The handler now manages selection state.
                # Do not set _is_selection_active or change cursor here.
                self.manual_area_selected.emit(rect_scene, self)
            else:
                 self._rubber_band.hide()
            event.accept()
        else:
            super().mouseReleaseEvent(event)

    def mouseMoveEvent(self, event):
        # Handle split line dragging
        if self._is_dragging_split_line and self._dragged_item:
            new_y = self.mapToScene(event.pos()).y()
            new_y = max(0, min(new_y, self.original_pixmap.height()))
            
            # Update visuals in real-time for smooth feedback
            self._dragged_item['line'].setLine(0, new_y, self.original_pixmap.width(), new_y)
            handle_rect = self._dragged_item['handle'].rect()
            self._dragged_item['handle'].setRect(handle_rect.x(), new_y - handle_rect.height() / 2, handle_rect.width(), handle_rect.height())
            
            # Notify handler of the new position to update the model
            self.split_indicator_requested.emit(self, int(new_y))
            event.accept()
            return

        if self._is_manual_select_active and self._rubber_band and not self._rubber_band_origin.isNull() and (event.buttons() & Qt.LeftButton):
            self._rubber_band.setGeometry(QRect(self._rubber_band_origin, event.pos()).normalized())
            event.accept()
        else:
            super().mouseMoveEvent(event)

    # --- MODIFIED: Renamed for clarity ---
    def clear_rubber_band(self):
        """Hides the temporary rubber band used for drawing a selection."""
        if self._rubber_band: self._rubber_band.hide()
        # Reset cursor if mode is still active
        self.set_manual_selection_enabled(self._is_manual_select_active)

    def hasHeightForWidth(self):
        return True

    def heightForWidth(self, width):
        if self.original_pixmap.isNull() or self.original_pixmap.width() == 0:
            return self.minimumHeight() if self.minimumHeight() > 0 else 50
        aspect_ratio = self.original_pixmap.height() / self.original_pixmap.width()
        return int(aspect_ratio * width)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        QTimer.singleShot(0, self.update_view_transform)
        
    def update_pixmap(self, new_pixmap: QPixmap):
        """Updates the label's pixmap and redraws the view."""
        self.original_pixmap = new_pixmap
        self.current_pixmap = new_pixmap
        
        if self.pixmap_item:
            self.pixmap_item.setPixmap(self.current_pixmap)
        
        self.scene().setSceneRect(0, 0, self.original_pixmap.width(), self.original_pixmap.height())
        
        # Trigger a resize and transform update to reflect the new pixmap
        self.updateGeometry()
        QTimer.singleShot(0, self.update_view_transform)

    def update_view_transform(self):
        if not self.scene() or self.original_pixmap.isNull() or not self.pixmap_item: return
        scene_rect = self.scene().sceneRect()
        if scene_rect.width() == 0 or scene_rect.height() == 0: return
        viewport_width = self.viewport().width()
        scale_factor = viewport_width / scene_rect.width()
        self.resetTransform()
        self.scale(scale_factor, scale_factor)
        self.viewport().update()

    def _ensure_gradient_defaults_for_ril(self, style_dict):
        style = style_dict.copy() if style_dict else {}
        if 'fill_type' not in style: style['fill_type'] = 'solid'
        if 'bg_color' not in style: style['bg_color'] = '#ffffffff'
        if 'bg_gradient' not in style: style['bg_gradient'] = {}
        style['bg_gradient'] = {'midpoint': 50, **style['bg_gradient']}
        if 'text_color_type' not in style: style['text_color_type'] = 'solid'
        if 'text_color' not in style: style['text_color'] = '#ff000000'
        if 'text_gradient' not in style: style['text_gradient'] = {}
        style['text_gradient'] = {'midpoint': 50, **style['text_gradient']}
        if 'midpoint' in style['bg_gradient']: style['bg_gradient']['midpoint'] = int(style['bg_gradient']['midpoint'])
        if 'midpoint' in style['text_gradient']: style['text_gradient']['midpoint'] = int(style['text_gradient']['midpoint'])
        return style

    def _combine_styles(self, default_style, custom_style):
        combined = self._ensure_gradient_defaults_for_ril(default_style)
        if custom_style:
            processed_custom = self._ensure_gradient_defaults_for_ril(custom_style)
            for key, value in processed_custom.items():
                 if key in ['bg_gradient', 'text_gradient'] and isinstance(value, dict):
                     combined[key].update(value)
                     if 'midpoint' in combined[key]: combined[key]['midpoint'] = int(combined[key]['midpoint'])
                 else:
                     combined[key] = value
        return combined

    def on_text_box_selected(self, selected, row_number):
        if selected:
            for tb in self.text_boxes:
                 if tb.row_number != row_number:
                     if tb.isSelected(): tb.setSelected(False)
            self.textBoxSelected.emit(row_number, self, selected)
        else:
            self.textBoxSelected.emit(row_number, self, selected)

    def deselect_all_text_boxes(self):
        for text_box in self.text_boxes:
            if text_box.isSelected(): text_box.setSelected(False)
    
    def select_text_box(self, row_number_to_select):
        """Finds and selects a specific text box, deselecting others."""
        box_to_select = None
        for tb in self.text_boxes:
            if tb.row_number == row_number_to_select:
                box_to_select = tb
                break
        
        if box_to_select:
            for tb in self.text_boxes:
                if tb is not box_to_select and tb.isSelected():
                    tb.setSelected(False)
            
            if not box_to_select.isSelected():
                box_to_select.setSelected(True)
            
            return box_to_select
        return None

    def handle_text_box_deleted(self, row_number):
        self.textBoxDeleted.emit(row_number)

    def remove_text_box_by_row(self, row_number):
        item_to_remove = None
        for tb in self.text_boxes:
            try:
                if tb.row_number == row_number or float(tb.row_number) == float(row_number):
                     item_to_remove = tb
                     break
            except (ValueError, TypeError):
                 if str(tb.row_number) == str(row_number):
                      item_to_remove = tb
                      break
        if item_to_remove:
            item_to_remove.cleanup()
            try:
                index_to_remove = -1
                for i, current_tb in enumerate(self.text_boxes):
                    if current_tb is item_to_remove:
                        index_to_remove = i
                        break
                if index_to_remove != -1:
                    del self.text_boxes[index_to_remove]
            except ValueError: pass
        else: pass

    def cleanup(self):
        try:
            self.textBoxDeleted.disconnect()
            self.textBoxSelected.disconnect()
            self.manual_area_selected.disconnect()
            self.stitching_selection_changed.disconnect()
            self.split_indicator_requested.disconnect()
        except TypeError: pass
        except RuntimeError: pass
        if self.scene():
            for tb in self.text_boxes[:]: tb.cleanup()
            self.text_boxes = []
            # --- MODIFICATION: Cleanup new visual items ---
            self.clear_selection_visuals()
            for visual in self.split_visuals:
                self.scene().removeItem(visual['line'])
                self.scene().removeItem(visual['handle'])
            self.split_visuals = []
            self.scene().clear()
        self.setScene(None)

    def get_text_boxes(self):
        return self.text_boxes