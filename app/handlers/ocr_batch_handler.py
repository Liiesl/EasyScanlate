import os, gc
from PySide6.QtCore import QObject, Signal
from app.core.ocr_processor import OCRProcessor
from app.core.project_model import ProjectModel
from app.ui.widgets import CustomProgressBar # Import the progress bar

class BatchOCRHandler(QObject):
    """
    Manages the entire batch OCR process for multiple images.
    This object lives in the main thread but orchestrates worker QThreads.
    """
    # --- DELETED: This signal is no longer needed ---
    # batch_progress = Signal(int)
    batch_finished = Signal(int)
    error_occurred = Signal(str)
    processing_stopped = Signal()

    # --- MODIFIED: Constructor now accepts the ProjectModel and the progress bar ---
    def __init__(self, image_paths, reader, settings, starting_row_number, model: ProjectModel, progress_bar: CustomProgressBar):
        super().__init__()
        self.image_paths = image_paths
        self.reader = reader
        self.settings = settings
        self.starting_row_number = starting_row_number
        self.model = model
        self.progress_bar = progress_bar # Store a reference to the progress bar

        self.current_image_index = 0
        self.next_global_row_number = self.starting_row_number
        self._is_stopped = False
        self.ocr_thread = None

    def start_processing(self):
        """Starts the batch process."""
        print("Batch Handler: Starting processing...")
        self._is_stopped = False
        # --- NEW: Directly control the progress bar ---
        self.progress_bar.start_initial_progress()
        self._process_next_image()
    
    # ... stop() and _process_next_image() remain the same ...
    def stop(self):
        """Requests the batch process to stop."""
        print("Batch Handler: Stop requested by user.")
        self._is_stopped = True
        if self.ocr_thread and self.ocr_thread.isRunning():
            self.ocr_thread.stop_requested = True
            
    def _process_next_image(self):
        """Processes a single image or finishes the batch if all are done."""
        if self._is_stopped:
            print("Batch Handler: Process was stopped, not starting next image.")
            self.processing_stopped.emit()
            return
            
        if self.current_image_index >= len(self.image_paths):
            print("Batch Handler: All images processed.")
            self._finish_batch()
            return

        if not self.reader:
            self.error_occurred.emit("OCR Reader not available. Cannot process next image.")
            return

        image_path = self.image_paths[self.current_image_index]
        print(f"Batch Handler: Creating thread for image {self.current_image_index + 1}/{len(self.image_paths)}: {os.path.basename(image_path)}")

        self.ocr_thread = OCRProcessor(
            image_path=image_path,
            reader=self.reader,
            **self.settings # Unpack the settings dictionary
        )

        self.ocr_thread.ocr_progress.connect(self._handle_image_progress)
        self.ocr_thread.ocr_finished.connect(self._handle_image_results)
        self.ocr_thread.error_occurred.connect(self._handle_image_error)
        self.ocr_thread.start()
    # --- MODIFIED: This method now directly updates the progress bar ---
    def _handle_image_progress(self, progress):
        """Calculates and updates the overall batch progress directly."""
        total_images = len(self.image_paths)
        if total_images == 0: return
        per_image_contribution = 80.0 / total_images
        current_image_progress = progress / 100.0
        overall_progress = 20 + (self.current_image_index * per_image_contribution) + (current_image_progress * per_image_contribution)
        # --- UPDATE a widget directly instead of emitting a signal ---
        self.progress_bar.update_target_progress(int(overall_progress))

    # ... _handle_image_results() remains the same ...
    def _handle_image_results(self, processed_results):
        """Receives results from a single image, updates the model directly, and starts the next."""
        if self._is_stopped:
            print("Batch Handler: Ignoring results from finished image due to stop request.")
            return

        current_image_path = self.image_paths[self.current_image_index]
        filename = os.path.basename(current_image_path)
        
        newly_numbered_results = []
        if processed_results:
            try:
                processed_results.sort(key=lambda r: min(p[1] for p in r.get('coordinates', [[0, float('inf')]])))
            except (ValueError, TypeError, IndexError) as e:
                print(f"Warning: Could not sort processed results for {filename}: {e}. Using processor order.")
            
            for result in processed_results:
                result['filename'] = filename
                result['row_number'] = self.next_global_row_number
                result['is_manual'] = False
                result['translations'] = {}
                newly_numbered_results.append(result)
                self.next_global_row_number += 1
        
        # --- UPDATE THE MODEL directly instead of emitting a signal ---
        if newly_numbered_results:
            self.model.add_new_ocr_results(newly_numbered_results)
            print(f"Batch Handler: Added {len(newly_numbered_results)} blocks from {filename} to model.")
        
        # Move to the next image
        self.current_image_index += 1
        self.ocr_thread = None
        gc.collect()

        self._process_next_image()

    # ... _handle_image_error() remains the same ...
    def _handle_image_error(self, message):
        """Handles an error from a worker thread."""
        print(f"Batch Handler: An error occurred: {message}")
        self._is_stopped = True
        self.error_occurred.emit(message)

    def _finish_batch(self):
        """Cleans up and signals that the entire batch is complete."""
        print("Batch Handler: Finishing run.")
        # --- NEW: Directly set the progress bar to 100% ---
        self.progress_bar.update_target_progress(100)
        self.batch_finished.emit(self.next_global_row_number)
        self.ocr_thread = None
        gc.collect()