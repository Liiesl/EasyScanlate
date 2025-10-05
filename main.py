# main.py
# Application entry point with a splash screen for a smooth startup.

import sys, os, urllib.request, json, py7zr, tempfile, shutil, ctypes, time, importlib.util

# --- Dependency Checking ---
# Check if we are running as a normal script.
IS_RUNNING_AS_SCRIPT = "__nuitka_version__" not in locals()

try:
    from PySide6.QtWidgets import QApplication, QSplashScreen, QMessageBox, QDialog
    from PySide6.QtCore import Qt, QThread, Signal, QSettings, QDateTime, QObject
    from PySide6.QtGui import QPixmap, QPainter, QFont, QColor
    from app.ui.window.download_dialog import DownloadDialog
except ImportError:
    # This entire block will only be executed if running as a script,
    if IS_RUNNING_AS_SCRIPT:
        try:
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

            msg_box.exec() # Show the dialog and wait for user interaction

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


class Preloader(QThread):
    """
    Performs initial, non-GUI tasks in a separate thread.
    This now includes loading the recent projects list.
    """
    finished = Signal(list)  # Signal will emit the list of loaded project data
    progress_update = Signal(str) 
    download_progress = Signal(int) # download progress bar
    preload_failed = Signal(str) # critical preload failures
    download_complete = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self._is_cancelled = False

    def cancel(self):
        """Public method to signal cancellation to the thread."""
        self._is_cancelled = True

    def check_for_updates(self):
        """Checks for a specific hardcoded update and launches the updater if found."""
        self.progress_update.emit("Checking for updates...")
        TARGET_VERSION = "v0.1.3"
        GH_OWNER = "Liiesl"
        GH_REPO = "EasyScanlate"

        try:
            api_url = f"https://api.github.com/repos/{GH_OWNER}/{GH_REPO}/releases/tags/{TARGET_VERSION}"
            
            with urllib.request.urlopen(api_url) as response:
                if response.status != 200:
                    self.progress_update.emit(f"Update {TARGET_VERSION} not found. Starting normally.")
                    return True # True means continue with normal startup

                release_data = json.loads(response.read().decode())

            manifest_url = None
            package_url = None
            for asset in release_data.get("assets", []):
                if asset['name'] == 'manifest.json':
                    manifest_url = asset['browser_download_url']
                elif asset['name'] == 'update-package.zip':
                    package_url = asset['browser_download_url']

            if not manifest_url or not package_url:
                self.progress_update.emit("Update package is incomplete. Starting normally.")
                return True

            # Use a QMessageBox on the main thread to ask the user.
            # NOTE: This is technically not ideal from a thread, but works for a modal dialog.
            msg_box = QMessageBox()
            msg_box.setIcon(QMessageBox.Information)
            msg_box.setWindowTitle("Update Available")
            msg_box.setText(f"A new version ({TARGET_VERSION}) is available.")
            msg_box.setInformativeText("Do you want to download and install it now?")
            update_button = msg_box.addButton("Update Now", QMessageBox.YesRole)
            later_button = msg_box.addButton("Later", QMessageBox.NoRole)
            msg_box.exec()

            if msg_box.clickedButton() == later_button:
                self.progress_update.emit("Update deferred by user. Starting normally.")
                return True

            self.progress_update.emit(f"Downloading update {TARGET_VERSION}...")

            update_temp_dir = tempfile.mkdtemp(prefix="easyscanlate-update-")

            manifest_path = os.path.join(update_temp_dir, "manifest.json")
            urllib.request.urlretrieve(manifest_url, manifest_path)
            self.progress_update.emit("Downloaded manifest.json...")

            package_path = os.path.join(update_temp_dir, "update-package.zip")
            urllib.request.urlretrieve(package_url, package_path)
            self.progress_update.emit("Downloaded update-package.zip...")
            
            self.progress_update.emit("Download complete. Launching updater...")

            # Assumes Updater.exe is in the same directory as main.exe
            install_dir = os.path.dirname(os.path.abspath(sys.executable))
            updater_exe = os.path.join(install_dir, "Updater.exe")

            if not os.path.exists(updater_exe):
                self.preload_failed.emit(f"Updater application not found at:\n{updater_exe}")
                return False # False means stop preloading.

            args = f'"{update_temp_dir}" "{install_dir}"'
            
            try:
                # Launch the updater. It has its own UAC manifest and will prompt for elevation.
                ctypes.windll.shell32.ShellExecuteW(None, "runas", updater_exe, args, None, 1)
                self.progress_update.emit("Updater has been launched. Closing main application...")
                time.sleep(2) # Give the OS a moment to launch the new process
                sys.exit(0) # Exit this application so the updater can work
            except Exception as e:
                self.preload_failed.emit(f"Failed to launch the updater application:\n{e}")
                return False

        except urllib.error.HTTPError as e:
            if e.code == 404:
                self.progress_update.emit(f"Update version {TARGET_VERSION} not found. Starting normally.")
                return True
            else:
                print(f"Update check (HTTPError): {e}")
                self.progress_update.emit("Update check failed. Starting normally.")
                return True
        except Exception as e:
            print(f"Update check failed: {e}")
            self.progress_update.emit("Update check failed. Starting normally.")
            return True

    def check_and_download_torch(self):
        """
        Checks for PyTorch. If not found, downloads, COMBINES, and extracts it.
        Handles multi-part, pausable, and resumable downloads.
        """
        if importlib.util.find_spec("torch") is not None:
            self.progress_update.emit("PyTorch libraries found.")
            return True
        else:
            self.progress_update.emit("PyTorch not found. Preparing download...")

        GH_OWNER = "Liiesl"
        GH_REPO = "EasyScanlate"
        ASSET_NAME_BASE = "torch_libs.7z"
        COMBINED_ARCHIVE_NAME = "torch_libs_combined.7z" # --- NEW ---
        MAX_RETRIES = 3

        if GH_OWNER == "YourGitHubUsername":
            error_msg = "Initial setup required: Please configure GitHub repository details in main.py."
            self.preload_failed.emit(error_msg)
            return False

        download_dir = os.path.join(tempfile.gettempdir(), "easyscanlate_torch_libs")
        os.makedirs(download_dir, exist_ok=True)
        downloaded_parts = []
        combined_archive_path = os.path.join(download_dir, COMBINED_ARCHIVE_NAME) # --- NEW ---

        try:
            api_url = f"https://api.github.com/repos/{GH_OWNER}/{GH_REPO}/releases/latest"
            self.progress_update.emit("Connecting to GitHub...")
            with urllib.request.urlopen(api_url) as response:
                if response.status != 200:
                    raise Exception(f"GitHub API returned status {response.status}")
                release_data = json.loads(response.read().decode())

            assets = sorted([asset for asset in release_data.get("assets", []) if asset["name"].startswith(ASSET_NAME_BASE)], key=lambda x: x['name'])
            if not assets:
                raise Exception(f"No assets matching '{ASSET_NAME_BASE}.*' found in the latest release.")

            total_download_size = sum(asset['size'] for asset in assets)
            total_bytes_downloaded = 0

            # Calculate already downloaded size for resume progress
            for asset in assets:
                local_path = os.path.join(download_dir, asset['name'])
                if os.path.exists(local_path):
                    total_bytes_downloaded += os.path.getsize(local_path)

            for asset in assets:
                if self._is_cancelled:
                    self.progress_update.emit("Download cancelled.")
                    return False

                asset_name = asset['name']
                download_url = asset['browser_download_url']
                asset_size = asset['size']
                local_path = os.path.join(download_dir, asset_name)
                downloaded_parts.append(local_path)

                current_size = 0
                if os.path.exists(local_path):
                    current_size = os.path.getsize(local_path)

                if current_size >= asset_size:
                    self.progress_update.emit(f"Part '{asset_name}' already downloaded.")
                    continue

                self.progress_update.emit(f"Downloading part '{asset_name}'...")

                for attempt in range(MAX_RETRIES):
                    if self._is_cancelled:
                        return False
                    try:
                        req = urllib.request.Request(download_url)
                        req.add_header('Range', f'bytes={current_size}-')
                        
                        with urllib.request.urlopen(req) as response:
                            if response.status not in [200, 206]:
                                raise Exception(f"Download failed for {asset_name}. Status: {response.status}")
                            
                            with open(local_path, 'ab') as f:
                                while chunk := response.read(8192):
                                    if self._is_cancelled:
                                        self.progress_update.emit("Download cancelled during part download.")
                                        return False
                                    
                                    f.write(chunk)
                                    total_bytes_downloaded += len(chunk)
                                    percent = (total_bytes_downloaded / total_download_size) * 100
                                    self.progress_update.emit(f"Downloading... {int(percent)}%")
                                    self.download_progress.emit(int(percent))
                        
                        break
                    
                    except Exception as e:
                        if attempt < MAX_RETRIES - 1:
                            self.progress_update.emit(f"Download of '{asset_name}' failed (Attempt {attempt + 1}). Retrying in 5 seconds... Error: {e}")
                            time.sleep(5)
                        else:
                            raise Exception(f"Failed to download '{asset_name}' after {MAX_RETRIES} attempts.") from e

            if self._is_cancelled: return False

            # Combine the downloaded parts before extraction
            self.progress_update.emit("Combining downloaded parts...")
            with open(combined_archive_path, 'wb') as combined_file:
                for part_path in downloaded_parts:
                    with open(part_path, 'rb') as part_file:
                        shutil.copyfileobj(part_file, combined_file)
            
            self.progress_update.emit("Extracting... This may take several minutes.")
            # Extract from the combined file
            with py7zr.SevenZipFile(combined_archive_path, mode='r') as z:
                z.extractall()
            
            self.progress_update.emit("Extraction complete.")
            self.download_complete.emit()
            return False

        except Exception as e:
            error_message = f"Failed to download required libraries.\n\nError: {e}\n\nPlease check your internet connection and try again."
            self.preload_failed.emit(error_message)
            return False
        finally:
            # Clean up all temporary files
            for part_path in downloaded_parts:
                if os.path.exists(part_path):
                    try:
                        os.remove(part_path)
                    except OSError as e:
                        print(f"Warning: Could not remove temporary file {part_path}. Error: {e}")
            if os.path.exists(combined_archive_path):
                try:
                    os.remove(combined_archive_path)
                except OSError as e:
                    print(f"Warning: Could not remove combined archive {combined_archive_path}. Error: {e}")


    def run(self):
        """The entry point for the thread."""
        
        # This is now the first and only primary check for this test.
        if not self.check_for_updates():
            return

        # All other checks (like for PyTorch) are effectively bypassed.
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
    Manages the transition between the splash screen and the download dialog.
    """
    def __init__(self, splash, preloader, parent=None):
        super().__init__(parent)
        self.splash = splash
        self.preloader = preloader
        self.download_dialog = None

        self.preloader.progress_update.connect(self.route_progress_message)
        self.preloader.download_complete.connect(self.handle_download_complete)
    
    def handle_download_complete(self):
        """Shows a success message and tells the user to restart the app."""
        if self.download_dialog:
            self.download_dialog.accept()

        if self.splash:
            self.splash.close()

        msg_box = QMessageBox()
        msg_box.setIcon(QMessageBox.Information)
        msg_box.setWindowTitle("Installation Successful")
        msg_box.setText("The required libraries have been successfully installed.")
        # Clear instructions for the user
        msg_box.setInformativeText("Please close and re-open the application to continue.")
        msg_box.setStandardButtons(QMessageBox.Ok)
        msg_box.exec()

        # Exit the application cleanly after the user clicks OK.
        sys.exit(0)

    def route_progress_message(self, message):
        """
        This slot determines whether to show a message on the splash screen
        or to create and show the download dialog.
        """
        if "Preparing download..." in message and not self.download_dialog:
            self.splash.hide()

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
                sys.exit(0)

            self.download_dialog = DownloadDialog()
            
            self.preloader.progress_update.disconnect(self.route_progress_message)
            self.preloader.progress_update.connect(self.download_dialog.update_status)
            self.preloader.download_progress.connect(self.download_dialog.update_progress)
            
            self.preloader.finished.connect(self.download_dialog.accept)
            self.preloader.preload_failed.connect(self.download_dialog.reject)

            result = self.download_dialog.exec()

            if result == QDialog.Rejected:
                self.preloader.cancel()
                self.preloader.wait()
                sys.exit(0)

            self.preloader.progress_update.connect(self.route_progress_message)
        else:
            self.splash.showMessage(message)

# Handler for critical startup failures
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
    # --- The admin and NEEDS_DOWNLOAD checks have been completely removed ---
    # This is now the clean entry point.
    
    app = QApplication(sys.argv)
    
    app.setApplicationName("EasyScanlate")
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