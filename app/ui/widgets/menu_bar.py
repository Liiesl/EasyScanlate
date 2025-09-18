from PySide6.QtWidgets import QMenuBar, QFileDialog
from PySide6.QtGui import QAction 
import qtawesome as qta
from enum import Enum, auto

# ADDED: State definition for the title bar/menu bar behavior
class TitleBarState(Enum):
    MAIN_WINDOW = auto() # For the main editor window with a project loaded
    HOME = auto()        # For the home/welcome screen
    NON_MAIN = auto()    # For dialogs, settings, etc. that shouldn't have a menu bar

class MenuBar(QMenuBar):
    # MODIFIED: __init__ now accepts a state to control its contents, defaulting to HOME.
    def __init__(self, parent, state=TitleBarState.HOME):
        super().__init__(parent)
        self.main_window = parent  # Reference to the parent window (e.g., MainWindow, Home)
        self.state = state         # Store the current state
        self.setStyleSheet("""
            QMenuBar {
                background-color: transparent;
            }
            QMenuBar::item {
                background-color: transparent;
                padding: 8px 16px;
                margin: 0px 2px;
                border-radius: 4px;
                color: white;
            }
            QMenuBar::item:selected {
                background-color: #4A4A4A;
                color: #FFFFFF;
            }
        """)                   
        
        # Only create menu bar contents if the state is not NON_MAIN.
        # This is a safe guard; the parent (CustomTitleBar) should already handle this.
        if self.state != TitleBarState.NON_MAIN:
            self.create_menu_bar()
        # Add other menus here
        
    def create_menu_bar(self):
        # File menu
        file_menu = self.addMenu("File")
        file_menu.setStyleSheet("""
            QMenu {
                background-color: #3A3A3A;
                color: #FFFFFF;
                border: none;
                border-radius: 4px;
            }
            QMenu::item {
                padding: 8px 16px;
                border-radius: 4px;
            }
            QMenu::item:selected {
                background-color: #4A4A4A;
            }
        """)
        
        file_menu_action = file_menu.menuAction()
        file_menu_action.setIcon(qta.icon('fa5s.file', color="white"))

        # --- Actions available in all states (HOME and MAIN_WINDOW) ---
        new_project_action = QAction(qta.icon('fa5s.file-alt', color="white"), "New Project", self)
        new_project_action.setShortcut("Ctrl+N")
        new_project_action.triggered.connect(self.new_project)
        file_menu.addAction(new_project_action)
        
        open_project_action = QAction(qta.icon('fa5s.folder-open', color="white"), "Open Project", self)
        open_project_action.setShortcut("Ctrl+O")
        open_project_action.triggered.connect(self.open_project)
        file_menu.addAction(open_project_action)

        import_wfwf_action = QAction(qta.icon('fa5s.file-import', color="white"), "Import from WFWF", self)
        import_wfwf_action.triggered.connect(self.import_from_wfwf)
        file_menu.addAction(import_wfwf_action)

        file_menu.addSeparator()
        
        # --- Create actions that depend on the state ---
        # These actions require an active project/main window context.
        save_action = QAction(qta.icon('fa5s.save', color="white"), "Save Project", self)
        save_as_action = QAction(qta.icon('fa5s.download', color="white"), "Save Project As...", self)
        
        file_menu.addAction(save_action)
        file_menu.addAction(save_as_action)
        
        file_menu.addSeparator()
        
        home_action = QAction(qta.icon('fa5s.home', color="white"), "Go to Home", self)
        file_menu.addAction(home_action)

        # --- Configure state-dependent actions ---
        if self.state == TitleBarState.MAIN_WINDOW:
            # In MAIN_WINDOW state, actions are fully enabled and connected.
            save_action.setShortcut("Ctrl+S")
            save_action.triggered.connect(self.main_window.save_project)
            
            save_as_action.setShortcut("Ctrl+Shift+S")
            save_as_action.triggered.connect(self.save_project_as)
            
            home_action.triggered.connect(self.go_to_home)
        else:
            # In HOME state, these actions are not applicable and are disabled.
            # This prevents calling methods that don't exist on the Home window.
            save_action.setEnabled(False)
            save_as_action.setEnabled(False)
            home_action.setEnabled(False)

    def new_project(self):
        from utils.project_processing import new_project
        new_project(self)

    def open_project(self):
        from utils.project_processing import open_project
        open_project(self)

    def import_from_wfwf(self):
        from utils.project_processing import import_from_wfwf
        import_from_wfwf(self)

    def correct_filenames(self, directory):
        from utils.project_processing import correct_filenames
        return correct_filenames(directory)

    def go_to_home(self):
        from main import Home
        self.home = Home()
        self.home.show()
        self.main_window.close()
    
    def save_project_as(self):
        """Handle Save As functionality"""
        options = QFileDialog.Options()
        file_path, _ = QFileDialog.getSaveFileName(
            self, 
            "Save Project As", 
            "", 
            "Manga Translation Project (*.mmtl)", 
            options=options
        )
        
        if file_path:
            if not file_path.endswith('.mmtl'):
                file_path += '.mmtl'
            self.main_window.mmtl_path = file_path
            self.main_window.save_project()  # Reuse existing save logic with new path