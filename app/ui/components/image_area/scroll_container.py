# scroll_container.py

import os
from PySide6.QtWidgets import (QScrollArea, QWidget, QVBoxLayout, QPushButton,
                               QMessageBox, QCheckBox, QSizePolicy, QLabel)
from PySide6.QtCore import Signal, Qt
from PySide6.QtGui import QPixmap
import qtawesome as qta

from app.ui.widgets.menus import Menu
from app.ui.components.image_area.label import ResizableImageLabel
from app.ui.widgets.handler_overlay import HandlerOverlay
from assets.styles import SCROLL_OVERLAY_STYLES, HANDLER_OVERLAY_STYLES


class CustomScrollArea(QScrollArea):
    """
    A custom QScrollArea that manages image labels reactively via ImageAreaViewModel.
    Action-mode overlays are owned by the View, not by services.
    """
    resized = Signal()

    def __init__(self, model, editor_viewmodel, image_area_viewmodel,
                 on_save_project, on_export_manhwa, get_display_text, on_text_edited,
                 get_settings, parent=None):
        super().__init__(parent)
        self.model = model
        self.editor_vm = editor_viewmodel
        self.image_area_vm = image_area_viewmodel
        self.on_save_project = on_save_project
        self.on_export_manhwa = on_export_manhwa
        self.get_display_text = get_display_text
        self.on_text_edited = on_text_edited
        self.get_settings = get_settings
        self.overlay_widget = None
        self._action_overlay = None

        self._init_overlay()
        self.resized.connect(self.update_handler_ui_positions)
        self.verticalScrollBar().valueChanged.connect(self.update_handler_ui_positions)

        # Internal content widget
        self._scroll_content = QWidget()
        self._scroll_content.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        self._scroll_layout = QVBoxLayout(self._scroll_content)
        self._scroll_layout.setContentsMargins(0, 0, 0, 0)
        self._scroll_layout.setSpacing(0)
        self.setWidget(self._scroll_content)
        self.setWidgetResizable(True)

        # Reactive rebuild wiring
        self.image_area_vm.images_changed.connect(self._rebuild_labels)
        self.model.text_updated.connect(self._on_model_updated)
        self.model.style_updated.connect(self._on_model_updated)
        self.model.rows_deleted.connect(self._on_model_updated)
        self.model.ocr_results_added.connect(self._on_model_updated)
        self.model.inpaint_updated.connect(self._on_model_updated)
        self.model.structural_updated.connect(self._on_model_updated)
        self.editor_vm.selected_row_changed.connect(self._on_vm_selection_changed)
        self.image_area_vm.text_visible_changed.connect(self._on_text_visibility_changed)
        self.image_area_vm.inpaints_visible_changed.connect(self._on_inpaints_visibility_changed)

        # Action mode wiring
        self.image_area_vm.active_action_mode_changed.connect(self._on_active_action_mode_changed)
        self.image_area_vm.can_confirm_action_changed.connect(self._on_can_confirm_changed)
        self.image_area_vm.can_reset_action_changed.connect(self._on_can_reset_changed)
        self.image_area_vm.action_mode_message_changed.connect(self._on_action_mode_message_changed)
        self.image_area_vm.action_mode_cancelled.connect(self._on_action_mode_cancelled)
        self.image_area_vm.selected_images_for_stitch_changed.connect(self._on_stitch_selection_changed)
        self.image_area_vm.split_point_changed.connect(self._on_split_point_changed)
        self.image_area_vm.inpaint_selection_paths_changed.connect(self._on_inpaint_selections_changed)
        self.image_area_vm.inpaint_edit_mode_active_changed.connect(self._on_inpaint_edit_mode_changed)
        self.image_area_vm.selected_inpaint_record_id_changed.connect(self._on_inpaint_record_selected)
        self.image_area_vm.manual_ocr_rect_changed.connect(self._on_manual_ocr_rect_changed)
        self.image_area_vm.manual_ocr_processing_changed.connect(self._on_manual_ocr_processing_changed)

    # ------------------------------------------------------------------
    # Label factory
    # ------------------------------------------------------------------
    def create_image_label(self, pixmap, filename):
        """Factory to create a ResizableImageLabel wired with all necessary callbacks."""
        label = ResizableImageLabel(
            pixmap, filename,
            self.model,
            self.get_display_text,
            self.on_text_edited,
            self._on_delete_row_confirmed,
            self
        )
        label.textBoxDeleted.connect(self._on_delete_row_confirmed)
        label.row_selected.connect(self.editor_vm.select_row)
        label.row_deselected.connect(self.editor_vm.maybe_deselect)
        label.inpaintRecordDeleted.connect(self.model.remove_inpaint_record)
        label.inpaintVisualSelected.connect(self.image_area_vm.select_inpaint_record)

        # Route action signals through CustomScrollArea to VM
        label.manual_area_selected.connect(self._on_manual_area_selected)
        label.stitching_selection_changed.connect(
            lambda lbl, sel: self.image_area_vm.toggle_image_selected_for_stitch(lbl.filename, sel)
        )
        label.split_indicator_requested.connect(
            lambda lbl, y: self.image_area_vm.set_split_point(lbl.filename, y)
        )
        return label

    # ------------------------------------------------------------------
    # Reactive rebuild
    # ------------------------------------------------------------------
    def _rebuild_labels(self, filenames):
        """Rebuilds ResizableImageLabels reactively from the ViewModel image list."""
        self.cancel_active_modes()
        layout = self._scroll_layout

        # Index existing labels by filename
        existing_labels = {}
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                existing_labels[widget.filename] = widget

        path_map = {os.path.basename(p): p for p in self.model.image_paths}

        # Remove labels that are no longer in the list
        for filename in list(existing_labels.keys()):
            if filename not in filenames:
                label = existing_labels.pop(filename)
                layout.removeWidget(label)
                if hasattr(label, 'cleanup'):
                    label.cleanup()
                label.deleteLater()

        # Add, move, or update labels to match the new order
        for idx, filename in enumerate(filenames):
            if filename in existing_labels:
                label = existing_labels[filename]
                # Check if label is already at the correct position
                current_item = layout.itemAt(idx)
                current_widget = current_item.widget() if current_item else None
                if current_widget is not label:
                    layout.removeWidget(label)
                    layout.insertWidget(idx, label)
                # Refresh pixmap in case the file was overwritten (e.g. stitch)
                path = path_map.get(filename)
                if path:
                    pixmap = QPixmap(path)
                    if not pixmap.isNull():
                        label.update_pixmap(pixmap)
            else:
                path = path_map.get(filename)
                if not path:
                    continue
                try:
                    pixmap = QPixmap(path)
                    if pixmap.isNull():
                        continue
                    label = self.create_image_label(pixmap, filename)
                    layout.insertWidget(idx, label)
                except Exception as e:
                    print(f"Error creating ResizableImageLabel for {path}: {e}")

        # Apply current visibility states
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_text_visibility(self.image_area_vm.text_visible)
                widget.set_inpaints_applied(self.image_area_vm.inpaints_visible)

    def _on_model_updated(self, affected_filenames):
        """
        Phase 2b side effect: CustomScrollArea walks its own widget tree to
        forward model updates to individual labels.
        """
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.refresh_visuals(affected_filenames)

    def refresh_all_labels(self):
        """Force refresh of all image labels (used on profile switch)."""
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.refresh_visuals()

    # ------------------------------------------------------------------
    # Deletion confirmation (View concern)
    # ------------------------------------------------------------------
    def _on_delete_row_confirmed(self, row_number):
        """Shows confirmation dialog (View concern) then delegates to VM."""
        show_warning = self.get_settings().value("show_delete_warning", "true") == "true"
        proceed = True
        if show_warning:
            msg = QMessageBox(self)
            msg.setIcon(QMessageBox.Warning)
            msg.setWindowTitle("Confirm Deletion Marking")
            msg.setText("<b>Mark for Deletion Warning</b>")
            msg.setInformativeText("Mark this entry for deletion? It will be hidden and excluded from exports.")
            dont_show_cb = QCheckBox("Remember choice", msg)
            msg.setCheckBox(dont_show_cb)
            msg.setStandardButtons(QMessageBox.Yes | QMessageBox.No)
            msg.setDefaultButton(QMessageBox.No)
            response = msg.exec()
            if dont_show_cb.isChecked():
                self.get_settings().setValue("show_delete_warning", "false")
            proceed = response == QMessageBox.Yes
        if proceed:
            self.editor_vm.delete_row(row_number)

    def _on_vm_selection_changed(self, row_number):
        """Forward VM selection changes to all image labels."""
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.on_external_selection_changed(row_number)

    # ------------------------------------------------------------------
    # Overlay / chrome
    # ------------------------------------------------------------------
    def _init_overlay(self):
        """Creates and configures the overlay widget and its buttons."""
        self.overlay_widget = QWidget(self)
        self.overlay_widget.setObjectName("ScrollButtonOverlay")
        self.overlay_widget.setStyleSheet(SCROLL_OVERLAY_STYLES)

        layout = QVBoxLayout(self.overlay_widget)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(8)

        btn_scroll_top = QPushButton(qta.icon('fa5s.arrow-up', color='white'), "")
        btn_scroll_top.setObjectName("ScrollArrowButton")
        btn_scroll_top.setFixedSize(40, 40)
        btn_scroll_top.clicked.connect(lambda: self.verticalScrollBar().setValue(0))
        layout.addWidget(btn_scroll_top)

        btn_save_menu = QPushButton(qta.icon('fa5s.save', color='white'), "Save")
        btn_save_menu.setObjectName("ScrollSaveButton")
        btn_save_menu.setFixedSize(100, 40)
        btn_save_menu.clicked.connect(self._show_save_menu)
        layout.addWidget(btn_save_menu)

        btn_scroll_bottom = QPushButton(qta.icon('fa5s.arrow-down', color='white'), "")
        btn_scroll_bottom.setObjectName("ScrollArrowButton")
        btn_scroll_bottom.setFixedSize(40, 40)
        btn_scroll_bottom.clicked.connect(
            lambda: self.verticalScrollBar().setValue(self.verticalScrollBar().maximum())
        )
        layout.addWidget(btn_scroll_bottom)

    def _show_save_menu(self):
        """Creates, populates, and shows the Save menu."""
        trigger_button = self.sender()
        if not isinstance(trigger_button, QWidget):
            return

        menu = Menu(self)

        btn_save_project = QPushButton(qta.icon('fa5s.save', color='white'), " Save Project (.mmtl)")
        btn_save_project.clicked.connect(self.on_save_project)
        menu.addButton(btn_save_project)

        btn_save_images = QPushButton(qta.icon('fa5s.images', color='white'), " Save Rendered Images")
        btn_save_images.clicked.connect(self.on_export_manhwa)
        menu.addButton(btn_save_images)

        menu.set_position_and_show(trigger_button, 'right')

    # ------------------------------------------------------------------
    # Action mode overlay management
    # ------------------------------------------------------------------
    def _on_manual_area_selected(self, rect, label):
        mode = self.image_area_vm.active_action_mode
        if mode == "inpaint":
            self.image_area_vm.handle_area_selected(label.filename, rect)
        elif mode == "manual_ocr":
            self.image_area_vm.handle_manual_area_selected(label.filename, rect)
        # Hide the temporary rubber band; the drawn selection path replaces it visually.
        label.clear_rubber_band()

    def _on_active_action_mode_changed(self, mode, message, can_confirm):
        # Update label interaction states and clear any leftover visual artifacts
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.enable_stitching_selection(mode == "stitch")
                widget.enable_splitting_selection(mode == "split")
                widget.set_manual_selection_enabled(mode in ("inpaint", "manual_ocr"))
                if not mode:
                    widget.clear_selection_visuals()
                    widget.clear_rubber_band()

        if mode:
            self._show_action_overlay(mode)
        else:
            self._hide_action_overlay()

    def _show_action_overlay(self, mode: str):
        self._hide_action_overlay()

        if mode == "stitch":
            overlay = HandlerOverlay(self, "StitchWidget", "", (380, 90))
            info_label = QLabel("Click on images to select them for stitching.")
            info_label.setAlignment(Qt.AlignCenter)
            overlay.add_widget(info_label)
            overlay.create_confirm_button("Confirm Stitch", "fa5s.check")
            overlay.create_cancel_button("Cancel", "fa5s.times")
            overlay._info_label = info_label
        elif mode == "split":
            overlay = HandlerOverlay(self, "SplitWidget", "", (380, 90))
            info_label = QLabel("Click on an image to place a split indicator.")
            info_label.setAlignment(Qt.AlignCenter)
            overlay.add_widget(info_label)
            overlay.create_confirm_button("Confirm Split", "fa5s.check")
            overlay.create_reset_button("Clear Indicator", "fa5s.undo")
            overlay.create_cancel_button("Cancel", "fa5s.times")
            overlay._info_label = info_label
        elif mode == "inpaint":
            overlay = HandlerOverlay(self, "ContextFillOverlay", "", (380, 80))
            overlay.create_confirm_button("Fill Selected Areas")
            overlay.create_reset_button("Reset All Selections")
            overlay.create_cancel_button("Exit Context Fill")
        elif mode == "manual_ocr":
            overlay = HandlerOverlay(self, "ManualOCROverlay", "", (350, 80))
            status_label = QLabel("Draw a box on an image to begin.")
            overlay.add_widget(status_label)
            overlay.create_confirm_button("OCR This Part")
            overlay.create_reset_button("Reset Selection")
            overlay.create_cancel_button("Cancel Manual OCR")
            overlay._status_label = status_label
        else:
            return

        overlay.setStyleSheet(HANDLER_OVERLAY_STYLES)
        overlay.confirmed.connect(self.image_area_vm.confirm_action_mode)
        overlay.cancelled.connect(self.image_area_vm.cancel_action_mode)
        overlay.reset_clicked.connect(self.image_area_vm.reset_action_mode)

        overlay.set_confirm_enabled(self.image_area_vm.can_confirm_action)
        overlay.set_reset_enabled(self.image_area_vm.can_reset_action)

        overlay.show_overlay()
        self._action_overlay = overlay

    def _hide_action_overlay(self):
        if self._action_overlay:
            self._action_overlay.hide_overlay()
            self._action_overlay.deleteLater()
            self._action_overlay = None

    def _on_can_confirm_changed(self, can_confirm):
        if self._action_overlay:
            self._action_overlay.set_confirm_enabled(can_confirm)

    def _on_can_reset_changed(self, can_reset):
        if self._action_overlay:
            self._action_overlay.set_reset_enabled(can_reset)

    def _on_action_mode_message_changed(self, message):
        if self._action_overlay:
            if hasattr(self._action_overlay, '_info_label'):
                self._action_overlay._info_label.setText(message)
            elif hasattr(self._action_overlay, '_status_label'):
                self._action_overlay._status_label.setText(message)

    def _on_action_mode_cancelled(self, mode):
        # Overlay is already hidden by _on_active_action_mode_changed (mode becomes empty)
        pass

    # ------------------------------------------------------------------
    # Mode-specific visual updates
    # ------------------------------------------------------------------
    def _on_stitch_selection_changed(self, filenames):
        # Labels handle their own overlay visuals via enable_stitching_selection
        pass

    def _on_split_point_changed(self, filename, y):
        for i in range(self._scroll_layout.count()):
            widget = self._scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                if widget.filename == filename and y > 0:
                    widget.set_selected_for_splitting(True)
                    widget.draw_split_lines([y])
                else:
                    widget.set_selected_for_splitting(False)

    def _on_inpaint_selections_changed(self, filename, paths):
        for i in range(self._scroll_layout.count()):
            widget = self._scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                if widget.filename == filename:
                    widget.draw_selections(paths)
                else:
                    widget.clear_selection_visuals()

    def _on_inpaint_edit_mode_changed(self, active):
        for i in range(self._scroll_layout.count()):
            widget = self._scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_inpaint_edit_mode(active)
                if active:
                    widget.set_text_visibility(False)
                else:
                    widget.set_text_visibility(self.image_area_vm.text_visible)
        if active:
            self._hide_action_overlay()
            if self.image_area_vm.selected_inpaint_record_id:
                self._on_inpaint_record_selected(self.image_area_vm.selected_inpaint_record_id)
            else:
                overlay = HandlerOverlay(self, "InpaintEditModeOverlay", "", (380, 80))
                info_label = QLabel("Click on a context fill to select it.")
                info_label.setAlignment(Qt.AlignCenter)
                overlay.add_widget(info_label)
                overlay.create_cancel_button("Exit Edit Mode", "fa5s.times")
                overlay.setStyleSheet(HANDLER_OVERLAY_STYLES)
                overlay.cancelled.connect(self.image_area_vm.toggle_inpaint_edit_mode)
                overlay.show_overlay()
                self._action_overlay = overlay
        else:
            self._hide_action_overlay()

    def _on_inpaint_record_selected(self, record_id):
        if self.image_area_vm.inpaint_edit_mode_active and record_id:
            self._hide_action_overlay()
            overlay = HandlerOverlay(self, "InpaintEditOverlay", "", (350, 80))
            overlay.create_confirm_button("Delete Selected")
            overlay.create_cancel_button("Cancel")
            overlay.setStyleSheet(HANDLER_OVERLAY_STYLES)
            overlay.confirmed.connect(self.image_area_vm.delete_selected_inpaint)
            overlay.cancelled.connect(self._hide_inpaint_edit_overlay)
            overlay.show_overlay()
            self._action_overlay = overlay
        elif self._action_overlay and self._action_overlay.objectName() == "InpaintEditOverlay":
            self._hide_action_overlay()
            # Restore generic edit mode overlay if still in edit mode
            if self.image_area_vm.inpaint_edit_mode_active:
                self._on_inpaint_edit_mode_changed(True)

    def _hide_inpaint_edit_overlay(self):
        self.image_area_vm.select_inpaint_record("")
        self._hide_action_overlay()
        if self.image_area_vm.inpaint_edit_mode_active:
            self._on_inpaint_edit_mode_changed(True)

    def _on_manual_ocr_rect_changed(self, filename, rect):
        for i in range(self._scroll_layout.count()):
            widget = self._scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                if widget.filename == filename and rect is not None:
                    widget.draw_selections([rect])
                else:
                    widget.clear_selection_visuals()

    def _on_manual_ocr_processing_changed(self, processing):
        # Button states are already synced via can_confirm / can_reset
        pass

    # ------------------------------------------------------------------
    # Visibility toggles
    # ------------------------------------------------------------------
    def _on_text_visibility_changed(self, visible):
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_text_visibility(visible)

    def _on_inpaints_visibility_changed(self, visible):
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_inpaints_applied(visible)

    # ------------------------------------------------------------------
    # Lifecycle helpers
    # ------------------------------------------------------------------
    def cancel_active_modes(self):
        """Deactivates any currently running action handler mode."""
        self.image_area_vm.cancel_action_mode()
        if self.image_area_vm.inpaint_edit_mode_active:
            self.image_area_vm.toggle_inpaint_edit_mode()

    def update_handler_ui_positions(self):
        """Updates the position of any active handler UI overlays."""
        if self._action_overlay and self._action_overlay.isVisible():
            self._action_overlay._update_widget_position()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self.update_overlay_position()
        self.resized.emit()

    def update_overlay_position(self):
        """Calculates and sets the correct position for the overlay widget."""
        if self.overlay_widget:
            overlay_width = 140
            overlay_height = 186
            viewport_width = self.viewport().width()
            viewport_height = self.viewport().height()
            x = 10
            y = viewport_height - overlay_height - 10
            self.overlay_widget.setGeometry(x, y, overlay_width, overlay_height)
            self.overlay_widget.raise_()
