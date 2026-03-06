# welcome_dialog.py

from PySide6.QtWidgets import QDialog, QVBoxLayout, QHBoxLayout, QLabel, QPushButton, QCheckBox, QMessageBox
from PySide6.QtCore import Qt, QSettings, QUrl
from PySide6.QtGui import QDesktopServices


class WelcomeDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self.setWindowTitle("Welcome to Easy Scanlate")
        self.setMinimumSize(450, 250)
        self.setWindowFlags(Qt.Dialog | Qt.WindowTitleHint | Qt.WindowCloseButtonHint)

        layout = QVBoxLayout()
        layout.setSpacing(15)

        title_label = QLabel("New to the app?")
        title_label.setStyleSheet("font-size: 20px; font-weight: bold;")
        title_label.setAlignment(Qt.AlignCenter)
        layout.addWidget(title_label)

        message_label = QLabel("Check out our user-manual documentation to get started.")
        message_label.setStyleSheet("font-size: 14px; color: #aaaaaa;")
        message_label.setAlignment(Qt.AlignCenter)
        message_label.setWordWrap(True)
        layout.addWidget(message_label)

        doc_button = QPushButton("Open Documentation")
        doc_button.setCursor(Qt.PointingHandCursor)
        doc_button.clicked.connect(self.open_documentation)
        doc_button.setMinimumHeight(35)
        layout.addWidget(doc_button)

        layout.addStretch()

        self.dont_show_checkbox = QCheckBox("Don't show this again")
        layout.addWidget(self.dont_show_checkbox)

        buttons_layout = QHBoxLayout()
        buttons_layout.addStretch()

        close_button = QPushButton("Close")
        close_button.setMinimumWidth(100)
        close_button.clicked.connect(self.accept)
        buttons_layout.addWidget(close_button)

        layout.addLayout(buttons_layout)

        self.setLayout(layout)
        self.setStyleSheet("""
            QDialog {
                background-color: #1D1D1D;
                color: #FFFFFF;
            }
            QLabel {
                background-color: transparent;
                color: #FFFFFF;
            }
            QPushButton {
                background-color: #3E3E3E;
                color: #FFFFFF;
                border: none;
                border-radius: 4px;
                padding: 8px 16px;
                font-size: 13px;
            }
            QPushButton:hover {
                background-color: #4E4E4E;
            }
            QPushButton:pressed {
                background-color: #2E2E2E;
            }
            QCheckBox {
                color: #aaaaaa;
            }
            QCheckBox::indicator {
                width: 16px;
                height: 16px;
            }
        """)

    def open_documentation(self):
        docs_url = "https://docs.easyscanlate.site/"
        QDesktopServices.openUrl(QUrl(docs_url))

    def accept(self):
        if self.dont_show_checkbox.isChecked():
            self.settings.setValue("show_welcome_dialog", "false")
        super().accept()

    @staticmethod
    def show_if_needed(parent):
        settings = QSettings("Liiesl", "EasyScanlate")
        show_welcome = settings.value("show_welcome_dialog", "true")
        if show_welcome == "true":
            dialog = WelcomeDialog(parent)
            dialog.exec()
