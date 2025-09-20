# main.py
# Application entry point with a splash screen for a smooth startup.
# FIXED to prevent multiple Home windows and handle app termination properly.
# FIXED to clean up partial downloads on cancellation.
# --- MODIFIED: Download archive to temp directory to avoid permission errors ---
# --- MODIFIED: Check for elevated privileges on Windows before download ---
# --- MODIFIED: Restart application with standard privileges after admin-level download and extraction is complete ---

import sys
import os
# --- NEW IMPORTS for downloader and 7z extraction ---
import urllib.request
import json
import py7zr # <--- ADDED for .7z support
import tempfile # <--- MODIFIED: Added to handle temporary file downloads
# --- NEW IMPORT for Windows administrator check ---
import ctypes
# --- NEW IMPORT for restarting the application ---
import subprocess


# --- Dependency Checking ---

# Nuitka provides the __nuitka_version__ attribute during compilation.
# We check if it's NOT defined, meaning we are running as a normal script.
IS_RUNNING_AS_SCRIPT = "__nuitka_version__" not in locals()

try:
    from PySide6.QtWidgets import QApplication, QSplashScreen, QMessageBox, QDialog
    from PySide6.QtCore import Qt, QThread, Signal, QSettings, QDateTime, QObject
    from PySide6.QtGui import QPixmap, QPainter, QFont, QColor
    # --- NEW: Import the download dialog ---
    from app.ui.window.download_dialog import DownloadDialog
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

            msg_box.exec() # Show the dialog and wait for user interaction

            # If the user clicked our custom "Copy" button, copy the commands
            if msg_box.clickedButton() == copy_button:
                try:
                    clipboard = QApplication.clipboard()
                    clipboard.setText(commands)
                    # Optional: Show a confirmation message
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
            # If py7zr is missing when running as a script, this provides a better error.
            if 'py7zr' not in sys.modules:
                 print("CRITICAL ERROR: The 'py7zr' library is not installed. Please run 'pip install py7zr'.")
            else:
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
    # --- NEW: Signal for the download progress bar ---
    download_progress = Signal(int)
    # --- NEW: Signal for handling critical preload failures ---
    preload_failed = Signal(str)
    # --- NEW: Signal to trigger an application restart to de-elevate privileges ---
    restart_required = Signal()

    def __init__(self, is_admin=False, parent=None):
        super().__init__(parent)
        self._is_cancelled = False
        self.is_admin = is_admin # --- MODIFIED: Store admin status

    def cancel(self):
        """Public method to signal cancellation to the thread."""
        self._is_cancelled = True

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
        GH_OWNER = "Liiesl"      # Your GitHub username
        GH_REPO = "test-easyocr-compile"              # Your repository name
        ASSET_NAME = "torch_libs.7z"          # <--- MODIFIED to use .7z
        
        if GH_OWNER == "YourGitHubUsername":
            error_msg = "Initial setup required: Please configure GitHub repository details in main.py inside the 'check_and_download_torch' function before compiling."
            self.progress_update.emit("ERROR: GitHub details not configured.")
            self.preload_failed.emit(error_msg)
            return False
            
        # --- MODIFIED: Use the system's temporary directory for the download ---
        # This prevents permission errors if the app is in a protected location.
        archive_path = os.path.join(tempfile.gettempdir(), ASSET_NAME)
            
        try:
            api_url = f"https://api.github.com/repos/{GH_OWNER}/{GH_REPO}/releases/latest"
            self.progress_update.emit("Connecting to GitHub...")
            with urllib.request.urlopen(api_url) as response:
                if response.status != 200: raise Exception(f"GitHub API returned status {response.status}")
                release_data = json.loads(response.read().decode())
            
            download_url = next((asset["browser_download_url"] for asset in release_data.get("assets", []) if asset["name"] == ASSET_NAME), None)
            if not download_url: raise Exception(f"Asset '{ASSET_NAME}' not found in the latest release.")

            self.progress_update.emit(f"Downloading '{ASSET_NAME}'...")

            with urllib.request.urlopen(download_url) as response:
                if response.status != 200: raise Exception(f"Download failed. Status: {response.status}")
                total_size = int(response.getheader('Content-Length', 0))
                bytes_downloaded = 0
                chunk_size = 8192
                with open(archive_path, 'wb') as f:
                    while chunk := response.read(chunk_size):
                        # --- MODIFIED: Check for cancellation during download ---
                        if self._is_cancelled:
                            self.progress_update.emit("Download cancelled.")
                            return False # Exit gracefully, finally block will clean up.

                        f.write(chunk)
                        bytes_downloaded += len(chunk)
                        if total_size > 0:
                            percent = (bytes_downloaded / total_size) * 100
                            self.progress_update.emit(f"Downloading... {int(percent)}%")
                            self.download_progress.emit(int(percent))
            
            # --- MODIFIED: Check for cancellation before extraction ---
            if self._is_cancelled:
                self.progress_update.emit("Operation cancelled before extraction.")
                return False # Exit gracefully, finally block will clean up.

            self.progress_update.emit("Download complete. Extracting files...")
            # By default, extractall() extracts to the current working directory,
            # which is the desired behavior.
            with py7zr.SevenZipFile(archive_path, mode='r') as z:
                z.extractall()
            
            self.progress_update.emit("Extraction complete. Verifying...")

            import torch
            self.progress_update.emit("PyTorch successfully installed.")

            # --- NEW: If download was performed with admin rights, trigger a restart to de-elevate ---
            if self.is_admin:
                self.progress_update.emit("Restarting to drop admin privileges...")
                self.restart_required.emit()
                return False # Stop further preloading; the app will restart.

            return True

        except Exception as e:
            error_message = f"Failed to download required libraries.\n\nError: {e}\n\nPlease check your internet connection and try again. If the issue persists, the asset may be missing from the GitHub release."
            self.preload_failed.emit(error_message)
            return False
        finally:
            # --- MODIFIED: This block ensures the archive is removed on success, failure, or cancellation ---
            if os.path.exists(archive_path):
                os.remove(archive_path)

    def run(self):
        """The entry point for the thread."""
        
        # --- NEW: Run the PyTorch check first ---
        if not self.check_and_download_torch():
            return  # Stop preloading on failure, cancellation, or if a restart is triggered

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

# --- NEW: UI Manager to handle switching between splash and download dialog ---
class UIManager(QObject):
    def __init__(self, splash, preloader, parent=None):
        super().__init__(parent)
        self.splash = splash
        self.preloader = preloader
        self.download_dialog = None

        # Connect signals that this manager will handle
        self.preloader.progress_update.connect(self.route_progress_message)
    
    def route_progress_message(self, message):
        """
        This slot determines whether to show a message on the splash screen
        or to create and show the download dialog.
        """
        if "Preparing download..." in message and not self.download_dialog:
            self.splash.hide()

            # --- Confirmation Dialog before starting the download ---
            msg_box = QMessageBox()
            msg_box.setIcon(QMessageBox.Information)
            msg_box.setWindowTitle("Download Required")
            msg_box.setText("<h3>Additional Libraries Required</h3>")
            msg_box.setInformativeText(
                "To function correctly, this application needs to download the PyTorch library, which is over 1.8GB (4GB when unpacked) in size.<br><br>"
                "This can take a significant amount of time depending on your internet connection. Please ensure you have a stable connection and enough free disk space before proceeding."
            )
            proceed_button = msg_box.addButton("Proceed", QMessageBox.YesRole)
            cancel_button = msg_box.addButton("I'll do it later", QMessageBox.NoRole)
            msg_box.setDefaultButton(proceed_button)
            msg_box.exec()

            if msg_box.clickedButton() == cancel_button:
                sys.exit(0) # Exit the application if user cancels

            # --- User proceeded, now show the actual download dialog ---
            self.download_dialog = DownloadDialog()
            
            self.preloader.progress_update.disconnect(self.route_progress_message)
            self.preloader.progress_update.connect(self.download_dialog.update_status)
            self.preloader.download_progress.connect(self.download_dialog.update_progress)
            
            self.preloader.finished.connect(self.download_dialog.accept)
            self.preloader.preload_failed.connect(self.download_dialog.reject)

            result = self.download_dialog.exec()

            # --- MODIFIED: Handle cancellation from the download dialog ---
            # If the user cancelled, the dialog is Rejected.
            if result == QDialog.Rejected:
                # Signal the thread to stop and clean up.
                self.preloader.cancel()
                # Crucially, wait for the thread to finish its cleanup before exiting.
                self.preloader.wait()
                # Now, exit the application cleanly.
                sys.exit(0)

            self.preloader.progress_update.connect(self.route_progress_message)
        # --- MODIFIED: Do not show the restart message on the splash screen ---
        elif "Restarting to drop" in message:
            pass # The restart handler will show its own message box.
        else:
            self.splash.showMessage(message)

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

    # Show a final message on the splash screen before it closes.
    splash.showMessage("Loading main window...")
    
    # --- Import Home from the correct module ---
    from app.ui.window.home_window import Home
    
    # Only create Home window instance if it doesn't exist
    if home_window is None:
        # Pass the preloader's progress signal to the Home window constructor.
        # Now the Home window can also send messages to the splash screen.
        home_window = Home(progress_signal=preloader.progress_update)
        
        splash.showMessage("Populating recent projects...")
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
    # It ensures the app's working directory is where the .exe is, not C:\Windows\System32
    if getattr(sys, 'frozen', False): # 'frozen' is true for a Nuitka/PyInstaller exe
        app_path = os.path.dirname(os.path.abspath(sys.executable))
        os.chdir(app_path)
    
    is_currently_admin = False # --- NEW: Initialize admin status flag
    # --- NEW: Administrator check for Windows before starting the app ---
    # This check is performed only if torch needs to be downloaded, as that is the
    # only operation that requires elevated privileges for extraction.
    if sys.platform == 'win32':
        NEEDS_DOWNLOAD = False
        try:
            # Check if torch is missing.
            import torch
        except ImportError:
            NEEDS_DOWNLOAD = True

        if NEEDS_DOWNLOAD:
            try:
                # Check if the application is already running as an administrator.
                is_currently_admin = ctypes.windll.shell32.IsUserAnAdmin()
            except Exception:
                is_currently_admin = False # Assume not admin if the check fails.

            if not is_currently_admin:
                # If not admin, a restart with elevated privileges is required.
                # A temporary QApplication instance is needed to show a message box.
                app = QApplication(sys.argv)
                
                msg_box = QMessageBox()
                msg_box.setIcon(QMessageBox.Warning)
                msg_box.setWindowTitle("Administrator Privileges Required")
                msg_box.setText("To download and install required libraries, this application needs to be run with administrator privileges.")
                msg_box.setInformativeText("Do you want to automatically restart the application as an administrator?")
                
                restart_button = msg_box.addButton("Restart as Admin", QMessageBox.YesRole)
                cancel_button = msg_box.addButton("Cancel", QMessageBox.NoRole)
                msg_box.setDefaultButton(restart_button)
                msg_box.exec()

                if msg_box.clickedButton() == restart_button:
                    # Re-run the script/executable with the 'runas' verb to request elevation.
                    try:
                        ctypes.windll.shell32.ShellExecuteW(None, "runas", sys.executable, " ".join(sys.argv), None, 1)
                    except Exception as e:
                         # If the re-launch fails, show a final error message.
                        error_msg = QMessageBox()
                        error_msg.setIcon(QMessageBox.Critical)
                        error_msg.setWindowTitle("Relaunch Failed")
                        error_msg.setText(f"Failed to restart with administrator privileges:\n{e}")
                        error_msg.exec()
                        sys.exit(1) # Exit with an error code.
                
                # Exit the current, non-elevated instance.
                # This happens whether the user chose to restart or cancel.
                sys.exit(0)

    # The dependency check is now at the top of the file, so if we get here,
    # we can assume PySide6 is installed.
    app = QApplication(sys.argv)

    # --- NEW: Function to handle restarting with standard privileges ---
    def restart_as_user():
        """
        Shows a confirmation message and restarts the app using explorer.exe to drop privileges.
        This function is connected to the preloader's signal and runs on the main thread.
        """
        QMessageBox.information(None, "Restart Required", 
                                "The required libraries have been installed successfully.\n\n"
                                "The application will now restart with normal user permissions.")
        
        # Using explorer.exe to launch the executable is a common trick to ensure
        # it starts non-elevated from an elevated parent process.
        # It also correctly handles passing command-line arguments.
        args = " ".join(sys.argv[1:])
        subprocess.Popen(f'explorer.exe "{sys.executable}" {args}')
        
        # Exit the current elevated application cleanly.
        app.quit()
    
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

    # --- MODIFIED: Pass the admin status to the preloader ---
    preloader = Preloader(is_admin=is_currently_admin)
    
    # --- NEW: Set up the UI Manager to handle UI transitions ---
    ui_manager = UIManager(splash, preloader)

    # --- Connect final outcome signals ---
    preloader.finished.connect(on_preload_finished)
    preloader.preload_failed.connect(on_preload_failed)
    # --- NEW: Connect the restart signal to its handler ---
    preloader.restart_required.connect(restart_as_user)
    
    preloader.start()

    sys.exit(app.exec())