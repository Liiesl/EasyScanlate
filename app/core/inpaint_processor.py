# app/core/inpaint_processor.py

import io
import os
import traceback
import uuid

import cv2
import numpy as np
from PIL import Image

from PySide6.QtCore import QThread, Signal, QBuffer
from PySide6.QtGui import QImage, QPixmap


class InpaintProcessor(QThread):
    """
    Runs inpainting in a background thread so the UI stays responsive.

    Algorithm:
      1. Load the target image once.
      2. Restore *all* existing inpaint patches onto the base image.
      3. Merge overlapping bounding boxes into groups (proximity grouping).
      4. For each group:
         a. Build one binary mask from all boxes in the group.
         b. Run cv2.inpaint ONCE per group on the (patched) base image.
         c. Extract one patch per original bounding box from the inpainted result.
         d. Register each patch as an independent inpaint record.
    """

    finished = Signal(bool, str)  # success, message
    progress = Signal(int)        # 0-100 for current image

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------
    def __init__(self, model, filename: str, bounding_boxes: list, *, parent=None):
        """
        Args:
            model:      ProjectModel (used for temp_dir / existing records).
            filename:   Target image filename (basename).
            bounding_boxes: List of polygons, each polygon is a list of [x, y].
            parent:     QObject parent.
        """
        super().__init__(parent)
        self._model = model
        self._filename = filename
        self._bounding_boxes = bounding_boxes or []
        self.stop_requested = False

    # ------------------------------------------------------------------
    # Run
    # ------------------------------------------------------------------
    def run(self):
        try:
            if not self._filename or not self._bounding_boxes:
                self.finished.emit(False, "No filename or bounding boxes provided.")
                return

            images_dir = os.path.join(self._model.temp_dir, 'images')
            image_path = os.path.join(images_dir, self._filename)

            if not os.path.exists(image_path):
                self.finished.emit(False, f"Image not found: {image_path}")
                return

            # --- 1. Load image (OpenCV BGR) ---
            image_cv = cv2.imread(image_path, cv2.IMREAD_COLOR)
            if image_cv is None:
                self.finished.emit(False, f"Could not load image: {self._filename}")
                return

            h_img, w_img = image_cv.shape[:2]

            # --- 2. Restore all existing inpaint patches onto base image ---
            existing_records = self._model.get_inpaint_records_for_image(self._filename)
            if existing_records:
                inpaint_dir = os.path.join(self._model.temp_dir, 'inpaint')
                for rec in existing_records:
                    patch_path = os.path.join(inpaint_dir, rec.get('patch_filename', ''))
                    if not os.path.exists(patch_path):
                        continue
                    coords = rec.get('coordinates', [])
                    if len(coords) != 4:
                        continue
                    x, y, w, h = map(int, coords)
                    if w <= 0 or h <= 0:
                        continue
                    # Load patch and paste
                    patch_cv = cv2.imread(patch_path, cv2.IMREAD_COLOR)
                    if patch_cv is None:
                        continue
                    ph, pw = patch_cv.shape[:2]
                    # Clip to image bounds
                    x1, y1 = max(0, x), max(0, y)
                    x2, y2 = min(w_img, x + pw), min(h_img, y + ph)
                    px2, py2 = x2 - x1, y2 - y1
                    if px2 <= 0 or py2 <= 0:
                        continue
                    image_cv[y1:y2, x1:x2] = patch_cv[0:py2, 0:px2]

            if self.stop_requested:
                return

            self.progress.emit(10)

            # --- 3. Proximity grouping ---
            groups = self._group_bounding_boxes_by_proximity(self._bounding_boxes)
            total_groups = len(groups)

            if total_groups == 0:
                self.finished.emit(False, "No valid bounding-box groups after proximity merge.")
                return

            # --- 4. Process each group ---
            all_added = 0
            for group_idx, group in enumerate(groups):
                if self.stop_requested:
                    return

                # Build mask for this group
                mask = np.zeros((h_img, w_img), dtype=np.uint8)
                for box in group:
                    pts = np.array([[int(p[0]), int(p[1])] for p in box], dtype=np.int32)
                    if pts.size > 0:
                        cv2.fillPoly(mask, [pts], 255)

                # Inpaint once per group
                inpainted = cv2.inpaint(image_cv, mask, 3, cv2.INPAINT_TELEA)

                if self.stop_requested:
                    return

                # Extract one patch per original box and register it
                for box in group:
                    xs = [int(p[0]) for p in box]
                    ys = [int(p[1]) for p in box]
                    x, y, x_max, y_max = min(xs), min(ys), max(xs), max(ys)
                    w = x_max - x
                    h = y_max - y
                    if w <= 0 or h <= 0:
                        continue

                    patch_cv = inpainted[y:y + h, x:x + w]
                    patch_rgb = cv2.cvtColor(patch_cv, cv2.COLOR_BGR2RGB)
                    patch_h, patch_w, ch = patch_rgb.shape
                    q_image = QImage(
                        patch_rgb.data, patch_w, patch_h, ch * patch_w, QImage.Format_RGB888
                    )
                    patch_pixmap = QPixmap.fromImage(q_image)

                    patch_filename = (
                        f"{os.path.splitext(self._filename)[0]}_{uuid.uuid4().hex[:8]}.png"
                    )
                    record = {
                        "id": str(uuid.uuid4()),
                        "patch_filename": patch_filename,
                        "target_image": self._filename,
                        "coordinates": [x, y, w, h],
                    }

                    success, error_msg = self._model.add_inpaint_record(record, patch_pixmap)
                    if success:
                        all_added += 1
                    else:
                        print(f"InpaintProcessor: Failed to add record: {error_msg}")

                # Update progress
                pct = 10 + int((group_idx + 1) / total_groups * 90)
                self.progress.emit(pct)

            self.progress.emit(100)
            self.finished.emit(
                True,
                f"Auto-inpainting complete: {all_added} patch(es) added to {self._filename}.",
            )

        except Exception as e:
            traceback.print_exc()
            self.finished.emit(False, f"InpaintProcessor error: {str(e)}")

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------
    def _group_bounding_boxes_by_proximity(self, bounding_boxes):
        """Groups bounding boxes that overlap or are within 20 px of each other."""
        PROXIMITY_MARGIN = 20
        if not bounding_boxes:
            return []

        def get_bounds(box):
            xs = [p[0] for p in box]
            ys = [p[1] for p in box]
            return min(xs), min(ys), max(xs), max(ys)

        def boxes_overlap_or_close(box1, box2, margin):
            x1_min, y1_min, x1_max, y1_max = get_bounds(box1)
            x2_min, y2_min, x2_max, y2_max = get_bounds(box2)
            expanded1 = (
                x1_min - margin, y1_min - margin, x1_max + margin, y1_max + margin
            )
            return not (
                x2_max < expanded1[0]
                or x2_min > expanded1[2]
                or y2_max < expanded1[1]
                or y2_min > expanded1[3]
            )

        groups = []
        assigned = set()
        for i, box in enumerate(bounding_boxes):
            if i in assigned:
                continue
            new_group = [box]
            assigned.add(i)
            for j, other_box in enumerate(bounding_boxes):
                if j in assigned:
                    continue
                if boxes_overlap_or_close(box, other_box, PROXIMITY_MARGIN):
                    new_group.append(other_box)
                    assigned.add(j)
            groups.append(new_group)
        return groups
