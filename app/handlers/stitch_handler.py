# app/handlers/stitch_handler.py

from PySide6.QtWidgets import QMessageBox, QLabel
from PySide6.QtCore import QObject, Qt
from PySide6.QtGui import QPixmap, QPainter
from app.ui.components.image_area.label import ResizableImageLabel
from app.ui.dialogs.error_dialog import ErrorDialog
from app.ui.widgets.handler_overlay import HandlerOverlay
from assets.styles import HANDLER_OVERLAY_STYLES
import os


class StitchHandler(QObject):
    """
    Manages the UI and logic for stitching images. Does not depend on MainWindow.
    """
    def __init__(self, scroll_area, model):
        super().__init__(scroll_area)
        self.scroll_area = scroll_area
        self.model = model
        self.is_active = False
        self.selected_images = []
        
        self._setup_ui()

    def _setup_ui(self):
        """Creates the UI widget using HandlerOverlay base."""
        self.stitch_widget = HandlerOverlay(
            self.scroll_area,
            "StitchWidget",
            "",
            (380, 90)
        )
        self.stitch_widget.setStyleSheet(HANDLER_OVERLAY_STYLES)

        self.info_label = QLabel("Click on images to select them for stitching.")
        self.info_label.setAlignment(Qt.AlignCenter)
        self.stitch_widget.add_widget(self.info_label)

        self.btn_confirm = self.stitch_widget.create_confirm_button("Confirm Stitch", "fa5s.check")
        self.btn_confirm.clicked.connect(self.confirm_stitch)
        self.btn_confirm.setEnabled(False)

        self.btn_cancel = self.stitch_widget.create_cancel_button("Cancel", "fa5s.times")
        self.btn_cancel.clicked.connect(self.cancel_stitching_mode)

    def _update_widget_position(self):
        """Positions the overlay widget at the top-center of the visible scroll area."""
        if not self.stitch_widget.isVisible(): return
        self.stitch_widget._update_widget_position()

    def start_stitching_mode(self):
        """Enters the image selection mode for stitching."""
        if self.is_active: return
        self.scroll_area.cancel_active_modes(exclude_handler=self)
        self.is_active = True
        self.selected_images.clear()
        self.btn_confirm.setEnabled(False)
        
        print("Entering stitching selection mode.")
        self._update_info_label()
        self.stitch_widget.show_overlay()

        for widget in self._get_image_labels():
            widget.enable_stitching_selection(True)
            widget.stitching_selection_changed.connect(self._handle_image_selection)

    def _handle_image_selection(self, image_label, is_selected):
        """Updates the list of selected images based on user interaction."""
        temp_selection = set(self.selected_images)
        if is_selected:
            temp_selection.add(image_label)
        else:
            if image_label in temp_selection:
                temp_selection.remove(image_label)
        
        ordered_selection = []
        for widget in self._get_image_labels():
            if widget in temp_selection:
                ordered_selection.append(widget)
        self.selected_images = ordered_selection
        
        print(f"Selected images (in order): {[img.filename for img in self.selected_images]}")
        self.btn_confirm.setEnabled(len(self.selected_images) >= 2)
        self._update_info_label()

    def _update_info_label(self):
        num_selected = len(self.selected_images)
        if num_selected == 0:
            self.info_label.setText("Click on images to select them for stitching.")
        elif num_selected == 1:
            self.info_label.setText(f"<b>{self.selected_images[0].filename}</b> selected.<br>Select at least one more image.")
        else:
            filenames = ", ".join([img.filename for img in self.selected_images])
            self.info_label.setText(f"<b>{num_selected}</b> images selected.<br>Click to reorder. (Top to Bottom)")

    def confirm_stitch(self):
        """Combines selected images, updates the data model, and refreshes the UI."""
        if len(self.selected_images) < 2:
            QMessageBox.warning(self.scroll_area, "Selection Error", "Please select at least two images to stitch.")
            return

        print("--- Starting Image Stitching Process ---")
        
        labels_to_stitch = self.selected_images
        first_label = labels_to_stitch[0]
        new_filename = first_label.filename
        images_dir = os.path.join(self.model.temp_dir, 'images')
        new_filepath = os.path.join(images_dir, new_filename)
        print(f"New combined image will be saved as: {new_filepath}")

        pixmaps = [label.original_pixmap for label in labels_to_stitch]
        
        if not pixmaps:
            QMessageBox.critical(self.scroll_area, "Stitch Error", "Could not retrieve image data.")
            self.cancel_stitching_mode()
            return

        total_width = pixmaps[0].width()
        total_height = sum(p.height() for p in pixmaps)
        combined_pixmap = QPixmap(total_width, total_height)
        combined_pixmap.fill(Qt.transparent)
        painter = QPainter(combined_pixmap)
        current_y = 0
        for pixmap in pixmaps:
            painter.drawPixmap(0, current_y, pixmap)
            current_y += pixmap.height()
        painter.end()

        if not combined_pixmap.save(new_filepath):
            QMessageBox.critical(self.scroll_area, "Save Error", f"Failed to save stitched image.")
            self.cancel_stitching_mode()
            return
        print("Stitched image saved successfully.")

        height_offset = 0
        for i, label in enumerate(labels_to_stitch):
            current_filename = label.filename
            if i > 0:
                height_offset += labels_to_stitch[i-1].original_pixmap.height()
            
            for result in self.model.ocr_results:
                if result.get('filename') == current_filename:
                    result['filename'] = new_filename
                    if height_offset > 0:
                        coords = result.get('coordinates', [])
                        if coords:
                             result['coordinates'] = [[p[0], p[1] + height_offset] for p in coords]

            for record in self.model.inpaint_data:
                if record.get('target_image') == current_filename:
                    record['target_image'] = new_filename
                    if height_offset > 0:
                        coords = record.get('coordinates', [])
                        if coords and len(coords) == 4:
                            record['coordinates'][1] += height_offset

        filenames_to_remove = [label.filename for label in labels_to_stitch[1:]]
        for filename in filenames_to_remove:
            full_path_to_remove = os.path.join(images_dir, filename)
            original_full_path = next((p for p in self.model.image_paths if os.path.basename(p) == filename), None)
            if original_full_path and original_full_path in self.model.image_paths:
                self.model.image_paths.remove(original_full_path)
            try:
                if os.path.exists(full_path_to_remove):
                    os.remove(full_path_to_remove)
            except Exception as e:
                print(f"Warning: Could not delete old image file {full_path_to_remove}. Error: {e}")

        scroll_layout = self.scroll_area.widget().layout()
        first_label_index = self._get_widget_index(first_label)
        if first_label_index == -1:
            QMessageBox.critical(self.scroll_area, "UI Error", "Could not find original image position.")
            self.cancel_stitching_mode()
            return

        for label in labels_to_stitch:
            scroll_layout.removeWidget(label)
            label.cleanup()
            label.deleteLater()
            
        new_label = self.scroll_area.create_image_label(combined_pixmap, new_filename)
        scroll_layout.insertWidget(first_label_index, new_label)

        self.model.sort_and_notify()
        # Success message - keep QMessageBox.information for non-error cases
        QMessageBox.information(self.scroll_area, "Stitch Successful", 
                                f"{len(labels_to_stitch)} images stitched into one.")
        self.cancel_stitching_mode()

    def cancel_stitching_mode(self):
        """Exits the stitching mode and cleans up the UI."""
        if not self.is_active: return
        for widget in self._get_image_labels():
            try:
                widget.stitching_selection_changed.disconnect(self._handle_image_selection)
            except (TypeError, RuntimeError): pass
            widget.enable_stitching_selection(False)
        self.is_active = False
        self.selected_images.clear()
        self.info_label.setText("Click on images to select them for stitching.")
        self.stitch_widget.hide_overlay()
        print("Exited stitching selection mode.")

    def _get_image_labels(self):
        labels = []
        layout = self.scroll_area.widget().layout()
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                labels.append(widget)
        return labels
    
    def _get_widget_index(self, widget_to_find):
        layout = self.scroll_area.widget().layout()
        for i in range(layout.count()):
            if layout.itemAt(i).widget() == widget_to_find:
                return i
        return -1
