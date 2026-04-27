from PySide6.QtWidgets import QMenuBar, QFileDialog
from PySide6.QtGui import QAction, QIcon
import qtawesome as qta
from enum import Enum, auto

# ADDED: State definition for the title bar/menu bar behavior
class TitleBarState(Enum):
    MAIN_WINDOW = auto() # For the main editor window with a project loaded
    HOME = auto()        # For the home/welcome screen
    NON_MAIN = auto()    # For dialogs, settings, etc. that shouldn't have a menu bar

class MenuBar(QMenuBar):
    def __init__(self, parent=None, state=TitleBarState.HOME,
                 app_viewmodel=None,
                 on_context_fill_start=None, on_context_fill_edit_toggled=None,
                 is_context_fill_edit_active=None,
                 on_split_clicked=None, on_stitch_clicked=None,
                 on_toggle_text_visibility=None, on_toggle_inpainting_visibility=None,
                 model=None):
        super().__init__(parent)
        self.state = state
        self.app_vm = app_viewmodel
        self.on_context_fill_start = on_context_fill_start
        self.on_context_fill_edit_toggled = on_context_fill_edit_toggled
        self.is_context_fill_edit_active = is_context_fill_edit_active
        self.on_split_clicked = on_split_clicked
        self.on_stitch_clicked = on_stitch_clicked
        self.on_toggle_text_visibility = on_toggle_text_visibility
        self.on_toggle_inpainting_visibility = on_toggle_inpainting_visibility
        self.model = model
        self._parent_window = parent
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

        if self.state != TitleBarState.NON_MAIN:
            self.create_menu_bar()
        
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

        # Import/Export (Parity with existing button)
        if self.state == TitleBarState.MAIN_WINDOW:
            import_trans_action = QAction(qta.icon('fa5s.file-import', color="white"), "Import Translation", self)
            if self.app_vm:
                import_trans_action.triggered.connect(self.app_vm.import_translation)
            files_menu.addAction(import_trans_action)

            export_ocr_action = QAction(qta.icon('fa5s.file-export', color="white"), "Export OCR Results", self)
            if self.app_vm:
                export_ocr_action.triggered.connect(self.app_vm.export_ocr_results)
            files_menu.addAction(export_ocr_action)
            files_menu.addSeparator()

        save_action = QAction(qta.icon('fa5s.save', color="white"), "Save Project", self)
        save_as_action = QAction(qta.icon('fa5s.download', color="white"), "Save Project As...", self)

        files_menu.addAction(save_action)
        files_menu.addAction(save_as_action)

        if self.state == TitleBarState.MAIN_WINDOW:
            save_action.setShortcut("Ctrl+S")
            if self.app_vm:
                save_action.triggered.connect(self.app_vm.save_project)

            save_as_action.setShortcut("Ctrl+Shift+S")
            if self.app_vm:
                save_as_action.triggered.connect(self.save_project_as)
        else:
            save_action.setEnabled(False)
            save_as_action.setEnabled(False)

        # --- 3. EDIT (Menu) ---
        if self.state == TitleBarState.MAIN_WINDOW:
            edit_menu = self.addMenu("Edit")
            edit_menu.setStyleSheet(menu_style)

            # Find/Replace (Parity with Toolbar/Ctrl+F)
            find_action = QAction("Find/Replace", self)
            if self.app_vm:
                find_action.triggered.connect(self.app_vm.toggle_find_widget)
            edit_menu.addAction(find_action)

            # Note: Undo/Redo/Select All are skipped as they don't exist in the current backend.

        # --- 4. PROCESS (Menu) ---
        if self.state == TitleBarState.MAIN_WINDOW:
            process_menu = self.addMenu("Process")
            process_menu.setStyleSheet(menu_style)

            # OCR Process
            ocr_action = QAction(qta.icon('fa5s.magic', color='white'), "Start/Stop OCR", self)
            if self.app_vm:
                ocr_action.triggered.connect(self.app_vm.toggle_ocr)
            process_menu.addAction(ocr_action)

            process_menu.addSeparator()

            # Manual OCR Mode
            self._manual_mode_action = QAction(QIcon("assets/icons/manual_ocr.svg"), "Manual OCR Mode", self)
            self._manual_mode_action.setCheckable(True)
            self._manual_mode_action.toggled.connect(self._on_manual_ocr_toggled)
            if self.app_vm:
                self.app_vm.image_area_vm.manual_ocr_mode_active_changed.connect(
                    self._on_manual_ocr_mode_active_changed
                )
            process_menu.addAction(self._manual_mode_action)

            # Context Fill Mode
            context_fill_action = QAction(qta.icon('fa5s.fill-drip', color="white"), "Context Fill Mode", self)
            if self.on_context_fill_start:
                 context_fill_action.triggered.connect(self.on_context_fill_start)
            process_menu.addAction(context_fill_action)

            # Edit Context Fill
            edit_context_action = QAction(qta.icon('fa5s.paint-brush', color="white"), "Edit Context Fills", self)
            edit_context_action.setCheckable(True)
            if self.is_context_fill_edit_active:
                edit_context_action.setChecked(self.is_context_fill_edit_active())
            if self.on_context_fill_edit_toggled:
                edit_context_action.toggled.connect(self.on_context_fill_edit_toggled)
            process_menu.addAction(edit_context_action)

            process_menu.addSeparator()

            # Split Images
            split_action = QAction(QIcon("assets/icons/split.svg"), "Split Images", self)
            if self.on_split_clicked:
                 split_action.triggered.connect(self.on_split_clicked)
            process_menu.addAction(split_action)

            # Stitch Images
            stitch_action = QAction(QIcon("assets/icons/stitch.svg"), "Stitch Images", self)
            if self.on_stitch_clicked:
                 stitch_action.triggered.connect(self.on_stitch_clicked)
            process_menu.addAction(stitch_action)


        # --- 5. VIEW (Menu) ---
        if self.state == TitleBarState.MAIN_WINDOW:
            view_menu = self.addMenu("View")
            view_menu.setStyleSheet(menu_style)

            # Toggle Chat


            # Profiles Submenu
            self.profiles_menu = view_menu.addMenu("Profiles")
            # Populate initially (though it might be empty until project loads)
            self.update_profiles_menu()

            view_menu.addSeparator()

            # Text Visibility
            toggle_text_action = QAction("Toggle Text Visibility", self)
            toggle_text_action.setCheckable(True)
            if self.on_toggle_text_visibility:
                toggle_text_action.triggered.connect(self.on_toggle_text_visibility)
            view_menu.addAction(toggle_text_action)
            self._toggle_text_action = toggle_text_action

            # Inpainting Visibility (Context Fill) - now in Context Fill Menu
            toggle_inpainting_action = QAction("Toggle Inpainting Visibility", self)
            if self.on_toggle_inpainting_visibility:
                toggle_inpainting_action.triggered.connect(self.on_toggle_inpainting_visibility)
            view_menu.addAction(toggle_inpainting_action)

            view_menu.addSeparator()

            # Panel Layout Toggle
            panel_layout_action = QAction("Translation Panel: Bottom", self)
            panel_layout_action.setCheckable(True)
            panel_layout_action.setChecked(True)
            if self.app_vm:
                panel_layout_action.triggered.connect(self.app_vm.toggle_panel_layout)
            view_menu.addAction(panel_layout_action)
            self._panel_layout_action = panel_layout_action

            # Advanced Mode
            advanced_action = QAction("(Legacy) Advanced Mode", self)
            advanced_action.setCheckable(True)
            advanced_action.setEnabled(False)
            view_menu.addAction(advanced_action)
            
    def update_profiles_menu(self):
        """Updates the Profiles submenu with available profiles."""
        if not hasattr(self, 'profiles_menu') or self.state != TitleBarState.MAIN_WINDOW:
            return

        self.profiles_menu.clear()

        if self.app_vm is None:
            return

        profiles = self.app_vm.get_profiles()
        if not profiles:
            return

        sorted_profiles = sorted([p for p in profiles if p != "Original"])
        sorted_profiles.insert(0, "Original")

        active_profile = self.app_vm.get_active_profile()

        for profile_name in sorted_profiles:
            action = QAction(profile_name, self)
            action.setCheckable(True)
            action.setChecked(profile_name == active_profile)
            # Use lambda with default arg to capture variable current value
            action.triggered.connect(lambda checked=False, p=profile_name: self.app_vm.switch_profile(p))
            self.profiles_menu.addAction(action)

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

    def go_to_home(self):
        from app.ui.window.home_window import Home
        self.home = Home()
        self.home.load_recent_projects_from_settings()
        self.home.show()
        if self._parent_window:
            self._parent_window.close()

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
            if self.app_vm:
                self.app_vm.save_project_as(file_path)

    def _on_manual_ocr_toggled(self, checked):
        """User toggled the menu action; delegate to VM."""
        if self.app_vm:
            if checked:
                self.app_vm.image_area_vm.start_action_mode("manual_ocr")
            else:
                self.app_vm.image_area_vm.cancel_action_mode()

    def _on_manual_ocr_mode_active_changed(self, active):
        """VM state changed; sync the menu action without re-emitting toggled."""
        if hasattr(self, '_manual_mode_action'):
            self._manual_mode_action.blockSignals(True)
            self._manual_mode_action.setChecked(active)
            self._manual_mode_action.blockSignals(False)