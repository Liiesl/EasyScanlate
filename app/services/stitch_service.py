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

        # Build offsets map: each filename -> cumulative y-offset
        offsets = {}
        height_offset = 0
        for i, filename in enumerate(filenames):
            offsets[filename] = height_offset
            if i < len(pixmaps):
                height_offset += pixmaps[i].height()

        self.model.stitch_images_update(filenames, new_filename, offsets)
        return True, f"{len(filenames)} images stitched into one."
