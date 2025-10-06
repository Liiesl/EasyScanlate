# main.py
# Application entry point with a splash screen for a smooth startup.

import sys, os, urllib.request, json, shutil, ctypes, time, importlib.util, py7zr

# --- Dependency Checking ---
# Check if we are running as a normal script.
IS_RUNNING_AS_SCRIPT = "__nuitka_version__" not in locals()

try:
    from PySide6.QtWidgets import QApplication, QSplashScreen, QMessageBox, QDialog
    # MODIFIED: Added QStandardPaths and QProcess for the new updater logic
    from PySide6.QtCore import Qt, QThread, Signal, QSettings, QDateTime, QObject, QStandardPaths, QProcess
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
    finished = Signal(list)
    progress_update = Signal(str)
    download_progress = Signal(int)
    preload_failed = Signal(str)
    # --- MODIFIED --- The signal now carries the updater path (str) and arguments (list)
    download_complete = Signal(str, list)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._is_cancelled = False

    def cancel(self):
        self._is_cancelled = True
        
    def check_and_download_update(self):
        """
        Checks for a new version. If found, it downloads all required packages
        and then signals the main thread to launch the updater.
        """
        self.progress_update.emit("Checking for updates...")
        TARGET_VERSION = "v0.1.4" 
        GH_OWNER = "Liiesl"
        GH_REPO = "EasyScanlate"

        try:
            # ... (Steps 1-7: Check, prompt, download manifest, download packages) ...
            # This part of the logic remains unchanged.
            api_url = f"https://api.github.com/repos/{GH_OWNER}/{GH_REPO}/releases/tags/{TARGET_VERSION}"
            with urllib.request.urlopen(api_url) as response:
                if response.status != 200:
                    self.progress_update.emit(f"Update {TARGET_VERSION} not found. Starting normally.")
                    return True
                release_data = json.loads(response.read().decode())

            manifest_url = None
            for asset in release_data.get("assets", []):
                if asset['name'] == 'manifest.json':
                    manifest_url = asset['browser_download_url']
                    break
            
            if not manifest_url:
                self.progress_update.emit("Update manifest not found. Starting normally.")
                return True

            self.progress_update.emit(f"Update available: {TARGET_VERSION}. Preparing download...")
            
            update_temp_dir = os.path.join(QStandardPaths.writableLocation(QStandardPaths.AppLocalDataLocation), "update_package")
            if os.path.exists(update_temp_dir):
                shutil.rmtree(update_temp_dir)
            os.makedirs(update_temp_dir, exist_ok=True)

            manifest_path = os.path.join(update_temp_dir, "manifest.json")
            self.progress_update.emit("Downloading update manifest...")
            urllib.request.urlretrieve(manifest_url, manifest_path)

            with open(manifest_path, 'r') as f:
                manifest_data = json.load(f)
            
            packages_to_download = []
            for to_version, packages in manifest_data.get("packages", {}).items():
                if to_version == TARGET_VERSION:
                    packages_to_download.extend(packages)
            
            if not packages_to_download:
                self.preload_failed.emit(f"No update package found for version {TARGET_VERSION} in the manifest.")
                return False

            total_size = sum(pkg['size'] for pkg in packages_to_download)
            total_bytes_downloaded = 0

            for package in packages_to_download:
                file_name = package['file']
                package_url = f"https://github.com/{GH_OWNER}/{GH_REPO}/releases/download/{TARGET_VERSION}/{file_name}"
                package_path = os.path.join(update_temp_dir, file_name)
                self.progress_update.emit(f"Downloading {file_name}...")
                
                req = urllib.request.Request(package_url)
                with urllib.request.urlopen(req) as response:
                    bytes_downloaded_for_file = 0
                    with open(package_path, 'wb') as f:
                        while chunk := response.read(8192):
                            if self._is_cancelled:
                                self.progress_update.emit("Download cancelled.")
                                shutil.rmtree(update_temp_dir, ignore_errors=True)
                                return False
                            
                            f.write(chunk)
                            bytes_downloaded_for_file += len(chunk)
                            
                            if total_size > 0:
                                percent = ((total_bytes_downloaded + bytes_downloaded_for_file) / total_size) * 100
                                self.progress_update.emit(f"Downloading update... {int(percent)}%")
                                self.download_progress.emit(int(percent))
                
                total_bytes_downloaded += os.path.getsize(package_path)

            # --- MODIFIED ---
            # 8. PREPARE to launch the updater, but do not launch it here.
            self.progress_update.emit("Download complete. Preparing to launch updater...")
            install_dir = os.path.dirname(os.path.abspath(sys.executable))
            updater_exe = os.path.join(install_dir, "updater", "Updater.exe")

            if not os.path.exists(updater_exe):
                self.preload_failed.emit(f"Updater application not found at:\n{updater_exe}")
                return False
            
            args = [update_temp_dir, install_dir]
            
            # 9. Emit the signal with the necessary info for the main thread.
            self.download_complete.emit(updater_exe, args)
            return False # False means we handled the update, so don't continue startup.

        except urllib.error.HTTPError as e:
            if e.code == 404:
                self.progress_update.emit(f"No new updates found. Starting normally.")
                return True
            self.progress_update.emit(f"Update check failed (HTTP {e.code}). Starting normally.")
            return True
        except Exception as e:
            print(f"Update check failed: {e}")
            self.preload_failed.emit(f"An unexpected error occurred during the update check:\n{e}")
            return False

    def run(self):
        """The entry point for the thread."""
        if not self.check_and_download_update():
            return

        self.progress_update.emit("Finding recent projects...")
        projects_data = []
        try:
            settings = QSettings("Liiesl", "EasyScanlate")
            recent_projects = settings.value("recent_projects", [])
            recent_timestamps = settings.value("recent_timestamps", {})

            MAX_RECENT_PROJECTS = 6
            if len(recent_projects) > MAX_RECENT_PROJECTS:
                self.progress_update.emit("Cleaning up old project entries...")
                projects_to_remove = recent_projects[MAX_RECENT_PROJECTS:]
                recent_projects = recent_projects[:MAX_RECENT_PROJECTS]
                settings.setValue("recent_projects", recent_projects)
                for path_to_remove in projects_to_remove:
                    if path_to_remove in recent_timestamps:
                        del recent_timestamps[path_to_remove]
                settings.setValue("recent_timestamps", recent_timestamps)
            
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
    def __init__(self, splash, preloader, parent=None):
        super().__init__(parent)
        self.splash = splash
        self.preloader = preloader
        self.download_dialog = None

        self.preloader.progress_update.connect(self.route_progress_message)
        self.preloader.download_complete.connect(self.handle_download_complete)

    # --- MODIFIED --- This slot now receives the updater path and args.
    def handle_download_complete(self, updater_exe, args):
        """Shows a confirmation message, then launches the updater and exits."""
        if self.download_dialog:
            self.download_dialog.accept()
        if self.splash:
            self.splash.close()

        msg_box = QMessageBox()
        msg_box.setIcon(QMessageBox.Information)
        msg_box.setWindowTitle("Update Ready")
        msg_box.setText("The update has been downloaded.")
        msg_box.setInformativeText("The application will now close and the updater will run. The updater may request administrator permissions.\n\nClick OK to proceed.")
        msg_box.setStandardButtons(QMessageBox.Ok)
        
        # This blocks until the user clicks OK.
        msg_box.exec()

        # --- MODIFIED --- The updater is launched HERE, after the user clicks OK.
        if not QProcess.startDetached(f'"{updater_exe}"', args):
            QMessageBox.critical(None, "Updater Error", "Failed to launch the updater process.")
            sys.exit(1) # Exit with an error code if launch fails
        else:
            sys.exit(0) # Exit gracefully

    def route_progress_message(self, message):
        """
        Determines whether to show a message on the splash screen
        or to create and show the download dialog for the update.
        """
        if "Update available" in message and not self.download_dialog:
            self.splash.hide()

            msg_box = QMessageBox()
            msg_box.setIcon(QMessageBox.Information)
            msg_box.setWindowTitle("Update Available")
            msg_box.setText(f"<h3>A new version is available!</h3>")
            msg_box.setInformativeText(
                "Do you want to download and install it now?<br><br>"
                "The application will close and an updater will run."
            )
            proceed_button = msg_box.addButton("Update Now", QMessageBox.YesRole)
            cancel_button = msg_box.addButton("Later", QMessageBox.NoRole)
            msg_box.setDefaultButton(proceed_button)
            msg_box.exec()

            if msg_box.clickedButton() == cancel_button:
                self.preloader.cancel()
                self.preloader.wait()
                sys.exit(0)

            self.download_dialog = DownloadDialog()
            self.download_dialog.setWindowTitle("Downloading Update")
            
            self.preloader.progress_update.disconnect(self.route_progress_message)
            self.preloader.progress_update.connect(self.download_dialog.update_status)
            self.preloader.download_progress.connect(self.download_dialog.update_progress)
            
            self.preloader.preload_failed.connect(self.download_dialog.reject)
            
            result = self.download_dialog.exec()

            if result == QDialog.Rejected: 
                self.preloader.cancel()
                self.preloader.wait()
                sys.exit(0)
            
            self.preloader.progress_update.connect(self.route_progress_message)
        else:
            self.splash.showMessage(message)

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
    app = QApplication(sys.argv)
    
    app.setApplicationName("ManhwaOCR")
    app.setApplicationVersion("1.0")

    pixmap = QPixmap(500, 250)
    pixmap.fill(QColor(45, 45, 45))
    painter = QPainter(pixmap)
    painter.setPen(QColor(220, 220, 220))
    font = QFont("Segoe UI", 24, QFont.Bold)
    painter.setFont(font)
    painter.drawText(pixmap.rect().adjusted(0, -20, 0, -20), Qt.AlignCenter, "ManhwaOCR")
    painter.end()

    splash = CustomSplashScreen(pixmap)
    splash.show()

    preloader = Preloader()
    
    ui_manager = UIManager(splash, preloader)

    preloader.finished.connect(on_preload_finished)
    preloader.preload_failed.connect(on_preload_failed)
    
    preloader.start()

    sys.exit(app.exec())