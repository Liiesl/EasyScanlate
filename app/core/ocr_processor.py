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

    def __init__(self, image_path, reader,
                 # Filters
                 min_text_height, max_text_height, min_confidence,
                 # Merging
                 distance_threshold, # <-- New: for merging
                 # EasyOCR Params
                 batch_size, decoder, adjust_contrast, resize_threshold
                ):
        super().__init__()
        self.image_path = image_path
        self.reader = reader
        self.stop_requested = False

        # Store all parameters
        self.min_text_height = min_text_height
        self.max_text_height = max_text_height
        self.min_confidence = min_confidence
        self.distance_threshold = distance_threshold # Store merging threshold
        self.batch_size = batch_size
        self.decoder = decoder
        self.adjust_contrast = adjust_contrast
        self.resize_threshold = resize_threshold

    def run(self):
        try:
            start_time_img = time.time()
            print(f"OCR Proc: Starting image {self.image_path}")
            # --- 1. Load and Preprocess Image ---
            img_pil = Image.open(self.image_path)
            original_width, original_height = img_pil.size

            # Convert to grayscale first
            img_pil_processed = img_pil.convert('L')

            # Optional Contrast Adjustment (before potential resize)
            if self.adjust_contrast > 0.0: # 0 means disabled or no effect
                try:
                    # Use a value slightly different from 1.0 for noticeable effect
                    # Clamp adjustment factor if necessary, e.g., 0.5 to 2.0
                    factor = max(0.1, 1.0 + self.adjust_contrast) # Example adjustment
                    enhancer = ImageEnhance.Contrast(img_pil_processed)
                    img_pil_processed = enhancer.enhance(factor)
                    print(f"OCR Proc: Applied contrast factor: {factor:.2f}")
                except Exception as enhance_err:
                    print(f"OCR Proc: Warning - Failed to apply contrast enhancement: {enhance_err}")
                    # Continue with the unenhanced image


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
                # Use LANCZOS (previously ANTIALIAS) for better quality downsampling
                img_pil_processed = img_pil_processed.resize((resized_width, resized_height), Image.Resampling.LANCZOS)

            # Convert final processed image to numpy array
            img_np = np.array(img_pil_processed)

            # Check for stop request before running OCR
            if self.stop_requested:
                print("OCR Proc: Stop requested before running reader."); return

            # --- 3. Run EasyOCR ---
            print(f"OCR Proc: Running reader.readtext (batch={self.batch_size}, decoder='{self.decoder}')")
            start_time_readtext = time.time()
            # Note: adjust_contrast is handled *before* readtext now via PIL
            raw_results = self.reader.readtext(
                img_np,
                batch_size=self.batch_size,
                decoder=self.decoder,
                detail=1 # Ensure coordinates, text, confidence
                # Removed adjust_contrast from here
            )
            readtext_duration = time.time() - start_time_readtext
            print(f"OCR Proc: reader.readtext found {len(raw_results)} regions in {readtext_duration:.2f}s.")

            # Emit 50% progress after readtext completes (as it's the main work)
            self.ocr_progress.emit(50)

            # Check for stop request after running OCR
            if self.stop_requested:
                print("OCR Proc: Stop requested after running reader."); return

            # --- 4. Scale Coordinates (if resized) ---
            scaled_results = []
            if was_resized:
                print("OCR Proc: Scaling coordinates back...")
                scale_x = original_width / resized_width
                scale_y = original_height / resized_height
                for coord_float, text, confidence in raw_results:
                    # Ensure coordinates are valid lists/tuples before scaling
                    try:
                        scaled_int_coord = [
                            [int(p[0] * scale_x), int(p[1] * scale_y)]
                            for p in coord_float
                        ]
                        scaled_results.append({'coordinates': scaled_int_coord, 'text': text, 'confidence': confidence})
                    except (TypeError, IndexError) as scale_err:
                        print(f"OCR Proc: Warning - Skipping result due to coordinate scaling error ({scale_err}): Text='{text[:30]}...'")
            else:
                # Convert coords to int even if not scaled, ensure consistent format
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
                if not result.get('coordinates'): continue # Skip if coords somehow became invalid

                try:
                    y_coords = [p[1] for p in result['coordinates']]
                    height = max(y_coords) - min(y_coords) if y_coords else 0
                except (ValueError, IndexError, TypeError): height = 0

                confidence = result['confidence']
                text = result['text']

                if (self.min_text_height <= height <= self.max_text_height and
                    confidence >= self.min_confidence):
                    filtered_results.append(result) # Keep the dictionary structure
                # else: # Optional: Log excluded results if needed for debugging
                #     print(f"OCR Proc: Excluded (H:{height:.0f}, C:{confidence:.2f}): '{text[:30]}...'")

                # Update progress during filtering (from 50% to 75%)
                if num_scaled > 0:
                    progress_percent = 50 + int((i + 1) / num_scaled * 25)
                    self.ocr_progress.emit(progress_percent)

            if self.stop_requested: return # Check again before merging
            print(f"OCR Proc: Filtered down to {len(filtered_results)} results.")

            # --- 6. Merge Results (Internal to this image) ---
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

            # Update progress after merging (from 75% to 100%)
            self.ocr_progress.emit(100)

            # Check for stop request one last time
            if self.stop_requested:
                print("OCR Proc: Stop requested before emitting results."); return

            # --- 7. Emit Final Results ---
            print(f"OCR Proc: Emitting {len(merged_results)} processed results for {self.image_path}.")
            self.ocr_finished.emit(merged_results) # Emit the list of merged result dicts

            img_duration = time.time() - start_time_img
            print(f"OCR Proc: Finished image {self.image_path} in {img_duration:.2f}s")

        except Exception as e:
            print(f"!!! OCR Processor Error in image {self.image_path}: {str(e)} !!!")
            print(traceback.format_exc())
            # Emit the error signal with details
            self.error_occurred.emit(f"Error processing {os.path.basename(self.image_path)}: {str(e)}")

# --- END OF FILE ocr_processor.py ---