# app/services/inpaint_service.py

import io
import os
import traceback
import uuid

import cv2
import numpy as np
from PIL import Image

from PySide6.QtCore import QObject, QBuffer, QRectF, QPointF
from PySide6.QtGui import (
    QImage, QPixmap, QPainterPath, QPolygonF, QPainter,
)


class InpaintService(QObject):
    """
    Pure service for context fill (inpainting).
    No QtWidgets, no layout mutation, no dialogs.
    """

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self.model = model

    def process_inpaint(self, filename: str, paths: list[QPainterPath]) -> tuple[bool, str]:
        """
        Performs cv2.inpaint on the given selection paths.
        Saves patch to model.
        Returns: (success, message)
        """
        if not paths or not filename:
            return False, "No area selected."

        images_dir = os.path.join(self.model.temp_dir, 'images')
        image_path = os.path.join(images_dir, filename)
        target_pixmap = QPixmap(image_path)
        if target_pixmap.isNull():
            return False, f"Could not load image: {filename}"

        try:
            temp_base_pixmap = target_pixmap.copy()
            new_selection_path = QPainterPath()
            for path in paths:
                new_selection_path = new_selection_path.united(path)

            PROXIMITY_MARGIN = 20
            selection_bounds = new_selection_path.boundingRect()
            proximity_rect = selection_bounds.adjusted(
                -PROXIMITY_MARGIN, -PROXIMITY_MARGIN,
                PROXIMITY_MARGIN, PROXIMITY_MARGIN,
            )

            all_records = self.model.get_inpaint_records_for_image(filename)
            proximal_records = []
            for record in all_records:
                coords = record.get('coordinates', [])
                if len(coords) == 4:
                    existing_rect = QRectF(coords[0], coords[1], coords[2], coords[3])
                    if proximity_rect.intersects(existing_rect):
                        proximal_records.append(record)

            if proximal_records:
                painter = QPainter(temp_base_pixmap)
                for record in proximal_records:
                    patch_pixmap = self.model.get_inpaint_patch_pixmap(record["patch_filename"])
                    if patch_pixmap:
                        coords = record['coordinates']
                        target_point = QPointF(coords[0], coords[1])
                        painter.drawPixmap(target_point, patch_pixmap)
                painter.end()

            buffer = QBuffer()
            buffer.open(QBuffer.ReadWrite)
            temp_base_pixmap.save(buffer, "PNG")
            pil_img = Image.open(io.BytesIO(buffer.data())).convert('RGB')
            image_np = np.array(pil_img)
            image_cv = cv2.cvtColor(image_np, cv2.COLOR_RGB2BGR)

            mask = np.zeros(image_cv.shape[:2], dtype=np.uint8)
            for path in paths:
                polygon = path.toFillPolygon().toPolygon()
                points = np.array([[p.x(), p.y()] for p in polygon], dtype=np.int32)
                if points.size > 0:
                    cv2.fillPoly(mask, [points], 255)

            inpainted_image_cv = cv2.inpaint(image_cv, mask, 3, cv2.INPAINT_TELEA)

            bounding_rect = new_selection_path.boundingRect().toRect()
            x, y, w, h = bounding_rect.x(), bounding_rect.y(), bounding_rect.width(), bounding_rect.height()
            if w <= 0 or h <= 0:
                return False, "Invalid selection area."

            patch_cv = inpainted_image_cv[y:y + h, x:x + w]
            patch_rgb = cv2.cvtColor(patch_cv, cv2.COLOR_BGR2RGB)

            patch_h, patch_w, ch = patch_rgb.shape
            q_image = QImage(patch_rgb.data, patch_w, patch_h, ch * patch_w, QImage.Format_RGB888)
            patch_pixmap = QPixmap.fromImage(q_image)

            patch_filename = f"{os.path.splitext(filename)[0]}_{uuid.uuid4().hex[:8]}.png"
            record = {
                "id": str(uuid.uuid4()),
                "patch_filename": patch_filename,
                "target_image": filename,
                "coordinates": [x, y, w, h],
            }

            success, error_msg = self.model.add_inpaint_record(record, patch_pixmap)
            if success:
                return True, "Context fill applied successfully."
            else:
                return False, error_msg

        except Exception as e:
            traceback.print_exc()
            return False, f"Inpainting error: {str(e)}"

    def delete_inpaint_record(self, record_id: str) -> tuple[bool, str]:
        """Delegates to model.remove_inpaint_record."""
        return self.model.remove_inpaint_record(record_id)

    def perform_auto_inpainting(self, filename: str, bounding_boxes: list):
        """
        Groups bounding boxes, converts to paths, calls process_inpaint for each group.
        Used by BatchOCR (Phase 5 bridge).
        """
        if not filename or not bounding_boxes:
            return

        groups = self._group_bounding_boxes_by_proximity(bounding_boxes)
        for group in groups:
            paths = []
            for box in group:
                path = QPainterPath()
                try:
                    poly = QPolygonF([QPointF(p[0], p[1]) for p in box])
                    path.addPolygon(poly)
                    paths.append(path)
                except (TypeError, IndexError):
                    continue
            if paths:
                self.process_inpaint(filename, paths)

    def _group_bounding_boxes_by_proximity(self, bounding_boxes):
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
            expanded1 = x1_min - margin, y1_min - margin, x1_max + margin, y1_max + margin
            return not (
                x2_max < expanded1[0] or x2_min > expanded1[2] or
                y2_max < expanded1[1] or y2_min > expanded1[3]
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
