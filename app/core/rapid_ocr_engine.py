"""
RapidOCR Engine Module
Manages Detection and Recognition engines for OCR processing.
Based on the optimal configuration from rapidocr_test_gui.py.
"""

import cv2
import numpy as np
from PIL import Image
from typing import List, Tuple, Optional, Any
from rapidocr import RapidOCR, EngineType, LangRec, LangDet, OCRVersion


def get_rotate_crop_image(img: np.ndarray, points) -> np.ndarray:
    """
    Manually crops and warps the image based on detection points.
    Uses INTER_CUBIC and BORDER_REPLICATE to maintain quality for recognition.
    
    Args:
        img: Input image as numpy array
        points: Detection points (4 corner points)
        
    Returns:
        Warped/cropped image as numpy array
    """
    points = np.array(points, dtype=np.float32)
    
    # Sort points to strictly ensure: [top-left, top-right, bottom-right, bottom-left]
    x_sorted = points[np.argsort(points[:, 0]), :]
    left_most, right_most = x_sorted[:2, :], x_sorted[2:, :]
    left_most = left_most[np.argsort(left_most[:, 1]), :]
    tl, bl = left_most
    right_most = right_most[np.argsort(right_most[:, 1]), :]
    tr, br = right_most
    points = np.array([tl, tr, br, bl], dtype=np.float32)
    
    # Determine target image dimensions
    width_A = np.linalg.norm(br - bl)
    width_B = np.linalg.norm(tr - tl)
    max_width = max(int(width_A), int(width_B))

    height_A = np.linalg.norm(tr - br)
    height_B = np.linalg.norm(tl - bl)
    max_height = max(int(height_A), int(height_B))

    dst_pts = np.array([
        [0, 0],
        [max_width - 1, 0],
        [max_width - 1, max_height - 1],
        [0, max_height - 1]
    ], dtype=np.float32)

    M = cv2.getPerspectiveTransform(points, dst_pts)
    
    # Crucial: Use INTER_CUBIC for better resizing quality 
    # and BORDER_REPLICATE to avoid black edges interfering with text
    warped = cv2.warpPerspective(img, M, (max_width, max_height), 
                                 flags=cv2.INTER_CUBIC, borderMode=cv2.BORDER_REPLICATE)
    return warped


class RapidOCREngine:
    """
    Wrapper for RapidOCR with separate Detection and Recognition engines.
    Implements the manual Det -> Crop -> Rec pipeline for optimal results.
    """
    
    def __init__(self):
        self.det_engine: Optional[RapidOCR] = None
        self.rec_engine: Optional[RapidOCR] = None
        self._initialize_engines()
    
    def _initialize_engines(self):
        """Initialize separate Detection and Recognition engines."""
        # 1. Init Detection Engine Only
        self.det_engine = RapidOCR(
            params={
                "Det.engine_type": EngineType.ONNXRUNTIME,
                "Det.lang_type": LangDet.CH,  # CH det usually works fine for generic shapes
                "Det.ocr_version": OCRVersion.PPOCRV5,
                "Global.use_det": True,
                "Global.use_rec": False,
                "Global.use_cls": True,
            }
        )

        # 2. Init Recognition Engine Only
        self.rec_engine = RapidOCR(
            params={
                "Rec.engine_type": EngineType.ONNXRUNTIME,
                "Rec.lang_type": LangRec.KOREAN,
                "Rec.ocr_version": OCRVersion.PPOCRV5,
                "Global.use_det": False,
                "Global.use_rec": True,
                "Global.use_cls": False,
            }
        )
    
    def readtext(self, img: np.ndarray, **kwargs) -> List[Tuple[Any, str, float]]:
        """
        Run OCR on an image using the manual Det -> Crop -> Rec pipeline.
        
        Args:
            img: Input image as numpy array (grayscale or RGB)
            **kwargs: Ignored for now (for EasyOCR compatibility)
            
        Returns:
            List of tuples: (coordinates, text, confidence)
            Format matches EasyOCR output: ([[x1,y1], [x2,y2], [x3,y3], [x4,y4]], text, confidence)
        """
        results = []
        
        # Ensure image is in correct format for RapidOCR
        if len(img.shape) == 2:
            # Grayscale to RGB
            img_rgb = cv2.cvtColor(img, cv2.COLOR_GRAY2RGB)
        else:
            img_rgb = img
        
        # 1. Run Detection Only
        det_output = self.det_engine(img_rgb)
        
        boxes = []
        
        # Attempt to extract boxes based on object structure
        if hasattr(det_output, 'boxes') and det_output.boxes is not None:
            # Structure found in some wrappers (TextDetOutput.boxes)
            boxes = det_output.boxes
        elif isinstance(det_output, (list, tuple)):
            # Structure: [boxes, scores, elapse] or just [boxes]
            if len(det_output) > 0 and isinstance(det_output[0], (list, np.ndarray)):
                boxes = det_output[0]
            elif len(det_output) > 0 and isinstance(det_output[0], tuple):
                # Maybe (box, score) tuples?
                boxes = [x[0] for x in det_output]
        elif hasattr(det_output, 'dt_boxes'):
            # Older RapidOCR versions
            boxes = det_output.dt_boxes
        
        if boxes is None or len(boxes) == 0:
            return results
        
        # 2. For each detected box: Crop -> Recognize
        for i, box in enumerate(boxes):
            try:
                # Ensure box is a numpy array or list of points
                if hasattr(box, 'box'):  # Handle object wrapper inside list
                    box = box.box
                
                # 3. Manual Crop
                cropped_img = get_rotate_crop_image(img_rgb, box)
                
                # 4. Run Recognition on Crop
                rec_out = self.rec_engine(cropped_img)
                
                text = ""
                score = 0.0
                
                # Parse Rec output
                if isinstance(rec_out, tuple):
                    # standard: (result_list, time)
                    # result_list is usually [(text, score)]
                    if rec_out[0] and len(rec_out[0]) > 0:
                        text, score = rec_out[0][0]
                elif hasattr(rec_out, 'txts'):
                    # TextRecOutput object
                    if rec_out.txts and len(rec_out.txts) > 0:
                        text = rec_out.txts[0]
                        score = rec_out.scores[0]
                elif isinstance(rec_out, list) and len(rec_out) > 0:
                    # Just a list [(text, score)]
                    text, score = rec_out[0]
                
                if text:
                    # Convert box to format expected by EasyOCR: [[x1,y1], [x2,y2], [x3,y3], [x4,y4]]
                    if hasattr(box, 'tolist'):
                        box_list = box.tolist()
                    else:
                        box_list = list(box)
                    
                    # Ensure box_list is a list of 4 points
                    if len(box_list) >= 4:
                        # Take only the 4 corner points
                        coords = []
                        for j in range(4):
                            if j < len(box_list):
                                p = box_list[j]
                                if hasattr(p, 'tolist'):
                                    p = p.tolist()
                                coords.append([float(p[0]), float(p[1])])
                        
                        results.append((coords, text, float(score)))
                    
            except Exception as inner_e:
                print(f"Error processing box: {inner_e}")
                import traceback
                traceback.print_exc()
                continue
        
        # Sort results by vertical position (top-to-bottom) for proper reading order
        # Use the minimum y-coordinate of each box as the sort key
        results.sort(key=lambda r: min(p[1] for p in r[0]) if len(r[0]) > 0 else float('inf'))
       
        return results
