# main_window.py

from PySide6.QtWidgets import (QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QSizePolicy, QCheckBox, QPushButton,
                             QMessageBox, QSplitter, QComboBox)
from PySide6.QtCore import Qt, QSettings, QPoint
from PySide6.QtGui import QPixmap, QKeySequence, QAction, QColor
import qtawesome as qta
from app.utils.file_io import export_ocr_results, import_translation_file, export_rendered_images
from app.ui.components import ResizableImageLabel, CustomScrollArea, ResultsWidget, TextBoxStylePanel, FindReplaceWidget
from app.ui.widgets.menu_bar import MenuBar
from app.ui.widgets.progress_bar import CustomProgressBar
from app.ui.widgets.menus import SaveMenu, ActionMenu, ImportExportMenu
from app.handlers import BatchOCRHandler, ManualOCRHandler, StitchHandler, SplitHandler
from app.core import ProjectModel
from app.ui.dialogs import SettingsDialog
from app.ui.window.translation_window import TranslationWindow
from assets import (COLORS, MAIN_STYLESHEET, ADVANCED_CHECK_STYLES, RIGHT_WIDGET_STYLES,
                    DEFAULT_TEXT_STYLE, DELETE_ROW_STYLES, get_style_diff)
import easyocr, os, gc, json, traceback

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Easy Scanlate")
        self.setGeometry(100, 100, 1200, 600)
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self._load_filter_settings()
        
        # The model is the single source of truth for project data.
        self.model = ProjectModel()
        self.model.project_loaded.connect(self.on_project_loaded)
        self.model.project_load_failed.connect(self.on_project_load_failed)
        self.model.model_updated.connect(self.on_model_updated)
        self.model.profiles_updated.connect(self.update_profile_selector)

        self.combine_action = QAction("Combine Rows", self)
        # Connection is deferred until after results_widget is created
        self.find_action = QAction("Find/Replace", self)
        self.find_action.triggered.connect(self.toggle_find_widget)
        self.addAction(self.find_action)
        self.update_shortcut()

        self.language_map = {
            "Korean": "ko",
            "Chinese": "ch_sim",
            "Japanese": "ja",
        }

        self._is_handling_selection = False # Flag to prevent signal loops

        self.init_ui()
        # Connect actions that depend on widgets created in init_ui
        self.combine_action.triggered.connect(self.results_widget.combine_selected_rows)

        self.manual_ocr_handler = ManualOCRHandler(self)
        self.stitch_handler = StitchHandler(self)
        self.split_handler = SplitHandler(self)
        # Connect stitch handler UI positioning ---
        self.scroll_area.resized.connect(lambda: self.stitch_handler._update_widget_position() if self.stitch_handler.is_active else None)
        # Connect split handler UI positioning ---
        self.scroll_area.resized.connect(lambda: self.split_handler._update_widget_position() if self.split_handler.is_active else None)

        self.scroll_content = QWidget()
        self.reader = None
        self.ocr_processor = None

        self.current_selected_row = None
        self.current_selected_image_label = None
        self.selected_text_box_item = None
        if hasattr(self, 'style_panel'):
             self.style_panel.style_changed.connect(self.update_text_box_style)
        
        # --- MODIFIED: State is now managed by a single handler instance ---
        self.batch_handler = None 

    def _load_filter_settings(self):
        self.min_text_height = int(self.settings.value("min_text_height", 40))
        self.max_text_height = int(self.settings.value("max_text_height", 100))
        self.min_confidence = float(self.settings.value("min_confidence", 0.2))
        self.distance_threshold = int(self.settings.value("distance_threshold", 100))
        print(f"Loaded settings: MinH={self.min_text_height}, MaxH={self.max_text_height}, MinConf={self.min_confidence}, DistThr={self.distance_threshold}")

    def init_ui(self):
        self.menuBar = MenuBar(self)
        self.setMenuBar(self.menuBar)
        main_widget = QWidget()
        main_layout = QHBoxLayout()
        self.colors = COLORS
        self.setStyleSheet(MAIN_STYLESHEET)
        # --- NEW: Call to initialize profile selector UI ---
        self.update_profile_selector()

        # Left Panel
        left_panel = QVBoxLayout()
        left_panel.setSpacing(20)

        #--- Settings and Progress Bar Layout ---
        settings_layout = QHBoxLayout()
        self.btn_settings = QPushButton(qta.icon('fa5s.cog', color='white'), "")
        self.btn_settings.setFixedSize(50, 50)
        self.btn_settings.clicked.connect(self.show_settings_dialog)
        settings_layout.addWidget(self.btn_settings)

        self.ocr_progress = CustomProgressBar()
        self.ocr_progress.setFixedHeight(20)
        settings_layout.addWidget(self.ocr_progress, 1) # Add stretch factor to fill space

        left_panel.addLayout(settings_layout)

        self.scroll_area = CustomScrollArea(self)
        self.scroll_area.save_requested.connect(
            lambda button: self._show_menu(SaveMenu, button, 'top right')
        )
        self.scroll_area.action_menu_requested.connect(
            lambda button: self._show_menu(ActionMenu, button, 'top left')
        )
        # Connection for stitch handler UI positioning ---
        self.scroll_area.resized.connect(lambda: self.stitch_handler._update_widget_position() if self.stitch_handler.is_active else None)

        self.scroll_content = QWidget()
        self.scroll_content.setStyleSheet("background-color: transparent;")
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
        right_panel.setContentsMargins(20, 20, 20, 20)
        right_panel.setSpacing(20)

        button_layout = QHBoxLayout()
        self.btn_process = QPushButton(qta.icon('fa5s.magic', color='white'), "Process OCR")
        self.btn_process.setFixedWidth(160)
        self.btn_process.clicked.connect(self.start_ocr)
        self.btn_process.setEnabled(False)
        button_layout.addWidget(self.btn_process)
        self.btn_stop_ocr = QPushButton(qta.icon('fa5s.stop', color='white'), "Stop OCR")
        self.btn_stop_ocr.setFixedWidth(160)
        self.btn_stop_ocr.clicked.connect(self.stop_ocr)
        self.btn_stop_ocr.setVisible(False)
        button_layout.addWidget(self.btn_stop_ocr)
        self.btn_manual_ocr = QPushButton(qta.icon('fa5s.crop-alt', color='white'), "Manual OCR")
        self.btn_manual_ocr.setFixedWidth(160)
        self.btn_manual_ocr.setCheckable(True)

        self.btn_manual_ocr.setEnabled(False)
        button_layout.addWidget(self.btn_manual_ocr)
        file_button_layout = QHBoxLayout()
        file_button_layout.setAlignment(Qt.AlignRight)
        file_button_layout.setSpacing(20)

        self.profile_selector = QComboBox(self)
        self.profile_selector.setFixedWidth(220)
        self.profile_selector.setToolTip("Switch between different text profiles (e.g., Original, User Edits, Translations).")
        # --- CORRECTED LINE ---
        self.profile_selector.activated.connect(self.on_profile_selected)
        file_button_layout.addWidget(self.profile_selector)

        self.btn_import_export_menu = QPushButton(qta.icon('fa5s.bars', color='white'), "")
        self.btn_import_export_menu.setFixedWidth(60)
        self.btn_import_export_menu.setToolTip("Open Import/Export Menu")
        self.btn_import_export_menu.clicked.connect(
            lambda: self._show_menu(ImportExportMenu, self.btn_import_export_menu, 'bottom right')
        )
        file_button_layout.addWidget(self.btn_import_export_menu)
        button_layout.addLayout(file_button_layout)
        right_panel.addLayout(button_layout)

        # --- FIX STARTS HERE ---
        # The creation of FindReplaceWidget is moved to after ResultsWidget is created.

        self.right_content_splitter = QSplitter(Qt.Horizontal)
        self.style_panel = TextBoxStylePanel(default_style=DEFAULT_TEXT_STYLE)
        self.style_panel.hide()
        self.right_content_splitter.addWidget(self.style_panel)

        # 1. Create ResultsWidget first, as FindReplaceWidget depends on it.
        self.results_widget = ResultsWidget(self, self.combine_action, self.find_action)
        self.results_widget.rowSelected.connect(self.on_result_row_selected)

        # 2. Now it is safe to create FindReplaceWidget.
        self.find_replace_widget = FindReplaceWidget(self)
        right_panel.addWidget(self.find_replace_widget) # Add it to the layout above the splitter
        self.find_replace_widget.hide()

        # 3. Add the now-created results_widget to the splitter.
        self.right_content_splitter.addWidget(self.results_widget)
        self.right_content_splitter.setStretchFactor(0, 0)
        self.right_content_splitter.setStretchFactor(1, 1)
        right_panel.addWidget(self.right_content_splitter, 1)
        self.style_panel_size = None

        # --- FIX ENDS HERE ---

        bottom_controls_layout = QHBoxLayout()
        self.btn_translate = QPushButton(qta.icon('fa5s.language', color='white'), "AI Translation")
        self.btn_translate.clicked.connect(self.start_translation)
        bottom_controls_layout.addWidget(self.btn_translate)

        self.advanced_mode_check = QCheckBox("Advanced Mode")
        self.advanced_mode_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.advanced_mode_check.setChecked(False)
        self.advanced_mode_check.setCursor(Qt.PointingHandCursor)
        self.advanced_mode_check.stateChanged.connect(self.toggle_advanced_mode)
        bottom_controls_layout.addWidget(self.advanced_mode_check)
        right_panel.addLayout(bottom_controls_layout)

        right_widget = QWidget()
        right_widget.setObjectName("RightWidget")
        right_widget.setLayout(right_panel)
        right_widget.setStyleSheet(RIGHT_WIDGET_STYLES)

        splitter = QSplitter(Qt.Horizontal)
        left_widget = QWidget()
        left_widget.setLayout(left_panel)
        splitter.addWidget(left_widget)
        splitter.addWidget(right_widget)

        main_layout.addWidget(splitter)
        main_widget.setLayout(main_layout)
        self.setCentralWidget(main_widget)

    # --- NEW METHOD TO HANDLE THE SIGNAL ---
    def on_profile_selected(self, index):
        """
        Slot to handle the QComboBox's activated(int) signal.
        It retrieves the text of the selected item and calls the appropriate method.
        """
        profile_name = self.profile_selector.itemText(index)
        if profile_name:
            self.switch_active_profile(profile_name)

    def _show_menu(self, menu_class, button, position):
        """ Creates, positions, and shows a popup menu.
        Args:
            menu_class: The class of the menu widget to create (e.g., SaveMenu).
            button: The button widget that triggered the menu.
            position: A string like 'top left', 'top right', 'bottom left', 'bottom right'
                      which determines where the menu is placed relative to the button. """
        menu = menu_class(self)
        menu_size = menu.sizeHint()

        # Get button's global corner positions
        button_top_left = button.mapToGlobal(button.rect().topLeft())
        button_top_right = button.mapToGlobal(button.rect().topRight())
        button_bottom_left = button.mapToGlobal(button.rect().bottomLeft())
        button_bottom_right = button.mapToGlobal(button.rect().bottomRight())

        menu_pos = QPoint()

        # Calculate the menu's top-left position based on the desired alignment
        if position == 'bottom left':
            # Align menu's top-left to button's bottom-left
            menu_pos = button_bottom_left
        elif position == 'bottom right':
            # Align menu's top-right to button's bottom-right
            menu_pos = QPoint(button_bottom_right.x() - menu_size.width(), button_bottom_right.y())
        elif position == 'top left':
            # Align menu's bottom-left to button's top-left
            menu_pos = QPoint(button_top_left.x(), button_top_left.y() - menu_size.height())
        elif position == 'top right':
            # Align menu's bottom-right to button's top-right
            menu_pos = QPoint(button_top_right.x() - menu_size.width(), button_top_right.y() - menu_size.height())
        else: # Default to bottom left as a fallback
            menu_pos = button_bottom_left

        menu.move(menu_pos)
        menu.show()
    
    def hide_text(self):
        # Placeholder for future implementation
        return
    
    # --- NEW: Method to start image splitting ---
    def split_images(self):
        """Starts the image splitting process via its handler."""
        self.split_handler.start_splitting_mode()

    def stitch_images(self):
        """Starts the image stitching process via its handler."""
        self.stitch_handler.start_stitching_mode()

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

    def switch_active_profile(self, profile_name):
        """Tells the model to switch the active profile."""
        if profile_name and profile_name in self.model.profiles and profile_name != self.model.active_profile_name:
            print(f"Switching to active profile: {profile_name}")
            self.model.active_profile_name = profile_name
            # An update is needed to show the text from the new profile
            self.on_model_updated(None) # None = full refresh

    def show_settings_dialog(self):
        dialog = SettingsDialog(self)
        if dialog.exec_():
            self._load_filter_settings()
            self.update_shortcut()

    def toggle_find_widget(self):
        if self.find_replace_widget.isVisible():
            self.find_replace_widget.close_widget()
        else:
            self.find_replace_widget.raise_()
            self.find_replace_widget.show()

    def update_find_shortcut(self):
        shortcut = self.settings.value("find_shortcut", "Ctrl+F")
        self.find_action.setShortcut(QKeySequence(shortcut))
        print(f"Find shortcut set to: {shortcut}")

    def process_mmtl(self, mmtl_path, temp_dir):
        """ DELEGATED: Asks the model to load the project. """
        self.model.load_project(mmtl_path, temp_dir)

    def on_project_load_failed(self, error_msg):
        """ SLOT: Handles the project_load_failed signal from the model. """
        QMessageBox.critical(self, "Project Load Error", error_msg)
        self.close()

    def on_project_loaded(self):
        """
        SLOT: Handles the project_loaded signal from the model.
        Populates the UI with the newly loaded data.
        """
        self._clear_layout(self.scroll_layout)
        if self.manual_ocr_handler.is_active:
            self.manual_ocr_handler.cancel_mode()
        # --- NEW: Cancel stitch/split modes on new project load ---
        if self.stitch_handler.is_active:
            self.stitch_handler.cancel_stitching_mode()
        if self.split_handler.is_active:
            self.split_handler.cancel_splitting_mode()

        image_paths = self.model.image_paths
        
        self.setWindowTitle(f"{self.model.project_name} | ManhwaOCR")
        self.btn_process.setEnabled(bool(image_paths))
        self.btn_manual_ocr.setEnabled(bool(image_paths))
        self.ocr_progress.setValue(0)
        
        if not image_paths:
            QMessageBox.warning(self, "No Images", "The project was loaded, but no images were found inside.")

        for image_path in image_paths:
            try:
                 pixmap = QPixmap(image_path)
                 if pixmap.isNull(): continue
                 filename = os.path.basename(image_path)
                 label = ResizableImageLabel(pixmap, filename)
                 label.textBoxDeleted.connect(self.delete_row)
                 label.textBoxSelected.connect(self.handle_text_box_selected)
                 label.manual_area_selected.connect(self.manual_ocr_handler.handle_area_selected)
                 self.scroll_layout.addWidget(label)
            except Exception as e:
                 print(f"Error creating ResizableImageLabel for {image_path}: {e}")

        # Trigger final UI updates
        self.update_profile_selector()
        self.on_model_updated(None) # None signifies a full refresh
        print(f"Project '{self.model.project_name}' loaded and UI populated.")
    
    def on_model_updated(self, affected_filenames):
        """ SLOT: Handles the model_updated signal. Refreshes all relevant views. """
        self.update_all_views(affected_filenames)

    def get_display_text(self, result):
        """ DELEGATED: Asks the model for the correct text to display. """
        return self.model.get_display_text(result)

    def on_result_row_selected(self, row_number):
        if self._is_handling_selection:
            return

        self._is_handling_selection = True
        try:
            target_result, _ = self.model._find_result_by_row_number(row_number)
            if not target_result:
                return

            filename = target_result.get('filename')
            if not filename:
                return
            
            target_image_label = None
            # Find the image widget corresponding to the result's filename
            for i in range(self.scroll_layout.count()):
                widget = self.scroll_layout.itemAt(i).widget()
                if isinstance(widget, ResizableImageLabel):
                    if widget.filename == filename:
                        target_image_label = widget
                        # SCROLLING LOGIC IS MOVED to after the text box is selected.
                    else:
                        # Deselect boxes on all other images
                        widget.deselect_all_text_boxes()

            if target_image_label:
                # Tell the found image widget to select the correct text box.
                # This now returns the QGraphicsItem for the text box.
                selected_box_item = target_image_label.select_text_box(row_number)
                if selected_box_item:
                    # 1. Get coordinates and dimensions
                    scroll_viewport = self.scroll_area.viewport()
                    viewport_height = scroll_viewport.height()
                    current_scroll_y = self.scroll_area.verticalScrollBar().value()
                    image_label_y_in_scroll = target_image_label.y()
                    # Scene coordinates of the text box
                    box_rect_scene = selected_box_item.sceneBoundingRect()
                    # Scale factor from the QGraphicsView transform
                    scale = target_image_label.transform().m11()
                    # Box position relative to the top of the image label, scaled
                    box_top_in_image = box_rect_scene.top() * scale
                    box_bottom_in_image = box_rect_scene.bottom() * scale
                    box_center_y_in_image = box_rect_scene.center().y() * scale
                    # Box absolute position in the entire scrollable area content
                    box_global_top = image_label_y_in_scroll + box_top_in_image
                    box_global_bottom = image_label_y_in_scroll + box_bottom_in_image

                    # 2. Check if the text box is fully visible in the viewport
                    is_visible = (box_global_top >= current_scroll_y) and \
                                 (box_global_bottom <= current_scroll_y + viewport_height)
                    
                    if not is_visible:
                        # 3. Calculate new scroll position to center the box
                        target_scroll_y = image_label_y_in_scroll + box_center_y_in_image - (viewport_height / 2)
                        
                        # 4. Clamp the value to be within scrollbar's valid range and scroll
                        scrollbar = self.scroll_area.verticalScrollBar()
                        clamped_scroll_y = max(scrollbar.minimum(), min(int(target_scroll_y), scrollbar.maximum()))
                        scrollbar.setValue(clamped_scroll_y)

        finally:
            self._is_handling_selection = False

    def handle_text_box_selected(self, row_number, image_label, selected):
        if self._is_handling_selection:
            return
        
        self._is_handling_selection = True
        try:
            if selected:
                self.current_selected_row = row_number
                self.current_selected_image_label = image_label
                self.selected_text_box_item = None
                for tb in image_label.get_text_boxes():
                    if tb.row_number == row_number:
                        self.selected_text_box_item = tb
                        break

                if self.selected_text_box_item:
                    current_style = self.get_style_for_row(row_number)
                    self.style_panel.update_style_panel(current_style)
                    self.style_panel.show()
                else:
                    print(f"ERROR: Could not find TextBoxItem for row {row_number} in label {image_label.filename}")
                    self.style_panel.clear_and_hide()

                for i in range(self.scroll_layout.count()):
                    widget = self.scroll_layout.itemAt(i).widget()
                    if isinstance(widget, ResizableImageLabel) and widget != image_label:
                        widget.deselect_all_text_boxes()
                self.results_widget.scroll_to_row(row_number)

            else:
                if row_number == self.current_selected_row and image_label == self.current_selected_image_label:
                    self.current_selected_row = None
                    self.current_selected_image_label = None
                    self.selected_text_box_item = None
                    self.style_panel.clear_and_hide()
        finally:
            self._is_handling_selection = False

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

    def update_text_box_style(self, new_style_dict):
        if not self.selected_text_box_item:
            print("Style changed but no text box selected.")
            return

        row_number = self.selected_text_box_item.row_number
        target_result, _ = self.model._find_result_by_row_number(row_number)

        if not target_result:
            print(f"Error: Could not find result for row {row_number} to apply style.")
            return

        if target_result.get('is_deleted', False):
             print(f"Warning: Attempting to style a deleted row ({row_number}). Ignoring.")
             return
        
        style_diff = get_style_diff(new_style_dict, DEFAULT_TEXT_STYLE)

        # The model doesn't need to be updated here unless styling is part of the save data
        if style_diff:
            target_result['custom_style'] = style_diff
            print(f"Stored custom style diff for row {row_number}: {style_diff}")
        elif 'custom_style' in target_result:
            del target_result['custom_style']
            print(f"Removed custom style for row {row_number} (back to default).")

        self.selected_text_box_item.apply_styles(new_style_dict)

    def _initialize_ocr_reader(self, context="OCR"):
        """Initializes the EasyOCR reader if it doesn't exist."""
        if self.reader:
            print("EasyOCR reader already initialized.")
            return True
        try:
            # Get language from the model
            lang_code = self.language_map.get(self.model.original_language, 'ko')
            use_gpu = self.settings.value("use_gpu", "true").lower() == "true"
            print(f"Initializing EasyOCR reader for {context}: Lang='{lang_code}', GPU={use_gpu}")
            self.reader = easyocr.Reader([lang_code], gpu=use_gpu, model_storage_directory='OCR/model')
            print("EasyOCR reader initialized successfully.")
            return True
        except Exception as e:
            error_msg = f"Failed to initialize OCR reader for {context}: {str(e)}\n\n" \
                        f"Common causes:\n" \
                        f"- Incorrect language code.\n" \
                        f"- Missing EasyOCR models (try running OCR once).\n" \
                        f"- If using GPU: CUDA/driver issues or insufficient VRAM."
            print(f"Error: {error_msg}")
            traceback.print_exc()
            QMessageBox.critical(self, "OCR Initialization Error", error_msg)
            self.reader = None
            return False

    def _find_result_by_row_number(self, row_number_to_find):
        """ DELEGATED: Asks the model to find the result. """
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
        # 1. Update the results table on the right panel.
        self.results_widget.update_views()

        # 2. Update the text boxes on the images in the left panel.
        # Group all relevant results from the model by filename for efficient lookup.
        grouped_results = {}
        for result in self.model.ocr_results:
            filename = result.get('filename')
            if filename:
                # If a filter is active, skip files not in the list.
                if affected_filenames and filename not in affected_filenames:
                    continue
                if filename not in grouped_results:
                    grouped_results[filename] = {}
                grouped_results[filename][result.get('row_number')] = result

        # 3. Iterate through the image widgets and update their displayed text.
        for i in range(self.scroll_layout.count()):
            widget = self.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel):
                image_filename = widget.filename
                # Only update widgets that are affected.
                if not affected_filenames or image_filename in affected_filenames:
                    # Get the pre-grouped results for this image; defaults to empty dict.
                    results_for_this_image = grouped_results.get(image_filename, {})
                    # Tell the image widget to redraw its text boxes using the provided data.
                    # Note: 'apply_translation' is a legacy name; it redraws text boxes.
                    widget.apply_translation(self, results_for_this_image, DEFAULT_TEXT_STYLE)

    # --- METHOD MODIFIED (Simplified) ---
    def start_ocr(self):
        if not self.model.image_paths:
            QMessageBox.warning(self, "Warning", "No images loaded to process.")
            return
        if self.batch_handler:
            QMessageBox.warning(self, "Warning", "OCR is already running.")
            return
        if self.manual_ocr_handler.is_active:
            QMessageBox.warning(self, "Warning", "Cannot start standard OCR while in Manual OCR mode.")
            return

        print("Starting standard OCR process...")
        if not self._initialize_ocr_reader("Standard OCR"):
            return

        self.btn_process.setVisible(False)
        self.btn_stop_ocr.setVisible(True)

        self.model.clear_standard_results()
        self.on_model_updated(None)
        
        self._load_filter_settings()
        ocr_settings = {
            "min_text_height": self.min_text_height, "max_text_height": self.max_text_height,
            "min_confidence": self.min_confidence, "distance_threshold": self.distance_threshold,
            "batch_size": int(self.settings.value("ocr_batch_size", 8)), "decoder": self.settings.value("ocr_decoder", "beamsearch"),
            "adjust_contrast": float(self.settings.value("ocr_adjust_contrast", 0.5)), "resize_threshold": int(self.settings.value("ocr_resize_threshold", 1024)),
        }
        
        # --- MODIFIED: Pass the progress bar widget directly to the handler ---
        self.batch_handler = BatchOCRHandler(
            image_paths=self.model.image_paths, 
            reader=self.reader, 
            settings=ocr_settings, 
            starting_row_number=self.model.next_global_row_number,
            model=self.model,
            progress_bar=self.ocr_progress # Pass the reference here
        )

        self.batch_handler.batch_finished.connect(self.on_batch_finished)
        self.batch_handler.error_occurred.connect(self.on_batch_error)
        self.batch_handler.processing_stopped.connect(self.on_batch_stopped)
        
        self.batch_handler.start_processing()
    def on_image_processed(self, new_results):
        """ DELEGATED: Adds new OCR results to the model. """
        # The model will emit a signal, and on_model_updated will handle the UI refresh.
        self.model.add_new_ocr_results(new_results)

    # --- METHOD MODIFIED (Simplified) ---
    def on_batch_finished(self, next_row_number):
        """Handles the successful completion of the entire batch."""
        print("MainWindow: Batch finished.")
        self.model.next_global_row_number = next_row_number
        self.cleanup_ocr_session()
        QMessageBox.information(self, "Finished", "OCR processing completed for all images.")
    
    # ... (rest of the file is the same) ...
    def on_batch_error(self, message):
        """Handles a critical error during the batch process."""
        print(f"MainWindow: Batch error received: {message}")
        self.cleanup_ocr_session()
        QMessageBox.critical(self, "OCR Error", message)

    def on_batch_stopped(self):
        """Handles the UI cleanup after the user manually stops the process."""
        print("MainWindow: Batch processing was stopped by user.")
        self.cleanup_ocr_session()
        QMessageBox.information(self, "Stopped", "OCR processing was stopped.")

    # --- METHOD MODIFIED (Centralized Cleanup) ---
    def cleanup_ocr_session(self):
        """Resets UI and state after an OCR run (success, error, or stop)."""
        self.btn_stop_ocr.setVisible(False)
        self.btn_process.setVisible(True)
        self.btn_process.setEnabled(bool(self.model.image_paths))
        self.ocr_progress.reset()
        if self.batch_handler:
            self.batch_handler.deleteLater() # Ensure proper cleanup of the QObject
            self.batch_handler = None
        gc.collect()
        
    def stop_ocr(self):
        """Stops the currently running OCR process by signaling the handler."""
        print("MainWindow: Sending stop request to batch handler...")
        if self.batch_handler:
            self.batch_handler.stop()
        else:
            print("No active batch handler to stop.")
            # If no handler, but UI is stuck, reset it
            self.cleanup_ocr_session()

    def update_image_text_box(self, row_number, new_text):
        """Finds a specific TextBoxItem by its row number and updates its text directly."""
        target_result, _ = self.model._find_result_by_row_number(row_number)
        if not target_result: return

        filename = target_result.get('filename')
        if not filename: return

        for i in range(self.scroll_layout.count()):
            widget = self.scroll_layout.itemAt(i).widget()
            if isinstance(widget, ResizableImageLabel) and widget.filename == filename:
                for tb in widget.get_text_boxes():
                    if tb.row_number == row_number:
                        if tb.text_item and tb.text_item.toPlainText() != new_text:
                            tb.text_item.setPlainText(new_text)
                            tb.adjust_font_size()
                        return # Found and updated, so we can exit

    def update_ocr_text(self, row_number, new_text):
        """
        DELEGATED: Asks the model to update the text, and manually updates the
        image view to prevent a destructive full UI refresh that interrupts editing.
        """
        # Disconnect the main model update signal to prevent the ResultsWidget
        # from being rebuilt, which would cancel the user's text editing.
        try:
            self.model.model_updated.disconnect(self.on_model_updated)
        except TypeError:
            # This is fine, it just means the signal was not connected.
            pass

        try:
            # Update the model's data store.
            if self.model.active_profile_name == "Original":
                 QMessageBox.information(self, "Edit Profile Created",
                                         f"First edit detected. A new profile 'User Edit 1' has been created and set as active. "
                                         "Your original OCR text is preserved.")
            self.model.update_text(row_number, new_text)

            # Manually and directly update the text on the corresponding image label's text box.
            # This fulfills the request for an immediate visual update without a slow global refresh.
            self.update_image_text_box(row_number, new_text)

        finally:
            # Always reconnect the signal to ensure that other operations (like deleting,
            # combining, or loading) still trigger a full UI refresh when needed.
            self.model.model_updated.connect(self.on_model_updated)

    def combine_rows_in_model(self, first_row_number, combined_text, min_confidence, rows_to_delete):
        """ DELEGATED: Asks the model to combine rows. """
        if self.model.active_profile_name == "Original":
             QMessageBox.information(self, "Edit Profile Created",
                                     f"First combination edit detected. A new profile 'User Edit 1' has been created and set as active.")
        
        message, success = self.model.combine_rows(first_row_number, combined_text, min_confidence, rows_to_delete)

        if success:
            if self.find_replace_widget.isVisible():
                self.find_replace_widget.find_text()
            QMessageBox.information(self, "Success", message)
        else:
            QMessageBox.critical(self, "Error", message)
    
    def toggle_advanced_mode(self, state):
        if state:
            self.results_widget.right_content_stack.setCurrentIndex(1)
        else:
            self.results_widget.right_content_stack.setCurrentIndex(0)
        self.results_widget.update_views()

    def delete_row(self, row_number_to_delete):
        """ DELEGATED: Asks the model to delete a row after confirming with the user. """
        # Confirmation logic stays in the UI layer
        show_warning = self.settings.value("show_delete_warning", "true") == "true"
        proceed = True
        if show_warning:
            msg = QMessageBox(self); msg.setIcon(QMessageBox.Warning)
            msg.setWindowTitle("Confirm Deletion Marking"); msg.setText("<b>Mark for Deletion Warning</b>")
            msg.setInformativeText("Mark this entry for deletion? It will be hidden and excluded from exports.")
            msg.setStyleSheet(DELETE_ROW_STYLES)
            dont_show_cb = QCheckBox("Remember choice", msg); msg.setCheckBox(dont_show_cb)
            msg.setStandardButtons(QMessageBox.Yes | QMessageBox.No); msg.setDefaultButton(QMessageBox.No)
            response = msg.exec_()
            if dont_show_cb.isChecked(): self.settings.setValue("show_delete_warning", "false")
            proceed = response == QMessageBox.Yes
        if not proceed: return

        # Delegate to model
        self.model.delete_row(row_number_to_delete)
        
        # UI-specific cleanup
        if self.find_replace_widget.isVisible(): self.find_replace_widget.find_text()
        if row_number_to_delete == self.current_selected_row:
            self.current_selected_row = None
            self.current_selected_image_label = None
            self.selected_text_box_item = None
            self.style_panel.clear_and_hide()

    def start_translation(self):
        api_key = self.settings.value("gemini_api_key", "")
        if not api_key:
            QMessageBox.critical(self, "API Key Missing", "Please set your Gemini API key in Settings.")
            return

        if not self.model.ocr_results:
            QMessageBox.warning(self, "No Data", "There are no OCR results to translate.")
            return
            
        model_name = self.settings.value("gemini_model", "gemini-2.5-flash-lite")

        dialog = TranslationWindow(
            api_key, model_name, self.model.ocr_results, list(self.model.profiles.keys()), self
        )
        dialog.translation_complete.connect(self.handle_translation_completed)
        dialog.exec_()

    def handle_translation_completed(self, profile_name, translated_data):
        """ DELEGATED: Asks the model to add the new profile and data. """
        try:
            self.model.add_profile(profile_name, translated_data)
            QMessageBox.information(self, "Success", 
                f"Translation successfully applied to profile:\n'{profile_name}'")
        except Exception as e:
            QMessageBox.critical(self, "Import Error", f"Failed to apply translation: {str(e)}")
            traceback.print_exc()

    def import_translation(self):
        """ DELEGATED: Uses a helper to get content, then asks the model to add it. """
        profile_name = "Imported Translation"
        try:
            # import_translation_file now needs to return content instead of directly acting on the window
            content = import_translation_file(self) # Assuming this is modified to return content
            if content:
                 # We can use the same import logic as the AI translation
                 translation_data = json.loads(content)
                 self.model.add_profile(profile_name, translation_data)
                 # The model signals will trigger the necessary UI updates
        except Exception as e:
            QMessageBox.critical(self, "Import Error", f"Failed to import and apply translation file: {str(e)}")

    def update_shortcut(self):
        combine_shortcut = self.settings.value("combine_shortcut", "Ctrl+G")
        self.combine_action.setShortcut(QKeySequence(combine_shortcut))
        self.update_find_shortcut()

    def export_manhwa(self):
        export_rendered_images(self)

    # --- NEW: Wrapper method for the menu to call ---
    def export_ocr_results(self):
        """Wrapper method that calls the utility function for exporting OCR results."""
        # The utility function needs the main window instance to access the data model.
        export_ocr_results(self)

    def save_project(self):
        """ DELEGATED: Asks the model to save the project. """
        result_message = self.model.save_project()
        if "successfully" in result_message:
            QMessageBox.information(self, "Saved", result_message)
        else:
            QMessageBox.critical(self, "Save Error", result_message)

    def closeEvent(self, event):
        # This now reads from self.model
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