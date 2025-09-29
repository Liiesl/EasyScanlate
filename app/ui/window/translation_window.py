from PySide6.QtWidgets import ( QDialog, QVBoxLayout, QHBoxLayout, QPushButton, QLabel, QComboBox,
                             QScrollArea, QTextEdit, QFrame, QGridLayout, QCheckBox, QProgressBar, 
                             QMessageBox, QWidget, QSplitter )
from PySide6.QtCore import Qt, QSize, Signal, QEvent, QTimer
from PySide6.QtGui import QShortcut, QKeySequence
import qtawesome as qta
from app.core.translations import TranslationThread, _get_text_for_profile_static, generate_for_translate_content, generate_retranslate_content, import_translation_file_content

from app.ui.dialogs import GEMINI_MODELS_WITH_INFO
from assets import ADVANCED_CHECK_STYLES

# Style constants for row highlighting
SELECTED_STYLE = "QFrame { background-color: #385675; border: 1px solid #78909c; border-radius: 4px; }"
DEFAULT_STYLE = "QFrame { background-color: #2E2E2E; border: 1px solid #444; border-radius: 4px; }"
PLACEHOLDER_STYLE = "QFrame { background-color: #252525; border: 1px solid #444; border-radius: 4px; color: #888; }"

class TranslationWindow(QDialog):
    """ A dialog window to manage the translation process with an integrated,
    multi-column comparison view and a chat-like interface for Gemini. """
    translation_complete = Signal(str, dict)

    def __init__(self, api_key, model_name, ocr_results, profiles, parent=None):
        super().__init__(parent)
        self.api_key = api_key
        self.model_name = model_name
        self.ocr_results = [res for res in ocr_results if not res.get('is_deleted', False)]
        self.profiles = profiles
        self.thread = None
        self.select_all_checkbox = None # For header checkbox
        
        self.translation_columns = []  # Manages data for each translation column
        self.active_translation_index = -1 # Tracks which column is being translated
        self.current_gemini_bubble_label = None # For streaming response

        # --- Row Selection and Widget Tracking ---
        self.row_widgets = {}           # Stores all widgets for a given row key
        self.clickable_frames = {}      # Maps a QFrame or QLabel widget back to its row key
        self.all_row_keys_in_order = [] # A list of all row keys, to enable Shift+Click
        self.last_clicked_row_key = None  # For Shift+Click range selection

        self.target_languages = [
            "English", "Japanese", "Chinese (Simplified)", "Korean", "Spanish", 
            "French", "German", "Bahasa Indonesia", "Vietnamese", "Thai", 
            "Russian", "Portuguese"
        ]

        self.setWindowTitle("Gemini Translation")
        self.setMinimumSize(1400, 800)
        self.init_ui()
        self._initialize_columns() # Populates columns from existing data or adds a default one.

    def eventFilter(self, source, event):
        if event.type() == QEvent.MouseButtonPress and source in self.clickable_frames:
            row_key = self.clickable_frames[source]
            modifiers = event.modifiers()

            if modifiers == Qt.ShiftModifier:
                if self.last_clicked_row_key and self.last_clicked_row_key in self.all_row_keys_in_order:
                    try:
                        start_idx = self.all_row_keys_in_order.index(self.last_clicked_row_key)
                        end_idx = self.all_row_keys_in_order.index(row_key)
                        
                        if start_idx > end_idx:
                            start_idx, end_idx = end_idx, start_idx
                        
                        # In shift-select, we add to the current selection
                        for i in range(start_idx, end_idx + 1):
                            key_in_range = self.all_row_keys_in_order[i]
                            checkbox = self.row_widgets.get(key_in_range, {}).get('checkbox')
                            if checkbox and not checkbox.isChecked():
                               checkbox.setChecked(True) # This triggers the signal handler
                    except ValueError:
                        pass # Should not happen with consistent data
            elif modifiers == Qt.ControlModifier:
                checkbox = self.row_widgets.get(row_key, {}).get('checkbox')
                if checkbox:
                    checkbox.setChecked(not checkbox.isChecked()) # Triggers the signal handler
                self.last_clicked_row_key = row_key
            else: # Normal click
                for key, widgets in self.row_widgets.items():
                    checkbox = widgets.get('checkbox')
                    if checkbox:
                        is_current_row = (key == row_key)
                        if checkbox.isChecked() != is_current_row:
                            checkbox.blockSignals(True)
                            checkbox.setChecked(is_current_row)
                            checkbox.blockSignals(False)
                            self._update_row_style(key) # Manually update style

                self.last_clicked_row_key = row_key
                
                # Since signals were blocked, we must now manually update global state
                self._update_send_button_state()
                self._update_select_all_checkbox_state()
            
            return True # Event was handled

        return super().eventFilter(source, event)

    def init_ui(self):
        main_layout = QVBoxLayout(self)
        splitter = QSplitter(Qt.Horizontal, self)
        
        # --- Left Panel (Multi-Column Comparison View) ---
        comparison_panel = QWidget()
        comparison_layout = QVBoxLayout(comparison_panel)
        comparison_layout.setContentsMargins(0, 0, 0, 0)
        
        self.source_profile_combo = QComboBox(self)
        non_gemini_profiles = [p for p in self.profiles if not p.startswith("Gemini Translation (")]
        sorted_profiles = sorted(non_gemini_profiles)
        if "Original" in sorted_profiles:
            sorted_profiles.remove("Original")
            sorted_profiles.insert(0, "Original")
        self.source_profile_combo.addItems(sorted_profiles)
        self.source_profile_combo.setCurrentText("Original")
        self.source_profile_combo.currentIndexChanged.connect(self._source_profile_changed)

        self.comparison_scroll_area = QScrollArea()
        self.comparison_scroll_area.setWidgetResizable(True)
        comparison_layout.addWidget(self.comparison_scroll_area)
        
        # --- Right Panel (Chat Interface) ---
        chat_panel = QWidget()
        chat_layout = QVBoxLayout(chat_panel)
        chat_layout.setContentsMargins(0, 0, 0, 0)
        chat_layout.setSpacing(0)
        
        self.chat_scroll_area = QScrollArea()
        self.chat_scroll_area.setWidgetResizable(True)
        self.chat_scroll_area.setFrameShape(QFrame.NoFrame)
        self.chat_scroll_area.setStyleSheet("QScrollArea { background-color: #2c2c2c; }")

        chat_container_widget = QWidget()
        self.chat_container_layout = QVBoxLayout(chat_container_widget)
        self.chat_container_layout.addStretch(1) 
        self.chat_scroll_area.setWidget(chat_container_widget)

        input_area_frame = QFrame()
        input_area_frame.setObjectName("inputAreaFrame")
        input_area_frame.setStyleSheet("#inputAreaFrame { background-color: #2c2c2c; border-top: 1px solid #444; }")
        input_area_layout = QVBoxLayout(input_area_frame)
        input_area_layout.setContentsMargins(10, 10, 10, 10)
        input_area_layout.setSpacing(10)
        
        self.prompt_input_edit = QTextEdit(self)
        self.prompt_input_edit.setMaximumHeight(120)
        self.prompt_input_edit.setPlaceholderText("Describe how to translate (e.g., 'Translate formally'). The target language profile is selected below. Ctrl+Enter to send.")
        self.prompt_input_edit.setStyleSheet("QTextEdit { border: 1px solid #555; border-radius: 18px; padding: 10px; padding-left: 15px; background-color: #383838; }")

        bottom_bar = QWidget()
        bottom_bar_layout = QHBoxLayout(bottom_bar)
        bottom_bar_layout.setContentsMargins(0, 0, 0, 0)
        bottom_bar_layout.setSpacing(10)

        self.attachment_widget = self._create_attachment_widget()
        self.prompt_target_combo = QComboBox(self)
        self.prompt_target_combo.currentIndexChanged.connect(self._update_prompt_text_with_language)
        
        self.send_button = QPushButton(self)
        # Icon and tooltip are set dynamically by _update_send_button_state()
        self.send_button.setIconSize(QSize(18, 18))
        self.send_button.setFixedSize(40, 40)
        self.send_button.setStyleSheet("QPushButton { background-color: #0b57d0; border-radius: 20px; padding: 5px; } QPushButton:hover { background-color: #1c6aeb; } QPushButton:pressed { background-color: #2f79f2; } QPushButton:disabled { background-color: #444; }")
        self.send_button.clicked.connect(self.start_translation_process)
        
        bottom_bar_layout.addWidget(self.attachment_widget)
        bottom_bar_layout.addWidget(self.prompt_target_combo, 1)
        bottom_bar_layout.addWidget(self.send_button)

        input_area_layout.addWidget(self.prompt_input_edit)
        input_area_layout.addWidget(bottom_bar)

        shortcut_send = QShortcut(QKeySequence("Ctrl+Return"), self.prompt_input_edit)
        shortcut_send.activated.connect(self.send_button.click)

        chat_layout.addWidget(self.chat_scroll_area, 1)
        chat_layout.addWidget(input_area_frame)
        
        splitter.addWidget(comparison_panel)
        splitter.addWidget(chat_panel)
        splitter.setSizes([800, 600])
        main_layout.addWidget(splitter, 1)

        button_layout = QHBoxLayout()

        # --- Gemini Model Selection Dropdown ---
        model_label = QLabel("Model:")
        self.model_combo = QComboBox(self)
        for model_name, model_info_text in GEMINI_MODELS_WITH_INFO:
            display_text = f"{model_name} | {model_info_text}"
            self.model_combo.addItem(display_text, userData=model_name)
        
        current_model_value = self.model_name
        for i in range(self.model_combo.count()):
            if self.model_combo.itemData(i) == current_model_value:
                self.model_combo.setCurrentIndex(i)
                break
        self.model_combo.setMinimumWidth(300)
        # --- End Model Selection ---

        self.progress_bar = QProgressBar(self)
        self.progress_bar.setVisible(False)
        self.progress_bar.setRange(0,0)
        self.progress_bar.setMaximumWidth(300)
        
        self.apply_button = QPushButton("Apply to Project", self)
        self.apply_button.setEnabled(False)
        self.apply_button.clicked.connect(self.apply_and_close)
        
        self.close_button = QPushButton("Close", self)
        self.close_button.clicked.connect(self.reject)
        
        button_layout.addWidget(model_label)
        button_layout.addWidget(self.model_combo)
        button_layout.addStretch()
        button_layout.addWidget(self.progress_bar)
        button_layout.addWidget(self.apply_button)
        button_layout.addWidget(self.close_button)
        main_layout.addLayout(button_layout)
        
        self.add_column_button = QPushButton(qta.icon('fa5s.plus'), "")
        self.add_column_button.setToolTip("Add new translation column")
        self.add_column_button.clicked.connect(self._handle_add_column_button)

    def _initialize_columns(self):
        """ Initializes translation columns, loading any existing translation
        profiles from the project data. If none exist, adds a default empty column. """
        prefix = "Gemini Translation ("
        existing_translation_profiles = sorted(
            [p for p in self.profiles if p.startswith(prefix) and p.endswith(')')]
        )

        if not existing_translation_profiles:
            self._add_translation_column()
        else:
            for profile_name in existing_translation_profiles:
                try:
                    language = profile_name[len(prefix):-1]
                    data = self._extract_data_for_profile(profile_name)
                    self._add_translation_column(language=language, initial_data=data)
                except Exception as e:
                    print(f"Could not load existing profile '{profile_name}': {e}")

        self._rebuild_grid()
        self._update_prompt_target_combo()

    def _extract_data_for_profile(self, profile_name):
        """ Gathers all translation data for a specific profile from the ocr_results. """
        profile_data = {}
        for result in self.ocr_results:
            filename = result.get('filename')
            row_number = str(result.get('row_number'))
            translated_text = result.get('translations', {}).get(profile_name)

            if all([filename, row_number, translated_text]):
                if filename not in profile_data:
                    profile_data[filename] = {}
                profile_data[filename][row_number] = translated_text
        return profile_data

    def _source_profile_changed(self):
        """Rebuilds the grid when the source profile changes."""
        self._rebuild_grid()

    def _get_text_for_profile(self, result, profile_name):
        """Gets the text for a given result based on the specified profile."""
        return _get_text_for_profile_static(result, profile_name)

    def _add_translation_column(self, language=None, initial_data=None):
        """Adds a new translation column to the data structure."""
        column_index = len(self.translation_columns)
        
        lang_combo = QComboBox(self)
        lang_combo.addItems(self.target_languages)
        
        if language and language in self.target_languages:
            lang_combo.setCurrentText(language)
        else:
            try: 
                lang_to_select = self.target_languages[column_index % len(self.target_languages)]
                lang_combo.setCurrentText(lang_to_select)
            except IndexError:
                lang_combo.setCurrentIndex(0)

        lang_combo.currentIndexChanged.connect(self._update_prompt_target_combo)

        column_data = {
            'id': column_index,
            'language_combo': lang_combo,
            'widgets': {}, # Now stores {row_key: label_widget}
            'translations': initial_data if initial_data else {}
        }
        self.translation_columns.append(column_data)
        
    def _handle_add_column_button(self):
        """Handles the click of the '+' button to add a new, empty column and refresh UI."""
        self._add_translation_column()
        self._rebuild_grid()
        self._update_prompt_target_combo()

    def _rebuild_grid(self):
        """Rebuilds the entire comparison grid, with a central checkbox column and row selection."""
        # Reset row-level widget trackers
        self.row_widgets.clear()
        self.all_row_keys_in_order.clear()
        self.clickable_frames.clear()
        self.last_clicked_row_key = None

        container = QWidget()
        grid = QGridLayout(container)
        grid.setSpacing(10)
        grid.setContentsMargins(10, 10, 10, 10)

        for col_data in self.translation_columns:
            col_data['widgets'].clear()

        # --- Grid Headers ---
        source_header_widget = QWidget()
        source_header_layout = QHBoxLayout(source_header_widget)
        source_header_layout.setContentsMargins(0, 0, 0, 0)
        source_header_layout.setSpacing(5)
        source_label = QLabel("<b>Source:</b>")
        source_header_layout.addWidget(source_label)
        source_header_layout.addWidget(self.source_profile_combo)
        grid.addWidget(source_header_widget, 0, 0)

        # "Select All" checkbox in the header row
        self.select_all_checkbox = QCheckBox()
        self.select_all_checkbox.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.select_all_checkbox.setTristate(True)
        self.select_all_checkbox.setToolTip("Select/Deselect All Rows")
        self.select_all_checkbox.stateChanged.connect(self._on_select_all_changed)
        grid.addWidget(self.select_all_checkbox, 0, 1, Qt.AlignCenter)

        for col_idx, col_data in enumerate(self.translation_columns):
            grid.addWidget(col_data['language_combo'], 0, 2 + col_idx)
        
        add_button_col = 2 + len(self.translation_columns)
        grid.addWidget(self.add_column_button, 0, add_button_col, Qt.AlignVCenter | Qt.AlignLeft)

        # --- Grid Rows ---
        current_source_profile = self.source_profile_combo.currentText()
        for row_idx, result in enumerate(self.ocr_results, start=1):
            filename = result.get('filename')
            row_number = str(result.get('row_number'))
            row_key = (filename, row_number)
            
            self.all_row_keys_in_order.append(row_key)
            self.row_widgets[row_key] = {'translation_boxes': [], 'translation_labels': []}

            # Col 0: Source Text Box
            source_text = self._get_text_for_profile(result, current_source_profile)
            source_box = self._create_text_box(source_text)
            source_box.installEventFilter(self)
            self.clickable_frames[source_box] = row_key
            source_label = source_box.findChild(QLabel, "contentLabel")
            if source_label:
                source_label.installEventFilter(self)
                self.clickable_frames[source_label] = row_key
            self.row_widgets[row_key]['source_box'] = source_box
            grid.addWidget(source_box, row_idx, 0)

            # Col 1: CheckBox
            checkbox = QCheckBox()
            checkbox.setStyleSheet(ADVANCED_CHECK_STYLES)
            checkbox.setChecked(True) # Default to checked
            checkbox.stateChanged.connect(lambda state, k=row_key: self._on_checkbox_state_changed(k))
            self.row_widgets[row_key]['checkbox'] = checkbox
            grid.addWidget(checkbox, row_idx, 1, Qt.AlignCenter)

            # Col 2+: Translation Text Boxes
            for col_idx, col_data in enumerate(self.translation_columns):
                translated_text = col_data.get('translations', {}).get(filename, {}).get(row_number, "...")
                
                translated_box = self._create_text_box(translated_text)
                translated_box.installEventFilter(self)
                self.clickable_frames[translated_box] = row_key
                
                translation_label = translated_box.findChild(QLabel, "contentLabel")
                if translation_label:
                    translation_label.installEventFilter(self)
                    self.clickable_frames[translation_label] = row_key
                    
                col_data['widgets'][row_key] = translation_label
                self.row_widgets[row_key]['translation_boxes'].append(translated_box)
                self.row_widgets[row_key]['translation_labels'].append(translation_label)

                grid.addWidget(translated_box, row_idx, 2 + col_idx)

        # After building the grid, apply initial styling and button state for all rows
        for key in self.all_row_keys_in_order:
            self._update_row_style(key)
        self._update_send_button_state()
        self._update_select_all_checkbox_state() # Set initial state

        # --- Grid Layout Stretching ---
        grid.setColumnStretch(0, 3) # Source
        grid.setColumnStretch(1, 0) # Checkbox
        for i in range(len(self.translation_columns)):
            grid.setColumnStretch(2 + i, 3)
        grid.setColumnStretch(2 + len(self.translation_columns), 0) # Add button column
        grid.setColumnStretch(3 + len(self.translation_columns), 1) # Spacer

        grid.setRowStretch(len(self.ocr_results) + 1, 1)
        self.comparison_scroll_area.setWidget(container)

    def _on_select_all_changed(self, state):
        """Handler for the main 'Select All' header checkbox."""
        if self.select_all_checkbox.sender() is not self.select_all_checkbox:
            return

        is_checked = (state != Qt.Unchecked)
        
        for row_key, widgets in self.row_widgets.items():
            checkbox = widgets.get('checkbox')
            if checkbox:
                # Block signals on the individual checkbox to prevent
                # _on_checkbox_state_changed from firing for every row.
                checkbox.blockSignals(True)
                checkbox.setChecked(is_checked)
                checkbox.blockSignals(False)
                
                # Since the signal was blocked, we must manually update the row's appearance.
                self._update_row_style(row_key)

        # After all rows are updated, update the global UI elements.
        self._update_send_button_state()
        
        # Finally, we must ensure the master checkbox itself reflects the new state,
        # in case a click on a 'PartiallyChecked' box should result in a 'Checked' one.
        self.select_all_checkbox.blockSignals(True)
        self.select_all_checkbox.setCheckState(Qt.Checked if is_checked else Qt.Unchecked)
        self.select_all_checkbox.blockSignals(False)


    def _update_select_all_checkbox_state(self):
        """Updates the header checkbox state based on the state of individual row checkboxes."""
        if not self.select_all_checkbox or not self.row_widgets:
            if self.select_all_checkbox:
                self.select_all_checkbox.blockSignals(True)
                self.select_all_checkbox.setCheckState(Qt.Unchecked)
                self.select_all_checkbox.blockSignals(False)
            return

        checked_count = sum(1 for w in self.row_widgets.values() if w['checkbox'].isChecked())
        total_rows = len(self.row_widgets)
        
        self.select_all_checkbox.blockSignals(True)
        if checked_count == 0:
            self.select_all_checkbox.setCheckState(Qt.Unchecked)
        elif checked_count == total_rows:
            self.select_all_checkbox.setCheckState(Qt.Checked)
        else:
            self.select_all_checkbox.setCheckState(Qt.PartiallyChecked)
        self.select_all_checkbox.blockSignals(False)

    def _on_checkbox_state_changed(self, row_key):
        """Handler for when a checkbox is toggled by any means."""
        self._update_row_style(row_key)
        self._update_send_button_state()
        self._update_select_all_checkbox_state() # Update header checkbox

    def _update_row_style(self, row_key):
        """Updates the background color of all frames in a row based on its checkbox state."""
        widgets = self.row_widgets.get(row_key)
        if not widgets:
            return

        is_checked = widgets['checkbox'].isChecked()

        if is_checked:
            style = SELECTED_STYLE
            widgets['source_box'].setStyleSheet(style)
            for box in widgets['translation_boxes']:
                box.setStyleSheet(style)
        else:
            # Restore default styles
            widgets['source_box'].setStyleSheet(DEFAULT_STYLE)
            
            # For translation boxes, style depends on content
            for i, box in enumerate(widgets['translation_boxes']):
                label = widgets['translation_labels'][i]
                if label.text() == "...":
                     box.setStyleSheet(PLACEHOLDER_STYLE)
                else:
                     box.setStyleSheet(DEFAULT_STYLE)

    def _update_send_button_state(self):
        """Updates the send button's icon and tooltip based on row selection."""
        all_selected = True
        if self.row_widgets:
            all_selected = all(widgets['checkbox'].isChecked() for widgets in self.row_widgets.values())

        if all_selected:
            self.send_button.setIcon(qta.icon('fa5s.paper-plane', color='#ffffff'))
            self.send_button.setToolTip("Translate All (Ctrl+Enter)")
        else:
            self.send_button.setIcon(qta.icon('fa5s.sync-alt', color='#ffffff'))
            self.send_button.setToolTip("Retranslate Selected (Ctrl+Enter)")

    def _update_prompt_target_combo(self):
        """Updates the dropdown for selecting a translation profile to target."""
        current_selection = self.prompt_target_combo.currentData()
        self.prompt_target_combo.clear()
        for i, col_data in enumerate(self.translation_columns):
            lang = col_data['language_combo'].currentText()
            self.prompt_target_combo.addItem(f"({lang})", i)
        
        if current_selection is not None and current_selection < self.prompt_target_combo.count():
            self.prompt_target_combo.setCurrentIndex(current_selection)

        self._update_prompt_text_with_language()

    def _update_prompt_text_with_language(self):
        """
        Updates the main prompt input with the correct target language from the combo box.
        """
        if self.prompt_target_combo.count() == 0:
            return

        combo_text = self.prompt_target_combo.currentText()
        try:
            target_language = combo_text.split('(')[1].strip(')')
        except IndexError:
            target_language = combo_text

        current_text = self.prompt_input_edit.toPlainText()
        base_prompt_template = "translate and change only the korean text to {target_lang}, keep everything else."
        sorted_langs = sorted(self.target_languages, key=len, reverse=True)
        found_lang_to_replace = None
        for lang in sorted_langs:
            if lang in current_text:
                found_lang_to_replace = lang
                break

        if found_lang_to_replace:
            new_prompt_text = current_text.replace(found_lang_to_replace, target_language)
            self.prompt_input_edit.setPlainText(new_prompt_text)
        elif not current_text.strip():
            new_prompt_text = base_prompt_template.format(target_lang=target_language)
            self.prompt_input_edit.setPlainText(new_prompt_text)

    def _add_chat_bubble(self, sender, text, is_streaming=False):
        message_widget = QWidget()
        message_layout = QHBoxLayout(message_widget)
        message_layout.setContentsMargins(10, 5, 10, 5)
        message_layout.setSpacing(0)

        bubble = QFrame()
        bubble.setFrameShape(QFrame.StyledPanel)
        bubble_layout = QVBoxLayout(bubble)
        bubble_layout.setContentsMargins(12, 8, 12, 8)

        name_label = QLabel(f"<b>{sender}</b>")
        text_label = QLabel(text)
        text_label.setWordWrap(True)
        text_label.setTextInteractionFlags(Qt.TextSelectableByMouse)
        text_label.setOpenExternalLinks(True)

        bubble_layout.addWidget(name_label)
        bubble_layout.addWidget(text_label)
        bubble.setMaximumWidth(int(self.chat_scroll_area.width() * 0.8))

        if sender == "You":
            bubble.setStyleSheet("background-color: #0b57d0; color: white; border-radius: 12px;")
            name_label.setStyleSheet("color: #e0e0e0; font-weight: bold; margin-bottom: 3px;")
            message_layout.addStretch()
            message_layout.addWidget(bubble)
        elif sender == "Gemini":
            bubble.setStyleSheet("background-color: #3c4043; color: #e8eaed; border-radius: 12px;")
            name_label.setStyleSheet("color: #bbb; font-weight: bold; margin-bottom: 3px;")
            if is_streaming:
                self.current_gemini_bubble_label = text_label
            message_layout.addWidget(bubble)
            message_layout.addStretch()
        elif sender == "Error":
            bubble.setStyleSheet("background-color: #4d2d2d; color: #ff8e8e; border: 1px solid #884444; border-radius: 12px;")
            name_label.setText("<b>SYSTEM ERROR</b>")
            name_label.setStyleSheet("color: #ffc9c9; font-weight: bold; margin-bottom: 3px;")
            message_layout.addWidget(bubble)
            message_layout.addStretch()

        self.chat_container_layout.insertWidget(self.chat_container_layout.count() - 1, message_widget)
        QTimer.singleShot(50, self._scroll_chat_to_bottom)

    def _scroll_chat_to_bottom(self):
        scroll_bar = self.chat_scroll_area.verticalScrollBar()
        scroll_bar.setValue(scroll_bar.maximum())

    def _create_attachment_widget(self):
        widget = QFrame()
        widget.setFrameShape(QFrame.NoFrame)
        widget.setFixedHeight(40)
        widget.setStyleSheet("QFrame { background-color: #3c4043; border: 1px solid #5f6368; border-radius: 20px; }")
        layout = QHBoxLayout(widget)
        layout.setContentsMargins(10, 5, 15, 5)
        layout.setSpacing(8)
        
        icon = qta.icon('fa5s.file-alt', color='#e8eaed')
        icon_label = QLabel()
        icon_label.setPixmap(icon.pixmap(18, 18))
        text_label = QLabel("Attached Content")
        text_label.setStyleSheet("color: #e8eaed; font-weight: 500;")
        
        layout.addWidget(icon_label)
        layout.addWidget(text_label)
        return widget

    def _create_text_box(self, text):
        frame = QFrame()
        frame.setFrameShape(QFrame.StyledPanel)
        frame.setFrameShadow(QFrame.Raised)
        # Default style is set by _update_row_style
        layout = QVBoxLayout(frame)
        label = QLabel(text)
        label.setWordWrap(True)
        label.setTextInteractionFlags(Qt.TextSelectableByMouse)
        label.setObjectName("contentLabel")
        layout.addWidget(label)
        return frame
        
    def _start_thread_and_update_ui(self, full_prompt, user_prompt):
        """Helper to avoid code duplication between translate and retranslate."""
        self.send_button.setEnabled(False)
        self.apply_button.setEnabled(False)

        for i in reversed(range(self.chat_container_layout.count() - 1)):
            item = self.chat_container_layout.itemAt(i)
            if item.widget():
                item.widget().deleteLater()
        self.current_gemini_bubble_label = None

        self._add_chat_bubble("You", user_prompt)
        self._add_chat_bubble("Gemini", "", is_streaming=True)
        
        self.progress_bar.setVisible(True)
        
        model_to_use = self.model_combo.currentData()
        self.thread = TranslationThread(self.api_key, full_prompt, model_to_use)
        self.thread.translation_progress.connect(self.on_progress)
        self.thread.translation_finished.connect(self.on_finished)
        self.thread.translation_failed.connect(self.on_failed)
        self.thread.start()

    def start_translation_process(self):
        """
        Starts a translation process. If all rows are selected, it translates everything.
        If only a subset is selected, it retranslates just those with context.
        """
        user_prompt = self.prompt_input_edit.toPlainText().strip()
        if not user_prompt:
            QMessageBox.warning(self, "No Prompt", "The prompt cannot be empty.")
            return

        if self.prompt_target_combo.count() == 0:
            QMessageBox.warning(self, "No Target", "No translation profile exists. Please add one with the '+' button.")
            return

        all_selected = True
        if self.row_widgets:
            all_selected = all(widgets['checkbox'].isChecked() for widgets in self.row_widgets.values())

        self.active_translation_index = self.prompt_target_combo.currentData()
        source_profile = self.source_profile_combo.currentText()
        content_to_translate = ""

        if all_selected:
            # Full translation logic
            content_to_translate = generate_for_translate_content(self.ocr_results, source_profile)
            if not content_to_translate.strip() or '<!-- file:' not in content_to_translate:
                QMessageBox.warning(self, "No Content", "There is no text content to translate from the selected source profile.")
                return
        else:
            # Partial re-translation logic
            selected_items = [key for key, widgets in self.row_widgets.items() if widgets['checkbox'].isChecked()]

            if not selected_items:
                # This case is unlikely if the button icon is 'retranslate', but is a good safeguard.
                QMessageBox.information(self, "No Selection", "Something went wrong. No rows are selected for re-translation.")
                return

            content_to_translate = generate_retranslate_content(self.ocr_results, source_profile, selected_items)
            if not content_to_translate.strip() or '<!-- file:' not in content_to_translate:
                QMessageBox.warning(self, "Error", "Could not generate content for retranslation from the selected rows.")
                return
            
        full_prompt = f"{user_prompt}\n\n{content_to_translate}"
        self._start_thread_and_update_ui(full_prompt, user_prompt)

    def on_progress(self, chunk):
        if self.current_gemini_bubble_label:
            current_text = self.current_gemini_bubble_label.text()
            self.current_gemini_bubble_label.setText(current_text + chunk)
            self._scroll_chat_to_bottom()

    def on_finished(self, full_text):
        self.progress_bar.setVisible(False)
        self.current_gemini_bubble_label = None
        try:
            parsed_translations = import_translation_file_content(full_text)
            self._update_comparison_panel(self.active_translation_index, parsed_translations)
            self.apply_button.setEnabled(True)
            self.apply_button.setFocus()
        except Exception as e:
            self.on_failed(f"Failed to parse the translated content: {e}")
        finally:
            self.send_button.setEnabled(True)
            self.active_translation_index = -1

    def _update_comparison_panel(self, column_index, parsed_data):
        if column_index < 0 or column_index >= len(self.translation_columns):
             QMessageBox.critical(self, "Update Error", f"Invalid column index {column_index} provided for update.")
             return
        
        target_column = self.translation_columns[column_index]
        widgets_to_update = target_column['widgets']

        for filename, rows in parsed_data.items():
            if filename not in target_column['translations']:
                target_column['translations'][filename] = {}
            for row_number, translated_text in rows.items():
                target_column['translations'][filename][str(row_number)] = translated_text

        # Now, update the UI labels with the new data
        for filename, rows in parsed_data.items():
            for row_number, translated_text in rows.items():
                key = (filename, str(row_number))
                if key in widgets_to_update:
                    translation_label = widgets_to_update[key]
                    translation_label.setText(translated_text)
                    
                    checkbox = self.row_widgets.get(key, {}).get('checkbox')
                    if checkbox and not checkbox.isChecked():
                        # The stateChanged signal will handle styling and button state updates.
                        checkbox.setChecked(True)

    def on_failed(self, error_message):
        self.progress_bar.setVisible(False)
        self.current_gemini_bubble_label = None
        self._add_chat_bubble("Error", error_message)
        QMessageBox.critical(self, "Translation Error", error_message)
        self.send_button.setEnabled(True)

    def apply_and_close(self):
        try:
            # Get all rows selected by the user
            checked_keys = {key for key, widgets in self.row_widgets.items() if widgets['checkbox'].isChecked()}

            if not checked_keys:
                QMessageBox.information(self, "Nothing to Apply", "No rows are selected. Please select the translations you want to use.")
                return

            anything_applied = False
            source_profile_name = self.source_profile_combo.currentText()

            for col_data in self.translation_columns:
                selected_lang = col_data['language_combo'].currentText()
                profile_name = f"Gemini Translation ({selected_lang})"
                complete_profile_translations = {}
                column_has_valid_translations_to_apply = False # Flag for this column

                for result in self.ocr_results:
                    filename = result.get('filename')
                    row_number = str(result.get('row_number'))
                    key = (filename, row_number)

                    if filename not in complete_profile_translations:
                        complete_profile_translations[filename] = {}

                    source_text = self._get_text_for_profile(result, source_profile_name)
                    text_to_apply = "" # Reset for each row

                    # If the row is selected, its current UI text is the definitive version.
                    if key in checked_keys:
                        label = col_data['widgets'].get(key)
                        if label:
                            widget_text = label.text()
                            if widget_text and widget_text != "...":
                                text_to_apply = widget_text
                                # A change is only "valid" for saving if a selected row differs from the source.
                                if widget_text != source_text:
                                    column_has_valid_translations_to_apply = True
                    # If the row is not selected, we must preserve any previous translation it had.
                    else: # key not in checked_keys
                        old_translation = col_data.get('translations', {}).get(filename, {}).get(row_number)
                        if old_translation and old_translation != "...":
                            text_to_apply = old_translation

                    # If after all checks, we have no text, fall back to the source.
                    # This handles unselected rows that never had a translation.
                    if not text_to_apply:
                        text_to_apply = source_text

                    complete_profile_translations[filename][row_number] = text_to_apply

                # Only emit the signal if this column has at least one actual, new translation.
                if column_has_valid_translations_to_apply:
                    self.translation_complete.emit(profile_name, complete_profile_translations)
                    anything_applied = True

            if anything_applied:
                self.accept()
            else:
                QMessageBox.information(self, "Nothing to Apply",
                                          "No new translations were applied. A profile is only saved if at least one selected row contains a valid translation that is different from the source text.")

        except Exception as e:
            QMessageBox.critical(self, "Apply Error", f"Failed to apply translations to project:\n{str(e)}")

    def closeEvent(self, event):
        if self.thread and self.thread.isRunning():
            self.thread.stop()
            self.thread.wait(500)
        event.accept()