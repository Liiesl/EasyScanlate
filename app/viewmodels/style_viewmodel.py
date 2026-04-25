# app/viewmodels/style_viewmodel.py

from PySide6.QtCore import Signal
from app.viewmodels.base_viewmodel import BaseViewModel
from assets import DEFAULT_TEXT_STYLE, get_style_diff
from PySide6.QtGui import QColor


class StyleViewModel(BaseViewModel):
    """
    Manages text box style state and applies style diffs to the model.
    """

    current_style_changed = Signal(dict)

    def __init__(self, model, editor_vm, parent=None):
        super().__init__(parent)
        self._model = model
        self._editor_vm = editor_vm
        self._current_style = self._build_default_style_dict()

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------
    @property
    def current_style(self):
        return self._current_style

    @current_style.setter
    def current_style(self, value):
        if self._current_style != value:
            self._current_style = value
            self.current_style_changed.emit(value)

    # ------------------------------------------------------------------
    # Commands
    # ------------------------------------------------------------------
    def apply_style(self, full_style_dict):
        """
        Computes a diff against DEFAULT_TEXT_STYLE and writes it to the model
        for the currently selected row.
        """
        row_number = self._editor_vm.selected_row
        if row_number is None:
            print("StyleViewModel: No row selected, ignoring style apply.")
            return

        style_diff = get_style_diff(full_style_dict, DEFAULT_TEXT_STYLE)
        # Convert QColor values to strings for JSON serialization
        style_diff = self._serialize_colors(style_diff)

        self._model.update_style(row_number, style_diff)

    def load_preset(self, preset_diff):
        """
        Merges a preset diff with the default style and updates current_style.
        The panel should listen to current_style_changed to refresh UI.
        """
        full_style = self._build_default_style_dict()
        for key, value in preset_diff.items():
            if isinstance(value, dict) and key in full_style:
                full_style[key].update(value)
            else:
                full_style[key] = value
        self.current_style = full_style

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------
    @staticmethod
    def _build_default_style_dict():
        style = {}
        for k, v in DEFAULT_TEXT_STYLE.items():
            if k in ('bg_color', 'border_color', 'text_color'):
                style[k] = QColor(v)
            else:
                style[k] = v
        return style

    @staticmethod
    def _serialize_colors(style_diff):
        """Converts QColor values in a diff dict to hex strings."""
        serialized = {}
        for k, v in style_diff.items():
            if isinstance(v, QColor):
                serialized[k] = v.name(QColor.HexArgb)
            elif isinstance(v, dict):
                serialized[k] = StyleViewModel._serialize_colors(v)
            else:
                serialized[k] = v
        return serialized
