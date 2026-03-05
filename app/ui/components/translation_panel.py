# translation_panel.py - Unified Translation and Results Panel

from PySide6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QFrame, QLabel, QTextEdit,
    QScrollArea, QComboBox, QPushButton, QSizePolicy, QApplication
)
from PySide6.QtCore import Qt, Signal, QTimer, QThread
from PySide6.QtCore import QSettings
import qtawesome as qta
import traceback

from app.core.translations import (
    TranslationThread, generate_for_translate_content,
    generate_retranslate_content, import_translation_file_content
)
from app.ui.dialogs.error_dialog import ErrorDialog
from app.ui.dialogs.settings_dialog import GEMINI_MODELS_WITH_INFO, MISTRAL_MODELS_WITH_INFO
from assets.styles import TRANSLATION_PANEL_STYLES


class FocusableTextEdit(QTextEdit):
    """Auto-resizing text edit that emits signal on focus."""
    focused = Signal()
    text_modified = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setAcceptRichText(False)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.document().setDocumentMargin(0)
        self.textChanged.connect(self._adjust_height)
        self.textChanged.connect(self._emit_text_modified)

    def setPlainText(self, text):
        super().setPlainText(text)
        self._adjust_height()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._adjust_height()

    def focusInEvent(self, event):
        super().focusInEvent(event)
        self.focused.emit()

    def _emit_text_modified(self):
        self.text_modified.emit(self.toPlainText())

    def _adjust_height(self):
        viewport_w = self.viewport().width()
        doc = self.document()
        doc.setTextWidth(viewport_w)
        doc.adjustSize()

        ideal_w = doc.idealWidth()
        text_w = min(ideal_w, viewport_w) if ideal_w > 0 else viewport_w
        doc.setTextWidth(text_w)

        doc_height = doc.documentLayout().documentSize().height()
        frame = self.frameWidth() * 2
        new_height = int(doc_height + frame + 16)
        self.setFixedHeight(max(40, new_height))


class TranslationCard(QFrame):
    """A card representing a single translation row."""
    clicked = Signal(object)
    text_changed = Signal(int, str)
    delete_requested = Signal(int)
    retranslate_requested = Signal(int)

    def __init__(self, row_number: int, source_text: str, target_text: str, parent=None):
        super().__init__(parent)
        self.row_number = row_number
        self.setObjectName("TranslationCard")
        self.setProperty("active", False)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)

        # Main layout
        layout = QHBoxLayout(self)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setSpacing(14)

        # ID Badge
        self.id_label = QLabel(str(row_number))
        self.id_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.id_label.setObjectName("TranslationCardIdBadge")

        # Source Column (KOR - read only)
        src_col = QVBoxLayout()
        src_col.setSpacing(6)
        src_col.setContentsMargins(0, 0, 0, 0)

        src_lbl = QLabel("KOR")
        src_lbl.setObjectName("TranslationCardLangLabel")

        self.source_edit = FocusableTextEdit()
        self.source_edit.setPlainText(source_text)
        self.source_edit.setReadOnly(True)
        self.source_edit.setObjectName("TranslationCardSourceInput")
        self.source_edit.focused.connect(lambda: self.clicked.emit(self))

        src_col.addWidget(src_lbl)
        src_col.addWidget(self.source_edit)

        # Target Column (ENG - editable)
        tgt_col = QVBoxLayout()
        tgt_col.setSpacing(6)
        tgt_col.setContentsMargins(0, 0, 0, 0)

        tgt_lbl = QLabel("ENG")
        tgt_lbl.setObjectName("TranslationCardLangLabel")

        self.target_edit = FocusableTextEdit()
        self.target_edit.setPlainText(target_text)
        self.target_edit.setObjectName("TranslationCardTargetInput")
        self.target_edit.focused.connect(lambda: self.clicked.emit(self))
        self.target_edit.text_modified.connect(self._on_text_changed)

        tgt_col.addWidget(tgt_lbl)
        tgt_col.addWidget(self.target_edit)

        # Actions Column
        actions_layout = QVBoxLayout()
        actions_layout.setContentsMargins(0, 4, 0, 0)
        actions_layout.setSpacing(12)
        actions_layout.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignCenter)

        # Retranslate button (magic sparkle icon)
        self.retranslate_btn = QPushButton(qta.icon('fa5s.magic', color='#a0a0a0'), "")
        self.retranslate_btn.setObjectName("TranslationCardRetranslateBtn")
        self.retranslate_btn.setFixedSize(28, 28)
        self.retranslate_btn.setToolTip("Retranslate this row")
        self.retranslate_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self.retranslate_btn.clicked.connect(self._on_retranslate)

        # Delete button (trash icon)
        self.delete_btn = QPushButton(qta.icon('fa5s.trash-alt', color='#FF453A'), "")
        self.delete_btn.setObjectName("TranslationCardDeleteBtn")
        self.delete_btn.setFixedSize(28, 28)
        self.delete_btn.setToolTip("Delete this row")
        self.delete_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self.delete_btn.clicked.connect(self._on_delete)

        actions_layout.addWidget(self.retranslate_btn)
        actions_layout.addWidget(self.delete_btn)

        actions_container = QWidget()
        actions_container.setLayout(actions_layout)
        actions_container.setFixedWidth(32)

        # Add to layout
        layout.addWidget(self.id_label, 0, Qt.AlignmentFlag.AlignTop)
        layout.addLayout(src_col, 1)
        layout.addLayout(tgt_col, 1)
        layout.addWidget(actions_container, 0, Qt.AlignmentFlag.AlignTop)

    def set_active(self, is_active: bool):
        self.setProperty("active", is_active)
        self.style().unpolish(self)
        self.style().polish(self)
        self.id_label.setProperty("active", is_active)
        self.id_label.style().unpolish(self.id_label)
        self.id_label.style().polish(self.id_label)

    def set_target_text(self, text: str):
        if self.target_edit.toPlainText() != text:
            self.target_edit.blockSignals(True)
            self.target_edit.setPlainText(text)
            self.target_edit.blockSignals(False)

    def get_target_text(self) -> str:
        return self.target_edit.toPlainText()

    def _on_text_changed(self, text: str):
        self.text_changed.emit(self.row_number, text)

    def _on_delete(self):
        self.delete_requested.emit(self.row_number)

    def _on_retranslate(self):
        self.retranslate_requested.emit(self.row_number)

    def mousePressEvent(self, event):
        super().mousePressEvent(event)
        self.clicked.emit(self)


class TranslationPanel(QFrame):
    """
    Unified panel combining OCR results display and translation controls.
    Replaces ResultsWidget and TranslationChatWidget.
    """
    # Signals
    text_changed = Signal(int, str)
    row_deleted = Signal(int)
    row_selected = Signal(int)
    translation_complete = Signal(str, dict)
    profile_changed = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("TranslationPanel")
        self.setStyleSheet(TRANSLATION_PANEL_STYLES)

        # Data
        self.cards = {}  # row_number -> TranslationCard
        self.active_card = None
        self.ocr_results = []
        self.get_display_text_func = None

        # Translation state
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self.translation_thread = None
        self._pending_retranslate_row = None

        self._init_ui()

    def _init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)

        # Header
        header = QFrame()
        header.setObjectName("TranslationPanelHeader")
        header_layout = QHBoxLayout(header)
        header_layout.setContentsMargins(20, 18, 20, 18)

        title = QLabel("Translations")
        title.setObjectName("TranslationPanelTitle")

        lbl_profile = QLabel("Profile:")
        lbl_profile.setObjectName("TranslationPanelHeaderLabel")

        self.profile_dropdown = QComboBox()
        self.profile_dropdown.setObjectName("TranslationPanelProfileDropdown")
        self.profile_dropdown.addItem("Original")
        self.profile_dropdown.setCursor(Qt.CursorShape.PointingHandCursor)
        self.profile_dropdown.currentTextChanged.connect(self._on_profile_changed)

        header_layout.addWidget(title)
        header_layout.addStretch()
        header_layout.addWidget(lbl_profile)
        header_layout.addWidget(self.profile_dropdown)

        # Scrollable Cards Area
        self.scroll_area = QScrollArea()
        self.scroll_area.setWidgetResizable(True)
        self.scroll_area.setObjectName("TranslationPanelScroll")
        self.scroll_area.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)

        self.cards_container = QFrame()
        self.cards_container.setObjectName("TranslationPanelCardsContainer")
        self.cards_layout = QVBoxLayout(self.cards_container)
        self.cards_layout.setContentsMargins(15, 15, 15, 20)
        self.cards_layout.setSpacing(12)
        self.cards_layout.setAlignment(Qt.AlignmentFlag.AlignTop)

        self.scroll_area.setWidget(self.cards_container)

        # Footer
        footer = QFrame()
        footer.setObjectName("TranslationPanelFooter")
        footer_layout = QVBoxLayout(footer)
        footer_layout.setContentsMargins(20, 15, 20, 20)
        footer_layout.setSpacing(12)

        # Dropdowns Row
        dropdowns_row = QHBoxLayout()
        dropdowns_row.setSpacing(8)

        # Provider
        provider_col = QVBoxLayout()
        provider_col.setSpacing(4)
        lbl_provider = QLabel("Provider")
        lbl_provider.setObjectName("TranslationPanelDropdownLabel")
        self.provider_combo = QComboBox()
        self.provider_combo.addItems(["Gemini", "Mistral"])
        self.provider_combo.currentTextChanged.connect(self._on_provider_changed)
        provider_col.addWidget(lbl_provider)
        provider_col.addWidget(self.provider_combo)

        # Model
        model_col = QVBoxLayout()
        model_col.setSpacing(4)
        lbl_model = QLabel("Model")
        lbl_model.setObjectName("TranslationPanelDropdownLabel")
        self.model_combo = QComboBox()
        self._populate_model_combo("Gemini")
        model_col.addWidget(lbl_model)
        model_col.addWidget(self.model_combo)

        # Target Lang
        lang_col = QVBoxLayout()
        lang_col.setSpacing(4)
        lbl_lang = QLabel("Target Lang")
        lbl_lang.setObjectName("TranslationPanelDropdownLabel")
        self.lang_combo = QComboBox()
        self.lang_combo.addItems([
            "English", "Japanese", "Chinese (Simplified)", "Korean", "Spanish",
            "French", "German", "Bahasa Indonesia", "Vietnamese", "Thai",
            "Russian", "Portuguese"
        ])
        lang_col.addWidget(lbl_lang)
        lang_col.addWidget(self.lang_combo)

        dropdowns_row.addLayout(provider_col, 2)
        dropdowns_row.addLayout(model_col, 3)
        dropdowns_row.addLayout(lang_col, 2)
        dropdowns_row.addStretch()

        self.batch_btn = QPushButton(qta.icon('fa5s.paper-plane', color='#ffffff'), "")
        self.batch_btn.setObjectName("TranslationPanelBatchBtn")
        self.batch_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self.batch_btn.clicked.connect(self._on_batch_translate)
        dropdowns_row.addWidget(self.batch_btn)

        footer_layout.addLayout(dropdowns_row)

        # Assemble
        layout.addWidget(header)
        layout.addWidget(self.scroll_area, 1)
        layout.addWidget(footer)

    def _populate_model_combo(self, provider: str):
        self.model_combo.clear()
        if provider == "Mistral":
            for model_name, _ in MISTRAL_MODELS_WITH_INFO:
                self.model_combo.addItem(model_name, userData=model_name)
            current_model = self.settings.value("mistral_model", "mistral-small-latest")
        else:
            for model_name, _ in GEMINI_MODELS_WITH_INFO:
                self.model_combo.addItem(model_name, userData=model_name)
            current_model = self.settings.value("gemini_model", "gemini-1.5-flash-latest")

        for i in range(self.model_combo.count()):
            if self.model_combo.itemData(i) == current_model:
                self.model_combo.setCurrentIndex(i)
                break

    def _on_provider_changed(self, provider: str):
        self._populate_model_combo(provider)

    def _on_profile_changed(self, profile_name: str):
        if profile_name:
            self.profile_changed.emit(profile_name)

    # Public API
    def populate(self, ocr_results: list, get_display_text_func):
        """Populate the panel with OCR results."""
        self.ocr_results = ocr_results
        self.get_display_text_func = get_display_text_func
        self._rebuild_cards()

    def _rebuild_cards(self):
        """Rebuild all cards from current ocr_results."""
        # Clear existing
        while self.cards_layout.count() > 0:
            item = self.cards_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()
        self.cards.clear()
        self.active_card = None

        # Build new cards
        visible_results = [r for r in self.ocr_results if not r.get('is_deleted', False)]

        for result in visible_results:
            row_number = int(result.get('row_number', 0))
            source_text = result.get('text', '')  # Original KOR text
            target_text = self.get_display_text_func(result) if self.get_display_text_func else source_text

            card = TranslationCard(row_number, source_text, target_text)
            card.clicked.connect(self._on_card_clicked)
            card.text_changed.connect(self._on_card_text_changed)
            card.delete_requested.connect(self.row_deleted.emit)
            card.retranslate_requested.connect(self._on_single_retranslate)

            self.cards_layout.addWidget(card)
            self.cards[row_number] = card

        self.cards_layout.addStretch(1)

    def update_row_text(self, row_number: int, text: str):
        """Update text for a specific row without rebuilding."""
        if row_number in self.cards:
            self.cards[row_number].set_target_text(text)

    def set_active_row(self, row_number: int):
        """Set the active/selected row."""
        if row_number in self.cards:
            self._on_card_clicked(self.cards[row_number])

    def scroll_to_row(self, row_number: int):
        """Scroll to make a row visible."""
        if row_number in self.cards:
            card = self.cards[row_number]
            self.scroll_area.ensureWidgetVisible(card, 50, 50)

    def set_profiles(self, profiles: list):
        """Update the profile dropdown."""
        current = self.profile_dropdown.currentText()
        self.profile_dropdown.blockSignals(True)
        self.profile_dropdown.clear()
        self.profile_dropdown.addItem("Original")
        for profile in profiles:
            if profile != "Original":
                self.profile_dropdown.addItem(profile)
        # Restore selection if possible
        idx = self.profile_dropdown.findText(current)
        if idx >= 0:
            self.profile_dropdown.setCurrentIndex(idx)
        self.profile_dropdown.blockSignals(False)

    def get_current_profile(self) -> str:
        """Get currently selected profile name."""
        return self.profile_dropdown.currentText()

    # Internal handlers
    def _on_card_clicked(self, card: TranslationCard):
        """Handle card selection."""
        if self.active_card and self.active_card != card:
            self.active_card.set_active(False)
        self.active_card = card
        card.set_active(True)
        self.row_selected.emit(card.row_number)

    def _on_card_text_changed(self, row_number: int, text: str):
        """Handle text edit in a card."""
        self.text_changed.emit(row_number, text)

    def _on_single_retranslate(self, row_number: int):
        """Handle retranslate request for a single row."""
        self._pending_retranslate_row = row_number

        # Get API key
        provider = self.provider_combo.currentText()
        if provider == "Mistral":
            api_key = self.settings.value("mistral_api_key", "")
            model_name = self.model_combo.currentData() or "mistral-small-latest"
        else:
            api_key = self.settings.value("gemini_api_key", "")
            model_name = self.model_combo.currentData() or "gemini-1.5-flash-latest"

        if not api_key:
            ErrorDialog.critical(self, "API Key Missing", f"Please set your {provider} API key in Settings.")
            return

        # Find the result
        result = None
        for r in self.ocr_results:
            if int(r.get('row_number', 0)) == row_number:
                result = r
                break

        if not result:
            return

        # Generate prompt for single item
        filename = result.get('filename', '')
        source_text = result.get('text', '')
        target_lang = self.lang_combo.currentText()

        prompt = f"""Translate the following text to {target_lang}. Respond ONLY with the translation, no explanation.

Text: {source_text}"""

        # Start translation
        self._start_translation_thread(api_key, prompt, model_name, provider, is_single=True)

    def _on_batch_translate(self):
        """Handle batch translate request."""
        provider = self.provider_combo.currentText()
        if provider == "Mistral":
            api_key = self.settings.value("mistral_api_key", "")
            model_name = self.model_combo.currentData() or "mistral-small-latest"
        else:
            api_key = self.settings.value("gemini_api_key", "")
            model_name = self.model_combo.currentData() or "gemini-1.5-flash-latest"

        if not api_key:
            ErrorDialog.critical(self, "API Key Missing", f"Please set your {provider} API key in Settings.")
            return

        if not self.ocr_results:
            ErrorDialog.critical(self, "No Data", "There are no OCR results to translate.")
            return

        target_lang = self.lang_combo.currentText()
        user_prompt = f"Translate the Korean text to {target_lang}, keep everything else. Respond only with the file."

        try:
            content = generate_for_translate_content(self.ocr_results, "Original")

            if not content.strip() or '<translations>' not in content:
                ErrorDialog.critical(self, "No Content", "There is no text content to translate.")
                return

            full_prompt = f"{user_prompt}\n\n{content}"
            self._start_translation_thread(api_key, full_prompt, model_name, provider, is_single=False)

        except Exception as e:
            ErrorDialog.critical(self, "Error", f"Failed to prepare translation: {str(e)}")

    def _start_translation_thread(self, api_key: str, prompt: str, model_name: str, provider: str, is_single: bool):
        """Start the translation thread."""
        self.batch_btn.setEnabled(False)

        # Clean up previous thread
        if self.translation_thread and self.translation_thread.isRunning():
            self.translation_thread.translation_finished.disconnect()
            self.translation_thread.translation_failed.disconnect()
            self.translation_thread.stop()
            self.translation_thread.wait(1000)
            if self.translation_thread.isRunning():
                # Thread didn't stop, but we've disconnected signals so it's safer
                pass

        # Create and start thread
        self.translation_thread = TranslationThread(api_key, prompt, model_name, provider=provider, parent=self)
        self.translation_thread.translation_finished.connect(
            lambda text: self._on_translation_finished(text, provider, is_single)
        )
        self.translation_thread.translation_failed.connect(self._on_translation_failed)

        self.translation_thread.start()

    def _on_translation_finished(self, full_text: str, provider: str, is_single: bool):
        """Handle completed translation."""
        self.batch_btn.setEnabled(True)

        try:
            if is_single and self._pending_retranslate_row is not None:
                # Extract translation for single row
                # Parse the response - it should just be the translated text
                translated_text = full_text.strip()
                # Remove any quotes if present
                if translated_text.startswith('"') and translated_text.endswith('"'):
                    translated_text = translated_text[1:-1]
                if translated_text.startswith("'") and translated_text.endswith("'"):
                    translated_text = translated_text[1:-1]

                # Update the card directly
                if self._pending_retranslate_row in self.cards:
                    self.cards[self._pending_retranslate_row].set_target_text(translated_text)
                    self.text_changed.emit(self._pending_retranslate_row, translated_text)

                self._pending_retranslate_row = None
            else:
                # Batch translation - parse XML
                parsed = import_translation_file_content(full_text)
                target_lang = self.lang_combo.currentText()
                profile_name = f"{provider} Translation ({target_lang})"
                self.translation_complete.emit(profile_name, parsed)

        except Exception as e:
            self._on_translation_failed(f"Failed to parse translation: {str(e)}")

    def _on_translation_failed(self, error_message: str):
        """Handle translation failure."""
        self.batch_btn.setEnabled(True)
        ErrorDialog.critical(self, "Translation Error", error_message)
        self._pending_retranslate_row = None

    def cleanup(self):
        """Clean up resources."""
        if self.translation_thread and self.translation_thread.isRunning():
            self.translation_thread.stop()
            self.translation_thread.wait(500)
