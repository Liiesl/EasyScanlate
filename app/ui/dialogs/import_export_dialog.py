from PySide6.QtWidgets import (
    QDialog, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QFileDialog, QComboBox, QGroupBox, QRadioButton, QButtonGroup,
    QCheckBox, QLineEdit, QDialogButtonBox, QSpinBox, QFormLayout
)
from PySide6.QtCore import Qt, QDir
from assets.styles4 import IMPORT_EXPORT_STYLES
import os


class ImportDialog(QDialog):
    """Dialog for importing translation files with profile selection."""
    
    def __init__(self, parent=None, available_profiles=None):
        super().__init__(parent)
        self.setWindowTitle("Import Translation File")
        self.setMinimumSize(550, 200)
        
        self.available_profiles = available_profiles or []
        self.file_path = None
        
        layout = QVBoxLayout()
        self.setLayout(layout)
        
        # File selection
        file_group = QGroupBox("File Selection")
        file_layout = QVBoxLayout()
        
        file_path_layout = QHBoxLayout()
        self.file_path_edit = QLineEdit()
        self.file_path_edit.setPlaceholderText("No file selected...")
        self.file_path_edit.setReadOnly(True)
        self.browse_btn = QPushButton("Browse...")
        self.browse_btn.clicked.connect(self.browse_file)
        file_path_layout.addWidget(self.file_path_edit)
        file_path_layout.addWidget(self.browse_btn)
        file_layout.addLayout(file_path_layout)
        
        file_group.setLayout(file_layout)
        layout.addWidget(file_group)
        
        # Translation file options
        translation_group = QGroupBox("Translation File Options")
        translation_layout = QFormLayout()
        
        self.profile_combo = QComboBox()
        self.profile_combo.addItem("<Create New Profile>")
        self.profile_combo.addItems(self.available_profiles)
        self.profile_combo.setCurrentIndex(0)  # Default to "Create New Profile"
        self.profile_combo.currentTextChanged.connect(self.on_profile_changed)
        translation_layout.addRow("Profile:", self.profile_combo)
        
        self.new_profile_edit = QLineEdit()
        self.new_profile_edit.setPlaceholderText("Enter new profile name...")
        translation_layout.addRow("New Profile Name:", self.new_profile_edit)
        
        translation_group.setLayout(translation_layout)
        layout.addWidget(translation_group)
        
        # Dialog buttons
        button_box = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel,
            Qt.Horizontal, self
        )
        button_box.accepted.connect(self.validate_and_accept)
        button_box.rejected.connect(self.reject)
        button_box.button(QDialogButtonBox.Ok).setText("Import")
        
        layout.addWidget(button_box)
        layout.addStretch()
        
        self.setObjectName("ImportDialog")
        self.setStyleSheet(IMPORT_EXPORT_STYLES)
        
        # Initialize UI state (new profile field visible by default)
        self.on_profile_changed("<Create New Profile>")
    
    def browse_file(self):
        """Open file dialog to select import file."""
        try:
            file_path, _ = QFileDialog.getOpenFileName(
                self, "Import Translation File", QDir.homePath(),
                "Translation Files (*.xml *.txt *.md);;XML Files (*.xml);;Text Files (*.txt);;Markdown Files (*.md);;All Files (*.*)"
            )
            
            if file_path:
                self.file_path = file_path
                self.file_path_edit.setText(file_path)
                
                # Set default profile name to filename (without extension)
                filename = os.path.splitext(os.path.basename(file_path))[0]
                if filename in self.available_profiles:
                    # If filename matches an existing profile, select it
                    self.profile_combo.setCurrentText(filename)
                else:
                    # If filename doesn't match, keep "Create New Profile" and prefill the name
                    self.profile_combo.setCurrentIndex(0)  # "<Create New Profile>"
                    self.new_profile_edit.setText(filename)
        except Exception as e:
            import traceback
            from app.ui.dialogs.error_dialog import ErrorDialog
            ErrorDialog.critical(
                self, "File Selection Error",
                f"Failed to select file:\n{str(e)}",
                traceback.format_exc()
            )
    
    def on_profile_changed(self, text):
        """Show/hide new profile name field based on selection."""
        is_new = text == "<Create New Profile>"
        self.new_profile_edit.setVisible(is_new)
    
    def validate_and_accept(self):
        """Validate inputs before accepting."""
        if not self.file_path or not os.path.exists(self.file_path):
            from PySide6.QtWidgets import QMessageBox
            QMessageBox.warning(self, "Invalid File", "Please select a valid file.")
            return
        
        profile_selection = self.profile_combo.currentText()
        if profile_selection == "<Create New Profile>":
            profile_name = self.new_profile_edit.text().strip()
            if not profile_name:
                from PySide6.QtWidgets import QMessageBox
                QMessageBox.warning(self, "Invalid Profile", "Please enter a new profile name.")
                return
        else:
            profile_name = profile_selection
        
        self.accept()
    
    def get_import_config(self):
        """Get the import configuration."""
        profile_selection = self.profile_combo.currentText()
        if profile_selection == "<Create New Profile>":
            profile_name = self.new_profile_edit.text().strip()
        else:
            profile_name = profile_selection
        
        return {
            'file_path': self.file_path,
            'import_type': 'translation',
            'profile_name': profile_name
        }


class ExportDialog(QDialog):
    """Dialog for exporting OCR results with configuration options."""
    
    def __init__(self, parent=None, available_profiles=None, project_name=None, project_directory=None):
        super().__init__(parent)
        self.setWindowTitle("Export OCR Results")
        self.setMinimumSize(550, 350)
        
        self.available_profiles = available_profiles or []
        self.project_name = project_name
        self.project_directory = project_directory
        
        layout = QVBoxLayout()
        self.setLayout(layout)
        
        # OCR export options
        self.ocr_options_group = QGroupBox("Export Options")
        ocr_layout = QFormLayout()
        
        self.ocr_format_combo = QComboBox()
        self.ocr_format_combo.addItems(["Master (JSON)", "For-Translate (XML)"])
        self.ocr_format_combo.currentIndexChanged.connect(self.on_ocr_format_changed)
        ocr_layout.addRow("Format:", self.ocr_format_combo)
        
        self.profile_label = QLabel("Profile:")
        self.profile_combo = QComboBox()
        self.profile_combo.addItems(self.available_profiles if self.available_profiles else ["Original"])
        self.profile_combo.currentIndexChanged.connect(self.update_default_path)
        ocr_layout.addRow(self.profile_label, self.profile_combo)
        
        self.file_format_label = QLabel("File Format:")
        self.file_format_combo = QComboBox()
        self.file_format_combo.addItems(["JSON", "XML", "TXT"])
        self.file_format_combo.currentIndexChanged.connect(self.on_file_format_changed)
        ocr_layout.addRow(self.file_format_label, self.file_format_combo)
        
        self.pretty_print_check = QCheckBox("Pretty print (indented)")
        self.pretty_print_check.setChecked(True)
        ocr_layout.addRow("", self.pretty_print_check)
        
        self.ocr_options_group.setLayout(ocr_layout)
        layout.addWidget(self.ocr_options_group)
        
        # Output location
        output_group = QGroupBox("Output Location")
        output_layout = QVBoxLayout()
        
        output_path_layout = QHBoxLayout()
        self.output_path_edit = QLineEdit()
        if self.project_directory and self.project_name:
            default_path = os.path.join(self.project_directory, f"{self.project_name}.json")
            self.output_path_edit.setText(default_path)
        self.output_path_edit.setPlaceholderText("Select output location...")
        self.browse_btn = QPushButton("Browse...")
        self.browse_btn.clicked.connect(self.browse_output)
        output_path_layout.addWidget(self.output_path_edit)
        output_path_layout.addWidget(self.browse_btn)
        output_layout.addLayout(output_path_layout)
        
        output_group.setLayout(output_layout)
        layout.addWidget(output_group)
        
        # Dialog buttons
        button_box = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel,
            Qt.Horizontal, self
        )
        button_box.accepted.connect(self.validate_and_accept)
        button_box.rejected.connect(self.reject)
        button_box.button(QDialogButtonBox.Ok).setText("Export")
        
        layout.addWidget(button_box)
        layout.addStretch()
        
        self.setObjectName("ExportDialog")
        self.setStyleSheet(IMPORT_EXPORT_STYLES)
        self.on_ocr_format_changed()
    
    def on_ocr_format_changed(self):
        """Update file format options when OCR format changes."""
        is_master = self.ocr_format_combo.currentIndex() == 0
        
        # Hide/show profile combo based on format
        self.profile_label.setVisible(not is_master)
        self.profile_combo.setVisible(not is_master)
        
        # Update file format options
        if is_master:
            # Lock to JSON for master format
            self.file_format_combo.clear()
            self.file_format_combo.addItems(["JSON"])
            self.file_format_combo.setEnabled(False)
            self.file_format_combo.setToolTip("Master format only supports JSON")
            self.file_format_label.setToolTip("Master format only supports JSON")
        else:
            self.file_format_combo.setEnabled(True)
            self.file_format_combo.setToolTip("")
            self.file_format_label.setToolTip("")
            if self.file_format_combo.count() == 0 or self.file_format_combo.currentText() == "JSON":
                self.file_format_combo.clear()
                self.file_format_combo.addItems(["XML", "TXT"])
        
        self.update_default_path()
    
    def update_default_path(self):
        """Update the default output path based on current settings."""
        if not self.project_directory:
            return
        
        is_master = self.ocr_format_combo.currentIndex() == 0
        
        if is_master:
            if self.project_name:
                ext = ".json"
                default_path = os.path.join(self.project_directory, f"{self.project_name}{ext}")
                self.output_path_edit.setText(default_path)
        else:
            # For translation, use profile name (except "Original" which becomes "translation")
            profile_name = self.profile_combo.currentText()
            if profile_name == "Original":
                filename = "translation"
            else:
                filename = profile_name
            # Get extension from file format combo
            file_format = self.file_format_combo.currentText().lower()
            ext = f".{file_format}"
            default_path = os.path.join(self.project_directory, f"{filename}{ext}")
            self.output_path_edit.setText(default_path)
    
    def on_file_format_changed(self):
        """Show warning if user tries to change format when master is selected, and update path."""
        is_master = self.ocr_format_combo.currentIndex() == 0
        if is_master and self.file_format_combo.currentText() != "JSON":
            from PySide6.QtWidgets import QMessageBox
            QMessageBox.information(
                self, 
                "Master Format Restriction", 
                "Master format only supports JSON.\nFile format has been set to JSON."
            )
            # Reset to JSON
            self.file_format_combo.blockSignals(True)
            self.file_format_combo.setCurrentIndex(0)
            self.file_format_combo.blockSignals(False)
        else:
            # Update path when file format changes (for translation exports)
            self.update_default_path()
    
    def browse_output(self):
        """Open file dialog to select output location."""
        try:
            # Determine file extension based on format
            format_idx = self.ocr_format_combo.currentIndex()
            file_format_idx = self.file_format_combo.currentIndex()
            
            if format_idx == 0:  # Master
                ext = "json"
                filter_str = "JSON Files (*.json)"
            else:  # For-Translate
                if file_format_idx == 0:  # XML
                    ext = "xml"
                    filter_str = "XML Files (*.xml);;Text Files (*.txt);;All Files (*.*)"
                else:  # TXT
                    ext = "txt"
                    filter_str = "Text Files (*.txt);;XML Files (*.xml);;All Files (*.*)"
            
            default_path = self.output_path_edit.text() or os.path.join(
                self.project_directory or QDir.homePath(),
                f"{self.project_name or 'export'}.{ext}"
            )
            
            file_path, _ = QFileDialog.getSaveFileName(
                self, "Export OCR Results", default_path, filter_str
            )
            
            if file_path:
                self.output_path_edit.setText(file_path)
        except Exception as e:
            import traceback
            from app.ui.dialogs.error_dialog import ErrorDialog
            ErrorDialog.critical(
                self, "Output Selection Error",
                f"Failed to select output location:\n{str(e)}",
                traceback.format_exc()
            )
    
    def validate_and_accept(self):
        """Validate inputs before accepting."""
        output_path = self.output_path_edit.text().strip()
        if not output_path:
            from PySide6.QtWidgets import QMessageBox
            QMessageBox.warning(self, "Invalid Path", "Please specify an output location.")
            return
        
        self.accept()
    
    def get_export_config(self):
        """Get the export configuration."""
        return {
            'export_type': 'ocr',
            'output_path': self.output_path_edit.text().strip(),
            'format': 'master' if self.ocr_format_combo.currentIndex() == 0 else 'for-translate',
            'profile_name': self.profile_combo.currentText() if self.profile_combo.isEnabled() else None,
            'file_format': self.file_format_combo.currentText().lower(),
            'pretty_print': self.pretty_print_check.isChecked()
        }

