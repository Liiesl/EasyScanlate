# main_window.py

from PySide6.QtWidgets import (QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QSizePolicy, QCheckBox, QPushButton,
                             QMessageBox, QSplitter, QComboBox)
from PySide6.QtCore import Qt, QSettings, QPoint, QRectF
from PySide6.QtGui import QPixmap, QKeySequence, QAction, QColor
import qtawesome as qta
from app.utils.file_io import export_ocr_results, import_translation_file, export_rendered_images
from app.utils.update import UpdateHandler # <-- IMPORT
from app.ui.components import ResizableImageLabel, CustomScrollArea, ResultsWidget, TextBoxStylePanel, FindReplaceWidget
from app.ui.widgets.menu_bar import MenuBar
from app.ui.widgets.progress_bar import CustomProgressBar
from app.ui.widgets.menus import Menu
from app.handlers import BatchOCRHandler, SelectionManager
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
        
        self.model = ProjectModel()
        self.model.project_loaded.connect(self.on_project_loaded)
        self.model.project_load_failed.connect(self.on_project_load_failed)
        self.model.model_updated.connect(self.on_model_updated)
        self.model.profiles_updated.connect(self.update_profile_selector)

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
        self.check_for_updates_on_startup() # <-- NEW METHOD CALL

    def check_for_updates_on_startup(self):
        """Creates an UpdateHandler to check for new versions if enabled."""
        if self.settings.value("auto_check_updates", "true") == "true":
            print("Checking for updates on startup...")
            # The handler must be an instance variable to avoid being garbage-collected
            self.update_handler = UpdateHandler(self)
            self.update_handler.update_check_finished.connect(self.on_startup_update_check_finished)
            self.update_handler.check_for_updates()

    def on_startup_update_check_finished(self, update_available, update_info):
        """Shows a message box if an update was found on startup."""
        if update_available:
            print(f"Update available: {update_info.get('version')}")
            msg_box = QMessageBox(self)
            msg_box.setIcon(QMessageBox.Information)
            msg_box.setText(f"A new version ({update_info.get('version')}) is available!")
            msg_box.setInformativeText("You can download and install it from the Settings menu.")
            open_button = msg_box.addButton("Open Settings", QMessageBox.ActionRole)
            msg_box.addButton(QMessageBox.Ok)
            msg_box.exec()
            
            if msg_box.clickedButton() == open_button:
                self.show_settings_dialog()
        else:
            print("No new updates found on startup.")
        
        # Clean up the handler
        self.update_handler.deleteLater()

    # --- NO OTHER CHANGES TO main_window.py ---
    
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
        main_widget.setObjectName("CentralWidget")
        main_layout = QHBoxLayout()
        self.colors = COLORS
        self.setStyleSheet(MAIN_STYLESHEET)
        self.update_profile_selector()

        left_panel = QVBoxLayout()
        left_panel.setSpacing(20)

        settings_layout = QHBoxLayout()
        self.btn_settings = QPushButton(qta.icon('fa5s.cog', color='white'), "")
        self.btn_settings.setFixedSize(50, 50)
        self.btn_settings.clicked.connect(self.show_settings_dialog)
        settings_layout.addWidget(self.btn_settings)

        self.ocr_progress = CustomProgressBar()
        self.ocr_progress.setFixedHeight(20)
        settings_layout.addWidget(self.ocr_progress, 1)
        left_panel.addLayout(settings_layout)

        self.scroll_area = CustomScrollArea(main_window=self)
        
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
        self.btn_manual_ocr.toggled.connect(self.scroll_area.manual_ocr_handler.toggle_mode)
        self.btn_manual_ocr.setEnabled(False)
        button_layout.addWidget(self.btn_manual_ocr)
        
        file_button_layout = QHBoxLayout()
        file_button_layout.setAlignment(Qt.AlignRight)
        file_button_layout.setSpacing(20)

        self.profile_selector = QComboBox(self)
        self.profile_selector.setFixedWidth(220)
        self.profile_selector.setToolTip("Switch between different text profiles (e.g., Original, User Edits, Translations).")
        self.profile_selector.activated.connect(self.on_profile_selected)
        file_button_layout.addWidget(self.profile_selector)

        self.btn_import_export_menu = QPushButton(qta.icon('fa5s.bars', color='white'), "")
        self.btn_import_export_menu.setFixedWidth(60)
        self.btn_import_export_menu.setToolTip("Open Import/Export Menu")
        self.btn_import_export_menu.clicked.connect(self.show_import_export_menu)
        file_button_layout.addWidget(self.btn_import_export_menu)
        button_layout.addLayout(file_button_layout)
        right_panel.addLayout(button_layout)

        self.right_content_splitter = QSplitter(Qt.Horizontal)
        self.style_panel = TextBoxStylePanel(default_style=DEFAULT_TEXT_STYLE)
        self.style_panel.hide()
        self.right_content_splitter.addWidget(self.style_panel)

        self.results_widget = ResultsWidget(self, self.combine_action, self.find_action, self.selection_manager)

        self.find_replace_widget = FindReplaceWidget(self)
        right_panel.addWidget(self.find_replace_widget)
        self.find_replace_widget.hide()

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

    def switch_active_profile(self, profile_name):
        """Tells the model to switch the active profile."""
        if profile_name and profile_name in self.model.profiles and profile_name != self.model.active_profile_name:
            print(f"Switching to active profile: {profile_name}")
            self.model.active_profile_name = profile_name
            self.on_model_updated(None)

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
        self.model.load_project(mmtl_path, temp_dir)

    def on_project_load_failed(self, error_msg):
        QMessageBox.critical(self, "Project Load Error", error_msg)
        self.close()

    def on_project_loaded(self):
        """ Populates the UI after the model has loaded a project. """
        self._clear_layout(self.scroll_layout)
        self.scroll_area.cancel_active_modes()

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

    def get_display_text(self, result):
        """ DELEGATED: Asks the model for the correct text to display. """
        return self.model.get_display_text(result)

    def on_selection_changed(self, row_number, source):
        """
        Updates the style panel based on the currently selected row.
        """
        if row_number is not None:
            current_style = self.get_style_for_row(row_number)
            self.style_panel.update_style_panel(current_style)
            self.style_panel.show()
        else:
            self.style_panel.clear_and_hide()

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
        """Initializes the EasyOCR reader if it doesn't exist."""
        if self.reader:
            print("EasyOCR reader already initialized.")
            return True
        try:
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

    def start_ocr(self):
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
            "auto_context_fill": self.settings.value("auto_context_fill", "false").lower() == "true"
        }
        self.batch_handler = BatchOCRHandler(
            image_paths=self.model.image_paths, 
            reader=self.reader, 
            settings=ocr_settings, 
            starting_row_number=self.model.next_global_row_number,
            model=self.model,
            progress_bar=self.ocr_progress
        )
        self.batch_handler.batch_finished.connect(self.on_batch_finished)
        self.batch_handler.error_occurred.connect(self.on_batch_error)
        self.batch_handler.processing_stopped.connect(self.on_batch_stopped)
        self.batch_handler.auto_inpaint_requested.connect(self.on_auto_inpaint_requested)
        self.batch_handler.start_processing()

    def on_image_processed(self, new_results):
        """ DELEGATED: Adds new OCR results to the model. """
        self.model.add_new_ocr_results(new_results)

    def on_batch_finished(self, next_row_number):
        """Handles the successful completion of the entire batch."""
        print("MainWindow: Batch finished.")
        self.model.next_global_row_number = next_row_number
        self.cleanup_ocr_session()
        QMessageBox.information(self, "Finished", "OCR processing completed for all images.")
    
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

    def cleanup_ocr_session(self):
        """Resets UI and state after an OCR run (success, error, or stop)."""
        self.btn_stop_ocr.setVisible(False)
        self.btn_process.setVisible(True)
        self.btn_process.setEnabled(bool(self.model.image_paths))
        self.ocr_progress.reset()
        if self.batch_handler:
            self.batch_handler.deleteLater()
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

    def on_auto_inpaint_requested(self, filename, bounding_boxes):
        """
        SLOT: Handles the request from BatchOCRHandler to perform automatic inpainting.
        """
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
        try:
            self.model.model_updated.disconnect(self.on_model_updated)
        except TypeError:
            pass

        try:
            if self.model.active_profile_name == "Original":
                 QMessageBox.information(self, "Edit Profile Created",
                                         f"First edit detected. A new profile 'User Edit 1' has been created and set as active. "
                                         "Your original OCR text is preserved.")
            self.model.update_text(row_number, new_text)
            self.update_image_text_box(row_number, new_text)
        finally:
            self.model.model_updated.connect(self.on_model_updated)

    def combine_rows_in_model(self, first_row_number, combined_text, min_confidence, rows_to_delete):
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
        self.results_widget.right_content_stack.setCurrentIndex(1 if state else 0)
        self.results_widget.update_views()

    def delete_row(self, row_number_to_delete):
        show_warning = self.settings.value("show_delete_warning", "true") == "true"
        proceed = True
        if show_warning:
            msg = QMessageBox(self)
            msg.setIcon(QMessageBox.Warning)
            msg.setWindowTitle("Confirm Deletion Marking")
            msg.setText("<b>Mark for Deletion Warning</b>")
            msg.setInformativeText("Mark this entry for deletion? It will be hidden and excluded from exports.")
            msg.setStyleSheet(DELETE_ROW_STYLES)
            dont_show_cb = QCheckBox("Remember choice", msg)
            msg.setCheckBox(dont_show_cb)
            dont_show_cb.setStyleSheet(ADVANCED_CHECK_STYLES)
            msg.setStandardButtons(QMessageBox.Yes | QMessageBox.No) 
            msg.setDefaultButton(QMessageBox.No)
            response = msg.exec_()
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
        dialog.exec_()

    def handle_translation_completed(self, profile_name, translated_data):
        try:
            self.model.add_profile(profile_name, translated_data)
            QMessageBox.information(self, "Success", 
                f"Translation successfully applied to profile:\n'{profile_name}'")
        except Exception as e:
            QMessageBox.critical(self, "Import Error", f"Failed to apply translation: {str(e)}")
            traceback.print_exc()

    def import_translation(self):
        profile_name = "Imported Translation"
        try:
            content = import_translation_file(self)
            if content:
                 translation_data = json.loads(content)
                 self.model.add_profile(profile_name, translation_data)
        except Exception as e:
            QMessageBox.critical(self, "Import Error", f"Failed to import and apply translation file: {str(e)}")

    def update_shortcut(self):
        combine_shortcut = self.settings.value("combine_shortcut", "Ctrl+G")
        self.combine_action.setShortcut(QKeySequence(combine_shortcut))
        self.update_find_shortcut()

    def export_manhwa(self):
        export_rendered_images(self)

    def export_ocr_results(self):
        export_ocr_results(self)

    def save_project(self):
        result_message = self.model.save_project()
        if "successfully" in result_message:
            QMessageBox.information(self, "Saved", result_message)
        else:
            QMessageBox.critical(self, "Save Error", result_message)

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