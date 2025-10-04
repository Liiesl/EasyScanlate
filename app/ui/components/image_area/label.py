# app/ui/components/image_area/label.py

from PySide6.QtWidgets import (QGraphicsScene, QSizePolicy, QGraphicsRectItem, QGraphicsView, 
                             QRubberBand, QGraphicsLineItem, QGraphicsEllipseItem, 
                             QGraphicsPathItem, QGraphicsItem)
from PySide6.QtCore import Qt, Signal, QRectF, QPoint, QRect, QSize, QTimer
from PySide6.QtGui import QPainter, QColor, QPen, QBrush
from app.ui.components.image_area.textbox import TextBoxItem

class ResizableImageLabel(QGraphicsView):
    # --- MODIFIED: textBoxSelected is no longer needed ---
    textBoxDeleted = Signal(object)
    manual_area_selected = Signal(QRectF, object)
    stitching_selection_changed = Signal(object, bool)
    split_indicator_requested = Signal(object, int)
    inpaintRecordDeleted = Signal(str)

    # --- MODIFIED: __init__ now accepts a selection_manager ---
    def __init__(self, pixmap, filename, main_window, selection_manager):
        super().__init__()
        self.main_window = main_window 
        self.selection_manager = selection_manager
        # --- NEW: Connect to the selection manager's signal ---
        self.selection_manager.selection_changed.connect(self.on_external_selection_changed)

        self.setScene(QGraphicsScene())
        self.setRenderHints(QPainter.Antialiasing | QPainter.SmoothPixmapTransform)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setAlignment(Qt.AlignTop | Qt.AlignLeft)
        
        self.original_pixmap = pixmap
        
        self.filename = filename
        self.pixmap_item = self.scene().addPixmap(self.original_pixmap)
        self.pixmap_item.setZValue(0) # Base image is at the bottom
        self.scene().setSceneRect(0, 0, self.original_pixmap.width(), self.original_pixmap.height())
        self.setInteractive(True)
        self.text_boxes = []
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        self.original_text_entries = {}
        self.selection_visuals = []

        self.inpaint_records = []
        self.inpaint_visuals = []
        self.inpaint_patch_items = [] 
        self._is_inpaint_edit_mode = False

        self._is_manual_select_active = False
        self._rubber_band = None
        self._rubber_band_origin = QPoint()

        self.setCursor(Qt.ArrowCursor)

        self._is_stitching_mode_active = False
        self._is_selected_for_stitching = False

        self.selection_overlay = QGraphicsRectItem()
        self.selection_overlay.setBrush(QColor(70, 130, 180, 100))
        self.selection_overlay.setPen(QPen(Qt.NoPen))
        self.selection_overlay.setZValue(1000)
        self.selection_overlay.hide()
        self.scene().addItem(self.selection_overlay)
        
        self._is_split_selection_active = False
        self._is_selected_for_splitting = False
        self.split_visuals = [] 
        self._is_dragging_split_line = False
        self._dragged_item = None

    def update_inpaint_data(self, records):
        self.inpaint_records = records
        self._draw_inpaint_borders()

    def _draw_inpaint_borders(self):
        for item in self.inpaint_visuals:
            if item.scene():
                self.scene().removeItem(item)
        self.inpaint_visuals.clear()

        pen = QPen(QColor(255, 165, 0), 2, Qt.DashLine) 
        pen.setCosmetic(True)
        brush = QBrush(QColor(255, 165, 0, 40))

        for record in self.inpaint_records:
            coords = record.get('coordinates')
            if not coords: continue
            
            rect = QRectF(coords[0], coords[1], coords[2], coords[3])
            item = QGraphicsRectItem(rect)
            item.setPen(pen)
            item.setBrush(brush)
            item.setZValue(1) 
            item.setData(0, record.get('id')) 
            
            item.setFlag(QGraphicsItem.ItemIsSelectable, self._is_inpaint_edit_mode)
            item.setVisible(self._is_inpaint_edit_mode)
            
            self.scene().addItem(item)
            self.inpaint_visuals.append(item)

    def set_inpaint_edit_mode(self, enabled):
        """Shows or hides the inpaint patch borders and updates internal state."""
        self._is_inpaint_edit_mode = enabled
        for item in self.inpaint_visuals:
            item.setVisible(enabled)
            item.setFlag(QGraphicsItem.ItemIsSelectable, enabled)

    def set_inpaints_applied(self, applied: bool):
        for item in self.inpaint_patch_items:
            item.setVisible(applied)

    def keyPressEvent(self, event):
        if event.key() in (Qt.Key_Delete, Qt.Key_Backspace):
            selected_items = self.scene().selectedItems()
            if not selected_items:
                super().keyPressEvent(event)
                return

            ids_to_delete = []
            for item in selected_items:
                if item in self.inpaint_visuals:
                    record_id = item.data(0)
                    if record_id:
                        ids_to_delete.append(record_id)
            
            if ids_to_delete:
                for record_id in ids_to_delete:
                    self.inpaintRecordDeleted.emit(record_id)
                event.accept()
                return

        super().keyPressEvent(event)

    def apply_inpaint_patch(self, patch_pixmap: QPixmap, coordinates: QRectF):
        patch_item = self.scene().addPixmap(patch_pixmap)
        patch_item.setPos(coordinates.topLeft())
        # --- Z-Value 1: Above base image, below text boxes ---
        patch_item.setZValue(1)
        self.inpaint_patch_items.append(patch_item)

    def revert_to_original(self):
        for item in self.inpaint_patch_items:
            if item.scene():
                self.scene().removeItem(item)
        self.inpaint_patch_items.clear()

    def apply_translation(self, main_window, text_entries_by_row, default_style):
        processed_default_style = self._ensure_gradient_defaults_for_ril(default_style)
        current_entries = {rn: entry for rn, entry in text_entries_by_row.items()
                           if not entry.get('is_deleted', False)}
        existing_boxes = {tb.row_number: tb for tb in self.text_boxes}
        rows_to_remove_from_list = []

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

                text_box.setZValue(2) # On top of inpaint patches
                text_box.signals.rowDeleted.connect(self.handle_text_box_deleted)
                text_box.signals.selectedChanged.connect(self.on_text_box_selected)
                self.scene().addItem(text_box)
                self.text_boxes.append(text_box)
        QTimer.singleShot(0, self.update_view_transform)
    
    def draw_selections(self, paths_or_rects):
        self.clear_selection_visuals()
        selection_brush = QBrush(QColor(255, 80, 80, 120))
        selection_pen = QPen(QColor(255, 0, 0), 1)
        for item_to_draw in paths_or_rects:
            path = QPainterPath()
            if isinstance(item_to_draw, QRectF):
                path.addRect(item_to_draw)
            elif isinstance(item_to_draw, QPainterPath):
                path = item_to_draw
            else:
                continue

            item = QGraphicsPathItem(path)
            item.setBrush(selection_brush)
            item.setPen(selection_pen)
            item.setZValue(1100)
            self.scene().addItem(item)
            self.selection_visuals.append(item)

    def clear_selection_visuals(self):
        for item in self.selection_visuals:
            if item.scene():
                self.scene().removeItem(item)
        self.selection_visuals.clear()
        
    def set_text_visibility(self, visible):
        for text_box in self.text_boxes:
            text_box.setVisible(visible)

    def enable_stitching_selection(self, enabled):
        self._is_stitching_mode_active = enabled
        if enabled:
            self.setCursor(Qt.PointingHandCursor)
        else:
            self._set_selected_for_stitching(False)
            if not self._is_split_selection_active:
                self.setCursor(Qt.ArrowCursor)

    def _set_selected_for_stitching(self, selected):
        if self._is_selected_for_stitching == selected: return
        self._is_selected_for_stitching = selected
        if self._is_selected_for_stitching:
            self.selection_overlay.setRect(self.scene().sceneRect())
            self.selection_overlay.show()
        else:
            self.selection_overlay.hide()
        self.stitching_selection_changed.emit(self, self._is_selected_for_stitching)

    def enable_splitting_selection(self, enabled):
        self._is_split_selection_active = enabled
        if enabled:
            self.setCursor(Qt.PointingHandCursor)
        else:
            self.set_selected_for_splitting(False)
            if not self._is_stitching_mode_active:
                self.setCursor(Qt.ArrowCursor)

    def set_selected_for_splitting(self, selected):
        if self._is_selected_for_splitting == selected: return
        self._is_selected_for_splitting = selected
        
        if self._is_selected_for_splitting:
            self.selection_overlay.setBrush(QColor(220, 20, 60, 100))
            self.selection_overlay.setRect(self.scene().sceneRect())
            self.selection_overlay.show()
            self.setCursor(Qt.CrossCursor)
        else:
            self.selection_overlay.hide()
            self.draw_split_lines([])
            if self._is_split_selection_active:
                self.setCursor(Qt.PointingHandCursor)
        
    def draw_split_lines(self, y_coords):
        for visual in self.split_visuals:
            if visual['line'].scene(): self.scene().removeItem(visual['line'])
            if visual['handle'].scene(): self.scene().removeItem(visual['handle'])
        self.split_visuals.clear()
        
        line_pen = QPen(QColor(0, 120, 215), 3, Qt.SolidLine)
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

    def set_manual_selection_enabled(self, enabled):
        """
        Enables or disables the visual components for manual area selection.
        """
        self._is_manual_select_active = enabled
        if enabled:
            self.setCursor(Qt.CrossCursor)
        else:
            if not self._is_stitching_mode_active and not self._is_split_selection_active:
                self.setCursor(Qt.ArrowCursor)

    def mousePressEvent(self, event):
        if event.button() != Qt.LeftButton:
            super().mousePressEvent(event)
            return

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

        if self._is_stitching_mode_active:
            self._set_selected_for_stitching(not self._is_selected_for_stitching)
            event.accept(); return

        if self._is_split_selection_active:
            pos_in_scene = self.mapToScene(event.pos())
            self.split_indicator_requested.emit(self, int(pos_in_scene.y()))
            event.accept(); return

        if self._is_manual_select_active:
            self._rubber_band_origin = event.pos()
            if not self._rubber_band:
                self._rubber_band = QRubberBand(QRubberBand.Rectangle, self)
            self._rubber_band.setGeometry(QRect(self._rubber_band_origin, QSize()))
            self._rubber_band.show()
            event.accept(); return
        
        super().mousePressEvent(event)

    def mouseReleaseEvent(self, event):
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
                self.manual_area_selected.emit(rect_scene, self)
            else:
                 self._rubber_band.hide()
                 self._is_selection_active = False
            event.accept()
        else:
            super().mouseReleaseEvent(event)

    def mouseMoveEvent(self, event):
        if self._is_dragging_split_line and self._dragged_item:
            new_y = self.mapToScene(event.pos()).y()
            new_y = max(0, min(new_y, self.original_pixmap.height()))
            
            self._dragged_item['line'].setLine(0, new_y, self.original_pixmap.width(), new_y)
            handle_rect = self._dragged_item['handle'].rect()
            self._dragged_item['handle'].setRect(handle_rect.x(), new_y - handle_rect.height() / 2, handle_rect.width(), handle_rect.height())
            
            self.split_indicator_requested.emit(self, int(new_y))
            event.accept()
            return

        if self._is_manual_select_active and self._rubber_band and not self._rubber_band_origin.isNull() and (event.buttons() & Qt.LeftButton):
            self._rubber_band.setGeometry(QRect(self._rubber_band_origin, event.pos()).normalized())
            event.accept()
        else:
            super().mouseMoveEvent(event)

    def clear_rubber_band(self):
        """Hides the temporary rubber band used for drawing a selection."""
        if self._rubber_band: self._rubber_band.hide()

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
        self._true_original_pixmap = new_pixmap
        self.original_pixmap = new_pixmap.copy()
        
        self._update_displayed_pixmap()
        
        self.scene().setSceneRect(0, 0, self.original_pixmap.width(), self.original_pixmap.height())
        
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

    # --- MODIFIED: This now reports to the selection manager ---
    def on_text_box_selected(self, selected, row_number):
        if selected:
            # Tell the manager about the new selection
            self.selection_manager.select(row_number, self)
            # Locally deselect other boxes on this image
            for tb in self.text_boxes:
                 if tb.row_number != row_number and tb.isSelected():
                     tb.setSelected(False)
        else:
            # If the currently selected box is deselected, clear the global selection
            if self.selection_manager.get_current_selection() == row_number:
                self.selection_manager.deselect(self)
    
    # --- NEW: Slot to handle external selection changes ---
    def on_external_selection_changed(self, row_number, source):
        # Ignore signals from self to prevent loops
        if source is self:
            return

        # If selection is cleared, deselect everything on this image
        if row_number is None:
            self.deselect_all_text_boxes()
            return
            
        # Check if the selected row belongs to this image
        target_result, _ = self.main_window.model._find_result_by_row_number(row_number)
        if target_result and target_result.get('filename') == self.filename:
            selected_item = self.select_text_box(row_number)
            if selected_item:
                self._scroll_to_box(selected_item)
        else:
            # The selection is for a different image, so deselect all boxes here
            self.deselect_all_text_boxes()
    
    # --- NEW: Method to scroll the scroll area to a specific text box ---
    def _scroll_to_box(self, selected_box_item):
        scroll_area = self.main_window.scroll_area
        scroll_viewport = scroll_area.viewport()
        viewport_height = scroll_viewport.height()
        current_scroll_y = scroll_area.verticalScrollBar().value()
        image_label_y_in_scroll = self.y()
        
        box_rect_scene = selected_box_item.sceneBoundingRect()
        scale = self.transform().m11()
        
        box_center_y_in_image = box_rect_scene.center().y() * scale
        box_global_top = image_label_y_in_scroll + (box_rect_scene.top() * scale)
        box_global_bottom = image_label_y_in_scroll + (box_rect_scene.bottom() * scale)

        is_visible = (box_global_top >= current_scroll_y) and \
                     (box_global_bottom <= current_scroll_y + viewport_height)
        
        if not is_visible:
            target_scroll_y = image_label_y_in_scroll + box_center_y_in_image - (viewport_height / 2)
            scrollbar = scroll_area.verticalScrollBar()
            clamped_scroll_y = max(scrollbar.minimum(), min(int(target_scroll_y), scrollbar.maximum()))
            scrollbar.setValue(clamped_scroll_y)

    def deselect_all_text_boxes(self):
        for text_box in self.text_boxes:
            if text_box.isSelected():
                text_box.setSelected(False)
    
    def select_text_box(self, row_number_to_select):
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
            except (TypeError, ValueError):
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

    def cleanup(self):
        try:
            self.textBoxDeleted.disconnect()
            self.selection_manager.selection_changed.disconnect(self.on_external_selection_changed)
            self.manual_area_selected.disconnect()
            self.stitching_selection_changed.disconnect()
            self.split_indicator_requested.disconnect()
            self.inpaintRecordDeleted.disconnect()
        except (TypeError, RuntimeError): pass
        if self.scene():
            for tb in self.text_boxes[:]: tb.cleanup()
            self.text_boxes = []
            self.clear_selection_visuals()
            
            for item in self.inpaint_patch_items:
                if item.scene(): self.scene().removeItem(item)
            self.inpaint_patch_items = []
            
            for item in self.inpaint_visuals:
                if item.scene(): self.scene().removeItem(item)
            self.inpaint_visuals = []
            for visual in self.split_visuals:
                if visual['line'].scene(): self.scene().removeItem(visual['line'])
                if visual['handle'].scene(): self.scene().removeItem(visual['handle'])
            self.split_visuals = []
            self.scene().clear()
        self.setScene(None)

    def get_text_boxes(self):
        return self.text_boxes