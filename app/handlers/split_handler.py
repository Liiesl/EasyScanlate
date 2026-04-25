# app/handlers/split_handler.py

from PySide6.QtWidgets import QMessageBox, QLabel
from PySide6.QtCore import QObject, Qt, QRectF
from PySide6.QtGui import QPixmap
from app.ui.components.image_area.label import ResizableImageLabel
from app.ui.dialogs.error_dialog import ErrorDialog
from app.ui.widgets.handler_overlay import HandlerOverlay
from assets.styles import HANDLER_OVERLAY_STYLES
import os


class SplitHandler(QObject):
    """
    Manages the UI and logic for splitting an image. Does not depend on MainWindow.
    """
    def __init__(self, scroll_area, model):
        super().__init__(scroll_area)
        self.scroll_area = scroll_area
        self.model = model
        self.is_active = False
        self.selected_label = None
        self.split_points = []

        self._setup_ui()

    def _setup_ui(self):
        """Creates the widget that appears during splitting mode using HandlerOverlay base."""
        self.split_widget = HandlerOverlay(
            self.scroll_area,
            "SplitWidget",
            "",
            (380, 90)
        )
        self.split_widget.setStyleSheet(HANDLER_OVERLAY_STYLES)

        self.info_label = QLabel("Click on an image to place a split indicator.")
        self.info_label.setAlignment(Qt.AlignCenter)
        self.split_widget.add_widget(self.info_label)

        self.btn_confirm = self.split_widget.create_confirm_button("Confirm Split", "fa5s.check")
        self.btn_confirm.clicked.connect(self.confirm_split)

        self.btn_clear = self.split_widget.create_reset_button("Clear Indicator", "fa5s.undo")
        self.btn_clear.clicked.connect(self.clear_split_points)

        self.btn_cancel = self.split_widget.create_cancel_button("Cancel", "fa5s.times")
        self.btn_cancel.clicked.connect(self.cancel_splitting_mode)

        self._update_button_states()

    def _update_widget_position(self):
        """Positions the overlay widget at the top-center of the visible scroll area."""
        if not self.split_widget.isVisible(): return
        self.split_widget._update_widget_position()

    def start_splitting_mode(self):
        """Enters the image splitting mode."""
        if self.is_active: return
        self.scroll_area.cancel_active_modes(exclude_handler=self)
        self.is_active = True
        self.selected_label = None
        self.split_points = []
        
        self.split_widget.show_overlay()
        self._update_info_label()
        self._update_button_states()

        for widget in self._get_image_labels():
            widget.enable_splitting_selection(True)
            widget.split_indicator_requested.connect(self._handle_indicator_placement)

    def _handle_indicator_placement(self, clicked_label, y_pos):
        """Moves the split indicator to the clicked position."""
        if self.selected_label and self.selected_label != clicked_label:
            self.selected_label.set_selected_for_splitting(False)

        self.selected_label = clicked_label
        self.split_points = [y_pos]
        self.selected_label.set_selected_for_splitting(True)
        self.selected_label.draw_split_lines(self.split_points)

        self._update_info_label()
        self._update_button_states()

    def confirm_split(self):
        """Slices the image and redistributes OCR data."""
        if not self.selected_label or not self.split_points:
            QMessageBox.warning(self.scroll_area, "Input Error", "Please place a split indicator.")
            return

        print("--- Starting Image Splitting Process ---")
        
        source_label = self.selected_label
        source_pixmap = source_label.original_pixmap
        source_filename = source_label.filename
        images_dir = os.path.join(self.model.temp_dir, 'images')
        basename, ext = os.path.splitext(source_filename)

        def generate_unique_filename(base_name, extension, existing_files):
            counter = 1
            while True:
                candidate = f"{base_name}_split_{counter}{extension}"
                if candidate not in existing_files: return candidate
                counter += 1

        existing_files = set(os.listdir(images_dir)) if os.path.exists(images_dir) else set()
        for path in self.model.image_paths:
            existing_files.add(os.path.basename(path))

        split_boundaries = [0] + self.split_points + [source_pixmap.height()]
        new_pixmaps = [source_pixmap.copy(QRectF(0, y_start, source_pixmap.width(), y_end - y_start).toRect())
                       for y_start, y_end in zip(split_boundaries, split_boundaries[1:])]

        new_image_data = []
        for pixmap in new_pixmaps:
            new_filename = generate_unique_filename(basename, ext, existing_files)
            existing_files.add(new_filename)
            new_filepath = os.path.join(images_dir, new_filename)
            if not pixmap.save(new_filepath):
                QMessageBox.critical(self.scroll_area, "Save Error", f"Failed to save {new_filepath}.")
                self.cancel_splitting_mode()
                return
            new_image_data.append({'filename': new_filename, 'pixmap': pixmap, 'path': new_filepath})

        # Update the data model before touching the UI
        self.model.redistribute_inpaint_for_split(source_filename, new_image_data, self.split_points)
        self.model.redistribute_ocr_for_split(source_filename, new_image_data, self.split_points)

        # Update UI
        scroll_layout = self.scroll_area.widget().layout()
        source_label_index = self._get_widget_index(source_label)
        if source_label_index == -1:
            QMessageBox.critical(self.scroll_area, "UI Error", "Could not find original image in layout.")
            self.cancel_splitting_mode()
            return

        scroll_layout.removeWidget(source_label)
        source_label.cleanup()
        source_label.deleteLater()

        for i, data in enumerate(new_image_data):
            new_label = self.scroll_area.create_image_label(data['pixmap'], data['filename'])
            scroll_layout.insertWidget(source_label_index + i, new_label)

        self.model.sort_and_notify()
        # Success message - keep QMessageBox.information for non-error cases
        QMessageBox.information(self.scroll_area, "Split Successful", f"Image split into {len(new_pixmaps)} parts.")
        self.cancel_splitting_mode()

    def cancel_splitting_mode(self):
        """Exits splitting mode and cleans up."""
        if not self.is_active: return
        
        if self.selected_label:
            try: self.selected_label.set_selected_for_splitting(False)
            except RuntimeError: pass
        
        for widget in self._get_image_labels():
            try: widget.split_indicator_requested.disconnect(self._handle_indicator_placement)
            except (TypeError, RuntimeError): pass
            widget.enable_splitting_selection(False)
        
        self.is_active = False
        self.selected_label = None
        self.split_points = []
        self.split_widget.hide_overlay()
        print("Exited splitting selection mode.")
    
    def clear_split_points(self):
        """Removes the split indicator and deselects the image."""
        if self.selected_label:
            self.selected_label.set_selected_for_splitting(False)
            self.selected_label = None
        self.split_points = []
        self._update_info_label()
        self._update_button_states()

    def _update_button_states(self):
        has_indicator = self.selected_label is not None and len(self.split_points) > 0
        self.btn_confirm.setEnabled(has_indicator)
        self.btn_clear.setEnabled(has_indicator)

    def _update_info_label(self):
        if not self.selected_label:
            self.info_label.setText("Click on an image to place a split indicator.")
        else:
            num_pieces = len(self.split_points) + 1
            self.info_label.setText(f"<b>{self.selected_label.filename}</b> selected.<br>"
                                    f"Click to move the indicator. (1 split / {num_pieces} pieces)")

    def _get_image_labels(self):
        labels = []
        layout = self.scroll_area.widget().layout()
        if not layout: return []
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                labels.append(widget)
        return labels

    def _get_widget_index(self, widget_to_find):
        layout = self.scroll_area.widget().layout()
        if not layout: return -1
        for i in range(layout.count()):
            if layout.itemAt(i).widget() == widget_to_find:
                return i
        return -1
