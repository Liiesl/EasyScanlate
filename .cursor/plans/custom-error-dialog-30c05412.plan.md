<!-- 30c05412-3721-4f45-a278-9c020bcffdbd 8b8336e4-c013-476f-bab9-f2f8219f2179 -->
# Custom Error Message Box Implementation

## Overview

Create a custom error dialog widget with Qt-like static methods (`critical()`, `warning()`, `information()`) that displays error messages, full traceback information, and includes a button to report issues to GitHub. Additionally, set up a global exception handler to automatically catch and display all unhandled exceptions throughout the application.

## Files to Create/Modify

### 1. Create `app/ui/dialogs/error_dialog.py`

   - New dialog class `ErrorDialog(QDialog)` with:
     - Static methods matching QMessageBox API:
       - `ErrorDialog.critical(parent, title, message, traceback_text=None)`
       - `ErrorDialog.warning(parent, title, message, traceback_text=None)` (optional)
       - `ErrorDialog.information(parent, title, message, traceback_text=None)` (optional)
     - Error message display area (read-only QTextEdit or QLabel)
     - Scrollable traceback text area (QTextEdit, read-only, monospace font)
     - "Copy Traceback" button to copy full error details to clipboard
     - "Report Issue to GitHub" button that opens GitHub issues page with pre-filled error details
     - "Close" button (default)
     - Returns dialog result code like QMessageBox
     - Styled to match existing dialogs using PySide6 widgets

### 2. Update `app/ui/dialogs/__init__.py`

   - Add import: `from app.ui.dialogs.error_dialog import ErrorDialog`

### 3. Create `app/utils/exception_handler.py`

   - Global exception handler module with:
     - `setup_global_exception_handler()` function
     - Custom `excepthook` function that captures unhandled exceptions
     - Handles case where QApplication may not exist yet (creates temporary instance if needed)
     - Formats exceptions using `traceback.format_exception()` and displays via ErrorDialog
     - Extracts exception type, message, and full traceback

### 4. Modify `main.py`

   - Import exception handler after QApplication imports
   - Call `setup_global_exception_handler()` right after `app = QApplication(sys.argv)` (around line 526)
   - Optionally replace `on_preload_failed()` with ErrorDialog usage

### 5. Modify existing error handlers (optional - can keep QMessageBox or switch to ErrorDialog)

   - `app/ui/window/home_window.py`: Optionally replace `QMessageBox.critical()` with `ErrorDialog.critical()`
   - `app/ui/window/main_window.py`: Optionally replace `QMessageBox.critical()` with `ErrorDialog.critical()`

## Implementation Details

### ErrorDialog Class Structure

- Static methods: `critical()`, `warning()`, `information()` (matching QMessageBox API signature)
- Each static method creates and shows ErrorDialog instance
- Constructor accepts: `parent`, `error_message`, `traceback_text` (optional)
- If traceback not provided, attempts to capture current exception context using `traceback.format_exc()`
- GitHub button opens URL using `QDesktopServices.openUrl()` with issue creation URL
- Copy button uses `QApplication.clipboard()` to copy formatted error + traceback
- Dialog should be modal (blocks interaction) like QMessageBox
- Returns: `QDialog.DialogCode.Accepted` or `Rejected` (to match QMessageBox behavior)

### Error Formatting

- Display error message prominently at top
- Full traceback in scrollable, selectable text area (QTextEdit with monospace font)
- Format: Error message on first line, then separator, then traceback
- Dialog should be resizable to accommodate long tracebacks
- Minimum and preferred sizes set appropriately

### GitHub Integration

- Repository URL: `https://github.com/Liiesl/EasyScanlate` (from `app/utils/update.py`)
- Issues URL: `https://github.com/Liiesl/EasyScanlate/issues/new`
- Pre-fill issue title with: `"[Exception Type]: [Error Message]"` (truncated if too long)
- Pre-fill issue body with formatted error details and full traceback using URL parameters or GitHub issue template format

### Global Exception Handler

- The `sys.excepthook` will be set up in `main.py` immediately after QApplication creation
- Handler checks if QApplication instance exists, creates temporary one if needed (for edge cases during early initialization)
- Exception handler extracts: exception type, exception message, full traceback using `traceback.format_exception(exc_type, exc_value, exc_traceback)`
- Displays ErrorDialog with captured exception information
- For critical errors during startup, may allow application to continue or exit based on user choice
- Handler ensures ErrorDialog is shown on main thread (Qt requirement)

## Usage Pattern

### Direct usage (like QMessageBox):

```python
from app.ui.dialogs import ErrorDialog

# Simple usage (QMessageBox-like)
ErrorDialog.critical(self, "Error Title", "Something went wrong!")

# With explicit traceback
import traceback
try:
    # code
except Exception as e:
    ErrorDialog.critical(self, "Error", str(e), traceback.format_exc())
```

### Automatic usage (via global handler):

All unhandled exceptions will automatically be caught and displayed without requiring explicit try/except blocks in code. This works for:

- Unhandled exceptions in main thread
- Exceptions raised in Qt slots and signal handlers
- Any exception that reaches the top level without being caught

## Styling

- Match existing dialog styles from `app/ui/dialogs/settings_dialog.py`
- Use appropriate colors/icons to indicate error severity (critical=red, warning=yellow, info=blue)
- Ensure text is readable and traceback uses monospace font
- Dialog should be resizable to accommodate long tracebacks

### To-dos

- [ ] Create app/ui/dialogs/error_dialog.py with ErrorDialog class containing error message, traceback display, copy button, and GitHub report button
- [ ] Update app/ui/dialogs/__init__.py to export ErrorDialog
- [ ] Replace QMessageBox.critical() calls in home_window.py, main_window.py, and main.py with ErrorDialog