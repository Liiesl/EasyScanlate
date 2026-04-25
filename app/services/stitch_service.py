# app/services/stitch_service.py

import os
from PySide6.QtCore import QObject, Qt
from PySide6.QtGui import QPixmap, QPainter


class StitchService(QObject):
    """
    Pure service for stitching images vertically.
    No QtWidgets, no layout mutation, no dialogs.
    """

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self.model = model

    def stitch_images(self, filenames: list[str]) -> tuple[bool, str]:
        """
        Stitches the given filenames (in order) vertically.
        Overwrites the first image's file with the combined image.
        Updates model.ocr_results (filename + y-offset) and model.inpaint_data.
        Removes other images from model.image_paths and deletes their files.
        Emits model.image_list_changed on success.
        Returns: (success, message)
        """
        if len(filenames) < 2:
            return False, "Please select at least two images to stitch."

        images_dir = os.path.join(self.model.temp_dir, 'images')
        new_filename = filenames[0]
        new_filepath = os.path.join(images_dir, new_filename)

        pixmaps = []
        for filename in filenames:
            path = os.path.join(images_dir, filename)
            pixmap = QPixmap(path)
            if pixmap.isNull():
                return False, f"Could not retrieve image data for {filename}."
            pixmaps.append(pixmap)

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
            return False, "Failed to save stitched image."

        # Update OCR results and inpaint data with new filename and y-offsets
        height_offset = 0
        for i, filename in enumerate(filenames):
            if i > 0:
                height_offset += pixmaps[i - 1].height()

            for result in self.model.ocr_results:
                if result.get('filename') == filename:
                    result['filename'] = new_filename
                    if height_offset > 0:
                        coords = result.get('coordinates', [])
                        if coords:
                            result['coordinates'] = [[p[0], p[1] + height_offset] for p in coords]

            for record in self.model.inpaint_data:
                if record.get('target_image') == filename:
                    record['target_image'] = new_filename
                    if height_offset > 0:
                        coords = record.get('coordinates', [])
                        if coords and len(coords) == 4:
                            record['coordinates'][1] += height_offset

        # Remove old images from model and disk.
        # The first image file was overwritten in-place, so its path in image_paths
        # is already correct — we only need to remove the other stitched images.
        filenames_to_remove = filenames[1:]
        for fname in filenames_to_remove:
            path = next((p for p in self.model.image_paths if os.path.basename(p) == fname), None)
            if path and path in self.model.image_paths:
                self.model.image_paths.remove(path)
            full_path = os.path.join(images_dir, fname)
            try:
                if os.path.exists(full_path):
                    os.remove(full_path)
            except Exception as e:
                print(f"Warning: Could not delete old image file {full_path}. Error: {e}")

        self.model.sort_and_notify()
        return True, f"{len(filenames)} images stitched into one."
