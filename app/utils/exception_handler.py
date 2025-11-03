# exception_handler.py
# Global exception handler for unhandled exceptions

import sys
import traceback
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QTimer, QObject, Signal, QThread


class ExceptionHandler(QObject):
    """
    Handles ALL types of exceptions and displays them in the main thread.
    
    This handler works with the global sys.excepthook to catch and display:
    - RuntimeError, ValueError, TypeError, AttributeError, KeyError, IndexError
    - ImportError, ModuleNotFoundError
    - FileNotFoundError, PermissionError, OSError
    - Any other Exception subclass that reaches the top level unhandled
    """
    exception_occurred = Signal(str, str, str)  # error_type, error_message, traceback_text
    
    def __init__(self):
        super().__init__()
        self._app_instance = None
    
    def set_app_instance(self, app):
        """Set the QApplication instance"""
        self._app_instance = app
    
    def handle_exception(self, exc_type, exc_value, exc_traceback):
        """Handle an exception by formatting it and emitting a signal"""
        # Format the exception
        error_type = exc_type.__name__ if exc_type else "Exception"
        error_message = str(exc_value) if exc_value else "An unknown error occurred"
        
        # Format the full traceback
        traceback_lines = traceback.format_exception(exc_type, exc_value, exc_traceback)
        traceback_text = ''.join(traceback_lines)
        
        # Ensure we're on the main thread
        app = self._get_app_instance()
        if app and QThread.currentThread() == app.thread():
            # Already on main thread, show dialog directly
            self._show_error_dialog(error_type, error_message, traceback_text)
        else:
            # Need to show dialog on main thread
            self._schedule_error_dialog(error_type, error_message, traceback_text)
    
    def _get_app_instance(self):
        """Get or create QApplication instance"""
        app = QApplication.instance()
        if app is None and self._app_instance:
            app = self._app_instance
        return app
    
    def _show_error_dialog(self, error_type, error_message, traceback_text):
        """Show the error dialog (must be called on main thread)"""
        try:
            from app.ui.dialogs.error_dialog import ErrorDialog
            # Format full error message
            full_message = f"{error_type}: {error_message}"
            ErrorDialog.critical(None, "Unhandled Exception", full_message, traceback_text)
        except ImportError as import_err:
            # If ErrorDialog itself can't be imported, try basic fallback
            try:
                from PySide6.QtWidgets import QMessageBox
                app = self._get_app_instance()
                if app:
                    msg_box = QMessageBox()
                    msg_box.setIcon(QMessageBox.Critical)
                    msg_box.setWindowTitle("Unhandled Exception")
                    msg_box.setText(f"{error_type}: {error_message}")
                    msg_box.setDetailedText(traceback_text)
                    msg_box.exec()
                else:
                    # Last resort: print to console
                    print(f"Unhandled exception (no QApplication): {error_type}: {error_message}")
                    print(f"Traceback:\n{traceback_text}")
                    print(f"Also failed to import ErrorDialog: {import_err}")
            except Exception as fallback_err:
                # Complete fallback: print to console
                print(f"Failed to show error dialog: {import_err}")
                print(f"Fallback also failed: {fallback_err}")
                print(f"Original error: {error_type}: {error_message}")
                print(f"Traceback:\n{traceback_text}")
        except Exception as e:
            # Fallback if ErrorDialog import fails for other reasons
            print(f"Failed to show error dialog: {e}")
            print(f"Original error: {error_type}: {error_message}")
            print(f"Traceback:\n{traceback_text}")
    
    def _schedule_error_dialog(self, error_type, error_message, traceback_text):
        """Schedule error dialog to be shown on main thread"""
        app = self._get_app_instance()
        if app:
            # Use QTimer to post to main thread's event loop
            def show_dialog():
                self._show_error_dialog(error_type, error_message, traceback_text)
            
            QTimer.singleShot(0, show_dialog)
        else:
            # No QApplication yet, print to console as fallback
            print(f"Unhandled exception (no QApplication): {error_type}: {error_message}")
            print(f"Traceback:\n{traceback_text}")


# Global exception handler instance
_handler = ExceptionHandler()


def _custom_excepthook(exc_type, exc_value, exc_traceback):
    """
    Custom exception hook to catch unhandled exceptions.
    This replaces sys.excepthook and displays errors in a dialog.
    
    Handles ALL exception types including:
    - RuntimeError, ValueError, TypeError, AttributeError, KeyError, IndexError
    - ImportError, ModuleNotFoundError
    - FileNotFoundError, PermissionError, OSError
    - Any other Exception subclass
    
    Only exception that bypasses this handler: KeyboardInterrupt (Ctrl+C)
    """
    # Don't handle KeyboardInterrupt (allow Ctrl+C to work)
    if exc_type is KeyboardInterrupt:
        sys.__excepthook__(exc_type, exc_value, exc_traceback)
        return
    
    # Handle all other exception types (RuntimeError, ValueError, ImportError, etc.)
    try:
        _handler.handle_exception(exc_type, exc_value, exc_traceback)
    except Exception as handler_error:
        # If the handler itself fails, at least print to console
        print(f"Exception handler failed: {handler_error}")
        print(f"Original error: {exc_type.__name__}: {exc_value}")
        if exc_traceback:
            print(f"Original traceback:\n{''.join(traceback.format_exception(exc_type, exc_value, exc_traceback))}")
    
    # Also print to console as fallback
    sys.__excepthook__(exc_type, exc_value, exc_traceback)


def setup_global_exception_handler(app=None):
    """
    Set up the global exception handler for ALL exception types.
    Should be called right after QApplication is created.
    
    This handler will catch and display in a custom dialog:
    - RuntimeError, ValueError, TypeError, AttributeError, etc.
    - ImportError, ModuleNotFoundError
    - FileNotFoundError, PermissionError, OSError
    - Any unhandled exceptions from threads, signal handlers, and main code
    
    Args:
        app: QApplication instance (optional, will be retrieved if not provided)
    """
    global _handler
    
    # Get or set app instance
    if app:
        _handler.set_app_instance(app)
    else:
        app = QApplication.instance()
        if app:
            _handler.set_app_instance(app)
    
    # Set the custom excepthook to catch all unhandled exceptions
    # This will catch exceptions from:
    # - Main thread code
    # - Qt signal/slot handlers (Qt calls sys.excepthook for unhandled exceptions in slots)
    # - Thread code (if not caught in thread)
    # - Any other unhandled exception
    sys.excepthook = _custom_excepthook
    
    print("Global exception handler installed (handles all exception types: RuntimeError, ValueError, ImportError, etc.)")

