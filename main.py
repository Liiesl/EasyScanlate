# main.py
# Application entry point with a splash screen for a smooth startup.
# FIXED to prevent multiple Home windows and handle app termination properly.

import sys
import os
# --- NEW IMPORTS for downloader ---
import urllib.request
import json
import zipfile

# --- Dependency Checking ---

# Nuitka provides the __nuitka_version__ attribute during compilation.
# We check if it's NOT defined, meaning we are running as a normal script.
IS_RUNNING_AS_SCRIPT = "__nuitka_version__" not in locals()

try:
    from PySide6.QtWidgets import QApplication, QSplashScreen, QMessageBox
    from PySide6.QtCore import Qt, QThread, Signal, QSettings, QDateTime, QObject
    from PySide6.QtGui import QPixmap, QPainter, QFont, QColor
except ImportError:
    # This entire block will only be executed if running as a script,
    # as Nuitka will bundle PySide6, preventing this error.
    if IS_RUNNING_AS_SCRIPT:
        try:
            # If this import succeeds, it means the user has PyQt5.
            # We need to inform them to install PySide6.
            # We will use PyQt5 components to show a more robust and helpful error message.
            from PyQt5.QtWidgets import QApplication, QMessageBox #type: ignore
            from PyQt5.QtCore import Qt #type: ignore

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
            
            # --- NEW: Make the informative text selectable by the user ---
            msg_box.setTextInteractionFlags(Qt.TextSelectableByMouse)

            # --- NEW: Place the commands in a collapsible "Details" section ---
            # This text is naturally copyable.
            commands = "pip uninstall PyQt5\npip install pyside6"
            msg_box.setDetailedText(
                "Run the following commands in your terminal or command prompt:\n\n" + commands
            )
            
            # --- NEW: Add a custom button to copy the commands to the clipboard ---
            copy_button = msg_box.addButton("Copy Commands", QMessageBox.ActionRole)
            msg_box.setDefaultButton(QMessageBox.Ok)

            msg_box.exec_() # Show the dialog and wait for user interaction

            # If the user clicked our custom "Copy" button, copy the commands
            if msg_box.clickedButton() == copy_button:
                try:
                    clipboard = QApplication.clipboard()
                    clipboard.setText(commands)
                    # Optional: Show a confirmation message
                    confirm_msg = QMessageBox()
                    confirm_msg.setIcon(QMessageBox.Information)
                    confirm_msg.setText("Commands copied to clipboard!")
                    confirm_msg.exec_()
                except Exception as e:
                    # Handle cases where clipboard access might fail
                    error_msg = QMessageBox()
                    error_msg.setIcon(QMessageBox.Warning)
                    error_msg.setText(f"Could not access clipboard:\n{e}")
                    error_msg.exec_()
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

# main.py

class Preloader(QThread):
    """
    Performs initial, non-GUI tasks in a separate thread.
    This now includes loading the recent projects list.
    """
    finished = Signal(list)  # Signal will emit the list of loaded project data
    progress_update = Signal(str)
    # --- NEW: Signal for handling critical preload failures ---
    preload_failed = Signal(str)

    def check_and_download_torch(self):
        """
        Checks if PyTorch ('torch') is installed. If not, downloads and extracts it
        from a specified GitHub release before the main app starts.
        Returns True on success or if already present, False on critical failure.
        """
        try:
            import torch
            self.progress_update.emit("PyTorch libraries found.")
            return True
        except ImportError:
            self.progress_update.emit("PyTorch not found. Preparing download...")

        # --- ACTION REQUIRED: Configure your GitHub repository details here ---
        GH_OWNER = "YourGitHubUsername"  # Your GitHub username
        GH_REPO = "YourRepoName"          # Your repository name
        ASSET_NAME = "torch_libs.zip"     # The exact name of the asset in your release
        
        if GH_OWNER == "YourGitHubUsername":
            error_msg = "Initial setup required: Please configure GitHub repository details in main.py inside the 'check_and_download_torch' function before compiling."
            self.progress_update.emit("ERROR: GitHub details not configured.")
            self.preload_failed.emit(error_msg)
            return False
            
        try:
            api_url = f"https://api.github.com/repos/{GH_OWNER}/{GH_REPO}/releases/latest"
            self.progress_update.emit("Connecting to GitHub...")
            with urllib.request.urlopen(api_url) as response:
                if response.status != 200: raise Exception(f"GitHub API returned status {response.status}")
                release_data = json.loads(response.read().decode())
            
            download_url = next((asset["browser_download_url"] for asset in release_data.get("assets", []) if asset["name"] == ASSET_NAME), None)
            if not download_url: raise Exception(f"Asset '{ASSET_NAME}' not found in the latest release.")

            self.progress_update.emit(f"Downloading '{ASSET_NAME}'...")
            zip_path = os.path.join(os.getcwd(), ASSET_NAME)

            with urllib.request.urlopen(download_url) as response:
                if response.status != 200: raise Exception(f"Download failed. Status: {response.status}")
                total_size = int(response.getheader('Content-Length', 0))
                bytes_downloaded = 0
                chunk_size = 8192
                with open(zip_path, 'wb') as f:
                    while chunk := response.read(chunk_size):
                        f.write(chunk)
                        bytes_downloaded += len(chunk)
                        if total_size > 0:
                            percent = (bytes_downloaded / total_size) * 100
                            self.progress_update.emit(f"Downloading... {int(percent)}%")
            
            self.progress_update.emit("Download complete. Extracting files...")
            with zipfile.ZipFile(zip_path, 'r') as zip_ref:
                zip_ref.extractall(".")
            
            os.remove(zip_path)
            self.progress_update.emit("Extraction complete. Verifying...")

            import torch
            self.progress_update.emit("PyTorch successfully installed.")
            return True

        except Exception as e:
            error_message = f"Failed to download required libraries.\n\nError: {e}\n\nPlease check your internet connection and try again. If the issue persists, the asset may be missing from the GitHub release."
            self.preload_failed.emit(error_message)
            return False

    def run(self):
        """The entry point for the thread."""
        
        # --- NEW: Run the PyTorch check first ---
        if not self.check_and_download_torch():
            return  # Stop preloading on failure

        self.progress_update.emit("Loading application settings...")
        
        # --- Actually preload the recent projects data ---
        self.progress_update.emit("Finding recent projects...")
        projects_data = []
        try:
            settings = QSettings("YourCompany", "MangaOCRTool")
            recent_projects = settings.value("recent_projects", [])
            recent_timestamps = settings.value("recent_timestamps", {})
            
            # --- IMPROVED LOOP ---
            for path in recent_projects:
                filename = os.path.basename(path)
                # Update the splash screen BEFORE the potentially blocking call
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
            # Continue with an empty list on error

        self.finished.emit(projects_data)


# --- Global variables to hold instances ---
splash = None
home_window = None

# --- NEW: Handler for critical startup failures ---
def on_preload_failed(error_message):
    """Shows a critical error message box and terminates the application."""
    global splash
    if splash:
        splash.close()
    QMessageBox.critical(None, "Application Startup Error", error_message)
    sys.exit(1)

def on_preload_finished(projects_data):
    """
    This slot runs on the main thread. It creates the Home window instance,
    populates it with the preloaded data, and then decides whether to show
    it or immediately launch a project.
    """
    global home_window, splash, preloader
    print("[ENTRY] Preloading finished. Handling window creation.")

    preloader.progress_update.emit("Importing necessary packages...")
    # --- Import Home from the correct module ---
    from app.ui.window.home_window import Home
    
    # Only create Home window instance if it doesn't exist
    if home_window is None:
        # Pass the preloader's progress signal to the Home window constructor.
        # Now the Home window can also send messages to the splash screen.
        home_window = Home(progress_signal=preloader.progress_update)
        
        preloader.progress_update.emit("Populating recent projects...")
        # --- Populate the home window with the preloaded data ---
        home_window.populate_recent_projects(projects_data)
        print("[ENTRY] Home window instance created and populated.")

    # Check for a project file in command-line arguments.
    project_to_open = None
    if len(sys.argv) > 1 and sys.argv[1].lower().endswith('.mmtl'):
        path = sys.argv[1]
        if os.path.exists(path):
            project_to_open = path
        else:
            # We need a parent for the message box. The invisible home_window is perfect.
            QMessageBox.critical(home_window, "Error", f"The project file could not be found:\n{path}")

    # --- THIS IS THE IMPROVED LOGIC ---
    if project_to_open:
        print("[ENTRY] Project file provided. Skipping Home window.")
        # Immediately close the splash screen. We use .close() instead of .finish()
        # because we are not transferring control to a visible main window yet.
        splash.close()

        # Now, call the method that creates and shows the LoadingDialog.
        # Because the dialog is modal (exec_()), this call will block until
        # the project is loaded or fails, preventing the script from exiting early.
        home_window.launch_main_app(project_to_open)
    else:
        print("[ENTRY] No project file. Showing Home window.")
        # Show the fully prepared Home window.
        home_window.show()

        # Gracefully close the splash screen, transferring focus to the Home window.
        splash.finish(home_window)

    print("[ENTRY] Initial launch sequence complete.")


if __name__ == '__main__':
    # The dependency check is now at the top of the file, so if we get here,
    # we can assume PySide6 is installed.
    app = QApplication(sys.argv)
    
    # Check if application is already running (optional)
    app.setApplicationName("ManhwaOCR")
    app.setApplicationVersion("1.0")

    # --- Create and configure the splash screen pixmap ---
    pixmap = QPixmap(500, 250)
    pixmap.fill(QColor(45, 45, 45))
    painter = QPainter(pixmap)
    painter.setPen(QColor(220, 220, 220))
    font = QFont("Segoe UI", 24, QFont.Bold)
    painter.setFont(font)
    painter.drawText(pixmap.rect().adjusted(0, -20, 0, -20), Qt.AlignCenter, "ManhwaOCR")
    painter.end()

    # --- Show splash and start preloader ---
    splash = CustomSplashScreen(pixmap)
    splash.show()

    preloader = Preloader()
    preloader.progress_update.connect(splash.showMessage)
    preloader.finished.connect(on_preload_finished)
    # --- NEW: Connect the failure signal ---
    preloader.preload_failed.connect(on_preload_failed)
    preloader.start()

    sys.exit(app.exec())