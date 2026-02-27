from PySide6.QtWidgets import (QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QLabel, 
                             QComboBox, QScrollArea, QFrame, QSplitter,
                             QProgressBar, QMessageBox, QCheckBox)
from PySide6.QtCore import Qt, Signal, QTimer, QThread, QSize
from PySide6.QtGui import QShortcut, QKeySequence
from PySide6.QtCore import QSettings
import qtawesome as qta
import traceback
import sys
from app.core.translations import TranslationThread, generate_for_translate_content, generate_retranslate_content, import_translation_file_content
from app.ui.dialogs.error_dialog import ErrorDialog
from app.ui.dialogs.settings_dialog import GEMINI_MODELS_WITH_INFO, MISTRAL_MODELS_WITH_INFO

class TranslationChatWidget(QWidget):
    """A chat-style widget for AI translation functionality."""
    
    translation_complete = Signal(str, dict)
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self.provider = self.settings.value("translation_provider", "Gemini")
        self.gemini_api_key = self.settings.value("gemini_api_key", "")
        self.gemini_model = self.settings.value("gemini_model", "gemini-1.5-flash-latest")
        self.mistral_api_key = self.settings.value("mistral_api_key", "")
        self.mistral_model = self.settings.value("mistral_model", "mistral-small-latest")
        self.ocr_results = []
        self.profiles = []
        self.thread = None
        
        # Chat state
        self.current_provider_bubble_label = None
        self.target_languages = [
            "English", "Japanese", "Chinese (Simplified)", "Korean", "Spanish", 
            "French", "German", "Bahasa Indonesia", "Vietnamese", "Thai", 
            "Russian", "Portuguese"
        ]
        
        self.init_ui()
        
    def init_ui(self):
        """Initialize the chat interface."""
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)
        
        # Chat area
        self.chat_scroll_area = QScrollArea()
        self.chat_scroll_area.setWidgetResizable(True)
        self.chat_scroll_area.setFrameShape(QFrame.NoFrame)

        chat_container_widget = QWidget()
        self.chat_container_layout = QVBoxLayout(chat_container_widget)
        self.chat_container_layout.addStretch(1) 
        self.chat_scroll_area.setWidget(chat_container_widget)

        # Input area
        input_area_frame = QFrame()
        input_area_frame.setObjectName("inputAreaFrame")
        input_area_layout = QVBoxLayout(input_area_frame)
        input_area_layout.setContentsMargins(10, 10, 10, 10)
        input_area_layout.setSpacing(10)
        
        # Model selection at top
        model_selection_bar = QWidget()
        model_layout = QHBoxLayout(model_selection_bar)
        model_layout.setContentsMargins(0, 0, 0, 0)
        
        provider_label = QLabel("Provider:")
        self.provider_combo = QComboBox()
        self.provider_combo.addItems(["Gemini", "Mistral"])
        self.provider_combo.setCurrentText(self.provider)
        self.provider_combo.currentTextChanged.connect(self.on_provider_changed)
        
        model_label = QLabel("Model:")
        self.model_combo = QComboBox()
        self._populate_model_combo(self.provider)
        
        model_layout.addWidget(provider_label)
        model_layout.addWidget(self.provider_combo)
        model_layout.addWidget(model_label)
        model_layout.addWidget(self.model_combo, 1)
        
        # Controls bar with language and progress
        controls_bar = QWidget()
        controls_layout = QHBoxLayout(controls_bar)
        controls_layout.setContentsMargins(0, 0, 0, 0)
        
        # Target language selection
        lang_label = QLabel("Target:")
        self.target_language_combo = QComboBox()
        self.target_language_combo.addItems(self.target_languages)
        self.target_language_combo.setCurrentText("English")
        
        # Progress bar
        self.progress_bar = QProgressBar()
        self.progress_bar.setVisible(False)
        self.progress_bar.setRange(0, 0)
        
        controls_layout.addWidget(lang_label)
        controls_layout.addWidget(self.target_language_combo, 1)
        controls_layout.addWidget(self.progress_bar)
        
        # Bottom bar with translate button
        bottom_bar = QWidget()
        bottom_bar_layout = QHBoxLayout(bottom_bar)
        bottom_bar_layout.setContentsMargins(0, 0, 0, 0)
        bottom_bar_layout.setSpacing(10)
        
        # Translate button with icon only (matching translation window)
        self.translate_button = QPushButton()
        self.translate_button.setIcon(qta.icon('fa5s.paper-plane', color='#ffffff'))
        self.translate_button.setToolTip("Translate (Ctrl+Enter)")
        self.translate_button.setIconSize(QSize(18, 18))
        self.translate_button.setFixedSize(40, 40)

        self.translate_button.clicked.connect(self.start_translation)
        
        # Options
        self.retranslate_selected_check = QCheckBox("Retranslate selected only")
        self.retranslate_selected_check.setChecked(False)
        
        bottom_bar_layout.addWidget(self.retranslate_selected_check)
        bottom_bar_layout.addStretch()
        bottom_bar_layout.addWidget(self.translate_button)
        
        # Keyboard shortcuts
        shortcut_send = QShortcut(QKeySequence("Ctrl+Return"), self)
        shortcut_send.activated.connect(self.translate_button.click)
        
        input_area_layout.addWidget(model_selection_bar)
        input_area_layout.addWidget(controls_bar)
        input_area_layout.addWidget(bottom_bar)
        
        main_layout.addWidget(self.chat_scroll_area, 1)
        main_layout.addWidget(input_area_frame)
        
    def set_data(self, api_key=None, model_name=None, ocr_results=None, profiles=None):
        """Set the data needed for translation."""
        # Always refresh from QSettings to ensure we have the latest values
        self.provider = self.settings.value("translation_provider", "Gemini")
        self.gemini_api_key = self.settings.value("gemini_api_key", "")
        self.gemini_model = self.settings.value("gemini_model", "gemini-1.5-flash-latest")
        self.mistral_api_key = self.settings.value("mistral_api_key", "")
        self.mistral_model = self.settings.value("mistral_model", "mistral-small-latest")
        
        if ocr_results is not None:
            self.ocr_results = [res for res in ocr_results if not res.get('is_deleted', False)]
        if profiles is not None:
            self.profiles = profiles
        
        # Update provider selection
        self.provider_combo.blockSignals(True)
        self.provider_combo.setCurrentText(self.provider)
        self.provider_combo.blockSignals(False)
        
        # Update model selection based on provider
        current_model = self.gemini_model if self.provider == "Gemini" else self.mistral_model
        for i in range(self.model_combo.count()):
            if self.model_combo.itemData(i) == current_model:
                self.model_combo.setCurrentIndex(i)
                break
    
    def _populate_model_combo(self, provider):
        """Populate model combo based on selected provider."""
        self.model_combo.clear()
        if provider == "Mistral":
            for model_name, model_info_text in MISTRAL_MODELS_WITH_INFO:
                self.model_combo.addItem(model_name, userData=model_name)
            current_model = self.settings.value("mistral_model", "mistral-small-latest")
        else:
            for model_name, model_info_text in GEMINI_MODELS_WITH_INFO:
                self.model_combo.addItem(model_name, userData=model_name)
            current_model = self.settings.value("gemini_model", "gemini-1.5-flash-latest")
        
        for i in range(self.model_combo.count()):
            if self.model_combo.itemData(i) == current_model:
                self.model_combo.setCurrentIndex(i)
                break
    
    def on_provider_changed(self, provider):
        """Handle provider selection change."""
        self.provider = provider
        self._populate_model_combo(provider)
    
    def _add_chat_bubble(self, sender, text, is_streaming=False):
        """Add a chat bubble to the conversation."""
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
            message_layout.addStretch()
            message_layout.addWidget(bubble)
        elif sender in ["Gemini", "Mistral"]:
            if is_streaming:
                self.current_provider_bubble_label = text_label
            message_layout.addWidget(bubble)
            message_layout.addStretch()
        elif sender == "Error":
            name_label.setText("<b>SYSTEM ERROR</b>")
            message_layout.addWidget(bubble)
            message_layout.addStretch()

        self.chat_container_layout.insertWidget(self.chat_container_layout.count() - 1, message_widget)
        QTimer.singleShot(50, self._scroll_chat_to_bottom)

    def _scroll_chat_to_bottom(self):
        """Scroll the chat to the bottom."""
        scroll_bar = self.chat_scroll_area.verticalScrollBar()
        scroll_bar.setValue(scroll_bar.maximum())

    def start_translation(self):
        """Start the translation process."""
        # Always check current QSettings value
        provider = self.provider_combo.currentText()
        if provider == "Mistral":
            api_key = self.settings.value("mistral_api_key", "")
            if not api_key:
                QMessageBox.critical(self, "API Key Missing", "Please set your Mistral API key in Settings.")
                return
        else:
            api_key = self.settings.value("gemini_api_key", "")
            if not api_key:
                QMessageBox.critical(self, "API Key Missing", "Please set your Gemini API key in Settings.")
                return
        
        # Update instance variable with current value
        self.api_key = api_key
            
        if not self.ocr_results:
            QMessageBox.warning(self, "No Data", "There are no OCR results to translate.")
            return

        user_prompt = f"Translate the Korean text to {self.target_language_combo.currentText()}, keep everything else. response only with the file.."

        # Determine if we're translating all or just selected
        retranslate_selected = self.retranslate_selected_check.isChecked()
        
        # Generate content based on mode
        
        try:
            if retranslate_selected:
                # For now, we'll use all results since selection management would need integration
                # In a full implementation, this would use selected rows
                selected_items = []  # This would be populated from actual selection
                content_to_translate = generate_retranslate_content(self.ocr_results, "Original", selected_items)
                if not content_to_translate.strip():
                    # Fallback to regular translation if no items selected
                    content_to_translate = generate_for_translate_content(self.ocr_results, "Original")
            else:
                content_to_translate = generate_for_translate_content(self.ocr_results, "Original")
                
            if not content_to_translate.strip() or '<translations>' not in content_to_translate:
                QMessageBox.warning(self, "No Content", "There is no text content to translate.")
                return
                
            full_prompt = f"{user_prompt}\n\n{content_to_translate}"
            self._start_translation_thread(full_prompt)
            
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Failed to prepare translation: {str(e)}")

    def _start_translation_thread(self, full_prompt):
        """Start the translation thread."""
        self.translate_button.setEnabled(False)
        
        # Clear previous chat
        for i in reversed(range(self.chat_container_layout.count() - 1)):
            item = self.chat_container_layout.itemAt(i)
            if item.widget():
                item.widget().deleteLater()
        self.current_provider_bubble_label = None
        
        # Add start message to chat
        self._add_chat_bubble("You", f"Translate to {self.target_language_combo.currentText()}")
        
        # Show progress bar
        self.progress_bar.setVisible(True)
        
        # Get provider and API key
        provider = self.provider_combo.currentText()
        if provider == "Mistral":
            api_key = self.settings.value("mistral_api_key", "")
            model_name = self.model_combo.currentData() or "mistral-small-latest"
        else:
            api_key = self.settings.value("gemini_api_key", "")
            model_name = self.model_combo.currentData() or "gemini-1.5-flash-latest"
        
        # Create and start translation thread
        self.thread = TranslationThread(api_key, full_prompt, model_name, provider=provider, parent=self)
        
        # Connect thread signals
        self.thread.translation_progress.connect(self.on_progress)
        self.thread.translation_finished.connect(self.on_finished)
        self.thread.translation_failed.connect(self.on_failed)
        
        # Start the thread
        self.thread.start()
        
        # Add initial provider bubble
        self._add_chat_bubble(provider, "", is_streaming=True)

    def on_progress(self, chunk):
        """Handle streaming translation progress."""
        if self.current_provider_bubble_label:
            current_text = self.current_provider_bubble_label.text()
            self.current_provider_bubble_label.setText(current_text + chunk)
            self._scroll_chat_to_bottom()

    def on_finished(self, full_text):
        """Handle completed translation."""
        self.progress_bar.setVisible(False)
        self.current_provider_bubble_label = None
        self.translate_button.setEnabled(True)
        
        try:
            parsed_translations = import_translation_file_content(full_text)
            target_language = self.target_language_combo.currentText()
            provider = self.provider_combo.currentText()
            profile_name = f"{provider} Translation ({target_language})"
            
            # Add completion message to chat
            self._add_chat_bubble(provider, f"Translation completed! Profile '{profile_name}' created.")
            
            # Emit signal for main window to handle
            self.translation_complete.emit(profile_name, parsed_translations)
            
        except Exception as e:
            self.on_failed(f"Failed to parse translation: {str(e)}")

    def on_failed(self, error_message):
        """Handle translation failure."""
        self.progress_bar.setVisible(False)
        self.current_provider_bubble_label = None
        self.translate_button.setEnabled(True)
        self._add_chat_bubble("Error", error_message)
        ErrorDialog.critical(self, "Translation Error", error_message)

    def clear_chat(self):
        """Clear the chat history."""
        for i in reversed(range(self.chat_container_layout.count() - 1)):
            item = self.chat_container_layout.itemAt(i)
            if item.widget():
                item.widget().deleteLater()

    def closeEvent(self, event):
        """Clean up when closing."""
        if self.thread and self.thread.isRunning():
            self.thread.stop()
            self.thread.wait(500)
        event.accept()
