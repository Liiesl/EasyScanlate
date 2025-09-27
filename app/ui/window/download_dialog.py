# app/ui/dialogs/download_dialog.py
# A dedicated dialog for showing download progress.

from PySide6.QtWidgets import QDialog, QVBoxLayout, QLabel, QProgressBar, QPushButton, QMessageBox
from PySide6.QtCore import Qt

class DownloadDialog(QDialog):
    """
    A modal dialog window to show the status and progress of a file download.
    """
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Downloading Dependencies")
        self.setModal(True)
        self.setFixedSize(450, 150)
        # Prevent the user from closing the window manually via the 'X' button
        self.setWindowFlags(Qt.Dialog | Qt.CustomizeWindowHint | Qt.WindowTitleHint)

        layout = QVBoxLayout(self)
        self.status_label = QLabel("Initializing download...", self)
        self.status_label.setAlignment(Qt.AlignCenter)
        self.status_label.setWordWrap(True)

        self.progress_bar = QProgressBar(self)
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        self.progress_bar.setTextVisible(True)

        self.cancel_button = QPushButton("Cancel", self)
        self.cancel_button.clicked.connect(self.handle_cancel_request)

        layout.addWidget(self.status_label)
        layout.addWidget(self.progress_bar)
        layout.addWidget(self.cancel_button, 0, Qt.AlignRight)

    def handle_cancel_request(self):
        """Shows a confirmation dialog when the user tries to cancel the download."""
        msg_box = QMessageBox(self)
        msg_box.setIcon(QMessageBox.Warning)
        msg_box.setWindowTitle("Cancel Download?")
        msg_box.setText("Are you sure you want to cancel the download?")
        msg_box.setInformativeText("If you cancel now, you will have to start over the next time you open the application.")
        
        continue_button = msg_box.addButton("Continue Downloading", QMessageBox.RejectRole)
        cancel_anyway_button = msg_box.addButton("Cancel Anyway", QMessageBox.AcceptRole)
        msg_box.setDefaultButton(continue_button)

        msg_box.exec()

        if msg_box.clickedButton() == cancel_anyway_button:
            self.reject() # Rejects the dialog, which can be caught by the caller

    def closeEvent(self, event):
        """Overrides the close event (e.g., clicking the 'X' button)."""
        self.handle_cancel_request()
        event.ignore() # Prevents the window from closing immediately

    def update_status(self, message: str):
        """Public slot to update the text label."""
        self.status_label.setText(message)

    def update_progress(self, value: int):
        """Public slot to update the progress bar's value."""
        self.progress_bar.setValue(value)