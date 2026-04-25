# scroll_container.py

import os
from PySide6.QtWidgets import (QScrollArea, QWidget, QVBoxLayout, QPushButton,
                               QMessageBox, QCheckBox, QSizePolicy)
from PySide6.QtCore import Signal, QPoint
from PySide6.QtGui import QPixmap
import qtawesome as qta
from app.handlers.stitch_handler import StitchHandler
from app.handlers.split_handler import SplitHandler
from app.handlers.context_fill_handler import ContextFillHandler
from app.handlers.manual_ocr_handler import ManualOCRHandler
# --- MODIFIED: Import the generic Menu class ---
from app.ui.widgets.menus import Menu
from app.ui.components.image_area.label import ResizableImageLabel
from assets.styles import SCROLL_OVERLAY_STYLES

class CustomScrollArea(QScrollArea):
    """
    A custom QScrollArea that now owns and manages all action handlers,
    making them independent of the main window.
    """
    resized = Signal()

    def __init__(self, model, editor_viewmodel, image_area_viewmodel, on_initialize_reader, on_save_project, on_export_manhwa,
                 get_display_text, on_text_edited, get_reader, get_settings, on_manual_ocr_cancelled,
                 parent=None):
        """ The scroll area instantiates its own action handlers, passing only
            the necessary components (self and the model). """
        super().__init__(parent)
        self.model = model
        self.editor_vm = editor_viewmodel
        self.image_area_vm = image_area_viewmodel
        self.on_initialize_reader = on_initialize_reader
        self.on_save_project = on_save_project
        self.on_export_manhwa = on_export_manhwa
        self.get_display_text = get_display_text
        self.on_text_edited = on_text_edited
        self.get_reader = get_reader
        self.get_settings = get_settings
        self.on_manual_ocr_cancelled = on_manual_ocr_cancelled
        self.overlay_widget = None

        # Instantiate all handlers, breaking the MainWindow dependency
        self.manual_ocr_handler = ManualOCRHandler(self, self.model)
        self.manual_ocr_handler.reader_initialization_requested.connect(self.on_initialize_reader)
        self.stitch_handler = StitchHandler(self, self.model)
        self.split_handler = SplitHandler(self, self.model)
        self.context_fill_handler = ContextFillHandler(self, self.model)

        self.action_handlers = [
            self.manual_ocr_handler, self.stitch_handler,
            self.split_handler, self.context_fill_handler
        ]

        self._init_overlay()
        self.resized.connect(self.update_handler_ui_positions)
        self.verticalScrollBar().valueChanged.connect(self.update_handler_ui_positions)

        # Internal content widget (owned by CustomScrollArea, not MainWindow)
        self._scroll_content = QWidget()
        self._scroll_content.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        self._scroll_layout = QVBoxLayout(self._scroll_content)
        self._scroll_layout.setContentsMargins(0, 0, 0, 0)
        self._scroll_layout.setSpacing(0)
        self.setWidget(self._scroll_content)
        self.setWidgetResizable(True)

        # Reactive rebuild wiring
        self.image_area_vm.images_changed.connect(self._rebuild_labels)
        self.model.model_updated.connect(self._on_model_updated)

        # Relay VM selection changes to image labels
        self.editor_vm.selected_row_changed.connect(self._on_vm_selection_changed)

        # React to ImageAreaViewModel visibility changes
        self.image_area_vm.text_visible_changed.connect(self._on_text_visibility_changed)
        self.image_area_vm.inpaints_visible_changed.connect(self._on_inpaints_visibility_changed)

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
        label.manual_area_selected.connect(self.manual_ocr_handler.handle_area_selected)
        label.manual_area_selected.connect(self.context_fill_handler.handle_area_selected)
        return label

    def _rebuild_labels(self, filenames):
        """Rebuilds ResizableImageLabels reactively from the ViewModel image list."""
        self.cancel_active_modes()
        layout = self._scroll_layout
        while layout.count():
            item = layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                if hasattr(widget, 'cleanup'):
                    widget.cleanup()
                widget.deleteLater()

        path_map = {os.path.basename(p): p for p in self.model.image_paths}
        for filename in filenames:
            path = path_map.get(filename)
            if not path:
                continue
            try:
                pixmap = QPixmap(path)
                if pixmap.isNull():
                    continue
                label = self.create_image_label(pixmap, filename)
                layout.addWidget(label)
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
        forward model updates to individual labels. Per-label signal plumbing
        is deferred to later phases.
        """
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.refresh_visuals(affected_filenames)

    def refresh_all_labels(self):
        """Force refresh of all image labels (used on profile switch).

        Phase 3 TODO: When TranslationViewModel owns profile switching, replace
        this ad-hoc call with a reactive binding (e.g. TranslationViewModel
        active_profile_changed -> CustomScrollArea.refresh_all_labels).
        """
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.refresh_visuals()

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

    def _init_overlay(self):
        """ Creates and configures the overlay widget and its buttons. """
        self.overlay_widget = QWidget(self)
        self.overlay_widget.setObjectName("ScrollButtonOverlay")
        self.overlay_widget.setStyleSheet(SCROLL_OVERLAY_STYLES)

        layout = QVBoxLayout(self.overlay_widget)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(8)

        # Scroll to Top Button
        btn_scroll_top = QPushButton(qta.icon('fa5s.arrow-up', color='white'), "")
        btn_scroll_top.setObjectName("ScrollArrowButton")
        btn_scroll_top.setFixedSize(40, 40)
        btn_scroll_top.clicked.connect(lambda: self.verticalScrollBar().setValue(0))
        layout.addWidget(btn_scroll_top)

        # Save Menu Button
        btn_save_menu = QPushButton(qta.icon('fa5s.save', color='white'), "Save")
        btn_save_menu.setObjectName("ScrollSaveButton")
        btn_save_menu.setFixedSize(100, 40)
        btn_save_menu.clicked.connect(self._show_save_menu)
        layout.addWidget(btn_save_menu)

        # Scroll to Bottom Button
        btn_scroll_bottom = QPushButton(qta.icon('fa5s.arrow-down', color='white'), "")
        btn_scroll_bottom.setObjectName("ScrollArrowButton")
        btn_scroll_bottom.setFixedSize(40, 40)
        btn_scroll_bottom.clicked.connect(lambda: self.verticalScrollBar().setValue(self.verticalScrollBar().maximum()))
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

    def cancel_active_modes(self, exclude_handler=None):
        """Deactivates any currently running action handler mode."""
        if self.context_fill_handler.is_edit_mode_active and self.context_fill_handler is not exclude_handler:
            self.context_fill_handler._disable_edit_mode()
        for handler in self.action_handlers:
            if handler is not exclude_handler and handler.is_active:
                if hasattr(handler, 'cancel_mode'):
                    handler.cancel_mode()
                elif hasattr(handler, 'cancel_stitching_mode'):
                    handler.cancel_stitching_mode()
                elif hasattr(handler, 'cancel_splitting_mode'):
                    handler.cancel_splitting_mode()

    def _on_text_visibility_changed(self, visible):
        """React to ImageAreaViewModel text visibility changes."""
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_text_visibility(visible)

    def _on_inpaints_visibility_changed(self, visible):
        """React to ImageAreaViewModel inpaint visibility changes."""
        layout = self._scroll_layout
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                widget.set_inpaints_applied(visible)

    def update_handler_ui_positions(self):
        """ Updates the position of any active handler UI overlays. """
        for handler in self.action_handlers:
            if handler.is_active and hasattr(handler, '_update_widget_position'):
                handler._update_widget_position()

    def resizeEvent(self, event):
        """ Repositions the overlay on resize. """
        super().resizeEvent(event)
        self.update_overlay_position()
        self.resized.emit()

    def update_overlay_position(self):
        """ Calculates and sets the correct position for the overlay widget. """
        if self.overlay_widget:
            overlay_width = 140
            overlay_height = 186
            viewport_width = self.viewport().width()
            viewport_height = self.viewport().height()
            x = 10
            y = viewport_height - overlay_height - 10
            self.overlay_widget.setGeometry(x, y, overlay_width, overlay_height)
            self.overlay_widget.raise_()
