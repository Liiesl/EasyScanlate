# translation_panel.py - Unified Translation and Results Panel

from PySide6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QFrame, QLabel, QTextEdit,
    QScrollArea, QComboBox, QPushButton, QSizePolicy, QApplication,
    QMessageBox, QCheckBox
)
from PySide6.QtCore import Qt, Signal, QTimer, QThread
from PySide6.QtCore import QSettings, QPropertyAnimation, QEasingCurve, Property
from PySide6.QtGui import QPainter, QLinearGradient, QColor
import qtawesome as qta
import traceback

from app.ui.dialogs.error_dialog import ErrorDialog
from app.ui.dialogs.settings_dialog import GEMINI_MODELS_WITH_INFO, MISTRAL_MODELS_WITH_INFO
from assets.styles import TRANSLATION_PANEL_STYLES


class TranslationProgressIndicator(QWidget):
    """Indeterminate sliding progress indicator for translation status."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("TranslationProgressIndicator")
        self.setFixedHeight(3)
        self._position = 0.0
        self._animation = None
        self._is_animating = False

    def _get_position(self):
        return self._position

    def _set_position(self, value):
        self._position = value
        self.update()

    _position_prop = Property(float, _get_position, _set_position)

    def paintEvent(self, event):
        if not self._is_animating:
            return

        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)

        width = self.width()
        chunk_width = width * 0.3

        gradient = QLinearGradient(0, 0, width, 0)
        gradient.setColorAt(0, QColor(0, 122, 204, 0))
        gradient.setColorAt(max(0, self._position - 0.15), QColor(0, 122, 204, 0))
        gradient.setColorAt(self._position, QColor(0, 122, 204, 255))
        gradient.setColorAt(min(1, self._position + 0.15), QColor(0, 122, 204, 255))
        gradient.setColorAt(1, QColor(0, 122, 204, 0))

        painter.fillRect(self.rect(), gradient)

    def start_animation(self):
        if self._is_animating:
            return
        self._is_animating = True
        self.show()

        self._animation = QPropertyAnimation(self, b"_position_prop")
        self._animation.setDuration(1500)
        self._animation.setStartValue(0.0)
        self._animation.setEndValue(1.0)
        self._animation.setEasingCurve(QEasingCurve.Type.InOutSine)
        self._animation.finished.connect(self._on_animation_loop)
        self._animation.start()

    def _on_animation_loop(self):
        if self._is_animating:
            self._animation.setDirection(
                QPropertyAnimation.Direction.Forward
                if self._animation.direction() == QPropertyAnimation.Direction.Backward
                else QPropertyAnimation.Direction.Backward
            )
            self._animation.start()

    def stop_animation(self):
        self._is_animating = False
        if self._animation:
            self._animation.stop()
            self._animation.deleteLater()
            self._animation = None
        self.hide()
        self._position = 0.0
        self.update()


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

        # Retranslate button (sync icon)
        self.retranslate_btn = QPushButton(qta.icon('fa5s.sync-alt', color='#a0a0a0'), "")
        self.retranslate_btn.setObjectName("TranslationCardRetranslateBtn")
        self.retranslate_btn.setFixedSize(32, 32)
        self.retranslate_btn.setToolTip("Retranslate this row")
        self.retranslate_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self.retranslate_btn.clicked.connect(self._on_retranslate)

        # Delete button (trash icon)
        self.delete_btn = QPushButton(qta.icon('fa5s.trash-alt', color='#FF453A'), "")
        self.delete_btn.setObjectName("TranslationCardDeleteBtn")
        self.delete_btn.setFixedSize(32, 32)
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

    Phase 3: Now binds to TranslationViewModel. All model mediation, thread
    orchestration, and API-key handling live in the ViewModel.
    """

    def __init__(self, source_language="Korean", editor_viewmodel=None,
                 translation_viewmodel=None, parent=None):
        super().__init__(parent)
        self.setObjectName("TranslationPanel")
        self.setStyleSheet(TRANSLATION_PANEL_STYLES)

        # Data
        self.source_language = source_language
        self.cards = {}  # row_number -> TranslationCard
        self.active_card = None
        self.ocr_results = []
        self.get_display_text_func = None
        self.editor_vm = editor_viewmodel
        self.translation_vm = translation_viewmodel

        # View-level settings only (delete warning, model defaults)
        self.settings = QSettings("Liiesl", "EasyScanlate")

        self._init_ui()

        if self.editor_vm:
            self.editor_vm.selected_row_changed.connect(self._on_editor_selection_changed)

        if self.translation_vm:
            self._bind_viewmodel()

    def _bind_viewmodel(self):
        """Wire reactive VM signals to panel updates."""
        vm = self.translation_vm
        vm.profiles_changed.connect(self.set_profiles)
        vm.active_profile_changed.connect(self._on_vm_active_profile_changed)
        vm.ocr_results_changed.connect(self._on_vm_ocr_results_changed)
        vm.row_text_updated.connect(self.update_row_text)
        vm.is_translating_changed.connect(self._on_vm_is_translating_changed)
        vm.translation_error_occurred.connect(self._on_vm_translation_error)
        vm.translation_complete_message.connect(self._on_vm_translation_complete)

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

        # Progress indicator (between scroll area and footer)
        self.progress_indicator = TranslationProgressIndicator()
        self.progress_indicator.hide()

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
        self.batch_btn.setFixedSize(36, 36)
        self.batch_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        self.batch_btn.clicked.connect(self._on_batch_translate)
        dropdowns_row.addWidget(self.batch_btn)

        footer_layout.addLayout(dropdowns_row)

        # Assemble
        layout.addWidget(header)
        layout.addWidget(self.scroll_area, 1)
        layout.addWidget(self.progress_indicator)
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
            current_model = self.settings.value("gemini_model", "gemini-3-flash-preview")

        for i in range(self.model_combo.count()):
            if self.model_combo.itemData(i) == current_model:
                self.model_combo.setCurrentIndex(i)
                break

    def _on_provider_changed(self, provider: str):
        self._populate_model_combo(provider)

    def _on_profile_changed(self, profile_name: str):
        if profile_name and self.translation_vm:
            self.translation_vm.set_active_profile(profile_name)

    # ------------------------------------------------------------------
    # VM reactive handlers
    # ------------------------------------------------------------------
    def _on_vm_active_profile_changed(self, profile_name: str):
        """Sync dropdown and repopulate because all display texts changed."""
        self.profile_dropdown.blockSignals(True)
        idx = self.profile_dropdown.findText(profile_name)
        if idx >= 0:
            self.profile_dropdown.setCurrentIndex(idx)
        self.profile_dropdown.blockSignals(False)
        # Repopulate cards with new profile's texts
        if self.translation_vm:
            self.populate(self.translation_vm.ocr_results, self.translation_vm.get_display_text)

    def _on_vm_ocr_results_changed(self, ocr_results: list):
        """Structural change: rebuild all cards."""
        if self.translation_vm:
            self.populate(ocr_results, self.translation_vm.get_display_text)

    def _on_vm_is_translating_changed(self, is_translating: bool):
        self.batch_btn.setEnabled(not is_translating)
        if is_translating:
            self.progress_indicator.start_animation()
        else:
            self.progress_indicator.stop_animation()

    def _on_vm_translation_error(self, title: str, message: str):
        ErrorDialog.critical(self, title, message)

    def _on_vm_translation_complete(self, message: str):
        QMessageBox.information(self, "Success", message)

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------
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
            card.delete_requested.connect(self._on_card_delete_requested)
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

    # ------------------------------------------------------------------
    # Internal handlers
    # ------------------------------------------------------------------
    def _on_editor_selection_changed(self, row_number):
        """React to EditorViewModel selection changes."""
        if row_number is not None and row_number in self.cards:
            self.set_active_row(row_number)
            self.scroll_to_row(row_number)
        elif row_number is None and self.active_card:
            self.active_card.set_active(False)
            self.active_card = None

    def _on_card_clicked(self, card: TranslationCard):
        """Handle card selection."""
        if self.active_card and self.active_card != card:
            self.active_card.set_active(False)
        self.active_card = card
        card.set_active(True)
        if self.editor_vm:
            self.editor_vm.select_row(card.row_number)

    def _on_card_text_changed(self, row_number: int, text: str):
        """Forward text edit to the ViewModel."""
        if self.translation_vm:
            self.translation_vm.update_text(row_number, text)

    def _on_card_delete_requested(self, row_number: int):
        """Show confirmation dialog and delegate deletion to EditorViewModel."""
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
            if dont_show_cb.isChecked():
                self.settings.setValue("show_delete_warning", "false")
            proceed = response == QMessageBox.Yes
        if proceed and self.editor_vm:
            self.editor_vm.delete_row(row_number)

    def _on_single_retranslate(self, row_number: int):
        """Forward retranslate request to the ViewModel."""
        if not self.translation_vm:
            return
        provider = self.provider_combo.currentText()
        model_name = self.model_combo.currentData() or ""
        target_lang = self.lang_combo.currentText()
        self.translation_vm.start_single_translation(row_number, provider, model_name, target_lang)

    def _on_batch_translate(self):
        """Forward batch translate request to the ViewModel."""
        if not self.translation_vm:
            return
        provider = self.provider_combo.currentText()
        model_name = self.model_combo.currentData() or ""
        target_lang = self.lang_combo.currentText()
        self.translation_vm.start_batch_translation(
            provider, model_name, target_lang, self.source_language
        )

    def cleanup(self):
        """Clean up resources."""
        if self.translation_vm:
            self.translation_vm.cleanup()
