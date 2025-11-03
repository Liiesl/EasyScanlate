# error_dialog.py
# Custom error dialog with traceback display and GitHub reporting

import traceback
import urllib.parse
import sys
import os
import platform
import subprocess
from PySide6.QtWidgets import (QDialog, QVBoxLayout, QHBoxLayout, QPushButton, 
                             QTextEdit, QLabel, QApplication)
from PySide6.QtGui import QFont, QDesktopServices
from PySide6.QtCore import Qt, QUrl

# GitHub repository information
GITHUB_REPO_URL = "https://github.com/Liiesl/EasyScanlate"
GITHUB_ISSUES_URL = "https://github.com/Liiesl/EasyScanlate/issues/new"


class ErrorDialog(QDialog):
    """
    Custom error dialog with traceback display, copy functionality, and GitHub issue reporting.
    Provides static methods matching QMessageBox API: critical(), warning(), information()
    """
    
    def __init__(self, parent=None, error_message="", traceback_text=None, icon_type="critical"):
        super().__init__(parent)
        self.icon_type = icon_type
        self.error_message = error_message
        self.traceback_text = traceback_text or ""
        
        # If no traceback provided, try to capture current exception context
        if not self.traceback_text:
            try:
                self.traceback_text = traceback.format_exc()
            except:
                self.traceback_text = ""
        
        self.setWindowTitle("Error" if icon_type == "critical" else 
                          "Warning" if icon_type == "warning" else "Information")
        self.setMinimumSize(600, 400)
        self.resize(700, 500)
        
        # Set modal behavior like QMessageBox
        self.setModal(True)
        
        self._setup_ui()
        self._apply_styling()
    
    def _setup_ui(self):
        """Set up the dialog UI components"""
        main_layout = QVBoxLayout(self)
        main_layout.setSpacing(12)
        main_layout.setContentsMargins(16, 16, 16, 16)
        
        # Error message label (top)
        self.message_label = QLabel(self.error_message)
        self.message_label.setWordWrap(True)
        self.message_label.setStyleSheet("""
            QLabel {
                font-size: 14px;
                font-weight: 500;
                padding: 12px;
                background-color: #2d2d2d;
                border-radius: 4px;
                color: #e8eaed;
            }
        """)
        main_layout.addWidget(self.message_label)
        
        # Traceback text area (scrollable, monospace)
        traceback_label = QLabel("Traceback:")
        traceback_label.setStyleSheet("font-size: 12px; color: #aaaaaa; font-weight: 500;")
        main_layout.addWidget(traceback_label)
        
        self.traceback_edit = QTextEdit()
        self.traceback_edit.setReadOnly(True)
        self.traceback_edit.setFont(QFont("Consolas", 10) if hasattr(QFont, "Consolas") else QFont("Courier", 10))
        self.traceback_edit.setStyleSheet("""
            QTextEdit {
                background-color: #1e1e1e;
                border: 1px solid #3d3d3d;
                border-radius: 4px;
                color: #d4d4d4;
                padding: 8px;
                font-family: 'Consolas', 'Courier New', monospace;
            }
        """)
        
        # Populate traceback text
        if self.traceback_text:
            formatted_text = f"{self.error_message}\n\n{'='*60}\n\n{self.traceback_text}"
            self.traceback_edit.setPlainText(formatted_text)
        else:
            self.traceback_edit.setPlainText(self.error_message)
        
        main_layout.addWidget(self.traceback_edit, stretch=1)
        
        # Button layout
        button_layout = QHBoxLayout()
        button_layout.setSpacing(8)
        
        # Copy Traceback button
        self.copy_button = QPushButton("Copy Traceback")
        self.copy_button.setMinimumWidth(120)
        self.copy_button.clicked.connect(self._copy_to_clipboard)
        button_layout.addWidget(self.copy_button)
        
        # Report Issue button
        self.report_button = QPushButton("Report Issue to GitHub")
        self.report_button.setMinimumWidth(150)
        self.report_button.clicked.connect(self._report_to_github)
        button_layout.addWidget(self.report_button)
        
        button_layout.addStretch()
        
        # Close button (default)
        self.close_button = QPushButton("Close")
        self.close_button.setDefault(True)
        self.close_button.setMinimumWidth(100)
        self.close_button.clicked.connect(self.accept)
        button_layout.addWidget(self.close_button)
        
        main_layout.addLayout(button_layout)
    
    def _apply_styling(self):
        """Apply dialog styling based on icon type"""
        # Determine icon color based on type
        if self.icon_type == "critical":
            icon_color = "#d32f2f"  # Red
            border_color = "#d32f2f"
        elif self.icon_type == "warning":
            icon_color = "#f57c00"  # Orange
            border_color = "#f57c00"
        else:  # information
            icon_color = "#1976d2"  # Blue
            border_color = "#1976d2"
        
        # Apply dialog background and border
        self.setStyleSheet(f"""
            QDialog {{
                background-color: #2d2d2d;
                border: 2px solid {border_color};
                border-radius: 8px;
            }}
            QPushButton {{
                background-color: #3d3d3d;
                border: 1px solid #5d5d5d;
                border-radius: 4px;
                padding: 8px 16px;
                color: #e8eaed;
                font-size: 13px;
                min-height: 32px;
            }}
            QPushButton:hover {{
                background-color: #4d4d4d;
                border-color: #6d6d6d;
            }}
            QPushButton:pressed {{
                background-color: #2d2d2d;
            }}
            QPushButton:default {{
                border: 2px solid {border_color};
                background-color: #3d3d3d;
            }}
        """)
    
    def _copy_to_clipboard(self):
        """Copy error message and traceback to clipboard"""
        clipboard = QApplication.clipboard()
        text_to_copy = self.traceback_edit.toPlainText()
        clipboard.setText(text_to_copy)
        # Show brief feedback (you could add a status label here if desired)
        self.copy_button.setText("Copied!")
        QApplication.processEvents()
        # Reset button text after a short delay
        from PySide6.QtCore import QTimer
        QTimer.singleShot(2000, lambda: self.copy_button.setText("Copy Traceback"))
    
    def _report_to_github(self):
        """Open GitHub issues page with pre-filled error details"""
        # Extract exception type and message from traceback or error message
        exc_type = "Error"
        exc_message = self.error_message
        
        # Try to parse exception type from traceback
        if self.traceback_text:
            lines = self.traceback_text.strip().split('\n')
            for line in reversed(lines):
                if ':' in line and any(x in line for x in ['Error', 'Exception', 'Warning']):
                    parts = line.split(':', 1)
                    if len(parts) == 2:
                        exc_type = parts[0].strip()
                        exc_message = parts[1].strip()
                        break
        
        # Format issue title (truncate if too long)
        issue_title = f"{exc_type}: {exc_message}"
        if len(issue_title) > 100:
            issue_title = issue_title[:97] + "..."
        
        # Collect system and environment information
        system_info = self._collect_system_info()
        
        # Format issue body
        issue_body = f"""**Error Details:**

{self.error_message}

**Full Traceback:**

```
{self.traceback_text if self.traceback_text else 'No traceback available'}
```

**System Information:**

{system_info}

**Additional Information:**
- Application: EasyScanlate
- Error Type: {self.icon_type}

---

**Please describe what you were doing when this error occurred:**

[Please replace this line with a description of what actions you were performing, what features you were using, or what operation you were trying to complete when the error happened. This information helps us understand the context and reproduce the issue.]

**Steps to Reproduce (if applicable):**
1. 
2. 
3. 

**Expected Behavior:**
[What you expected to happen]

**Actual Behavior:**
[What actually happened]
"""
        
        # Create GitHub issue URL with pre-filled data
        params = {
            'title': issue_title,
            'body': issue_body
        }
        url = f"{GITHUB_ISSUES_URL}?{urllib.parse.urlencode(params)}"
        
        # Open URL in default browser
        QDesktopServices.openUrl(QUrl(url))
    
    def _collect_system_info(self):
        """Collect system and environment information for debugging"""
        info_lines = []
        
        # Operating System
        try:
            os_name = platform.system()
            os_version = platform.version()
            os_release = platform.release()
            info_lines.append(f"- **OS:** {os_name} {os_release} ({os_version})")
        except Exception:
            info_lines.append("- **OS:** Unknown")
        
        # Architecture
        try:
            machine = platform.machine()
            arch = platform.architecture()[0]
            info_lines.append(f"- **Architecture:** {machine} ({arch})")
        except Exception:
            pass
        
        # Application Version
        try:
            from app.utils.update import get_app_version
            app_version = get_app_version()
            info_lines.append(f"- **App Version:** {app_version}")
        except Exception:
            info_lines.append("- **App Version:** Unknown")
        
        # Running Mode (Script vs Compiled)
        try:
            is_frozen = getattr(sys, 'frozen', False)
            # Check for Nuitka using the same method as main.py
            # In main.py: IS_RUNNING_AS_SCRIPT = "__nuitka_version__" not in locals()
            # So if __nuitka_version__ exists in globals, it's compiled with Nuitka
            is_nuitka = "__nuitka_version__" in globals()
            
            if is_frozen or is_nuitka:
                running_mode = "Compiled Executable"
            else:
                running_mode = "Python Script"
            info_lines.append(f"- **Running Mode:** {running_mode}")
        except Exception:
            pass
        
        # Python Version
        try:
            python_version = sys.version.split()[0]  # Get version without build info
            python_impl = platform.python_implementation()
            info_lines.append(f"- **Python:** {python_impl} {python_version}")
        except Exception:
            pass
        
        # PySide6 Version
        try:
            import PySide6
            pyside6_version = PySide6.__version__
            info_lines.append(f"- **PySide6:** {pyside6_version}")
        except Exception:
            info_lines.append("- **PySide6:** Not available")
        
        # Git Information (only when running as script)
        git_info = self._get_git_info()
        if git_info:
            info_lines.extend(git_info)
        
        return "\n".join(info_lines)
    
    def _get_git_info(self):
        """Get Git branch and commit status information when running as a script"""
        info_lines = []
        
        # Only get git info if running as a script
        try:
            is_frozen = getattr(sys, 'frozen', False)
            is_nuitka = "__nuitka_version__" in globals()
            if is_frozen or is_nuitka:
                return []  # Not running as script, skip git info
        except Exception:
            return []
        
        # Find the git repository root by walking up from the main.py location
        try:
            # Get the base path of the application (where main.py would be)
            base_path = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
            if not os.path.exists(base_path):
                return []
            
            # Walk up to find .git directory
            git_root = None
            current_path = base_path
            while current_path != os.path.dirname(current_path):  # Stop at filesystem root
                git_dir = os.path.join(current_path, ".git")
                if os.path.exists(git_dir):
                    git_root = current_path
                    break
                current_path = os.path.dirname(current_path)
            
            if not git_root:
                return []  # Not a git repository
            
            # Get current branch name
            try:
                result = subprocess.run(
                    ["git", "rev-parse", "--abbrev-ref", "HEAD"],
                    cwd=git_root,
                    capture_output=True,
                    text=True,
                    timeout=2,
                    check=False
                )
                if result.returncode == 0:
                    branch = result.stdout.strip()
                    if branch:
                        info_lines.append(f"- **Git Branch:** {branch}")
            except (subprocess.TimeoutExpired, subprocess.SubprocessError, FileNotFoundError):
                pass  # Git not available or command failed
            
            # Check if on latest commit from remote
            try:
                # Fetch latest info from remote (non-blocking, quick fetch)
                subprocess.run(
                    ["git", "fetch", "--quiet"],
                    cwd=git_root,
                    capture_output=True,
                    timeout=5,
                    check=False
                )
                
                # Get current commit hash
                result = subprocess.run(
                    ["git", "rev-parse", "HEAD"],
                    cwd=git_root,
                    capture_output=True,
                    text=True,
                    timeout=2,
                    check=False
                )
                if result.returncode != 0:
                    return info_lines
                
                current_commit = result.stdout.strip()
                if not current_commit:
                    return info_lines
                
                # Get remote branch name (try common names)
                remote_branch = None
                for remote_name in ["origin/main", "origin/master", "origin/develop"]:
                    result = subprocess.run(
                        ["git", "rev-parse", remote_name],
                        cwd=git_root,
                        capture_output=True,
                        text=True,
                        timeout=2,
                        check=False
                    )
                    if result.returncode == 0:
                        remote_branch = remote_name
                        break
                
                if remote_branch:
                    result = subprocess.run(
                        ["git", "rev-parse", remote_branch],
                        cwd=git_root,
                        capture_output=True,
                        text=True,
                        timeout=2,
                        check=False
                    )
                    if result.returncode == 0:
                        remote_commit = result.stdout.strip()
                        if current_commit == remote_commit:
                            info_lines.append(f"- **Git Status:** On latest commit from remote")
                        else:
                            # Check if ahead or behind
                            result = subprocess.run(
                                ["git", "rev-list", "--left-right", "--count", f"{remote_branch}...HEAD"],
                                cwd=git_root,
                                capture_output=True,
                                text=True,
                                timeout=2,
                                check=False
                            )
                            if result.returncode == 0:
                                counts = result.stdout.strip().split()
                                if len(counts) == 2:
                                    behind, ahead = int(counts[0]), int(counts[1])
                                    if behind > 0 and ahead > 0:
                                        info_lines.append(f"- **Git Status:** {behind} commits behind, {ahead} commits ahead of remote")
                                    elif behind > 0:
                                        info_lines.append(f"- **Git Status:** {behind} commit(s) behind remote")
                                    elif ahead > 0:
                                        info_lines.append(f"- **Git Status:** {ahead} commit(s) ahead of remote")
                            else:
                                info_lines.append(f"- **Git Status:** Not on latest commit from remote")
            except (subprocess.TimeoutExpired, subprocess.SubprocessError, FileNotFoundError):
                pass  # Git command failed or timed out
            
        except Exception:
            pass  # Silently fail if git operations don't work
        
        return info_lines
    
    @staticmethod
    def critical(parent, title, message, traceback_text=None):
        """
        Static method matching QMessageBox.critical() API.
        Shows a critical error dialog.
        
        Args:
            parent: Parent widget (can be None)
            title: Dialog title
            message: Error message to display
            traceback_text: Optional traceback text to display
            
        Returns:
            QDialog.DialogCode result
        """
        dialog = ErrorDialog(parent, message, traceback_text, "critical")
        dialog.setWindowTitle(title)
        return dialog.exec()
    
    @staticmethod
    def warning(parent, title, message, traceback_text=None):
        """
        Static method matching QMessageBox.warning() API.
        Shows a warning dialog.
        
        Args:
            parent: Parent widget (can be None)
            title: Dialog title
            message: Warning message to display
            traceback_text: Optional traceback text to display
            
        Returns:
            QDialog.DialogCode result
        """
        dialog = ErrorDialog(parent, message, traceback_text, "warning")
        dialog.setWindowTitle(title)
        return dialog.exec()
    
    @staticmethod
    def information(parent, title, message, traceback_text=None):
        """
        Static method matching QMessageBox.information() API.
        Shows an information dialog.
        
        Args:
            parent: Parent widget (can be None)
            title: Dialog title
            message: Information message to display
            traceback_text: Optional traceback text to display
            
        Returns:
            QDialog.DialogCode result
        """
        dialog = ErrorDialog(parent, message, traceback_text, "information")
        dialog.setWindowTitle(title)
        return dialog.exec()

