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
        # Common Styles
        menu_style = """
            QMenu {
                background-color: #3A3A3A;
                color: #FFFFFF;
                border: 1px solid #555555;
                font-family: "Segoe UI";
            }
            QMenu::item {
                padding: 6px 24px;
                background-color: transparent;
            }
            QMenu::item:selected {
                background-color: #4A4A4A;
            }
            QMenu::separator {
                height: 1px;
                background: #555555;
                margin: 4px 0px;
            }
        """

        # --- 1. HOME (Action) ---
        # Only show Home button if we are in Main Window
        if self.state == TitleBarState.MAIN_WINDOW:
            home_action = QAction("Home", self)
            home_action.triggered.connect(self.go_to_home)
            self.addAction(home_action)

        # --- 2. FILES (Menu) ---
        files_menu = self.addMenu("Files")
        files_menu.setStyleSheet(menu_style)
        
        # Actions
        new_project_action = QAction(qta.icon('fa5s.file-alt', color="white"), "New Project", self)
        new_project_action.setShortcut("Ctrl+N")
        new_project_action.triggered.connect(self.new_project)
        files_menu.addAction(new_project_action)
        
        open_project_action = QAction(qta.icon('fa5s.folder-open', color="white"), "Open Project", self)
        open_project_action.setShortcut("Ctrl+O")
        open_project_action.triggered.connect(self.open_project)
        files_menu.addAction(open_project_action)

        import_wfwf_action = QAction(qta.icon('fa5s.file-import', color="white"), "Import from WFWF", self)
        import_wfwf_action.triggered.connect(self.import_from_wfwf)
        files_menu.addAction(import_wfwf_action)

        files_menu.addSeparator()
        
        save_action = QAction(qta.icon('fa5s.save', color="white"), "Save Project", self)
        save_as_action = QAction(qta.icon('fa5s.download', color="white"), "Save Project As...", self)
        
        files_menu.addAction(save_action)
        files_menu.addAction(save_as_action)
        
        if self.state == TitleBarState.MAIN_WINDOW:
            save_action.setShortcut("Ctrl+S")
            save_action.triggered.connect(self.main_window.save_project)
            
            save_as_action.setShortcut("Ctrl+Shift+S")
            save_as_action.triggered.connect(self.save_project_as)
        else:
            save_action.setEnabled(False)
            save_as_action.setEnabled(False)

        # --- 3. VIEW (Menu) ---
        if self.state == TitleBarState.MAIN_WINDOW:
            view_menu = self.addMenu("View")
            view_menu.setStyleSheet(menu_style)

            # Find/Replace
            find_action = QAction("Find/Replace", self)
            find_action.setShortcut("Ctrl+F")
            find_action.triggered.connect(self.main_window.toggle_find_widget)
            view_menu.addAction(find_action)

            # Toggle Chat
            chat_action = QAction("Translation Chat", self)
            chat_action.setCheckable(True)
            if hasattr(self.main_window, 'translation_chat'):
                chat_action.setChecked(self.main_window.translation_chat.isVisible())
            chat_action.triggered.connect(self.main_window.toggle_chat)
            view_menu.addAction(chat_action)
            
            view_menu.addSeparator()

            # Profiles Submenu
            self.profiles_menu = view_menu.addMenu("Profiles")
            # Populate initially (though it might be empty until project loads)
            self.update_profiles_menu()

            view_menu.addSeparator()

            # Text Visibility
            toggle_text_action = QAction("Toggle Text Visibility", self)
            toggle_text_action.setCheckable(True)
            if hasattr(self.main_window, 'btn_toggle_text'):
                toggle_text_action.setChecked(self.main_window.btn_toggle_text.isChecked())
                # Sync: Menu -> Button
                toggle_text_action.toggled.connect(self.main_window.btn_toggle_text.setChecked)
                # Sync: Button -> Menu
                self.main_window.btn_toggle_text.toggled.connect(toggle_text_action.setChecked)
            view_menu.addAction(toggle_text_action)

            # Inpainting Visibility (Context Fill)
            toggle_inpainting_action = QAction("Toggle Inpainting Visibility", self)
            toggle_inpainting_action.setCheckable(True)
            if hasattr(self.main_window, 'btn_toggle_inpainting'):
                toggle_inpainting_action.setChecked(self.main_window.btn_toggle_inpainting.isChecked())
                 # Sync: Menu -> Button
                toggle_inpainting_action.toggled.connect(self.main_window.btn_toggle_inpainting.setChecked)
                # Sync: Button -> Menu
                self.main_window.btn_toggle_inpainting.toggled.connect(toggle_inpainting_action.setChecked)
            view_menu.addAction(toggle_inpainting_action)

            view_menu.addSeparator()

            # Advanced Mode
            advanced_action = QAction("Advanced Mode", self)
            advanced_action.setCheckable(True)
            # Check state against results_widget if possible, else default false
            if hasattr(self.main_window, 'results_widget'):
                 advanced_action.setChecked(self.main_window.results_widget.is_advanced_mode)
            
            # Connect directly to toggle_advanced_mode
            advanced_action.toggled.connect(self.main_window.toggle_advanced_mode)
            view_menu.addAction(advanced_action)
            
    def update_profiles_menu(self):
        """Updates the Profiles submenu with available profiles."""
        if not hasattr(self, 'profiles_menu') or self.state != TitleBarState.MAIN_WINDOW:
            return
            
        self.profiles_menu.clear()
        
        if not hasattr(self.main_window, 'model'):
            return

        sorted_profiles = sorted([p for p in self.main_window.model.profiles.keys() if p != "Original"])
        sorted_profiles.insert(0, "Original")
        
        active_profile = self.main_window.model.active_profile_name
        
        for profile_name in sorted_profiles:
            action = QAction(profile_name, self)
            action.setCheckable(True)
            action.setChecked(profile_name == active_profile)
            # Use lambda with default arg to capture variable current value
            action.triggered.connect(lambda checked=False, p=profile_name: self.main_window.switch_active_profile(p))
            self.profiles_menu.addAction(action)

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