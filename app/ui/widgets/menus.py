from PySide6.QtWidgets import QWidget, QVBoxLayout, QPushButton
from PySide6.QtCore import Qt
import qtawesome as qta
from assets import MENU_STYLES

class ImportExportMenu(QWidget):
    """
    A custom popup menu for import and export actions.
    It directly triggers actions on the main window.
    """

    def __init__(self, main_window):
        """ The menu now takes the main_window instance to call its methods. """
        super().__init__(main_window)
        self.main_window = main_window
        
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.Popup)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setAttribute(Qt.WA_DeleteOnClose)

        # --- Styling ---
        self.setStyleSheet(MENU_STYLES)

        # --- Layout and Widgets ---
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(1)

        # Import Button - Connects to the main window's action
        btn_import = QPushButton(qta.icon('fa5s.file-import', color='white'), " Import Translation")
        btn_import.clicked.connect(lambda: (self.main_window.import_translation(), self.close()))
        layout.addWidget(btn_import)

        # Export Button - Connects to the new main window wrapper method
        btn_export = QPushButton(qta.icon('fa5s.file-export', color='white'), " Export OCR Results")
        btn_export.clicked.connect(lambda: (self.main_window.export_ocr_results(), self.close()))
        layout.addWidget(btn_export)

        self.setFixedSize(self.sizeHint())


class SaveMenu(QWidget):
    """
    A custom popup menu widget for save actions.
    It directly triggers actions on the main window.
    """
    def __init__(self, main_window):
        """ The menu now takes the main_window instance to call its methods. """
        super().__init__(main_window)
        self.main_window = main_window
        
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.Popup)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setAttribute(Qt.WA_DeleteOnClose)

        # --- Styling ---
        self.setStyleSheet(MENU_STYLES)

        # --- Layout and Widgets ---
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(1)

        # Save Project Button
        btn_save_project = QPushButton(qta.icon('fa5s.save', color='white'), " Save Project (.mmtl)")
        btn_save_project.clicked.connect(lambda: (self.main_window.save_project(), self.close()))
        layout.addWidget(btn_save_project)

        # Save Rendered Images Button
        btn_save_images = QPushButton(qta.icon('fa5s.images', color='white'), " Save Rendered Images")
        btn_save_images.clicked.connect(lambda: (self.main_window.export_manhwa(), self.close()))
        layout.addWidget(btn_save_images)

        self.setFixedSize(self.sizeHint())


class ActionMenu(QWidget):
    """
    A custom popup menu for various image/text actions.
    """
    def __init__(self, main_window):
        """ The menu now takes the main_window instance to call its methods. """
        super().__init__(main_window)
        self.main_window = main_window
        
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.Popup)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setAttribute(Qt.WA_DeleteOnClose)

        # --- Styling ---
        self.setStyleSheet(MENU_STYLES)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(1)

        # Action Buttons
        btn_hide_text = QPushButton(qta.icon('fa5s.eye-slash', color='white'), " Hide Text")
        btn_hide_text.clicked.connect(lambda: (self.main_window.hide_text(), self.close()))
        layout.addWidget(btn_hide_text)

        btn_split_images = QPushButton(qta.icon('fa5s.object-ungroup', color='white'), " Split Images")
        btn_split_images.clicked.connect(lambda: (self.main_window.split_images(), self.close()))
        layout.addWidget(btn_split_images)
        
        btn_stitch_images = QPushButton(qta.icon('fa5s.object-group', color='white'), " Stitch Images")
        btn_stitch_images.clicked.connect(lambda: (self.main_window.stitch_images(), self.close()))
        layout.addWidget(btn_stitch_images)

        # Placeholders
        btn_hide_text.setEnabled(False)
        btn_split_images.setEnabled(True)
        btn_stitch_images.setEnabled(True)

        self.setFixedSize(self.sizeHint())