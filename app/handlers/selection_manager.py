# app/ui/selection_manager.py

from PySide6.QtCore import QObject, Signal

class SelectionManager(QObject):
    """
    Manages the currently selected OCR result across different widgets.
    Acts as a single source of truth for the selection state.
    """
    # Signal: selection_changed(row_number, source_widget)
    # Emits the row_number of the new selection, or None if deselected.
    # The source_widget is the widget that initiated the selection change.
    selection_changed = Signal(object, object)

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self.model = model
        self._current_row = None

    def select(self, row_number, source):
        """
        Selects a row and notifies listeners.

        Args:
            row_number: The unique identifier for the OCR result row.
            source: The widget instance initiating the selection.
        """
        if self._current_row == row_number:
            return
        
        self._current_row = row_number
        self.selection_changed.emit(self._current_row, source)

    def deselect(self, source):
        """
        Clears the current selection and notifies listeners.

        Args:
            source: The widget instance initiating the deselection.
        """
        if self._current_row is None:
            return
            
        self._current_row = None
        self.selection_changed.emit(None, source)

    def get_current_selection(self):
        """Returns the currently selected row number, or None."""
        return self._current_row