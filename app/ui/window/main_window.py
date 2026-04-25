# main_window.py - ocr functionality disabled

from PySide6.QtWidgets import (QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QSizePolicy, QCheckBox, QPushButton,
                             QMessageBox, QSplitter, QComboBox)
import traceback
import sys
import json
from app.ui.dialogs.error_dialog import ErrorDialog
from PySide6.QtCore import Qt, QSettings, QPoint, QRectF, QEvent
from PySide6.QtGui import QPixmap, QKeySequence, QAction, QColor, QIcon
import qtawesome as qta
from app.utils.file_io import export_ocr_results, import_translation_file, export_rendered_images
from app.ui.components.image_area.label import ResizableImageLabel
from app.ui.components.image_area.scroll_container import CustomScrollArea
from app.ui.components.translation_panel import TranslationPanel
from app.ui.components.textbox_style.panel import TextBoxStylePanel
from app.ui.widgets.menu_bar import MenuBar, TitleBarState
from app.ui.window.chrome import CustomTitleBar, WindowResizer
from app.ui.widgets.progress_bar import CustomProgressBar
from app.ui.widgets.menus import Menu, ToggleButton, ToggleWithProgress
from app.handlers.ocr_batch_handler import BatchOCRHandler
from app.core.project_model import ProjectModel
from app.viewmodels import AppViewModel
from app.ui.dialogs.settings_dialog import SettingsDialog
from app.ui.components.background import AuroraCanvas
from assets import (DEFAULT_TEXT_STYLE, get_style_diff, RIGHT_PANEL_STYLES, UNIVERSAL_STYLES)
import os, gc, json, traceback
from app.core.rapid_ocr_engine import RapidOCREngine

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Easy Scanlate")
        self.setGeometry(100, 100, 1200, 600)
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self._load_filter_settings()
        
        self.model = ProjectModel()
        self.model.project_loaded.connect(self.on_project_loaded)
        self.model.project_load_failed.connect(self.on_project_load_failed)
        self.model.model_updated.connect(self.on_model_updated)
        self.model.profiles_updated.connect(self._on_profile_list_changed)
        self.model.profile_created_for_user_edit.connect(self._on_profile_created_for_user_edit)

        self.app_vm = AppViewModel(self.model, self)
        self.editor_vm = self.app_vm.editor_vm

        self.find_action = QAction("Find/Replace", self)
        self.find_action.triggered.connect(self.toggle_find_widget)
        self.addAction(self.find_action)
        self.update_shortcut()

        self.language_map = { "Korean": "ko", "Chinese": "ch_sim", "Japanese": "ja" }

        self.init_ui()

        self.reader = None 
        self.ocr_processor = None
        
        if hasattr(self, 'style_panel'):
             self.style_panel.style_changed.connect(self.update_text_box_style)
        
        self.batch_handler = None
        self._panel_layout_vertical = True  # Track layout state (True = vertical/bottom, False = horizontal/right)
    
    def _load_filter_settings(self):
        self.min_text_height = int(self.settings.value("min_text_height", 40))
        self.max_text_height = int(self.settings.value("max_text_height", 100))
        self.min_confidence = float(self.settings.value("min_confidence", 0.2))
        self.distance_threshold = int(self.settings.value("distance_threshold", 100))
        print(f"Loaded settings: MinH={self.min_text_height}, MaxH={self.max_text_height}, MinConf={self.min_confidence}, DistThr={self.distance_threshold}")

    def init_ui(self):
        # Window Setup
        self.setWindowFlags(Qt.FramelessWindowHint)

        # Root Container
        self.background_canvas = AuroraCanvas()
        root_layout = QVBoxLayout(self.background_canvas)
        root_layout.setContentsMargins(0, 0, 0, 0)
        root_layout.setSpacing(0)

        self.title_bar = CustomTitleBar(self)
        root_layout.addWidget(self.title_bar)
        
        # Main Content Widget
        main_widget = QWidget()
        main_widget.setObjectName("CentralWidget")
        main_widget.setStyleSheet(UNIVERSAL_STYLES)
        main_layout = QHBoxLayout(main_widget)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)

        root_layout.addWidget(main_widget)
        self.setCentralWidget(self.background_canvas)

        self.scroll_area = CustomScrollArea(
            model=self.model,
            editor_viewmodel=self.editor_vm,
            image_area_viewmodel=self.app_vm.image_area_vm,
            on_initialize_reader=self._initialize_ocr_reader,
            on_save_project=self.on_save_project_triggered,
            on_export_manhwa=self.export_manhwa,
            get_display_text=self.get_display_text,
            on_text_edited=self.update_ocr_text,
            get_reader=lambda: self.reader,
            get_settings=lambda: self.settings,
            on_manual_ocr_cancelled=self._on_manual_ocr_cancelled,
            parent=self
        )
        
        # Create vertical toolbar (VS Code style)
        self.vertical_toolbar = QWidget()
        self.vertical_toolbar.setObjectName("VerticalToolBar")
        self.vertical_toolbar.setFixedWidth(50)  # Fixed width like VS Code
        vertical_toolbar_layout = QVBoxLayout(self.vertical_toolbar)
        vertical_toolbar_layout.setContentsMargins(5, 10, 5, 10)
        vertical_toolbar_layout.setSpacing(10)
        
        # Settings button (moved from settings_layout)
        self.btn_settings = QPushButton(qta.icon('fa5s.cog', color='white'), "")
        self.btn_settings.setFixedSize(40, 40)
        self.btn_settings.setToolTip("Settings")
        self.btn_settings.clicked.connect(self.show_settings_dialog)
        vertical_toolbar_layout.addWidget(self.btn_settings)
        
        # Spacer after settings for visual separation
        vertical_toolbar_layout.addSpacing(10)
        
        # Manual OCR button (moved from button_layout)
        self.btn_manual_ocr = QPushButton(QIcon("assets/icons/manual_ocr.svg"), "")
        self.btn_manual_ocr.setFixedSize(40, 40)
        self.btn_manual_ocr.setToolTip("Manual OCR Mode")
        self.btn_manual_ocr.setCheckable(True)
        self.btn_manual_ocr.toggled.connect(self.scroll_area.manual_ocr_handler.toggle_mode)
        self.btn_manual_ocr.setEnabled(False)  # Keep original enabled state
        vertical_toolbar_layout.addWidget(self.btn_manual_ocr)

        # --- NEW ACTION BUTTONS ---

        # Toggle Text Visibility
        self.btn_toggle_text = ToggleButton(
            off_text="", on_text="",
            off_icon=qta.icon('fa5s.eye', color='white'),
            on_icon=qta.icon('fa5s.eye-slash', color='white')
        )
        self.btn_toggle_text.setFixedSize(40, 40)
        self.btn_toggle_text.setToolTip("Toggle Text Visibility")
        self.btn_toggle_text.clicked.connect(self.app_vm.image_area_vm.toggle_text_visibility)
        self.btn_toggle_text.setState(False)
        vertical_toolbar_layout.addWidget(self.btn_toggle_text)

        # Toggle Inpainting - now part of Context Fill Menu
        # Context Fill Menu Button (replaces btn_context_fill, btn_edit_context_fill, btn_toggle_inpainting)
        self.btn_context_fill_menu = QPushButton(qta.icon('fa5s.fill-drip', color='white'), "")
        self.btn_context_fill_menu.setFixedSize(40, 40)
        self.btn_context_fill_menu.setToolTip("Context Fill Options")
        self.btn_context_fill_menu.clicked.connect(self.show_context_fill_menu)
        vertical_toolbar_layout.addWidget(self.btn_context_fill_menu)

        # Split Images
        self.btn_split = QPushButton(QIcon("assets/icons/split.svg"), "")
        self.btn_split.setFixedSize(40, 40)
        self.btn_split.setToolTip("Split Images")
        self.btn_split.clicked.connect(self.scroll_area.split_handler.start_splitting_mode)
        vertical_toolbar_layout.addWidget(self.btn_split)

        # Stitch Images
        self.btn_stitch = QPushButton(QIcon("assets/icons/stitch.svg"), "")
        self.btn_stitch.setFixedSize(40, 40)
        self.btn_stitch.setToolTip("Stitch Images")
        self.btn_stitch.clicked.connect(self.scroll_area.stitch_handler.start_stitching_mode)
        vertical_toolbar_layout.addWidget(self.btn_stitch)

        # Add stretch to push buttons to top
        vertical_toolbar_layout.addStretch()

        # Rest of the UI setup continues...
        left_panel = QVBoxLayout()
        left_panel.setContentsMargins(10, 10, 5, 10)
        left_panel.setSpacing(20)
        
        left_panel.addWidget(self.scroll_area)

        # Right Panel
        right_panel = QVBoxLayout()
        right_panel.padding = 30
        right_panel.setContentsMargins(5, 10, 10, 10)
        right_panel.setSpacing(20)

        button_layout = QHBoxLayout()
        
        # ToggleWithProgress Button
        self.btn_ocr_toggle = ToggleWithProgress(
            start_text="Detect Text", 
            stop_text="Stop detecting",
            start_icon=qta.icon('fa5s.magic', color='white'),
            stop_icon=qta.icon('fa5s.stop', color='white'),
            parent=self
        )
        self.btn_ocr_toggle.clicked.connect(self.toggle_ocr)
        self.btn_ocr_toggle.setEnabled(False) # Disabled until project loaded
        button_layout.addWidget(self.btn_ocr_toggle)
        button_layout.addStretch()

        # Orientation dropdown for right splitter
        self.orientation_combo = QComboBox()
        self.orientation_combo.addItems(["Bottom", "Right"])
        self.orientation_combo.setFixedWidth(80)
        self.orientation_combo.currentTextChanged.connect(self._on_orientation_changed)
        self.orientation_combo.setEnabled(False)  # Enable when project loaded

        # Progress Controller (Hidden Logic)
        self.progress_controller = CustomProgressBar()
        self.progress_controller.setVisible(False)
        self.progress_controller.valueChanged.connect(self.btn_ocr_toggle.setValue)
        # Assuming 0-100 range, we can also sync max if needed, but CustomProgressBar defaults to 100
        self.btn_ocr_toggle.setMaximum(100)
        
        file_button_layout = QHBoxLayout()
        file_button_layout.setAlignment(Qt.AlignRight)
        file_button_layout.setSpacing(20)

        file_button_layout.addWidget(self.orientation_combo)
        file_button_layout.addSpacing(10)

        self.btn_import_export_menu = QPushButton(qta.icon('fa5s.bars', color='white'), "")
        self.btn_import_export_menu.setFixedWidth(60)
        self.btn_import_export_menu.setToolTip("Open Import/Export Menu")
        self.btn_import_export_menu.clicked.connect(self.show_import_export_menu)
        file_button_layout.addWidget(self.btn_import_export_menu)
        button_layout.addLayout(file_button_layout)
        right_panel.addLayout(button_layout)

        # Style panel - always visible above results with resizable splitter
        self.style_panel = TextBoxStylePanel(default_style=DEFAULT_TEXT_STYLE, editor_viewmodel=self.editor_vm)
        self.style_panel.setMinimumHeight(70)
        self.style_panel.setMaximumHeight(480)

        # Create unified translation panel (replaces ResultsWidget + TranslationChatWidget)
        self.translation_panel = TranslationPanel(source_language=self.model.original_language, editor_viewmodel=self.editor_vm)
        self.translation_panel.text_changed.connect(self.update_ocr_text)
        self.translation_panel.translation_complete.connect(self.handle_translation_completed)
        self.translation_panel.profile_changed.connect(self.app_vm.switch_profile)
        
        # Create vertical splitter for resizable layout
        right_splitter = QSplitter(Qt.Vertical)
        right_splitter.addWidget(self.style_panel)
        right_splitter.addWidget(self.translation_panel)
        right_splitter.setStretchFactor(0, 0)
        right_splitter.setStretchFactor(1, 1)
        right_splitter.setHandleWidth(10)
        
        # Find/replace widget (temporarily disabled)
        # self.find_replace_widget = FindReplaceWidget(self)
        # right_panel.addWidget(self.find_replace_widget)
        # self.find_replace_widget.hide()
        self.style_panel_size = None
        
        right_panel.addWidget(right_splitter, 1)

        right_widget = QWidget()
        right_widget.setObjectName("RightWidget")
        right_widget.setLayout(right_panel)

        # === APPLY STYLES ===
        for w in [self.style_panel, self.translation_panel]:
            w.setObjectName("TransparentPanel")
            w.setAttribute(Qt.WA_StyledBackground, True)
        
        # Apply the stylesheet to the parent widget so children can inherit or use the ID selector
        right_widget.setStyleSheet(RIGHT_PANEL_STYLES + UNIVERSAL_STYLES)

        splitter = QSplitter(Qt.Horizontal)
        left_widget = QWidget()
        left_widget.setLayout(left_panel)
        splitter.addWidget(left_widget)
        splitter.addWidget(right_widget)
        splitter.setSizes([250, 450])

        # MODIFIED: Add toolbar and splitter to main layout
        main_layout.addWidget(self.vertical_toolbar)  # Add vertical toolbar first
        main_layout.addWidget(splitter)  # Then the main content splitter

        # Connect Window Resizer
        self.resizer = WindowResizer(self)

        # Initialize Title Bar State (needs all other widgets to be created first)
        self.menu_bar = MenuBar(
            parent=self,
            state=TitleBarState.MAIN_WINDOW,
            app_viewmodel=self.app_vm,
            on_context_fill_start=self.scroll_area.context_fill_handler.start_mode,
            on_context_fill_edit_toggled=self.scroll_area.context_fill_handler.toggle_edit_mode,
            is_context_fill_edit_active=lambda: getattr(self.scroll_area.context_fill_handler, 'is_edit_mode_active', False),
            on_split_clicked=self.btn_split.click,
            on_stitch_clicked=self.btn_stitch.click,
            on_toggle_text_visibility=self.app_vm.image_area_vm.toggle_text_visibility,
            on_toggle_inpainting_visibility=self.app_vm.image_area_vm.toggle_inpaint_visibility,
            get_is_manual_ocr_checked=lambda: self.btn_manual_ocr.isChecked(),
            on_manual_ocr_toggled=self.btn_manual_ocr.setChecked,
            model=self.model
        )
        # VM-driven sync for text visibility UI state
        self.app_vm.image_area_vm.text_visible_changed.connect(self._on_text_visibility_changed)
        self.title_bar.setState(TitleBarState.MAIN_WINDOW, self.menu_bar)

        # Connect AppViewModel signals
        self.app_vm.profile_switched.connect(self._on_profile_switched)
        self.app_vm.project_saved.connect(self.on_project_saved)
        self.app_vm.ocr_toggled.connect(self.toggle_ocr)
        self.app_vm.panel_layout_toggled.connect(self.toggle_panel_layout)
        self.app_vm.find_widget_toggled.connect(self.toggle_find_widget)
        self.app_vm.import_translation_requested.connect(self.import_translation)
        self.app_vm.export_ocr_results_requested.connect(self.export_ocr_results)

    def nativeEvent(self, eventType, message):
        # Use getattr to safely check if resizer exists and is fully initialized
        resizer = getattr(self, 'resizer', None)
        if resizer:
            handled, result = resizer.handle_windows_native(message)
            if handled:
                return True, result
                
        return super().nativeEvent(eventType, message)

    def changeEvent(self, event):
        if event.type() == QEvent.WindowStateChange:
            if hasattr(self, 'title_bar'):
                self.title_bar.update_maximize_icon()
        super().changeEvent(event)

    def show_import_export_menu(self):
        """Creates, populates, and shows the Import/Export menu."""
        menu = Menu(self)
        
        btn_import = QPushButton(qta.icon('fa5s.file-import', color='white'), " Import Translation")
        btn_import.clicked.connect(self.import_translation)
        menu.addButton(btn_import)

        btn_export = QPushButton(qta.icon('fa5s.file-export', color='white'), " Export OCR Results")
        btn_export.clicked.connect(self.export_ocr_results)
        menu.addButton(btn_export)

        menu.set_position_and_show(self.btn_import_export_menu, 'bottom right')

    def show_context_fill_menu(self):
        """Creates, populates, and shows the Context Fill menu."""
        menu = Menu(self)

        btn_context_fill_mode = QPushButton(qta.icon('fa5s.fill-drip', color='white'), " Context Fill Mode")
        btn_context_fill_mode.clicked.connect(self.scroll_area.context_fill_handler.start_mode)
        menu.addButton(btn_context_fill_mode)

        btn_edit_context_fill = QPushButton(qta.icon('fa5s.paint-brush', color='white'), " Edit Context Fills")
        btn_edit_context_fill.clicked.connect(self.scroll_area.context_fill_handler.toggle_edit_mode)
        menu.addButton(btn_edit_context_fill)

        btn_toggle_fill_visibility = ToggleButton(
            off_text=" Show Fills", on_text=" Hide Fills",
            off_icon=qta.icon('fa5s.eye', color='white'),
            on_icon=qta.icon('fa5s.eye-slash', color='white')
        )
        btn_toggle_fill_visibility.setToolTip("Toggle Fill Visibility")
        btn_toggle_fill_visibility.setState(self.app_vm.image_area_vm.inpaints_visible)
        btn_toggle_fill_visibility.clicked.connect(self.app_vm.image_area_vm.toggle_inpaint_visibility)
        menu.addButton(btn_toggle_fill_visibility, close_on_click=False)

        menu.set_position_and_show(self.btn_context_fill_menu, 'right')

    def update_profile_selector(self):
        """Syncs profile UIs (menu bar and translation panel) with the model."""
        # Sync Menu Bar profiles if available
        if hasattr(self, 'title_bar') and hasattr(self.title_bar, 'menu_bar'):
             self.title_bar.menu_bar.update_profiles_menu()

        # Also update translation panel profiles
        self._update_translation_panel_data()

    def _on_profile_switched(self, profile_name):
        """React to AppViewModel profile switch.

        Phase 3 TODO: Profile switching moves into TranslationViewModel.
        This method (and the manual label refresh) will be replaced by a
        reactive VM -> View binding. Do not add more logic here.
        """
        self._on_profile_changed()
        self.scroll_area.refresh_all_labels()

    def _on_profile_changed(self):
        """Handles profile changes by notifying find widget."""
        if hasattr(self, 'find_replace_widget'):
            self.find_replace_widget.on_profile_changed()

    def _on_profile_list_changed(self):
        """Handles profile list changes (additions/deletions)."""
        if hasattr(self, 'find_replace_widget'):
            self.find_replace_widget.on_profile_changed()

    def show_settings_dialog(self):
        dialog = SettingsDialog(self)
        if dialog.exec():
            self._load_filter_settings()
            self.update_shortcut()

    def toggle_find_widget(self):
        pass  # Find/replace disabled

    def update_find_shortcut(self):
        shortcut = self.settings.value("find_shortcut", "Ctrl+F")
        self.find_action.setShortcut(QKeySequence(shortcut))
        print(f"Find shortcut set to: {shortcut}")

    def process_mmtl(self, mmtl_path, temp_dir):
        self.model.load_project(mmtl_path, temp_dir)

    def on_project_load_failed(self, error_msg):
        ErrorDialog.critical(self, "Project Load Error", error_msg)
        self.close()

    def on_project_loaded(self):
        """ Populates the UI after the model has loaded a project. """
        self.scroll_area.cancel_active_modes()

        # Re-initialize OCR reader if language changed
        if self.reader and self.reader.language != self.model.original_language:
            print(f"Re-initializing OCR reader for language: {self.model.original_language}")
            self.reader = RapidOCREngine(language=self.model.original_language)

        image_paths = self.model.image_paths
        self.setWindowTitle(f"{self.model.project_name} | ManhwaOCR")
        self.btn_ocr_toggle.setEnabled(bool(image_paths))
        self.btn_manual_ocr.setEnabled(bool(image_paths))
        self.orientation_combo.setEnabled(bool(image_paths))

        if not image_paths:
            QMessageBox.warning(self, "No Images", "The project was loaded, but no images were found inside.")

        # ImageAreaViewModel auto-syncs images from model.image_list_changed;
        # CustomScrollArea reactively rebuilds labels from images_changed.
        self.update_profile_selector()
        self._update_translation_panel_data()
        print(f"Project '{self.model.project_name}' loaded and UI populated.")
    
    def handle_inpaint_record_deleted(self, record_id):
        """Delegates the inpaint record deletion request to the model."""
        self.model.remove_inpaint_record(record_id)
    
    def on_model_updated(self, affected_filenames):
        """ SLOT: Handles the model_updated signal. Refreshes all relevant views. """
        self.update_all_views(affected_filenames)
        self._update_translation_panel_data()

    def get_display_text(self, result):
        """ DELEGATED: Asks the model for the correct text to display. """
        return self.model.get_display_text(result)

    def find_textbox_item(self, row_number):
        """Finds and returns the TextBoxItem widget for a given row number."""
        target_result, _ = self.model._find_result_by_row_number(row_number)
        if not target_result: return None
        filename = target_result.get('filename')
        if not filename: return None

        layout = self.scroll_area.widget().layout()
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel) and widget.filename == filename:
                for tb in widget.get_text_boxes():
                    # Need to handle float vs int comparison carefully
                    try:
                        if float(tb.row_number) == float(row_number):
                            return tb
                    except (ValueError, TypeError):
                        if str(tb.row_number) == str(row_number):
                            return tb
        return None

    def update_text_box_style(self, new_style_dict):
        row_number = self.editor_vm.selected_row
        if row_number is None:
            print("Style changed but no text box selected.")
            return

        target_result, _ = self.model._find_result_by_row_number(row_number)
        if not target_result:
            print(f"Error: Could not find result for row {row_number} to apply style.")
            return

        if target_result.get('is_deleted', False):
             print(f"Warning: Attempting to style a deleted row ({row_number}). Ignoring.")
             return
        
        style_diff = get_style_diff(new_style_dict, DEFAULT_TEXT_STYLE)

        if style_diff:
            target_result['custom_style'] = style_diff
        elif 'custom_style' in target_result:
            del target_result['custom_style']

        # Find the UI item and apply style visually
        target_item = self.find_textbox_item(row_number)
        if target_item:
            target_item.apply_styles(new_style_dict)
        else:
            print(f"Warning: Could not find visual text box for row {row_number} to apply style.")


    def _initialize_ocr_reader(self, context="OCR"):
        """Initializes the RapidOCR reader if it doesn't exist."""
        if self.reader:
            print("RapidOCR reader already initialized.")
            return True
        try:
            language = getattr(self.model, 'original_language', 'Korean')
            print(f"Initializing RapidOCR reader for {context} (language: {language})")
            self.reader = RapidOCREngine(language=language)
            print("RapidOCR reader initialized successfully.")
            return True
        except Exception as e:
            error_msg = f"Failed to initialize OCR reader for {context}: {str(e)}\n\n" \
                        f"Common causes:\n" \
                        f"- Missing RapidOCR models (try running initmodels.py).\n" \
                        f"- ONNX Runtime not properly installed."
            print(f"Error: {error_msg}")
            traceback.print_exc()
            exc_type, exc_value, exc_traceback = sys.exc_info()
            traceback_text = ''.join(traceback.format_exception(exc_type, exc_value, exc_traceback))
            ErrorDialog.critical(self, "OCR Initialization Error", error_msg, traceback_text)
            self.reader = None
        return False

    def _find_result_by_row_number(self, row_number_to_find):
        return self.model._find_result_by_row_number(row_number_to_find)

    def update_all_views(self, affected_filenames=None):
        """
        Refreshes the translation panel. Image-label updates are handled
        reactively by CustomScrollArea via model_updated -> refresh_visuals.
        """
        if hasattr(self, 'translation_panel'):
            self.translation_panel.populate(self.model.ocr_results, self.get_display_text)
            self.translation_panel.set_profiles(list(self.model.profiles.keys()))

    def toggle_ocr(self):
        if self.btn_ocr_toggle.isChecked():
            self.start_ocr()
        else:
            self.stop_ocr()

    def start_ocr(self):
        """OCR functionality disabled - shows message instead"""
        if not self.model.image_paths:
            QMessageBox.warning(self, "Warning", "No images loaded to process.")
            return
        if self.batch_handler:
            QMessageBox.warning(self, "Warning", "OCR is already running.")
            return
        if self.scroll_area.manual_ocr_handler.is_active:
            QMessageBox.warning(self, "Warning", "Cannot start standard OCR while in Manual OCR mode.")
            return
        
        has_existing_results = any(not res.get('is_manual', False) for res in self.model.ocr_results)
        if has_existing_results:
            reply = QMessageBox.question(self, 'Confirm Overwrite',
                                         "This will overwrite all existing OCR data (except for manual entries). Do you want to continue?",
                                         QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
            if reply == QMessageBox.No:
                return

        if not self._initialize_ocr_reader("Standard OCR"):
            return

        self.btn_ocr_toggle.transition_to_active()

        self.model.clear_standard_results()
        self.on_model_updated(None)
        
        self._load_filter_settings()
        ocr_settings = {
            "min_text_height": self.min_text_height, "max_text_height": self.max_text_height,
            "min_confidence": self.min_confidence, "distance_threshold": self.distance_threshold,
            "adjust_contrast": float(self.settings.value("ocr_adjust_contrast", 0.5)), "resize_threshold": int(self.settings.value("ocr_resize_threshold", 1024)),
            "auto_context_fill": self.settings.value("auto_context_fill", "false").lower() == "true"
        }
        self.batch_handler = BatchOCRHandler(
            image_paths=self.model.image_paths, 
            reader=self.reader, 
            settings=ocr_settings, 
            starting_row_number=self.model.next_global_row_number,
            model=self.model,
            progress_bar=self.progress_controller # Use controller for logic
        )
        self.progress_controller.start_initial_progress() # Start animation
        self.batch_handler.batch_finished.connect(self.on_batch_finished)
        self.batch_handler.error_occurred.connect(self.on_batch_error)
        self.batch_handler.processing_stopped.connect(self.on_batch_stopped)
        self.batch_handler.auto_inpaint_requested.connect(self.on_auto_inpaint_requested)
        self.batch_handler.start_processing()

    def on_image_processed(self, new_results):
        """ DELEGATED: Adds new OCR results to the model. """
        """OCR functionality disabled - placeholder method"""
        self.model.add_new_ocr_results(new_results)

    def on_batch_finished(self, next_row_number):
        """Handles the successful completion of the entire batch."""
        """OCR functionality disabled - placeholder method"""
        print("MainWindow: Batch finished.")
        self.model.next_global_row_number = next_row_number
        self.cleanup_ocr_session()
        # Success message - keep QMessageBox.information for non-error cases
        QMessageBox.information(self, "Finished", "OCR processing completed for all images.")
    
    def on_batch_error(self, message):
        """Handles a critical error during the batch process."""
        """OCR functionality disabled - placeholder method"""
        print(f"MainWindow: Batch error received: {message}")
        self.cleanup_ocr_session()
        ErrorDialog.critical(self, "OCR Error", message)

    def on_batch_stopped(self):
        """Handles the UI cleanup after the user manually stops the process."""
        """OCR functionality disabled - placeholder method"""
        print("MainWindow: Batch processing was stopped by user.")
        self.cleanup_ocr_session()
        QMessageBox.information(self, "Stopped", "OCR processing was stopped.")

    def cleanup_ocr_session(self):
        """Resets UI and state after an OCR run (success, error, or stop)."""
        """OCR functionality disabled - placeholder method"""
        self.btn_ocr_toggle.transition_to_idle()
        self.btn_ocr_toggle.setEnabled(bool(self.model.image_paths))
        self.orientation_combo.setEnabled(bool(self.model.image_paths))
        self.progress_controller.reset() # Reset controller
        if self.batch_handler:
            self.batch_handler.deleteLater()
            self.batch_handler = None
        gc.collect()
        
    def stop_ocr(self):
        """Stops the currently running OCR process by signaling the handler."""
        """OCR functionality disabled - placeholder method"""
        print("MainWindow: Sending stop request to batch handler...")
        if self.batch_handler:
            self.batch_handler.stop()
        else:
            print("No active batch handler to stop.")
            # If no handler, but UI is stuck, reset it
            self.cleanup_ocr_session()

    def on_auto_inpaint_requested(self, filename, bounding_boxes):
        """SLOT: Handles the request from BatchOCRHandler to perform automatic inpainting."""
        """OCR functionality disabled - placeholder method"""
        target_label = None
        layout = self.scroll_area.widget().layout()
        for i in range(layout.count()):
            widget = layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel) and widget.filename == filename:
                target_label = widget
                break
        
        if target_label:
            self.scroll_area.context_fill_handler.perform_auto_inpainting(target_label, bounding_boxes)
 
    def update_image_text_box(self, row_number, new_text):
        target_item = self.find_textbox_item(row_number)
        if target_item:
            if target_item.text_item and target_item.text_item.toPlainText() != new_text:
                target_item.text_item.setPlainText(new_text)
                target_item.adjust_font_size()

    def update_ocr_text(self, row_number, new_text):
        # Temporarily block signals to avoid triggering model_updated during the update
        was_blocked = self.model.blockSignals(True)
        profile_created_for_user = False
        was_original_before = self.model.active_profile_name == "Original"
        try:
            result = self.model.update_text(row_number, new_text, is_user_edit=True)
            # Handle both old return format (error, success) and new format (error, success, profile_created, should_show_message)
            if len(result) >= 3:
                if len(result) == 4:
                    _, _, _, should_show_message = result
                    profile_created_for_user = should_show_message
                else:
                    # Old 3-value format
                    _, _, profile_created_for_user = result
            
            # Check if profile was routed (switched from Original to user edit)
            # This happens when we route to an existing user edit profile
            profile_routed = was_original_before and self.model.active_profile_name != "Original"
            
            # Get the actual text that was saved (might be different from new_text if deleted content was preserved)
            result_data, _ = self.model._find_result_by_row_number(row_number)
            if result_data:
                actual_saved_text = self.model.get_display_text(result_data)
                # Update translation panel with actual saved text
                if hasattr(self, 'translation_panel'):
                    self.translation_panel.update_row_text(row_number, actual_saved_text)
                self.update_image_text_box(row_number, actual_saved_text)
            else:
                self.update_image_text_box(row_number, new_text)
        finally:
            # Restore previous signal blocking state
            self.model.blockSignals(was_blocked)
            # Emit signals after unblocking if profile was created for user edit
            if profile_created_for_user:
                self.model.profiles_updated.emit()
                self.model.profile_created_for_user_edit.emit()
            # If profile was routed to existing user edit, update UI to reflect the switch
            elif profile_routed:
                # Update profile selector to show we're now on user edit profile
                # This ensures the UI reflects that we're editing in user edit profile, not Original
                self.update_profile_selector()
                # Note: We don't do a full view update here to avoid interrupting the user's editing session
                # The widget already has the correct text (the edited/deleted text), and other widgets
                # will update naturally when model_updated is emitted from update_text
    
    def _on_profile_created_for_user_edit(self):
        """Shows message when a profile is created for a user edit."""
        QMessageBox.information(self, "Edit Profile Created",
                                f"First edit detected. A new profile 'User Edit 1' has been created and set as active. "
                                "Your original OCR text is preserved.")

    def combine_rows_in_model(self, first_row_number, combined_text, min_confidence, rows_to_delete):
        if self.model.active_profile_name == "Original":
             QMessageBox.information(self, "Edit Profile Created",
                                     f"First combination edit detected. A new profile 'User Edit 1' has been created and set as active.")
        
        message, success = self.model.combine_rows(first_row_number, combined_text, min_confidence, rows_to_delete)
        if success:
            if hasattr(self, 'find_replace_widget') and self.find_replace_widget.isVisible():
                self.find_replace_widget.find_text()
            # Success message - keep QMessageBox.information for non-error cases
            QMessageBox.information(self, "Success", message)
        else:
            ErrorDialog.critical(self, "Error", message)
    
    def handle_translation_completed(self, profile_name, translated_data):
        try:
            self.model.add_profile(profile_name, translated_data)
            # Success message - keep QMessageBox.information for non-error cases
            QMessageBox.information(self, "Success", 
                f"Translation successfully applied to profile:\n'{profile_name}'")
        except Exception as e:
            exc_type, exc_value, exc_traceback = sys.exc_info()
            traceback_text = ''.join(traceback.format_exception(exc_type, exc_value, exc_traceback))
            ErrorDialog.critical(self, "Import Error", f"Failed to apply translation: {str(e)}", traceback_text)
            traceback.print_exc()

    def import_translation(self):
        """Import translation file - delegates to file_io handler."""
        import_translation_file(self)

    def update_shortcut(self):
        self.update_find_shortcut()

    def _on_text_visibility_changed(self, visible):
        """Sync text visibility UI state (button + menu action) from VM."""
        # Button/action checked = hidden (eye-slash)
        checked = not visible
        self.btn_toggle_text.setChecked(checked)
        if hasattr(self, 'menu_bar') and hasattr(self.menu_bar, '_toggle_text_action'):
            self.menu_bar._toggle_text_action.setChecked(checked)

    def export_manhwa(self):
        export_rendered_images(self)

    def export_ocr_results(self):
        export_ocr_results(self)

    def _update_translation_panel_data(self):
        """Update the translation panel with current OCR results and profiles."""
        if hasattr(self, 'translation_panel'):
            self.translation_panel.populate(self.model.ocr_results, self.get_display_text)
            self.translation_panel.set_profiles(list(self.model.profiles.keys()))

    def toggle_panel_layout(self, checked: bool) -> None:
        """Toggle between vertical (bottom) and horizontal (right) panel layout."""
        self._panel_layout_vertical = checked
        
        # Update menu text
        if hasattr(self, 'menu_bar') and hasattr(self.menu_bar, '_panel_layout_action'):
            self.menu_bar._panel_layout_action.setText(
                "Translation Panel: Bottom" if checked else "Translation Panel: Right"
            )
        
        # Update combo box
        if hasattr(self, 'orientation_combo'):
            new_text = "Bottom" if checked else "Right"
            if self.orientation_combo.currentText() != new_text:
                self.orientation_combo.setCurrentText(new_text)
        
        # Get parent widget and layout
        right_widget = self.findChild(QWidget, "RightWidget")
        if not right_widget:
            return
        
        right_layout = right_widget.layout()
        
        # Remove existing splitter
        old_splitter = None
        for i in range(right_layout.count()):
            item = right_layout.itemAt(i)
            if item and item.widget() and isinstance(item.widget(), QSplitter):
                old_splitter = item.widget()
                break
        
        if old_splitter:
            old_splitter.setParent(None)
        
        # Create new splitter with appropriate orientation
        new_splitter = QSplitter(Qt.Vertical if checked else Qt.Horizontal)
        new_splitter.addWidget(self.style_panel)
        new_splitter.addWidget(self.translation_panel)
        new_splitter.setStretchFactor(0, 0)
        new_splitter.setStretchFactor(1, 1)
        new_splitter.setHandleWidth(10)
        
        # Update size constraints for panels
        if checked:
            # Vertical: constrain height
            self.style_panel.setMinimumHeight(70)
            self.style_panel.setMaximumHeight(480)
            self.style_panel.setMinimumWidth(0)
            self.style_panel.setMaximumWidth(16777215)
        else:
            # Horizontal: constrain width
            self.style_panel.setMinimumWidth(70)
            self.style_panel.setMaximumWidth(480)
            self.style_panel.setMinimumHeight(0)
            self.style_panel.setMaximumHeight(16777215)
        
        # Update internal layout of style panel
        # When main layout is vertical (checked), use horizontal internal layout (side-by-side)
        # When main layout is horizontal (unchecked), use vertical internal layout (stacked)
        if hasattr(self.style_panel, 'set_internal_layout_horizontal'):
            self.style_panel.set_internal_layout_horizontal(checked)
        
        right_layout.addWidget(new_splitter, 1)

    def _on_orientation_changed(self, text: str) -> None:
        """Handle orientation combo box changes."""
        checked = (text == "Bottom")
        self.toggle_panel_layout(checked)

    def _on_manual_ocr_cancelled(self):
        if self.btn_manual_ocr.isChecked():
            self.btn_manual_ocr.setChecked(False)

    def on_project_saved(self, result_message):
        if "successfully" in result_message:
            QMessageBox.information(self, "Saved", result_message)
        else:
            ErrorDialog.critical(self, "Save Error", result_message)

    def on_save_project_triggered(self):
        """Called by CustomScrollArea overlay save button."""
        self.app_vm.save_project()

    def closeEvent(self, event):
        # Cleanup translation panel
        if hasattr(self, 'translation_panel'):
            self.translation_panel.cleanup()
        
        if hasattr(self.model, 'temp_dir') and self.model.temp_dir and os.path.exists(self.model.temp_dir):
            try:
                import shutil
                print(f"Cleaning up temporary directory: {self.model.temp_dir}")
                shutil.rmtree(self.model.temp_dir)
            except Exception as e:
                print(f"Warning: Could not remove temporary directory {self.model.temp_dir}: {e}")
        if self.ocr_processor and self.ocr_processor.isRunning():
            print("Stopping OCR processor on close...")
            self.ocr_processor.stop_requested = True
            self.ocr_processor.wait(500)
            if self.ocr_processor.isRunning():
                 self.ocr_processor.terminate()
        super().closeEvent(event)