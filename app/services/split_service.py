# app/services/split_service.py

import os
from PySide6.QtCore import QObject
from PySide6.QtGui import QPixmap
from PySide6.QtCore import QRectF


class SplitService(QObject):
    """
    Pure service for splitting an image horizontally.
    No QtWidgets, no layout mutation, no dialogs.
    """

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self.model = model

    def split_image(self, filename: str, split_y: int) -> tuple[bool, str, list[str]]:
        """
        Splits image at given Y coordinate into 2 pieces.
        Saves new images with _split_1, _split_2 suffixes.
        Updates OCR results and inpaint data via model.redistribute_* methods.
        Updates model.image_paths preserving order.
        Emits model.image_list_changed on success.
        Returns: (success, message, new_filenames)
        """
        if not filename or split_y <= 0:
            return False, "Please place a split indicator.", []

        images_dir = os.path.join(self.model.temp_dir, 'images')
        source_path = os.path.join(images_dir, filename)
        source_pixmap = QPixmap(source_path)
        if source_pixmap.isNull():
            return False, f"Could not load image: {filename}.", []

        basename, ext = os.path.splitext(filename)

        def generate_unique_filename(base_name, extension, existing_files):
            counter = 1
            while True:
                candidate = f"{base_name}_split_{counter}{extension}"
                if candidate not in existing_files:
                    return candidate
                counter += 1

        existing_files = set(os.listdir(images_dir)) if os.path.exists(images_dir) else set()
        for path in self.model.image_paths:
            existing_files.add(os.path.basename(path))

        split_boundaries = [0, split_y, source_pixmap.height()]
        new_pixmaps = [
            source_pixmap.copy(
                QRectF(0, y_start, source_pixmap.width(), y_end - y_start).toRect()
            )
            for y_start, y_end in zip(split_boundaries, split_boundaries[1:])
        ]

        new_image_data = []
        new_filenames = []
        for pixmap in new_pixmaps:
            new_filename = generate_unique_filename(basename, ext, existing_files)
            existing_files.add(new_filename)
            new_filepath = os.path.join(images_dir, new_filename)
            if not pixmap.save(new_filepath):
                return False, f"Failed to save {new_filepath}.", []
            new_image_data.append({'filename': new_filename, 'pixmap': pixmap, 'path': new_filepath})
            new_filenames.append(new_filename)

        new_filenames = self.model.split_image_update(filename, new_image_data, [split_y])
        return True, f"Image split into {len(new_pixmaps)} parts.", new_filenames
