# app/ui/window/main_window.py
# A minimal mock of the MainWindow for testing purposes.

import os
from PySide6.QtWidgets import (
    QMainWindow, 
    QWidget, 
    QVBoxLayout, 
    QLabel, 
    QApplication
)
from PySide6.QtCore import Qt

class MainWindow(QMainWindow):
    """
    A mock MainWindow that provides the necessary interface for Home and
    project_processing to call. It displays basic information to confirm
    that it was launched correctly.
    """
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Main Application - Mock")
        self.setMinimumSize(800, 600)
        self.temp_dir = None

        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        
        self.layout = QVBoxLayout(self.central_widget)
        
        # Initial label
        self.info_label = QLabel("Main Application Window")
        self.info_label.setAlignment(Qt.AlignCenter)
        self.info_label.setStyleSheet("font-size: 24px; font-weight: bold; color: #CCCCCC;")
        
        # Project info labels
        self.project_path_label = QLabel("Project Path: [Not Loaded]")
        self.project_path_label.setStyleSheet("font-size: 14px; color: #AAAAAA;")
        self.project_path_label.setAlignment(Qt.AlignCenter)

        self.temp_dir_label = QLabel("Workspace: [N/A]")
        self.temp_dir_label.setStyleSheet("font-size: 14px; color: #AAAAAA;")
        self.temp_dir_label.setAlignment(Qt.AlignCenter)
        
        self.layout.addWidget(self.info_label)
        self.layout.addWidget(self.project_path_label)
        self.layout.addWidget(self.temp_dir_label)
        self.layout.addStretch()
        
        # Basic styling to match the theme
        self.setStyleSheet("""
            QMainWindow {
                background-color: #1D1D1D;
                color: #FFFFFF;
            }
            QLabel {
                background-color: transparent;
            }
        """)

    def process_mmtl(self, mmtl_path, temp_dir):
        """
        This method is called after the main window is shown.
        It receives the project file path and the temporary extraction directory.
        """
        print(f"Mock MainWindow: Processing '{mmtl_path}'")
        print(f"Mock MainWindow: Using temporary directory '{temp_dir}'")
        
        # Store the temp directory for cleanup
        self.temp_dir = temp_dir
        
        # Update labels to show the received information
        self.project_path_label.setText(f"Project Path: {os.path.basename(mmtl_path)}")
        self.temp_dir_label.setText(f"Workspace: {temp_dir}")

    def closeEvent(self, event):
        """
        Ensure the application quits when this main window is closed.
        """
        print("Mock MainWindow: Closing and quitting application.")
        QApplication.quit()
        event.accept()