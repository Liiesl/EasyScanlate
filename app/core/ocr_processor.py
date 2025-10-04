# --- START OF FILE ocr_processor.py ---

from PySide6.QtCore import QThread, Signal
import os
import numpy as np
from PIL import Image, ImageEnhance # Added ImageEnhance
import traceback
import time
from app.utils.data_processing import group_and_merge_text # Import merging function

class OCRProcessor(QThread):
    ocr_progress = Signal(int)  # Progress for the current image (0-100)
    ocr_finished = Signal(list)  # Results for the current image (list of dicts)
    error_occurred = Signal(str)
    auto_inpaint_requested = Signal(str, list)

    # --- MODIFIED: Add image_data parameter to accept in-memory images ---
    def __init__(self, reader,
                 # Filters
                 min_text_height, max_text_height, min_confidence,
                 # Merging
                 distance_threshold,
                 # EasyOCR Params
                 batch_size, decoder, adjust_contrast, resize_threshold,
                 # Inpainting
                 auto_context_fill=False,
                 # Image Sources (one must be provided)
                 image_path=None, image_data=None
                ):
        super().__init__()
        self.image_path = image_path
        self.image_data = image_data # Store the in-memory image data
        self.reader = reader
        self.stop_requested = False

        # Store all parameters
        self.min_text_height = min_text_height
        self.max_text_height = max_text_height
        self.min_confidence = min_confidence
        self.distance_threshold = distance_threshold
        self.batch_size = batch_size
        self.decoder = decoder
        self.adjust_contrast = adjust_contrast
        self.resize_threshold = resize_threshold
        self.auto_context_fill = auto_context_fill
        
        # --- NEW: Add validation ---
        if self.image_path is None and self.image_data is None:
            raise ValueError("OCRProcessor requires either an image_path or image_data.")

    def run(self):
        try:
            start_time_img = time.time()
            # --- NEW: Create a friendly name for logging ---
            image_name = os.path.basename(self.image_path) if self.image_path else "in-memory selection"
            print(f"OCR Proc: Starting image {image_name}")

            # --- 1. Load and Preprocess Image ---
            # --- MODIFIED: Load image from path OR from memory ---
            if self.image_data is not None:
                img_pil = self.image_data
            else:
                img_pil = Image.open(self.image_path)
            
            original_width, original_height = img_pil.size

            # Convert to grayscale first
            img_pil_processed = img_pil.convert('L')

            # Optional Contrast Adjustment (before potential resize)
            if self.adjust_contrast > 0.0: # 0 means disabled or no effect
                try:
                    factor = max(0.1, 1.0 + self.adjust_contrast)
                    enhancer = ImageEnhance.Contrast(img_pil_processed)
                    img_pil_processed = enhancer.enhance(factor)
                    print(f"OCR Proc: Applied contrast factor: {factor:.2f}")
                except Exception as enhance_err:
                    print(f"OCR Proc: Warning - Failed to apply contrast enhancement: {enhance_err}")


            # --- 2. Resize Image (if needed) ---
            resized_width, resized_height = original_width, original_height
            was_resized = False
            if self.resize_threshold > 0 and original_width > self.resize_threshold:
                was_resized = True
                max_width = self.resize_threshold
                ratio = max_width / original_width
                resized_height = int(original_height * ratio)
                resized_width = max_width
                print(f"OCR Proc: Resizing image {original_width}x{original_height} -> {resized_width}x{resized_height} (Threshold: {self.resize_threshold}px)")
                img_pil_processed = img_pil_processed.resize((resized_width, resized_height), Image.Resampling.LANCZOS)

            img_np = np.array(img_pil_processed)

            if self.stop_requested:
                print("OCR Proc: Stop requested before running reader."); return

            # --- 3. Run EasyOCR ---
            print(f"OCR Proc: Running reader.readtext (batch={self.batch_size}, decoder='{self.decoder}')")
            start_time_readtext = time.time()
            raw_results = self.reader.readtext(
                img_np,
                batch_size=self.batch_size,
                decoder=self.decoder,
                detail=1 # Ensure coordinates, text, confidence
                # Removed adjust_contrast from here
            )
            readtext_duration = time.time() - start_time_readtext
            print(f"OCR Proc: reader.readtext found {len(raw_results)} regions in {readtext_duration:.2f}s.")

            self.ocr_progress.emit(50)

            if self.stop_requested:
                print("OCR Proc: Stop requested after running reader."); return

            # --- 4. Scale Coordinates (if resized) ---
            # This logic now correctly works for both full images and cropped sections
            scaled_results = []
            if was_resized:
                print("OCR Proc: Scaling coordinates back...")
                scale_x = original_width / resized_width
                scale_y = original_height / resized_height
                for coord_float, text, confidence in raw_results:
                    try:
                        scaled_int_coord = [
                            [int(p[0] * scale_x), int(p[1] * scale_y)]
                            for p in coord_float
                        ]
                        scaled_results.append({'coordinates': scaled_int_coord, 'text': text, 'confidence': confidence})
                    except (TypeError, IndexError) as scale_err:
                        print(f"OCR Proc: Warning - Skipping result due to coordinate scaling error ({scale_err}): Text='{text[:30]}...'")
            else:
                for coord_float, text, confidence in raw_results:
                    try:
                        int_coord = [ [int(p[0]), int(p[1])] for p in coord_float ]
                        scaled_results.append({'coordinates': int_coord, 'text': text, 'confidence': confidence})
                    except (TypeError, IndexError) as int_err:
                         print(f"OCR Proc: Warning - Skipping result due to coordinate conversion error ({int_err}): Text='{text[:30]}...'")

            # --- 5. Filter Results ---
            filtered_results = []
            num_scaled = len(scaled_results)
            print(f"OCR Proc: Filtering {num_scaled} results (MinH={self.min_text_height}, MaxH={self.max_text_height}, MinConf={self.min_confidence:.2f})...")
            for i, result in enumerate(scaled_results):
                if self.stop_requested: print("OCR Proc: Stop requested during filtering."); break
                if not result.get('coordinates'): continue

                try:
                    y_coords = [p[1] for p in result['coordinates']]
                    height = max(y_coords) - min(y_coords) if y_coords else 0
                except (ValueError, IndexError, TypeError): height = 0

                confidence = result['confidence']

                if (self.min_text_height <= height <= self.max_text_height and
                    confidence >= self.min_confidence):
                    filtered_results.append(result)

                if num_scaled > 0:
                    progress_percent = 50 + int((i + 1) / num_scaled * 25)
                    self.ocr_progress.emit(progress_percent)

            if self.stop_requested: return
            print(f"OCR Proc: Filtered down to {len(filtered_results)} results.")
            
            # --- 6. Request Inpainting (BEFORE merging) ---
            # --- MODIFIED: Only request inpainting if we have a file path ---
            if self.auto_context_fill and filtered_results and self.image_path:
                try:
                    filename = os.path.basename(self.image_path)
                    all_coordinates = [res['coordinates'] for res in filtered_results]
                    print(f"OCR Proc: Requesting auto-inpaint for {len(all_coordinates)} regions in {filename} (pre-merge).")
                    self.auto_inpaint_requested.emit(filename, all_coordinates)
                except KeyError:
                    print(f"OCR Proc: Warning - Could not get coordinates for auto-inpaint from filtered results for {filename}.")

            # --- 7. Merge Results ---
            if not filtered_results:
                 print("OCR Proc: No results remaining after filtering to merge.")
                 merged_results = []
            else:
                 print(f"OCR Proc: Merging {len(filtered_results)} results (DistThr={self.distance_threshold})...")
                 # The merging function expects 'filename' key, add a placeholder
                 for res in filtered_results: res['filename'] = "placeholder"
                 merged_results = group_and_merge_text(
                     filtered_results,
                     distance_threshold=self.distance_threshold
                 )
                 # Remove the placeholder filename before emitting
                 for res in merged_results: res.pop('filename', None)
                 print(f"OCR Proc: Merged into {len(merged_results)} final blocks.")

            # --- Apply Transparent Style for Auto-Inpaint ---
            if self.auto_context_fill and merged_results and self.image_path:
                print(f"OCR Proc: Applying transparent background style to {len(merged_results)} results for auto-inpaint.")
                for result in merged_results:
                    result['custom_style'] = {'bg_color': '#00000000'}

            self.ocr_progress.emit(100)

            if self.stop_requested:
                print("OCR Proc: Stop requested before emitting results."); return

            # --- 8. Emit Final Results ---
            print(f"OCR Proc: Emitting {len(merged_results)} processed results for {image_name}.")
            self.ocr_finished.emit(merged_results)

            img_duration = time.time() - start_time_img
            print(f"OCR Proc: Finished image {image_name} in {img_duration:.2f}s")

        except Exception as e:
            image_name_err = os.path.basename(self.image_path) if self.image_path else "in-memory selection"
            print(f"!!! OCR Processor Error in image {image_name_err}: {str(e)} !!!")
            print(traceback.format_exc())
            self.error_occurred.emit(f"Error processing {image_name_err}: {str(e)}")