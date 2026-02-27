# ui_test.py
# Test entry point for UI development
# Bypasses splash screen and data loading, directly shows main window with fake data

import sys
import os
import json
import tempfile
import zipfile
from pathlib import Path

# Add the project root to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QSettings
from app.ui.window.main_window import MainWindow
from app.core.project_model import ProjectModel
from app.utils.exception_handler import setup_global_exception_handler


def create_fake_project_data(temp_dir):
    """Creates fake project data in the temporary directory for testing."""
    
    # Create images directory with a simple test image
    images_dir = os.path.join(temp_dir, 'images')
    os.makedirs(images_dir, exist_ok=True)
    
    # Create a simple test image (1x1 pixel PNG)
    from PySide6.QtGui import QPixmap, QColor
    pixmap = QPixmap(800, 600)
    pixmap.fill(QColor(100, 100, 100))
    
    # Create multiple test images
    for i in range(1, 4):
        img_path = os.path.join(images_dir, f'page_{i:03d}.png')
        pixmap.save(img_path)
        print(f"[TEST] Created fake image: {img_path}")
    
    # Create master.json with sample OCR results
    ocr_results = [
        {
            "row_number": 0,
            "filename": "page_001.png",
            "coordinates": [[100, 100], [200, 100], [200, 150], [100, 150]],
            "text": "Sample Text 1",
            "translations": {
                "Original": "Sample Text 1"
            }
        },
        {
            "row_number": 1,
            "filename": "page_001.png",
            "coordinates": [[100, 200], [300, 200], [300, 250], [100, 250]],
            "text": "Sample Text 2",
            "translations": {
                "Original": "Sample Text 2"
            }
        },
        {
            "row_number": 2,
            "filename": "page_002.png",
            "coordinates": [[150, 100], [400, 100], [400, 200], [150, 200]],
            "text": "This is a longer sample text for testing",
            "translations": {
                "Original": "This is a longer sample text for testing"
            }
        },
        {
            "row_number": 3,
            "filename": "page_003.png",
            "coordinates": [[50, 50], [150, 50], [150, 100], [50, 100]],
            "text": "More test content",
            "translations": {
                "Original": "More test content"
            }
        }
    ]
    
    master_path = os.path.join(temp_dir, 'master.json')
    with open(master_path, 'w', encoding='utf-8') as f:
        json.dump(ocr_results, f, indent=2, ensure_ascii=False)
    print(f"[TEST] Created master.json with {len(ocr_results)} OCR results")
    
    # Create meta.json with project metadata
    meta_data = {
        'original_language': 'Korean',
        'active_profile_name': 'Original'
    }
    meta_path = os.path.join(temp_dir, 'meta.json')
    with open(meta_path, 'w', encoding='utf-8') as f:
        json.dump(meta_data, f, indent=2, ensure_ascii=False)
    print(f"[TEST] Created meta.json")
    
    # Create inpaint.json (empty for now)
    inpaint_path = os.path.join(temp_dir, 'inpaint.json')
    with open(inpaint_path, 'w', encoding='utf-8') as f:
        json.dump([], f, indent=2, ensure_ascii=False)
    print(f"[TEST] Created inpaint.json")


def create_fake_mmtl_file(temp_dir):
    """Creates a fake .mmtl file (which is just a ZIP file)."""
    mmtl_path = os.path.join(tempfile.gettempdir(), 'test_project.mmtl')
    
    # Create a zip file with the temp_dir contents
    with zipfile.ZipFile(mmtl_path, 'w', zipfile.ZIP_DEFLATED) as zf:
        for root, dirs, files in os.walk(temp_dir):
            for file in files:
                file_path = os.path.join(root, file)
                arcname = os.path.relpath(file_path, temp_dir)
                zf.write(file_path, arcname)
    
    print(f"[TEST] Created fake .mmtl file: {mmtl_path}")
    return mmtl_path


if __name__ == '__main__':
    print("[TEST] Starting UI test with fake data...")
    
    # Create QApplication
    app = QApplication(sys.argv)
    app.setApplicationName("ManhwaOCR - UI Test")
    app.setApplicationVersion("1.0-TEST")
    
    # Set up global exception handler
    setup_global_exception_handler(app)
    
    # Create a temporary directory for fake project data
    temp_dir = tempfile.mkdtemp(prefix='ui_test_project_')
    print(f"[TEST] Created temporary directory: {temp_dir}")
    
    try:
        # Generate fake project data
        create_fake_project_data(temp_dir)
        
        # Create fake .mmtl file
        mmtl_path = create_fake_mmtl_file(temp_dir)
        
        # Create and initialize the main window
        print("[TEST] Creating MainWindow instance...")
        main_window = MainWindow()
        
        # Create the project model and load fake data
        print("[TEST] Loading fake project into model...")
        main_window.model.load_project(mmtl_path, temp_dir)
        
        # The model will emit project_loaded signal which triggers on_project_loaded
        # which handles all UI population. This is already connected in MainWindow.__init__
        print("[TEST] UI should be populated by model signals...")
        
        # Show the main window
        print("[TEST] Showing MainWindow...")
        main_window.show()
        
        print("[TEST] UI test ready. Running event loop.")
        print(f"[TEST] Temp directory (will be cleaned on exit): {temp_dir}")
        
        # Run the application
        sys.exit(app.exec())
        
    except Exception as e:
        print(f"[TEST] ERROR: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
