from PySide6.QtWidgets import (QDialog, QDoubleSpinBox, QVBoxLayout, QFormLayout,
                             QComboBox, QSpinBox, QDialogButtonBox, QTabWidget,
                             QWidget, QLineEdit, QKeySequenceEdit, QCheckBox) # Added QLabel
from PySide6.QtGui import QKeySequence
from assets import ADVANCED_CHECK_STYLES

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
        self.settings = parent.settings  # Access main window's QSettings

        # Main layout for the dialog
        main_layout = QVBoxLayout()

        # Create tab widget
        self.tab_widget = QTabWidget()

        # --- General Tab ---
        general_tab = QWidget()
        general_layout = QFormLayout()

        self.show_delete_warning_check = QCheckBox()
        self.show_delete_warning_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.show_delete_warning_check.setChecked(
            self.settings.value("show_delete_warning", "true") == "true"
        )
        general_layout.addRow("Show delete confirmation dialog:", self.show_delete_warning_check)

        # --- Add GPU Setting ---
        self.use_gpu_check = QCheckBox()
        self.use_gpu_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.use_gpu_check.setChecked(
            self.settings.value("use_gpu", "true").lower() == "true" # Default to True
        )
        self.use_gpu_check.setToolTip("Requires compatible NVIDIA GPU and CUDA drivers. Restart may be needed.")
        general_layout.addRow("Use GPU for OCR (if available):", self.use_gpu_check)
        # --- End GPU Setting ---

        self.auto_context_fill_check = QCheckBox()
        self.auto_context_fill_check.setStyleSheet(ADVANCED_CHECK_STYLES)
        self.auto_context_fill_check.setChecked(
            self.settings.value("auto_context_fill", "false") == "true"
        )
        self.auto_context_fill_check.setToolTip("Automatically inpaint the background behind detected text during Batch OCR.\nThis can improve text rendering but may slow down processing.")
        general_layout.addRow("Auto Context Fill Background on Batch OCR:", self.auto_context_fill_check)

        general_tab.setLayout(general_layout)
        self.tab_widget.addTab(general_tab, "General")

        # --- OCR Processing Settings Tab ---
        processing_tab = QWidget()
        form_layout = QFormLayout()

        # Min Text Height Setting
        self.min_text_spin = QSpinBox()
        self.min_text_spin.setRange(0, 10000) # More reasonable max
        self.min_text_spin.setSuffix(" px")
        self.min_text_spin.setValue(int(self.settings.value("min_text_height", 40)))
        form_layout.addRow("Minimum Text Height:", self.min_text_spin)

        # Max Text Height Setting
        self.max_text_spin = QSpinBox()
        self.max_text_spin.setRange(0, 10000) # More reasonable max
        self.max_text_spin.setSuffix(" px")
        self.max_text_spin.setValue(int(self.settings.value("max_text_height", 100)))
        form_layout.addRow("Maximum Text Height:", self.max_text_spin)

        # Min Confidence Setting
        self.confidence_spin = QDoubleSpinBox()
        self.confidence_spin.setRange(0.0, 1.0)
        self.confidence_spin.setSingleStep(0.05) # Smaller step
        self.confidence_spin.setDecimals(2)
        self.confidence_spin.setValue(float(self.settings.value("min_confidence", 0.2)))
        form_layout.addRow("Minimum Confidence:", self.confidence_spin)

        # Distance Threshold Setting
        self.distance_spin = QSpinBox()
        self.distance_spin.setRange(0, 1000) # More reasonable max
        self.distance_spin.setSuffix(" px")
        self.distance_spin.setValue(int(self.settings.value("distance_threshold", 100)))
        form_layout.addRow("Merge Distance Threshold:", self.distance_spin)

        # --- NEW EASYOCR SETTINGS ---

        # Batch Size
        self.batch_size_spin = QSpinBox()
        self.batch_size_spin.setRange(1, 64) # Range 1 to 64
        self.batch_size_spin.setValue(int(self.settings.value("ocr_batch_size", 8))) # Default 8
        self.batch_size_spin.setToolTip("Number of image patches processed simultaneously (higher needs more GPU VRAM).")
        form_layout.addRow("OCR Batch Size:", self.batch_size_spin)

        # Decoder
        self.decoder_combo = QComboBox()
        self.decoder_combo.addItems(["beamsearch", "greedy"])
        self.decoder_combo.setCurrentText(self.settings.value("ocr_decoder", "beamsearch")) # Default beamsearch
        self.decoder_combo.setToolTip("'beamsearch' is generally more accurate but slower. 'greedy' is faster.")
        form_layout.addRow("OCR Decoder:", self.decoder_combo)

        # Adjust Contrast
        self.contrast_spin = QDoubleSpinBox()
        self.contrast_spin.setRange(0.0, 1.0)
        self.contrast_spin.setSingleStep(0.1)
        self.contrast_spin.setDecimals(1)
        self.contrast_spin.setValue(float(self.settings.value("ocr_adjust_contrast", 0.5))) # Default 0.5
        self.contrast_spin.setToolTip("Automatically adjust image contrast (0.0 to disable). May help or hurt depending on image.")
        form_layout.addRow("OCR Adjust Contrast:", self.contrast_spin)

        # Resize Threshold (Max Width)
        self.resize_threshold_spin = QSpinBox()
        self.resize_threshold_spin.setRange(0, 8192) # Allow up to 8k, 0 for disable
        self.resize_threshold_spin.setSuffix(" px")
        self.resize_threshold_spin.setSpecialValueText("Disabled") # Show text when value is 0
        self.resize_threshold_spin.setValue(int(self.settings.value("ocr_resize_threshold", 1024))) # Default 1024
        self.resize_threshold_spin.setToolTip("Resize images wider than this before OCR. Set to 0 to disable resizing.")
        form_layout.addRow("OCR Resize Threshold (Max Width):", self.resize_threshold_spin)

        # --- END NEW EASYOCR SETTINGS ---

        processing_tab.setLayout(form_layout)
        self.tab_widget.addTab(processing_tab, "OCR Processing")

        # --- API Settings Tab ---
        api_tab = QWidget()
        api_layout = QFormLayout()

        # Gemini API Key
        self.api_key_edit = QLineEdit()
        self.api_key_edit.setEchoMode(QLineEdit.Password)
        self.api_key_edit.setText(self.settings.value("gemini_api_key", ""))
        api_layout.addRow("Gemini API Key:", self.api_key_edit)

        # Gemini Model Selection
        self.model_combo = QComboBox()
        
        # Define model information (actual_name, display_info_text)
        # Order matches the original list provided in the problem description

        for model_name, model_info_text in GEMINI_MODELS_WITH_INFO:
            display_text = f"{model_name} | {model_info_text}"
            self.model_combo.addItem(display_text, userData=model_name) # Store actual model name as userData

        current_model_value = self.settings.value("gemini_model", "gemini-2.5-flash-lite")
        # Find the index of the item whose userData matches the saved model name
        for i in range(self.model_combo.count()):
            if self.model_combo.itemData(i) == current_model_value:
                self.model_combo.setCurrentIndex(i)
                break
        # If the saved model is not found in the list, it will default to the first item (index 0)

        api_layout.addRow("Gemini Model:", self.model_combo)


        # Target Language
        self.lang_combo = QComboBox()
        self.lang_combo.addItems([ # Add more languages if needed
            "English", "Japanese", "Chinese (Simplified)", "Korean",
            "Spanish", "French", "German", "Bahasa Indonesia", # Renamed Bahasa Indonesia
            "Vietnamese", "Thai", "Russian", "Portuguese"
        ])
        current_lang = self.settings.value("target_language", "English")
        self.lang_combo.setCurrentText(current_lang)
        api_layout.addRow("Target Language:", self.lang_combo)

        api_tab.setLayout(api_layout)
        self.tab_widget.addTab(api_tab, "Translations")

        # --- Keyboard Shortcuts Tab ---
        shortcuts_tab = QWidget()
        shortcuts_layout = QFormLayout()

        # Combine Rows Shortcut
        self.combine_shortcut_edit = QKeySequenceEdit(
            QKeySequence(self.settings.value("combine_shortcut", "Ctrl+G"))
        )
        shortcuts_layout.addRow("Combine Rows Shortcut:", self.combine_shortcut_edit)

        # Find/Replace Shortcut
        self.find_shortcut_edit = QKeySequenceEdit(
            QKeySequence(self.settings.value("find_shortcut", "Ctrl+F"))
        )
        shortcuts_layout.addRow("Find/Replace Shortcut:", self.find_shortcut_edit)
        shortcuts_tab.setLayout(shortcuts_layout)
        self.tab_widget.addTab(shortcuts_tab, "Keyboard Shortcuts")

        # Add the tab widget to the main layout
        main_layout.addWidget(self.tab_widget)

        # Buttons
        buttons = QDialogButtonBox(QDialogButtonBox.Save | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)

        self.setLayout(main_layout)

    def accept(self):
        # Save General settings
        self.settings.setValue("show_delete_warning",
            "true" if self.show_delete_warning_check.isChecked() else "false")
        self.settings.setValue("use_gpu",
            "true" if self.use_gpu_check.isChecked() else "false")
        self.settings.setValue("auto_context_fill",
            "true" if self.auto_context_fill_check.isChecked() else "false")
        # Save OCR Processing settings
        self.settings.setValue("min_text_height", self.min_text_spin.value())
        self.settings.setValue("max_text_height", self.max_text_spin.value())
        self.settings.setValue("min_confidence", self.confidence_spin.value())
        self.settings.setValue("distance_threshold", self.distance_spin.value())
        # Save new EasyOCR settings
        self.settings.setValue("ocr_batch_size", self.batch_size_spin.value())
        self.settings.setValue("ocr_decoder", self.decoder_combo.currentText())
        self.settings.setValue("ocr_adjust_contrast", self.contrast_spin.value())
        self.settings.setValue("ocr_resize_threshold", self.resize_threshold_spin.value())

        # Save API settings
        self.settings.setValue("gemini_api_key", self.api_key_edit.text())
        self.settings.setValue("gemini_model", self.model_combo.currentData()) # Use currentData() to get actual model name
        self.settings.setValue("target_language", self.lang_combo.currentText())

        # Save Keyboard Shortcuts
        self.settings.setValue("combine_shortcut", self.combine_shortcut_edit.keySequence().toString())
        # Use NativeText format for saving find shortcut if needed, standard toString might be fine
        self.settings.setValue("find_shortcut", self.find_shortcut_edit.keySequence().toString(QKeySequence.NativeText))

        super().accept()