# app/viewmodels/editor_viewmodel.py

from PySide6.QtCore import Signal
from app.viewmodels.base_viewmodel import BaseViewModel
from app.handlers.selection_manager import SelectionManager
from assets import DEFAULT_TEXT_STYLE
from PySide6.QtGui import QColor

class EditorViewModel(BaseViewModel):
    """
    Manages editor-level state: current text selection, row operations,
    and style data for the currently selected row.
    """

    # --- Signals ---
    selected_row_changed = Signal(object)  # int or None
    selected_row_style_changed = Signal(dict)
    can_delete_changed = Signal(bool)
    can_combine_changed = Signal(bool)

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self._model = model
        self._selection_manager = SelectionManager(model, self)
        self._selection_manager.selection_changed.connect(self._on_selection_changed)

        self._selected_row = None
        self._can_delete = False
        self._can_combine = False

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------
    @property
    def selected_row(self):
        return self._selected_row

    @selected_row.setter
    def selected_row(self, value):
        if self._selected_row != value:
            self._selected_row = value
            self.selected_row_changed.emit(value)
            self._update_capabilities()
            self._update_style_for_selected_row()

    @property
    def can_delete(self):
        return self._can_delete

    @can_delete.setter
    def can_delete(self, value):
        if self._can_delete != value:
            self._can_delete = value
            self.can_delete_changed.emit(value)

    @property
    def can_combine(self):
        return self._can_combine

    @can_combine.setter
    def can_combine(self, value):
        if self._can_combine != value:
            self._can_combine = value
            self.can_combine_changed.emit(value)

    # ------------------------------------------------------------------
    # Selection
    # ------------------------------------------------------------------
    def select_row(self, row_number):
        """Called by Views when a text box or card is selected."""
        self._selection_manager.select(row_number, self)

    def deselect(self):
        """Called by Views to clear the current selection."""
        self._selection_manager.deselect(self)

    def maybe_deselect(self, row_number):
        """Called by Views when a specific row is deselected locally.
        Only clears the global selection if this row is still the active one."""
        if self._selected_row == row_number:
            self.deselect()

    def _on_selection_changed(self, row_number, source):
        """Relay from SelectionManager."""
        self.selected_row = row_number

    # ------------------------------------------------------------------
    # Row Operations
    # ------------------------------------------------------------------
    def delete_row(self, row_number_to_delete):
        """Delegates deletion to the model."""
        if self._selected_row == row_number_to_delete:
            self.deselect()
        self._model.delete_row(row_number_to_delete)

    def delete_selected_row(self):
        if self._selected_row is not None:
            self.delete_row(self._selected_row)

    def combine_selected_row(self):
        """Stub – will be wired once a UI trigger exists."""
        pass

    def combine_rows(self, first_row_number, combined_text, min_confidence, rows_to_delete):
        """Delegates combine operation to the model and returns the result."""
        message, success = self._model.combine_rows(
            first_row_number, combined_text, min_confidence, rows_to_delete
        )
        return success, message

    # ------------------------------------------------------------------
    # Style helpers
    # ------------------------------------------------------------------
    def _update_capabilities(self):
        """Recompute can_delete / can_combine whenever selection changes."""
        has_selection = self._selected_row is not None
        self.can_delete = has_selection
        self.can_combine = has_selection  # TODO: refine logic when UI exists

    def _update_style_for_selected_row(self):
        if self._selected_row is not None:
            style = self._get_style_for_row(self._selected_row)
            self.selected_row_style_changed.emit(style)
        else:
            self.selected_row_style_changed.emit(self._build_default_style_dict())

    def _get_style_for_row(self, row_number):
        style = self._build_default_style_dict()
        target_result, _ = self._model._find_result_by_row_number(row_number)
        if target_result:
            custom_style = target_result.get('custom_style', {})
            for k, v in custom_style.items():
                if k in ('bg_color', 'border_color', 'text_color'):
                    style[k] = QColor(v)
                else:
                    style[k] = v
        return style

    @staticmethod
    def _build_default_style_dict():
        style = {}
        for k, v in DEFAULT_TEXT_STYLE.items():
            if k in ('bg_color', 'border_color', 'text_color'):
                style[k] = QColor(v)
            else:
                style[k] = v
        return style
