# main.py
# Application entry point with a splash screen for a smooth startup.

import sys, os

# Check if we are running as a normal script.
IS_RUNNING_AS_SCRIPT = "__nuitka_version__" not in locals()

try:
    from PySide6.QtWidgets import QApplication, QSplashScreen, QMessageBox
    from PySide6.QtCore import Qt, QThread, Signal, QSettings, QDateTime, QObject
    from PySide6.QtGui import QPixmap, QPainter, QFont, QColor
    from app.utils.exception_handler import setup_global_exception_handler
    from app.ui.dialogs.error_dialog import ErrorDialog
except ImportError:
    # This entire block will only be executed if running as a script,
    if IS_RUNNING_AS_SCRIPT:
        try:
            from PyQt5.QtWidgets import QApplication, QMessageBox  # type: ignore
            from PyQt5.QtCore import Qt  # type: ignore

            app = QApplication(sys.argv)
            msg_box = QMessageBox()
            msg_box.setIcon(QMessageBox.Critical)
            msg_box.setWindowTitle("Dependency Error")
            msg_box.setText("Incorrect GUI Library Detected")

            # Main informational text
            msg_box.setInformativeText(
                "This application requires the PySide6 library, but it appears you have PyQt5 installed instead.\n\n"
                "To resolve this, please uninstall PyQt5 and then install PySide6."
            )

            # Make the informative text selectable by the user
            msg_box.setTextInteractionFlags(Qt.TextSelectableByMouse)

            # Place the commands in a collapsible "Details" section
            commands = "pip uninstall PyQt5\npip install pyside6"
            msg_box.setDetailedText(
                "Run the following commands in your terminal or command prompt:\n\n" + commands
            )

            # Add a custom button to copy the commands to the clipboard
            copy_button = msg_box.addButton("Copy Commands", QMessageBox.ActionRole)
            msg_box.setDefaultButton(QMessageBox.Ok)

            msg_box.exec()  # Show the dialog and wait for user interaction

            if msg_box.clickedButton() == copy_button:
                try:
                    clipboard = QApplication.clipboard()
                    clipboard.setText(commands)
                    confirm_msg = QMessageBox()
                    confirm_msg.setIcon(QMessageBox.Information)
                    confirm_msg.setText("Commands copied to clipboard!")
                    confirm_msg.exec()
                except Exception as e:
                    # Handle cases where clipboard access might fail
                    error_msg = QMessageBox()
                    error_msg.setIcon(QMessageBox.Warning)
                    error_msg.setText(f"Could not access clipboard:\n{e}")
                    error_msg.exec()
        except ImportError:
            print("CRITICAL ERROR: PySide6 is not installed...")
        sys.exit(1)

class CustomSplashScreen(QSplashScreen):
    """A custom splash screen to show loading messages."""
    def __init__(self, pixmap):
        super().__init__(pixmap)
        self.message = "Initializing..."
        self.setStyleSheet("QSplashScreen { border: 1px solid #555; }")

    def drawContents(self, painter):
        """Draw the pixmap and the custom message."""
        super().drawContents(painter)
        text_rect = self.rect().adjusted(10, 0, -10, -10)
        painter.setPen(QColor(220, 220, 220)) # Light gray text
        painter.drawText(text_rect, Qt.AlignBottom | Qt.AlignLeft, self.message)

    def showMessage(self, message, alignment=Qt.AlignLeft, color=Qt.white):
        """Override to repaint the splash screen with the new message."""
        self.message = message
        super().showMessage(message, alignment, color)
        self.repaint()
        QApplication.processEvents()


def get_relative_time(timestamp_str):
    """Calculates a human-readable relative time string from an ISO date string."""
    if not timestamp_str: return "Never opened"
    timestamp = QDateTime.fromString(timestamp_str, Qt.ISODate)
    seconds = timestamp.secsTo(QDateTime.currentDateTime())
    if seconds < 0: return timestamp.toString("MMM d, yyyy h:mm AP")
    if seconds < 60: return "Just now"
    minutes = seconds // 60
    if minutes < 60: return f"{minutes} minute{'s' if minutes > 1 else ''} ago"
    hours = seconds // 3600
    if hours < 24: return f"{hours} hour{'s' if hours > 1 else ''} ago"
    days = seconds // 86400
    if days < 7: return f"{days} day{'s' if days > 1 else ''} ago"
    weeks = seconds // 604800
    if weeks < 4: return f"{weeks} week{'s' if weeks > 1 else ''} ago"
    months = seconds // 2592000
    if months < 12: return f"{months} month{'s' if months > 1 else ''} ago"
    years = seconds // 31536000
    return f"{years} year{'s' if years > 1 else ''} ago"


class Preloader(QThread):
    """
    Performs initial, non-GUI tasks in a separate thread.
    This now includes loading the recent projects list.
    """
    finished = Signal(list)  # Signal will emit the list of loaded project data
    progress_update = Signal(str)
    preload_failed = Signal(str)  # critical preload failures

    def __init__(self, parent=None):
        super().__init__(parent)

    def run(self):
        """The entry point for the thread."""

        self.progress_update.emit("Loading application settings...")
        
        self.progress_update.emit("Finding recent projects...")
        projects_data = []
        try:
            settings = QSettings("Liiesl", "EasyScanlate")
            recent_projects = settings.value("recent_projects", [])
            recent_timestamps = settings.value("recent_timestamps", {})

            # --- START: Added logic to limit recent projects ---
            MAX_RECENT_PROJECTS = 6
            if len(recent_projects) > MAX_RECENT_PROJECTS:
                self.progress_update.emit("Cleaning up old project entries...")
                # Get the list of projects to remove (the oldest ones)
                projects_to_remove = recent_projects[MAX_RECENT_PROJECTS:]
                # Trim the main list to the 6 most recent
                recent_projects = recent_projects[:MAX_RECENT_PROJECTS]
                
                # Update the settings with the trimmed list
                settings.setValue("recent_projects", recent_projects)
                
                # Remove the timestamps associated with the old projects
                for path_to_remove in projects_to_remove:
                    if path_to_remove in recent_timestamps:
                        del recent_timestamps[path_to_remove]
                
                # Update the settings with the cleaned timestamps dictionary
                settings.setValue("recent_timestamps", recent_timestamps)
            # --- END: Added logic ---
            
            for path in recent_projects:
                filename = os.path.basename(path)
                self.progress_update.emit(f"Verifying: {filename}...")
                
                if os.path.exists(path):
                    timestamp = recent_timestamps.get(path, "")
                    last_opened = get_relative_time(timestamp)
                    projects_data.append({
                        "name": filename,
                        "path": path,
                        "last_opened": last_opened
                    })
        except Exception as e:
            print(f"Could not preload recent projects: {e}")

        self.finished.emit(projects_data)

splash = None
home_window = None

class UIManager(QObject):
    """
    Manages the transition between the splash screen and showing progress messages.
    """
    def __init__(self, splash, preloader, parent=None):
        super().__init__(parent)
        self.splash = splash
        self.preloader = preloader

        self.preloader.progress_update.connect(self.splash.showMessage)

# Handler for critical startup failures
def on_preload_failed(error_message):
    """Shows a critical error message box and terminates the application."""
    global splash
    if splash:
        splash.close()
    ErrorDialog.critical(None, "Application Startup Error", error_message)
    sys.exit(1)

def on_preload_finished(projects_data):
    """
    This slot runs on the main thread. It creates the Home window instance,
    populates it with the preloaded data, and then decides whether to show
    it or immediately launch a project.
    """
    global home_window, splash, preloader
    print("[ENTRY] Preloading finished. Handling window creation.")

    splash.showMessage("Loading main window...")
    
    from app.ui.window.home_window import Home
    
    if home_window is None:
        home_window = Home(progress_signal=preloader.progress_update)
        
        splash.showMessage("Populating recent projects...")
        home_window.populate_recent_projects(projects_data)
        print("[ENTRY] Home window instance created and populated.")

    project_to_open = None
    if len(sys.argv) > 1 and sys.argv[1].lower().endswith('.mmtl'):
        path = sys.argv[1]
        if os.path.exists(path):
            project_to_open = path
        else:
            QMessageBox.critical(home_window, "Error", f"The project file could not be found:\n{path}")

    if project_to_open:
        print("[ENTRY] Project file provided. Skipping Home window.")
        splash.close()
        home_window.launch_main_app(project_to_open)
    else:
        print("[ENTRY] No project file. Showing Home window.")
        home_window.show()
        splash.finish(home_window)

    print("[ENTRY] Initial launch sequence complete.")


if __name__ == '__main__':
    app = QApplication(sys.argv)
    
    # Set up global exception handler to catch unhandled exceptions
    setup_global_exception_handler(app)
    
    app.setApplicationName("ManhwaOCR")
    app.setApplicationVersion("1.0")

    pixmap = QPixmap(500, 250)
    pixmap.fill(QColor(45, 45, 45))
    painter = QPainter(pixmap)
    painter.setPen(QColor(220, 220, 220))
    font = QFont("Segoe UI", 24, QFont.Bold)
    painter.setFont(font)
    painter.drawText(pixmap.rect().adjusted(0, -20, 0, -20), Qt.AlignCenter, "EasyScanlate")
    painter.end()

    splash = CustomSplashScreen(pixmap)
    splash.show()

    preloader = Preloader()
    
    ui_manager = UIManager(splash, preloader)

    preloader.finished.connect(on_preload_finished)
    preloader.preload_failed.connect(on_preload_failed)
    
    preloader.start()

    sys.exit(app.exec())