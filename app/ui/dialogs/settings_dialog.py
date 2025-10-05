# settings_dialog.py

import os
from PySide6.QtWidgets import (QDialog, QDoubleSpinBox, QVBoxLayout, QHBoxLayout, QFormLayout,
                             QComboBox, QSpinBox, QDialogButtonBox, QTabWidget,
                             QWidget, QLineEdit, QKeySequenceEdit, QCheckBox,
                             QGroupBox, QPushButton, QLabel, QProgressBar, QMessageBox)
from PySide6.QtGui import QKeySequence
from PySide6.QtCore import QSettings
from assets import ADVANCED_CHECK_STYLES
from app.utils.update import UpdateHandler
GEMINI_MODELS_WITH_INFO = [
    ("gemini-2.5-flash", "250 req/day (free tier)"),
    ("gemini-2.5-pro", "100 req/day (free tier)"),
    ("gemini-2.5-flash-lite", "1000 req/day (free tier)"),
    ("gemini-2.0-flash", "200 req/day (free tier)"),
    ("gemini-2.0-flash-lite", "200 req/day (free tier)"),
    ("gemma-3-27b-it", "14400 req/day"),
    ("gemma-3n-e4b-it", "14400 req/day"),
]

class SettingsDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Settings")
        self.settings = parent.settings
        self.downloaded_update_path = ""

        # --- UPDATE MECHANISM SETUP ---
        self.update_handler = UpdateHandler(self)

        main_layout = QVBoxLayout()
        self.tab_widget = QTabWidget()

        # --- General Tab ---
        general_tab = QWidget()
        general_layout = QFormLayout()

        self.show_delete_warning_check = QCheckBox()
        self.show_delete_warning_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.show_delete_warning_check.setChecked(self.settings.value("show_delete_warning", "true") == "true")
        general_layout.addRow("Show delete confirmation dialog:", self.show_delete_warning_check)

        self.use_gpu_check = QCheckBox()
        self.use_gpu_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.use_gpu_check.setChecked(self.settings.value("use_gpu", "true").lower() == "true")
        self.use_gpu_check.setToolTip("Requires compatible NVIDIA GPU and CUDA drivers. Restart may be needed.")
        general_layout.addRow("Use GPU for OCR (if available):", self.use_gpu_check)

        self.auto_context_fill_check = QCheckBox()
        self.auto_context_fill_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.auto_context_fill_check.setChecked(self.settings.value("auto_context_fill", "false") == "true")
        self.auto_context_fill_check.setToolTip("Automatically inpaint background during Batch OCR. Can improve text rendering but may slow processing.")
        general_layout.addRow("Auto Context Fill on Batch OCR:", self.auto_context_fill_check)
        
        self.auto_check_updates_check = QCheckBox()
        self.auto_check_updates_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.auto_check_updates_check.setChecked(self.settings.value("auto_check_updates", "true") == "true")
        general_layout.addRow("Auto-check for updates on startup:", self.auto_check_updates_check)

        # --- UPDATE WIDGETS ---
        update_group = QGroupBox("Application Updates")
        update_layout = QVBoxLayout()
        
        self.update_status_label = QLabel(f"Current Version: {self.update_handler.get_current_version()}")
        update_layout.addWidget(self.update_status_label)
        
        self.update_progress_bar = QProgressBar()
        self.update_progress_bar.setVisible(False)
        update_layout.addWidget(self.update_progress_bar)
        
        update_button_layout = QHBoxLayout()
        self.check_updates_button = QPushButton("Check for Updates")
        self.check_updates_button.clicked.connect(self.update_handler.check_for_updates)
        self.check_updates_button.clicked.connect(lambda: self.check_updates_button.setEnabled(False))
        update_button_layout.addWidget(self.check_updates_button)

        self.download_update_button = QPushButton("Download Update")
        self.download_update_button.setVisible(False)
        self.download_update_button.clicked.connect(self.update_handler.start_update_download)
        update_button_layout.addWidget(self.download_update_button)

        self.restart_update_button = QPushButton("Restart & Update")
        self.restart_update_button.setVisible(False)
        self.restart_update_button.clicked.connect(self.apply_update)
        update_button_layout.addWidget(self.restart_update_button)
        
        update_layout.addLayout(update_button_layout)
        update_group.setLayout(update_layout)
        general_layout.addRow(update_group)
        general_tab.setLayout(general_layout)
        self.tab_widget.addTab(general_tab, "General")

        # --- OCR Processing Settings Tab (No changes) ---
        processing_tab = QWidget()
        form_layout = QFormLayout()
        self.min_text_spin = QSpinBox()
        self.min_text_spin.setRange(0, 10000); self.min_text_spin.setSuffix(" px")
        self.min_text_spin.setValue(int(self.settings.value("min_text_height", 40)))
        form_layout.addRow("Minimum Text Height:", self.min_text_spin)
        self.max_text_spin = QSpinBox()
        self.max_text_spin.setRange(0, 10000); self.max_text_spin.setSuffix(" px")
        self.max_text_spin.setValue(int(self.settings.value("max_text_height", 100)))
        form_layout.addRow("Maximum Text Height:", self.max_text_spin)
        self.confidence_spin = QDoubleSpinBox()
        self.confidence_spin.setRange(0.0, 1.0); self.confidence_spin.setSingleStep(0.05); self.confidence_spin.setDecimals(2)
        self.confidence_spin.setValue(float(self.settings.value("min_confidence", 0.2)))
        form_layout.addRow("Minimum Confidence:", self.confidence_spin)
        self.distance_spin = QSpinBox()
        self.distance_spin.setRange(0, 1000); self.distance_spin.setSuffix(" px")
        self.distance_spin.setValue(int(self.settings.value("distance_threshold", 100)))
        form_layout.addRow("Merge Distance Threshold:", self.distance_spin)
        self.batch_size_spin = QSpinBox()
        self.batch_size_spin.setRange(1, 64)
        self.batch_size_spin.setValue(int(self.settings.value("ocr_batch_size", 8)))
        self.batch_size_spin.setToolTip("Number of image patches processed simultaneously (higher needs more GPU VRAM).")
        form_layout.addRow("OCR Batch Size:", self.batch_size_spin)
        self.decoder_combo = QComboBox()
        self.decoder_combo.addItems(["beamsearch", "greedy"])
        self.decoder_combo.setCurrentText(self.settings.value("ocr_decoder", "beamsearch"))
        self.decoder_combo.setToolTip("'beamsearch' is generally more accurate but slower. 'greedy' is faster.")
        form_layout.addRow("OCR Decoder:", self.decoder_combo)
        self.contrast_spin = QDoubleSpinBox()
        self.contrast_spin.setRange(0.0, 1.0); self.contrast_spin.setSingleStep(0.1); self.contrast_spin.setDecimals(1)
        self.contrast_spin.setValue(float(self.settings.value("ocr_adjust_contrast", 0.5)))
        self.contrast_spin.setToolTip("Automatically adjust image contrast (0.0 to disable). May help or hurt depending on image.")
        form_layout.addRow("OCR Adjust Contrast:", self.contrast_spin)
        self.resize_threshold_spin = QSpinBox()
        self.resize_threshold_spin.setRange(0, 8192); self.resize_threshold_spin.setSuffix(" px"); self.resize_threshold_spin.setSpecialValueText("Disabled")
        self.resize_threshold_spin.setValue(int(self.settings.value("ocr_resize_threshold", 1024)))
        self.resize_threshold_spin.setToolTip("Resize images wider than this before OCR. Set to 0 to disable resizing.")
        form_layout.addRow("OCR Resize Threshold (Max Width):", self.resize_threshold_spin)
        processing_tab.setLayout(form_layout)
        self.tab_widget.addTab(processing_tab, "OCR Processing")

        # --- API Settings Tab (No changes) ---
        api_tab = QWidget()
        api_layout = QFormLayout()
        self.api_key_edit = QLineEdit()
        self.api_key_edit.setEchoMode(QLineEdit.Password)
        self.api_key_edit.setText(self.settings.value("gemini_api_key", ""))
        api_layout.addRow("Gemini API Key:", self.api_key_edit)
        self.model_combo = QComboBox()
        for model_name, model_info_text in GEMINI_MODELS_WITH_INFO:
            self.model_combo.addItem(f"{model_name} | {model_info_text}", userData=model_name)
        current_model_value = self.settings.value("gemini_model", "gemini-1.5-flash-latest")
        for i in range(self.model_combo.count()):
            if self.model_combo.itemData(i) == current_model_value:
                self.model_combo.setCurrentIndex(i); break
        api_layout.addRow("Gemini Model:", self.model_combo)
        self.lang_combo = QComboBox()
        self.lang_combo.addItems(["English", "Japanese", "Chinese (Simplified)", "Korean", "Spanish", "French", "German", "Bahasa Indonesia", "Vietnamese", "Thai", "Russian", "Portuguese"])
        self.lang_combo.setCurrentText(self.settings.value("target_language", "English"))
        api_layout.addRow("Target Language:", self.lang_combo)
        api_tab.setLayout(api_layout)
        self.tab_widget.addTab(api_tab, "Translations")

        # --- Keyboard Shortcuts Tab (No changes) ---
        shortcuts_tab = QWidget()
        shortcuts_layout = QFormLayout()
        self.combine_shortcut_edit = QKeySequenceEdit(QKeySequence(self.settings.value("combine_shortcut", "Ctrl+G")))
        shortcuts_layout.addRow("Combine Rows Shortcut:", self.combine_shortcut_edit)
        self.find_shortcut_edit = QKeySequenceEdit(QKeySequence(self.settings.value("find_shortcut", "Ctrl+F")))
        shortcuts_layout.addRow("Find/Replace Shortcut:", self.find_shortcut_edit)
        shortcuts_tab.setLayout(shortcuts_layout)
        self.tab_widget.addTab(shortcuts_tab, "Keyboard Shortcuts")

        main_layout.addWidget(self.tab_widget)
        buttons = QDialogButtonBox(QDialogButtonBox.Save | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)
        self.setLayout(main_layout)

        # Connect signals from handler to UI slots
        self.connect_signals()
        self.check_for_existing_download()

    def connect_signals(self):
        self.update_handler.status_changed.connect(self.update_status_label.setText)
        self.update_handler.update_check_finished.connect(self.on_update_check_complete)
        self.update_handler.download_progress.connect(self.on_download_progress)
        self.update_handler.download_finished.connect(self.on_download_complete)
        self.update_handler.error_occurred.connect(self.on_update_error)

    def check_for_existing_download(self):
        path = self.update_handler.check_for_existing_download()
        if path:
            self.downloaded_update_path = path
            self.update_status_label.setText("Previously downloaded update is ready.")
            self.check_updates_button.setVisible(False)
            self.restart_update_button.setVisible(True)

    def on_update_check_complete(self, update_available, update_info):
        self.check_updates_button.setEnabled(True)
        if update_available:
            self.update_status_label.setText(f"Update available: {update_info.get('version')}")
            self.download_update_button.setVisible(True)
            self.check_updates_button.setVisible(False)
        else:
            # Status is already set by the handler
            pass

    def on_download_progress(self, bytes_received, bytes_total):
        self.download_update_button.setEnabled(False)
        self.update_progress_bar.setVisible(True)
        if bytes_total > 0:
            self.update_progress_bar.setValue(int((bytes_received / bytes_total) * 100))

    def on_download_complete(self, success, file_path):
        self.update_progress_bar.setVisible(False)
        if success:
            self.downloaded_update_path = file_path
            self.download_update_button.setVisible(False)
            self.restart_update_button.setVisible(True)
        else:
            self.download_update_button.setEnabled(True)

    def on_update_error(self, message):
        self.update_status_label.setText("Update check failed.")
        QMessageBox.critical(self, "Update Error", message)
        self.check_updates_button.setEnabled(True)
        self.download_update_button.setEnabled(True)

    def apply_update(self):
        self.update_handler.apply_update(self.downloaded_update_path)

    def accept(self):
        # General
        self.settings.setValue("show_delete_warning", 
                               "true" if self.show_delete_warning_check.isChecked() else "false")
        self.settings.setValue("use_gpu", 
                               "true" if self.use_gpu_check.isChecked() else "false")
        self.settings.setValue("auto_context_fill", 
                               "true" if self.auto_context_fill_check.isChecked() else "false")
        self.settings.setValue("auto_check_updates", 
                               "true" if self.auto_check_updates_check.isChecked() else "false")
        # OCR Processing
        self.settings.setValue("min_text_height", self.min_text_spin.value())
        self.settings.setValue("max_text_height", self.max_text_spin.value())
        self.settings.setValue("min_confidence", self.confidence_spin.value())
        self.settings.setValue("distance_threshold", self.distance_spin.value())
        self.settings.setValue("ocr_batch_size", self.batch_size_spin.value())
        self.settings.setValue("ocr_decoder", self.decoder_combo.currentText())
        self.settings.setValue("ocr_adjust_contrast", self.contrast_spin.value())
        self.settings.setValue("ocr_resize_threshold", self.resize_threshold_spin.value())
        # API
        self.settings.setValue("gemini_api_key", self.api_key_edit.text())
        self.settings.setValue("gemini_model", self.model_combo.currentData())
        self.settings.setValue("target_language", self.lang_combo.currentText())
        # Shortcuts
        self.settings.setValue("combine_shortcut", self.combine_shortcut_edit.keySequence().toString())
        self.settings.setValue("find_shortcut", self.find_shortcut_edit.keySequence().toString(QKeySequence.NativeText))
        super().accept()