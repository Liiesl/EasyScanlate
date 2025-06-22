# mmtl_launcher.py
# A launcher for the ManhwOCR that can run in two modes:
# 1. Fast path: Launches the main application without a GUI, using command line arguments.
# 2. Slow path: Shows a configuration dialog to set paths for the main application and  
# Bundle it yourself and set the executable as the default application for .mmtl files.

import sys
import os
import subprocess

# --- Non-GUI Helper Functions (for the fast path) ---
def show_native_error(title, message):
    """
    Displays a native Windows message box without needing PyQt5.
    This is very fast and ideal for showing errors when the GUI isn't loaded.
    """
    import ctypes
    # MB_OK | MB_ICONERROR
    # For more options, see: https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-messageboxw
    ctypes.windll.user32.MessageBoxW(0, message, title, 0x0 | 0x10)


def get_settings_value(key):
    """
    Reads a value from QSettings without needing a QApplication instance.
    We must import QSettings locally to avoid loading it when not needed.
    Note: QSettings can be slow. For maximum speed, a simple .ini or .json
    file could be even faster, but QSettings is fine for this use case.
    """
    from PyQt5.QtCore import QSettings
    settings = QSettings("YourCompany", "MangaOCRTool")
    return settings.value(key)

# --- Main Application Logic ---

def launch_main_app_fast(mmtl_path):
    """
    The 'fast path' launcher. It does not import or initialize any GUI components.
    It reads settings and launches the main application.
    """
    MAIN_PY_PATH_KEY = "launcher/main_py_path"
    PYTHON_EXE_PATH_KEY = "launcher/python_exe_path"

    # Read paths from settings
    main_py_path = get_settings_value(MAIN_PY_PATH_KEY)
    python_executable = get_settings_value(PYTHON_EXE_PATH_KEY)

    # Validate paths and show native error messages if something is wrong
    if not python_executable or not os.path.exists(python_executable):
        show_native_error(
            "Launcher Configuration Error",
            "The path to the Python interpreter (python.exe) is not set or is invalid.\n\n"
            "Please run the launcher executable directly (without a file) to configure the paths."
        )
        return

    if not main_py_path or not os.path.exists(main_py_path):
        show_native_error(
            "Launcher Configuration Error",
            "The path to main.py is not set or is invalid.\n\n"
            "Please run the launcher executable directly (without a file) to configure the paths."
        )
        return

    # Launch the main app as a separate process and exit immediately.
    command = [python_executable, main_py_path, mmtl_path]
    try:
        # Popen is non-blocking. The launcher will exit as soon as it's called.
        # The `creationflags` argument was removed to allow the console window of main.py to appear.
        subprocess.Popen(command)
        print(f"Executing command: {' '.join(command)}") # For debugging
    except Exception as e:
        show_native_error(
            "Execution Error",
            f"Failed to launch the main application.\n\n"
            f"Error: {e}\n\n"
            f"Command: {' '.join(command)}"
        )


def show_configuration_dialog():
    """
    The 'slow path' that imports PyQt5 and shows the configuration GUI.
    This is only called when the launcher is run without arguments.
    """
    # --- LAZY IMPORTS: PyQt5 is only imported when this function is called ---
    from PyQt5.QtWidgets import (QApplication, QWidget, QVBoxLayout, QHBoxLayout,
                                 QLabel, QLineEdit, QPushButton, QFileDialog,
                                 QMessageBox, QFrame)
    from PyQt5.QtCore import QSettings

    SETTINGS = QSettings("YourCompany", "MangaOCRTool")
    MAIN_PY_PATH_KEY = "launcher/main_py_path"
    PYTHON_EXE_PATH_KEY = "launcher/python_exe_path"

    class LauncherConfigDialog(QWidget):
        """A simple GUI to set the path to main.py and the python executable."""
        def __init__(self):
            super().__init__()
            self.setWindowTitle("MangaOCR Launcher Configuration")
            self.setMinimumWidth(600)
            self.init_ui()
            self.load_settings()

        def init_ui(self):
            layout = QVBoxLayout(self)
            py_label = QLabel("1. Set the path to the Python executable (python.exe).")
            py_label.setStyleSheet("font-weight: bold;")
            layout.addWidget(py_label)
            py_info_label = QLabel("This is often located in a 'Scripts' folder inside a virtual environment (venv).")
            layout.addWidget(py_info_label)
            py_path_layout = QHBoxLayout()
            self.python_path_edit = QLineEdit()
            self.python_path_edit.setPlaceholderText("Click 'Browse' to find python.exe")
            self.browse_py_btn = QPushButton("Browse...")
            self.browse_py_btn.clicked.connect(self.browse_for_python_exe)
            py_path_layout.addWidget(self.python_path_edit)
            py_path_layout.addWidget(self.browse_py_btn)
            layout.addLayout(py_path_layout)
            separator = QFrame()
            separator.setFrameShape(QFrame.HLine)
            separator.setFrameShadow(QFrame.Sunken)
            layout.addWidget(separator)
            main_label = QLabel("2. Set the full path to your main.py file.")
            main_label.setStyleSheet("font-weight: bold;")
            layout.addWidget(main_label)
            path_layout = QHBoxLayout()
            self.path_edit = QLineEdit()
            self.path_edit.setPlaceholderText("Click 'Browse' to find main.py")
            self.browse_btn = QPushButton("Browse...")
            self.browse_btn.clicked.connect(self.browse_for_main_py)
            path_layout.addWidget(self.path_edit)
            path_layout.addWidget(self.browse_btn)
            layout.addLayout(path_layout)
            self.save_btn = QPushButton("Save and Close")
            self.save_btn.clicked.connect(self.save_and_close)
            layout.addWidget(self.save_btn)

        def load_settings(self):
            main_py_path = SETTINGS.value(MAIN_PY_PATH_KEY, "")
            python_exe_path = SETTINGS.value(PYTHON_EXE_PATH_KEY, "")
            self.path_edit.setText(main_py_path)
            self.python_path_edit.setText(python_exe_path)

        def browse_for_python_exe(self):
            file_path, _ = QFileDialog.getOpenFileName(self, "Find python.exe", "", "Executables (*.exe);;All Files (*)")
            if file_path: self.python_path_edit.setText(file_path)

        def browse_for_main_py(self):
            file_path, _ = QFileDialog.getOpenFileName(self, "Find main.py", "", "Python Files (*.py);;All Files (*)")
            if file_path: self.path_edit.setText(file_path)

        def save_and_close(self):
            python_path = self.python_path_edit.text()
            main_path = self.path_edit.text()
            if not python_path or not os.path.exists(python_path):
                QMessageBox.warning(self, "Invalid Path", "The Python executable file does not exist.")
                return
            if not os.path.basename(python_path).lower() == 'python.exe':
                 QMessageBox.warning(self, "Incorrect File", "The selected file does not appear to be 'python.exe'.")
                 return
            if not main_path or not os.path.exists(main_path):
                QMessageBox.warning(self, "Invalid Path", "The main.py file does not exist.")
                return
            if not os.path.basename(main_path) == 'main.py':
                 QMessageBox.warning(self, "Incorrect File", "The selected file does not appear to be 'main.py'.")
                 return
            SETTINGS.setValue(PYTHON_EXE_PATH_KEY, python_path)
            SETTINGS.setValue(MAIN_PY_PATH_KEY, main_path)
            QMessageBox.information(self, "Success", "Paths saved successfully!")
            self.close()

    # This is the standard boilerplate to run a PyQt5 app
    app = QApplication(sys.argv)
    config_window = LauncherConfigDialog()
    config_window.show()
    sys.exit(app.exec_())


if __name__ == "__main__":
    # This is now the main dispatcher. It decides which path to take.
    if len(sys.argv) > 1:
        # A file path was passed. Use the fast, non-GUI launcher.
        file_path = sys.argv[1].strip('"')

        if file_path.lower().endswith(".mmtl"):
            launch_main_app_fast(file_path)
            # No sys.exit() needed here, the process ends after the function returns.
        else:
            show_native_error("Invalid File", "This launcher only opens .mmtl files.")
            sys.exit(1)
    else:
        # No arguments were passed. Show the configuration GUI.
        show_configuration_dialog()