# capture_ui.py
import sys
import os
import tempfile
import shutil
from pathlib import Path

# Add project root to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QTimer, QSize
from PySide6.QtGui import QImage, QPainter

# Import your modules
from app.ui.window.main_window import MainWindow
# We import the data generator from your existing ui_test.py to avoid code duplication
# Assumes ui_test.py is in the root directory
from ui_test import create_fake_project_data, create_fake_mmtl_file
from app.utils.exception_handler import setup_global_exception_handler
from PySide6.QtGui import QGuiApplication, QScreen # ADD THIS

def capture_and_exit(window, output_path):
    print(f"Capturing screenshot to {output_path}...")
    
    # Resize window
    window.resize(1280, 800)
    
    # Center the window to ensure it's not cut off
    screen_geometry = window.screen().availableGeometry()
    x = (screen_geometry.width() - window.width()) // 2
    y = (screen_geometry.height() - window.height()) // 2
    window.move(x, y)

    # Allow time for move/resize events to process
    QApplication.processEvents()

    # --- CHANGED: CAPTURE ENTIRE SCREEN ---
    # grabWindow(0) captures the entire desktop. 
    # This includes the window + OS borders + shadows + wallpaper.
    screen = QGuiApplication.primaryScreen()
    pixmap = screen.grabWindow(0)
    
    # Save
    pixmap.save(output_path)
    print("Screenshot saved.")
    
    QApplication.quit()

if __name__ == '__main__':
    # Initialize App
    # 'offscreen' platform is needed for macOS in CI environment
    if sys.platform == 'darwin':
        os.environ["QT_QPA_PLATFORM"] = "offscreen"
        
    app = QApplication(sys.argv)
    setup_global_exception_handler(app)

    # Setup Fake Data
    temp_dir = tempfile.mkdtemp(prefix='ci_test_project_')
    try:
        create_fake_project_data(temp_dir)
        mmtl_path = create_fake_mmtl_file(temp_dir)

        # Setup Window
        main_window = MainWindow()
        main_window.model.load_project(mmtl_path, temp_dir)
        main_window.show()

        # Determine output filename based on OS
        os_name = sys.platform
        output_file = f"screenshot_{os_name}.png"
        
        # Use QTimer to wait for layout to settle (1000ms), then capture
        QTimer.singleShot(1000, lambda: capture_and_exit(main_window, output_file))

        # Run Loop
        exit_code = app.exec()
        
        # Cleanup
        shutil.rmtree(temp_dir, ignore_errors=True)
        sys.exit(exit_code)

    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)