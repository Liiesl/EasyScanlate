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
from app.ui.components.results_tables import ResultsWidget
from app.ui.components.textbox_style.panel import TextBoxStylePanel
from app.ui.components.find_replace import FindReplaceWidget
from app.ui.components.translation_chat import TranslationChatWidget
from app.ui.widgets.menu_bar import MenuBar, TitleBarState
from app.ui.window.chrome import CustomTitleBar, WindowResizer
from app.ui.widgets.progress_bar import CustomProgressBar
from app.ui.widgets.menus import Menu, ToggleButton, ToggleWithProgress
from app.handlers.ocr_batch_handler import BatchOCRHandler
from app.handlers.selection_manager import SelectionManager
from app.core.project_model import ProjectModel
from app.ui.dialogs.settings_dialog import SettingsDialog
from app.ui.window.translation_window import TranslationWindow
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
        self.model.profiles_updated.connect(self.update_profile_selector)
        self.model.profiles_updated.connect(self._on_profile_list_changed)
        self.model.profile_created_for_user_edit.connect(self._on_profile_created_for_user_edit)

        self.selection_manager = SelectionManager(self.model, self)
        self.selection_manager.selection_changed.connect(self.on_selection_changed)

        self.combine_action = QAction("Combine Rows", self)
        self.find_action = QAction("Find/Replace", self)
        self.find_action.triggered.connect(self.toggle_find_widget)
        self.addAction(self.find_action)
        self.update_shortcut()

        self.language_map = { "Korean": "ko", "Chinese": "ch_sim", "Japanese": "ja" }

        self.init_ui()
        self.combine_action.triggered.connect(self.results_widget.combine_selected_rows)

        self.scroll_content = QWidget()
        self.reader = None 
        self.ocr_processor = None
        
        if hasattr(self, 'style_panel'):
             self.style_panel.style_changed.connect(self.update_text_box_style)
        
        self.batch_handler = None
    
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

        self.scroll_area = CustomScrollArea(main_window=self)
        
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
        
        # Translation button (moved from bottom_controls_layout)
        self.btn_translate = QPushButton(qta.icon('fa5s.language', color='white'), "")
        self.btn_translate.setFixedSize(40, 40)
        self.btn_translate.setToolTip("AI Translation")
        self.btn_translate.clicked.connect(self.start_translation)
        vertical_toolbar_layout.addWidget(self.btn_translate)

        # --- NEW ACTION BUTTONS ---

        # Toggle Text Visibility
        self.btn_toggle_text = ToggleButton(
            off_text="", on_text="",
            off_icon=qta.icon('fa5s.eye', color='white'),
            on_icon=qta.icon('fa5s.eye-slash', color='white')
        )
        self.btn_toggle_text.setFixedSize(40, 40)
        self.btn_toggle_text.setToolTip("Toggle Text Visibility")
        self.btn_toggle_text.toggled.connect(self.scroll_area.toggle_text_visibility)
        self.btn_toggle_text.setState(False) 
        vertical_toolbar_layout.addWidget(self.btn_toggle_text)

        # Toggle Inpainting
        self.btn_toggle_inpainting = ToggleButton(
             off_text="", on_text="",
             off_icon=qta.icon('fa5s.eye', color='white'), # Using eye for "Inpainting is Visible"
             on_icon=qta.icon('fa5s.eraser', color='white') # Using eraser/slash for "Inpainting is Hidden"
        )
        
        self.btn_toggle_inpainting = ToggleButton(
            off_text="", on_text="",
            off_icon=qta.icon('fa5s.eraser', color='white'), # Hidden state
            on_icon=qta.icon('fa5s.fill', color='white')   # Visible state
        )
        self.btn_toggle_inpainting.setFixedSize(40, 40)
        self.btn_toggle_inpainting.setToolTip("Toggle Context Fill Visibility")
        self.btn_toggle_inpainting.setState(True) # Default is visible
        self.btn_toggle_inpainting.toggled.connect(self.scroll_area.toggle_inpainting_visibility)

        vertical_toolbar_layout.addWidget(self.btn_toggle_inpainting)

        # Context Fill (Normal Button)
        self.btn_context_fill = QPushButton(qta.icon('fa5s.fill-drip', color='white'), "")
        self.btn_context_fill.setFixedSize(40, 40)
        self.btn_context_fill.setToolTip("Context Fill Mode")
        self.btn_context_fill.clicked.connect(self.scroll_area.context_fill_handler.start_mode)
        vertical_toolbar_layout.addWidget(self.btn_context_fill)

        # Edit Context Fill (Toggle Button)
        self.btn_edit_context_fill = ToggleButton(
            off_text="", on_text="",
            off_icon=qta.icon('fa5s.paint-brush', color='white'),
            on_icon=qta.icon('fa5s.check-circle', color='white')
        )
        self.btn_edit_context_fill.setFixedSize(40, 40)
        self.btn_edit_context_fill.setToolTip("Edit Context Fills")
        self.btn_edit_context_fill.clicked.connect(self.scroll_area.context_fill_handler.toggle_edit_mode)
        vertical_toolbar_layout.addWidget(self.btn_edit_context_fill)

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
        self.update_profile_selector()

        left_panel = QVBoxLayout()
        left_panel.setContentsMargins(10, 10, 5, 10)
        left_panel.setSpacing(20)
        
        self.scroll_content = QWidget()
        self.scroll_content.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        self.scroll_layout = QVBoxLayout(self.scroll_content)
        self.scroll_layout.setContentsMargins(0, 0, 0, 0)
        self.scroll_layout.setSpacing(0)
        self.scroll_area.setWidget(self.scroll_content)
        self.scroll_area.setWidgetResizable(True)
        left_panel.addWidget(self.scroll_area)

        # Right Panel
        right_panel = QVBoxLayout()
        right_panel.padding = 30
        right_panel.setContentsMargins(5, 10, 10, 10)
        right_panel.setSpacing(20)

        button_layout = QHBoxLayout()
        
        # ToggleWithProgress Button
        self.btn_ocr_toggle = ToggleWithProgress(
            start_text="Process OCR", 
            stop_text="Stop OCR",
            start_icon=qta.icon('fa5s.magic', color='white'),
            stop_icon=qta.icon('fa5s.stop', color='white'),
            parent=self
        )
        self.btn_ocr_toggle.setFixedWidth(200) # Slightly wider to accommodate progress
        self.btn_ocr_toggle.clicked.connect(self.toggle_ocr)
        self.btn_ocr_toggle.setEnabled(False) # Disabled until project loaded
        button_layout.addWidget(self.btn_ocr_toggle)

        # Progress Controller (Hidden Logic)
        self.progress_controller = CustomProgressBar()
        self.progress_controller.setVisible(False)
        self.progress_controller.valueChanged.connect(self.btn_ocr_toggle.setValue)
        # Assuming 0-100 range, we can also sync max if needed, but CustomProgressBar defaults to 100
        self.btn_ocr_toggle.setMaximum(100)
        
        file_button_layout = QHBoxLayout()
        file_button_layout.setAlignment(Qt.AlignRight)
        file_button_layout.setSpacing(20)

        self.profile_selector = QComboBox(self)
        self.profile_selector.setFixedWidth(220)
        self.profile_selector.setToolTip("Switch between different text profiles (e.g., Original, User Edits, Translations).")
        self.profile_selector.activated.connect(self.on_profile_selected)
        file_button_layout.addWidget(self.profile_selector)

        # Chat toggle button (between profile selector and action menu)
        self.btn_chat_toggle = QPushButton(qta.icon('fa5s.comments', color='white'), "")
        self.btn_chat_toggle.setFixedSize(40, 40)
        self.btn_chat_toggle.setToolTip("Toggle Chat Widget")
        self.btn_chat_toggle.setCheckable(True)
        self.btn_chat_toggle.clicked.connect(self.toggle_chat)
        file_button_layout.addWidget(self.btn_chat_toggle)

        self.btn_import_export_menu = QPushButton(qta.icon('fa5s.bars', color='white'), "")
        self.btn_import_export_menu.setFixedWidth(60)
        self.btn_import_export_menu.setToolTip("Open Import/Export Menu")
        self.btn_import_export_menu.clicked.connect(self.show_import_export_menu)
        file_button_layout.addWidget(self.btn_import_export_menu)
        button_layout.addLayout(file_button_layout)
        right_panel.addLayout(button_layout)

        # Create results widget first
        self.results_widget = ResultsWidget(self, self.combine_action, self.find_action, self.selection_manager)
        
        # Style panel - always visible above results with resizable splitter
        self.style_panel = TextBoxStylePanel(default_style=DEFAULT_TEXT_STYLE)
        self.style_panel.setMinimumHeight(70)
        self.style_panel.setMaximumHeight(480)
        
        # Create vertical splitter for resizable layout
        right_splitter = QSplitter(Qt.Vertical)
        right_splitter.addWidget(self.style_panel)
        right_splitter.addWidget(self.results_widget)
        right_splitter.setStretchFactor(0, 0)
        right_splitter.setStretchFactor(1, 1)
        right_splitter.setHandleWidth(10)

        # Find/replace widget
        self.find_replace_widget = FindReplaceWidget(self)
        right_panel.addWidget(self.find_replace_widget)
        self.find_replace_widget.hide()
        self.style_panel_size = None

        # Create translation chat component
        self.translation_chat = TranslationChatWidget()
        self.translation_chat.translation_complete.connect(self.handle_translation_completed)
        self.translation_chat.hide()  # Hide by default

        # Initialize translation chat with current data
        self._update_translation_chat_data()
        
        # Create horizontal splitter for results/style panel and translation chat
        content_splitter = QSplitter(Qt.Horizontal)
        content_splitter.addWidget(right_splitter)
        content_splitter.addWidget(self.translation_chat)
        content_splitter.setStretchFactor(0, 2)
        content_splitter.setStretchFactor(1, 1)
        content_splitter.setHandleWidth(5)
        right_panel.addWidget(content_splitter, 1)

        right_widget = QWidget()
        right_widget.setObjectName("RightWidget")
        right_widget.setLayout(right_panel)

        # === APPLY STYLES ===
        for w in [self.style_panel, self.results_widget, self.translation_chat]:
            w.setObjectName("TransparentPanel")
            w.setAttribute(Qt.WA_StyledBackground, True)
        
        # Apply the stylesheet to the parent widget so children can inherit or use the ID selector
        right_widget.setStyleSheet(RIGHT_PANEL_STYLES + UNIVERSAL_STYLES)

        splitter = QSplitter(Qt.Horizontal)
        left_widget = QWidget()
        left_widget.setLayout(left_panel)
        splitter.addWidget(left_widget)
        splitter.addWidget(right_widget)

        # MODIFIED: Add toolbar and splitter to main layout
        main_layout.addWidget(self.vertical_toolbar)  # Add vertical toolbar first
        main_layout.addWidget(splitter)  # Then the main content splitter

        # Connect Window Resizer
        self.resizer = WindowResizer(self)

        # Initialize Title Bar State (needs all other widgets to be created first)
        self.title_bar.setState(TitleBarState.MAIN_WINDOW)

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

    def on_profile_selected(self, index):
        profile_name = self.profile_selector.itemText(index)
        if profile_name:
            self.switch_active_profile(profile_name)

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

    def update_profile_selector(self):
        """Syncs the profile dropdown with the profiles from the model."""
        if not hasattr(self, 'profile_selector'): return
        self.profile_selector.blockSignals(True)
        self.profile_selector.clear()
        profiles_list = sorted([p for p in self.model.profiles.keys() if p != "Original"])
        profiles_list.insert(0, "Original")
        self.profile_selector.addItems(profiles_list)
        if self.model.active_profile_name in self.model.profiles:
            index = self.profile_selector.findText(self.model.active_profile_name)
            if index != -1: self.profile_selector.setCurrentIndex(index)
        self.profile_selector.blockSignals(False)
        
        # Sync Menu Bar profiles if available
        if hasattr(self, 'title_bar') and hasattr(self.title_bar, 'menu_bar'):
             self.title_bar.menu_bar.update_profiles_menu()
        
        # Also update translation chat profiles
        self._update_translation_chat_data()

    def switch_active_profile(self, profile_name):
        """Tells the model to switch the active profile."""
        if profile_name and profile_name in self.model.profiles and profile_name != self.model.active_profile_name:
            print(f"Switching to active profile: {profile_name}")
            
            # Set flag to prevent textChanged events from deleting translations during profile switch
            # This is crucial because clearing highlighters triggers textChanged events
            if hasattr(self, 'results_widget') and self.results_widget:
                self.results_widget._is_updating_views = True
            
            self.model.active_profile_name = profile_name
            self._on_profile_changed()
            self.on_model_updated(None)
    
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
        if self.find_replace_widget.isVisible():
            self.find_replace_widget.close_widget()
        else:
            self.find_replace_widget.raise_()
            self.find_replace_widget.show()

    def toggle_chat(self):
        """Toggle the visibility of the translation chat widget."""
        if self.translation_chat.isVisible():
            self.translation_chat.hide()
            self.btn_chat_toggle.setChecked(False)
        else:
            self.translation_chat.show()
            self.btn_chat_toggle.setChecked(True)

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
        self._clear_layout(self.scroll_layout)
        self.scroll_area.cancel_active_modes()

        image_paths = self.model.image_paths
        self.setWindowTitle(f"{self.model.project_name} | ManhwaOCR")
        self.setWindowTitle(f"{self.model.project_name} | ManhwaOCR")
        self.btn_ocr_toggle.setEnabled(bool(image_paths))
        self.btn_manual_ocr.setEnabled(bool(image_paths))
        # self.ocr_progress.setValue(0) # Removed
        
        if not image_paths:
            QMessageBox.warning(self, "No Images", "The project was loaded, but no images were found inside.")

        for image_path in image_paths:
            try:
                 pixmap = QPixmap(image_path)
                 if pixmap.isNull(): continue
                 filename = os.path.basename(image_path)
                 label = ResizableImageLabel(pixmap, filename, self, self.selection_manager)
                 label.textBoxDeleted.connect(self.delete_row)

                 label.inpaintRecordDeleted.connect(self.handle_inpaint_record_deleted)
                 label.manual_area_selected.connect(self.scroll_area.manual_ocr_handler.handle_area_selected)
                 label.manual_area_selected.connect(self.scroll_area.context_fill_handler.handle_area_selected)
                 self.scroll_layout.addWidget(label)
            except Exception as e:
                 print(f"Error creating ResizableImageLabel for {image_path}: {e}")
        
        self._apply_inpaints()

        self.update_profile_selector()
        self.on_model_updated(None)
        self._update_translation_chat_data()
        print(f"Project '{self.model.project_name}' loaded and UI populated.")
    
    def handle_inpaint_record_deleted(self, record_id):
        """Delegates the inpaint record deletion request to the model."""
        self.model.remove_inpaint_record(record_id)
    
    def _apply_inpaints(self):
        """Iterates through inpaint data and applies patches to the correct image labels."""
        labels_by_filename = {
            widget.filename: widget
            for i in range(self.scroll_layout.count())
            if isinstance((widget := self.scroll_layout.itemAt(i).widget()), ResizableImageLabel)
        }
        
        inpaint_dir = os.path.join(self.model.temp_dir, 'inpaint')

        for record in self.model.inpaint_data:
            target_label = labels_by_filename.get(record['target_image'])
            if target_label:
                patch_path = os.path.join(inpaint_dir, record['patch_filename'])
                if os.path.exists(patch_path):
                    patch_pixmap = QPixmap(patch_path)
                    coords = record['coordinates']
                    if not patch_pixmap.isNull():
                        target_label.apply_inpaint_patch(patch_pixmap, QRectF(coords[0], coords[1], coords[2], coords[3]))
                    else:
                        print(f"Warning: Could not load patch pixmap from {patch_path}")
                else:
                    print(f"Warning: Inpaint patch file not found: {patch_path}")

    def on_model_updated(self, affected_filenames):
        """ SLOT: Handles the model_updated signal. Refreshes all relevant views. """
        if affected_filenames:
            for filename in affected_filenames:
                for i in range(self.scroll_layout.count()):
                    widget = self.scroll_layout.itemAt(i).widget()
                    if isinstance(widget, ResizableImageLabel) and widget.filename == filename:
                        widget.revert_to_original()
                        self._apply_inpaints()
                        break

        self.update_all_views(affected_filenames)
        self._update_translation_chat_data()

    def get_display_text(self, result):
        """ DELEGATED: Asks the model for the correct text to display. """
        return self.model.get_display_text(result)

    def on_selection_changed(self, row_number, source):
        """
        Updates the style panel based on the currently selected row.
        The style panel is always visible, but its content changes based on selection.
        """
        if row_number is not None:
            current_style = self.get_style_for_row(row_number)
            self.style_panel.update_style_panel(current_style)
        else:
            # When no textbox is selected, reset to default style
            self.style_panel.update_style_panel(DEFAULT_TEXT_STYLE)

    def get_style_for_row(self, row_number):
        style = {}
        for k, v in DEFAULT_TEXT_STYLE.items():
             if k in ['bg_color', 'border_color', 'text_color']:
                 style[k] = QColor(v)
             else:
                 style[k] = v

        target_result, _ = self.model._find_result_by_row_number(row_number)
        if target_result:
            custom_style = target_result.get('custom_style', {})
            for k, v in custom_style.items():
                 if k in ['bg_color', 'border_color', 'text_color']:
                     style[k] = QColor(v)
                 else:
                     style[k] = v
        return style

    def find_textbox_item(self, row_number):
        """Finds and returns the TextBoxItem widget for a given row number."""
        target_result, _ = self.model._find_result_by_row_number(row_number)
        if not target_result: return None
        filename = target_result.get('filename')
        if not filename: return None

        for i in range(self.scroll_layout.count()):
            widget = self.scroll_layout.itemAt(i).widget()
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
        row_number = self.selection_manager.get_current_selection()
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
            print(f"Initializing RapidOCR reader for {context}")
            self.reader = RapidOCREngine()
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

    def _clear_layout(self, layout):
        if layout is not None:
            while layout.count():
                item = layout.takeAt(0)
                widget = item.widget()
                if widget is not None: widget.deleteLater()

    def update_all_views(self, affected_filenames=None):
        """
        Refreshes all views that depend on the model's data, including the
        results table and the text boxes rendered on the images.
        """
        self.results_widget.update_views()
        grouped_results = {}
        for result in self.model.ocr_results:
            filename = result.get('filename')
            if filename:
                if affected_filenames and filename not in affected_filenames:
                    continue
                if filename not in grouped_results:
                    grouped_results[filename] = {}
                grouped_results[filename][result.get('row_number')] = result

        for i in range(self.scroll_layout.count()):
            widget = self.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                image_filename = widget.filename
                if not affected_filenames or image_filename in affected_filenames:
                    results_for_this_image = grouped_results.get(image_filename, {})
                    records_for_this_image = [
                        r for r in self.model.inpaint_data if r.get('target_image') == image_filename
                    ]
                    widget.update_inpaint_data(records_for_this_image)
                    widget.apply_translation(self, results_for_this_image, DEFAULT_TEXT_STYLE)

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
            "batch_size": int(self.settings.value("ocr_batch_size", 8)), "decoder": self.settings.value("ocr_decoder", "beamsearch"),
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
        for i in range(self.scroll_layout.count()):
            widget = self.scroll_layout.itemAt(i).widget()
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
                # If the saved text is different from what user typed, update the widget to show the saved text
                if actual_saved_text != new_text:
                    # The model preserved the user edit - update widget to reflect this
                    # Update both simple view and table view if visible
                    if hasattr(self, 'results_widget') and self.results_widget:
                        self.results_widget._update_simple_view_text_if_visible(row_number, actual_saved_text)
                        self.results_widget._update_table_cell_if_visible(row_number, 0, actual_saved_text)
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
            if self.find_replace_widget.isVisible():
                self.find_replace_widget.find_text()
            # Success message - keep QMessageBox.information for non-error cases
            QMessageBox.information(self, "Success", message)
        else:
            ErrorDialog.critical(self, "Error", message)
    
    def toggle_advanced_mode(self, state):
        self.results_widget.toggle_advanced_mode(state)

    def delete_row(self, row_number_to_delete):
        show_warning = self.settings.value("show_delete_warning", "true") == "true"
        proceed = True
        if show_warning:
            msg = QMessageBox(self)
            msg.setIcon(QMessageBox.Warning)
            msg.setWindowTitle("Confirm Deletion Marking")
            msg.setText("<b>Mark for Deletion Warning</b>")
            msg.setInformativeText("Mark this entry for deletion? It will be hidden and excluded from exports.")
            dont_show_cb = QCheckBox("Remember choice", msg)
            msg.setCheckBox(dont_show_cb)
            msg.setStandardButtons(QMessageBox.Yes | QMessageBox.No) 
            msg.setDefaultButton(QMessageBox.No)
            response = msg.exec()
            if dont_show_cb.isChecked(): self.settings.setValue("show_delete_warning", "false")
            proceed = response == QMessageBox.Yes
        if not proceed: return

        if self.selection_manager.get_current_selection() == row_number_to_delete:
            self.selection_manager.deselect(self)

        self.model.delete_row(row_number_to_delete)
        if self.find_replace_widget.isVisible(): self.find_replace_widget.find_text()

    def start_translation(self):
        api_key = self.settings.value("gemini_api_key", "")
        if not api_key:
            QMessageBox.critical(self, "API Key Missing", "Please set your Gemini API key in Settings.")
            return
        if not self.model.ocr_results:
            QMessageBox.warning(self, "No Data", "There are no OCR results to translate.")
            return
        model_name = self.settings.value("gemini_model", "gemini-1.5-flash-latest")
        dialog = TranslationWindow(
            api_key, model_name, self.model.ocr_results, list(self.model.profiles.keys()), self
        )
        dialog.translation_complete.connect(self.handle_translation_completed)
        dialog.exec()

    def handle_translation_completed(self, profile_name, translated_data):
        try:
            # Set flag to prevent textChanged events from deleting translations during profile switch
            # add_profile switches to the new profile and emits signals that clear highlighters
            if hasattr(self, 'results_widget') and self.results_widget:
                self.results_widget._is_updating_views = True
            
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
        combine_shortcut = self.settings.value("combine_shortcut", "Ctrl+G")
        self.combine_action.setShortcut(QKeySequence(combine_shortcut))
        self.update_find_shortcut()

    def export_manhwa(self):
        export_rendered_images(self)

    def export_ocr_results(self):
        export_ocr_results(self)

    def _update_translation_chat_data(self):
        """Update the translation chat widget with current OCR results and profiles."""
        if hasattr(self, 'translation_chat'):
            api_key = self.settings.value("gemini_api_key", "")
            model_name = self.settings.value("gemini_model", "gemini-1.5-flash-latest")
            
            # Pass current OCR results and profiles to the translation chat
            self.translation_chat.set_data(
                api_key=api_key,
                model_name=model_name,
                ocr_results=self.model.ocr_results,
                profiles=list(self.model.profiles.keys())
            )

    def save_project(self):
        result_message = self.model.save_project()
        if "successfully" in result_message:
            # Success message - keep QMessageBox.information for non-error cases
            QMessageBox.information(self, "Saved", result_message)
        else:
            ErrorDialog.critical(self, "Save Error", result_message)

    def closeEvent(self, event):
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