# app/viewmodels/base_viewmodel.py

from PySide6.QtCore import QObject, Signal

class BaseViewModel(QObject):
    """Base class for all ViewModels. Provides observable property helpers."""

    def __init__(self, parent=None):
        super().__init__(parent)

    def _notify(self, signal: Signal, old_value, new_value):
        if old_value != new_value:
            signal.emit(new_value)
