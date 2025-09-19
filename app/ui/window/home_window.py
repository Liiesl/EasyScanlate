# home_window.py
# Contains the main "Home" window, its components, and related logic.
import os
import zipfile
import tempfile
import time
from shutil import rmtree
import traceback

from PySide6.QtWidgets import (QApplication, QWidget, QVBoxLayout, QPushButton, QFrame, QMainWindow, QLabel, QMessageBox,
                             QScrollArea, QHBoxLayout, QDialog)
from PySide6.QtCore import Qt, QSettings, QDateTime, QThread, Signal, QEvent
from assets.styles import (HOME_STYLES, HOME_LEFT_LAYOUT_STYLES)
from app.ui.window.chrome import CustomTitleBar, WindowResizer
from app.ui.widgets.menu_bar import TitleBarState


class ProjectItemWidget(QFrame):
    """Custom widget for displaying a single project item"""
    def __init__(self, name, path, last_opened="", main_window=None):
        super().__init__()
        self.path = path
        self.main_window = main_window  # Store reference to the main window
        self.setObjectName("projectItem")
        self.setStyleSheet("""
            #projectItem {
                background-color: none;
                border-radius: 0px;
                padding: 10px;
            }
            #projectItem:hover {
                background-color: #3a3a3a;
            }
        """)
        
        # Layout
        self.layout = QHBoxLayout(self)
        self.layout.setContentsMargins(10, 8, 10, 8)
        
        # Project name
        self.name_label = QLabel(name)
        self.name_label.setStyleSheet("font-size: 14px;")
        
        # Last opened (display formatted date instead of path)
        self.last_opened_label = QLabel(last_opened if last_opened else "Never opened")
        self.last_opened_label.setStyleSheet("font-size: 14px; color: #aaaaaa;")
        
        # Add widgets to layout
        self.layout.addWidget(self.name_label)
        self.layout.addStretch()
        self.layout.addWidget(self.last_opened_label)
        
        # Make the widget clickable
        self.setCursor(Qt.PointingHandCursor)
        
    def mouseDoubleClickEvent(self, event):
        if event.button() == Qt.LeftButton:
            window = self.window()
            if isinstance(window, Home):
                window.open_project_from_path(self.path)
        super().mouseDoubleClickEvent(event)

class ProjectsListWidget(QWidget):
    """Custom widget for displaying a list of projects"""
    def __init__(self):
        super().__init__()
        
        # Main layout
        self.layout = QVBoxLayout(self)
        self.layout.setContentsMargins(0, 0, 0, 0)
        self.layout.setSpacing(0)
        
        # Header
        self.header = QWidget()
        self.header.setStyleSheet("padding: 10px; background-color: #3E3E3E; border-top-right-radius: 15px; border-top-left-radius: 15px;")
        self.header_layout = QHBoxLayout(self.header)
        self.header_layout.setContentsMargins(10, 10, 10, 10)
        
        self.name_header = QLabel("Name")
        self.name_header.setStyleSheet("font-weight: bold; color: #cccccc;")
        
        self.last_opened_header = QLabel("Last Opened")
        self.last_opened_header.setStyleSheet("font-weight: bold; color: #cccccc;")
        
        self.header_layout.addWidget(self.name_header)
        self.header_layout.addStretch()
        self.header_layout.addWidget(self.last_opened_header)
        
        self.layout.addWidget(self.header)
        
        # Create scroll area for projects
        self.scroll_area = QScrollArea()
        self.scroll_area.setWidgetResizable(True)
        self.scroll_area.setFrameShape(QFrame.NoFrame)
        self.scroll_area.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        
        # Container for project items
        self.projects_container = QWidget()
        self.projects_container.setStyleSheet("background-color: #2B2B2B;")
        self.projects_layout = QVBoxLayout(self.projects_container)
        self.projects_layout.setContentsMargins(0, 0, 0, 0)
        self.projects_layout.setSpacing(1)
        self.projects_layout.addStretch()
        
        self.scroll_area.setWidget(self.projects_container)
        self.layout.addWidget(self.scroll_area, 1)
        
    def add_project(self, name, path, last_opened=""):
        project_item = ProjectItemWidget(name, path, last_opened)
        self.projects_layout.insertWidget(self.projects_layout.count() - 1, project_item)
        return project_item
    
    def clear(self):
        while self.projects_layout.count() > 1:
            item = self.projects_layout.itemAt(0)
            if item.widget():
                item.widget().deleteLater()
            self.projects_layout.removeItem(item)

class LoadingDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Opening Project...")
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.Dialog)
        self.setWindowModality(Qt.ApplicationModal)
        self.setFixedSize(300, 120)
        
        layout = QVBoxLayout()
        self.title_label = QLabel("Opening Project")
        self.title_label.setAlignment(Qt.AlignCenter)
        self.title_label.setStyleSheet("font-size: 16px; font-weight: bold; padding: 5px;")
        
        self.progress_label = QLabel("Initializing...")
        self.progress_label.setAlignment(Qt.AlignCenter)
        self.progress_label.setWordWrap(True)

        layout.addWidget(self.title_label)
        layout.addWidget(self.progress_label)
        layout.addStretch()

        self.setLayout(layout)
        self.setStyleSheet("""
            QDialog {
                background-color: #1D1D1D;
                color: #FFFFFF;
                border: 1px solid #3E3E3E;
                border-radius: 8px;
            }
            QLabel {
                background-color: transparent;
                color: #CCCCCC;
            }
        """)

    def update_message(self, message):
        self.progress_label.setText(message)
        QApplication.processEvents()

class ProjectLoaderThread(QThread):
    finished = Signal(str, str)
    error = Signal(str)
    progress_update = Signal(str)

    def __init__(self, mmtl_path):
        super().__init__()
        self.mmtl_path = mmtl_path

    def run(self):
        temp_dir = ""
        try:
            self.progress_update.emit("Creating secure temporary workspace...")
            temp_dir = tempfile.mkdtemp()
            time.sleep(0.5)

            self.progress_update.emit(f"Extracting '{os.path.basename(self.mmtl_path)}'...")
            with zipfile.ZipFile(self.mmtl_path, 'r') as zipf:
                zipf.extractall(temp_dir)
            time.sleep(0.5)

            self.progress_update.emit("Verifying project structure...")
            required = ['meta.json', 'master.json', 'images/']
            if not all(os.path.exists(os.path.join(temp_dir, p)) for p in required):
                raise Exception("Invalid .mmtl file structure.")
            time.sleep(0.5)

            self.progress_update.emit("Loading main application...")
            time.sleep(0.7)

            self.finished.emit(self.mmtl_path, temp_dir)
        except Exception as e:
            self.error.emit(str(e))
            if temp_dir and os.path.exists(temp_dir):
                rmtree(temp_dir, ignore_errors=True)

class Home(QMainWindow):
    def __init__(self, progress_signal=None):
        super().__init__()
        self.progress_signal = progress_signal
        self.settings = QSettings("YourCompany", "MangaOCRTool")
        self.setWindowFlags(Qt.FramelessWindowHint)
        self.setAttribute(Qt.WA_TranslucentBackground)
        
        self.init_ui()
        self.resizer = WindowResizer(self)
        
    def init_ui(self):
        def report_progress(message):
            """Helper function to report progress if the signal is available."""
            if self.progress_signal:
                self.progress_signal.emit(message)
                time.sleep(0.15) # Pause to make the message readable

        report_progress("Initializing main window...")
        self.setMinimumSize(800, 600)
        
        self.container = QFrame()
        self.main_layout = QVBoxLayout(self.container)
        self.main_layout.setContentsMargins(1, 1, 1, 1)
        self.main_layout.setSpacing(0)
        
        report_progress("Creating custom title bar...")
        self.title_bar = CustomTitleBar(self)
        self.main_layout.addWidget(self.title_bar)
        self.title_bar.setState(TitleBarState.HOME)

        report_progress("Applying styles...")
        self.setStyleSheet(HOME_STYLES)
        
        self.content_widget = QWidget()
        self.main_layout.addWidget(self.content_widget)
        self.setCentralWidget(self.container)
        
        self.content_layout_hbox = QHBoxLayout(self.content_widget)
        self.content_layout_hbox.setContentsMargins(10, 10, 10, 10)
    
        report_progress("Building action panel...")
        self.left_layout_layout = QVBoxLayout()
        self.left_layout_layout.setContentsMargins(10, 10, 10, 10)
        self.left_layout_layout.setSpacing(15)
        
        self.btn_new = QPushButton("New Project")
        self.btn_import = QPushButton("Import from WFWF")
        self.btn_open = QPushButton("Open Project")
        
        self.left_layout_layout.addWidget(self.btn_new)
        self.left_layout_layout.addWidget(self.btn_import)
        self.left_layout_layout.addWidget(self.btn_open)
        self.left_layout_layout.addStretch()

        self.btn_new.clicked.connect(self.new_project)
        self.btn_open.clicked.connect(self.open_project)
        self.btn_import.clicked.connect(self.import_from_wfwf)

        self.left_layout = QWidget()
        self.left_layout.setLayout(self.left_layout_layout)
        self.left_layout.setMaximumWidth(200)
        self.left_layout.setStyleSheet(HOME_LEFT_LAYOUT_STYLES)
        
        report_progress("Configuring project list...")
        self.content = QWidget()
        self.content_layout = QVBoxLayout(self.content)
        
        self.recent_label = QLabel("Recent Projects")
        self.recent_label.setStyleSheet("font-size: 24px; margin-bottom: 20px;")
        self.content_layout.addWidget(self.recent_label)
        
        self.projects_list = ProjectsListWidget()
        self.content_layout.addWidget(self.projects_list)
        
        report_progress("Assembling final layout...")
        self.content_layout_hbox.addWidget(self.left_layout)
        self.content_layout_hbox.addWidget(self.content, 1)
        
        # REMOVED call to self.load_recent_projects()

    def populate_recent_projects(self, projects_data):
        """Populates the project list from preloaded data."""
        self.projects_list.clear()
        for project in projects_data:
            project_item = self.projects_list.add_project(
                name=project["name"],
                path=project["path"],
                last_opened=project["last_opened"]
            )
            project_item.main_window = self

    def changeEvent(self, event):
        if event.type() == QEvent.WindowStateChange:
            self.title_bar.update_maximize_icon()
        super().changeEvent(event)

    def open_project_from_path(self, path):
        if os.path.exists(path):
            self.launch_main_app(path)
        else:
            QMessageBox.warning(self, "Error", "Project file no longer exists")
            # Refresh the list in-memory if a file is not found.
            # This is a simple way to handle it without re-reading settings.
            all_items = [self.projects_list.projects_layout.itemAt(i).widget() for i in range(self.projects_list.projects_layout.count() - 1)]
            for item in all_items:
                if item and item.path == path:
                    item.deleteLater()

    def new_project(self):
        from app.utils.project_processing import new_project
        new_project(self)

    def open_project(self):
        from app.utils.project_processing import open_project
        open_project(self)

    def import_from_wfwf(self):
        from app.utils.project_processing import import_from_wfwf
        import_from_wfwf(self)

    def correct_filenames(self, directory):
        from app.utils.project_processing import correct_filenames
        return correct_filenames(directory)

    def update_recent_projects(self, project_path):
        recent = self.settings.value("recent_projects", [])
        if project_path in recent:
            recent.remove(project_path)
        recent.insert(0, project_path)
        self.settings.setValue("recent_projects", recent[:10])
        
        timestamps = self.settings.value("recent_timestamps", {})
        current_time = QDateTime.currentDateTime().toString(Qt.ISODate)
        timestamps[project_path] = current_time
        self.settings.setValue("recent_timestamps", timestamps)

    def launch_main_app(self, mmtl_path):
        self.loading_dialog = LoadingDialog(self)
        self.loader_thread = ProjectLoaderThread(mmtl_path)
        
        self.loader_thread.finished.connect(self.handle_project_loaded)
        self.loader_thread.error.connect(self.handle_project_error)
        self.loader_thread.progress_update.connect(self.loading_dialog.update_message)
        
        self.loader_thread.start()
        self.loading_dialog.exec_()

    def handle_project_loaded(self, mmtl_path, temp_dir):
        try:
            from app.ui.window.main_window import MainWindow # Defer heavy import
            self.update_recent_projects(mmtl_path)
            
            self.main_window = MainWindow()
            self.loading_dialog.update_message("Done!")
            self.loading_dialog.accept()

            self.main_window.show()
            self.main_window.process_mmtl(mmtl_path, temp_dir)

            self.hide()
        except Exception as e:
            traceback.print_exc()
            self.loading_dialog.accept()
            QMessageBox.critical(self, "Error", f"Failed to launch project: {str(e)}")
            if temp_dir and os.path.exists(temp_dir):
                rmtree(temp_dir, ignore_errors=True)

    def handle_project_error(self, error_msg):
        self.loading_dialog.accept()
        QMessageBox.critical(self, "Error", f"Failed to open project:\n{error_msg}")
        print(f"Error loading project: {error_msg}")

    def closeEvent(self, event):
        QApplication.quit()
        event.accept()