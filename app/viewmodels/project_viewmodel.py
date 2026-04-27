# app/viewmodels/project_viewmodel.py

import os
import shutil
from PySide6.QtCore import Signal
from app.viewmodels.base_viewmodel import BaseViewModel


class ProjectViewModel(BaseViewModel):
    """
    ViewModel for project-level I/O (load, save, close).
    Owns project lifecycle state and emits signals for the View to react to.
    """

    # --- Property change signals ---
    is_project_loaded_changed = Signal(bool)
    project_name_changed = Signal(str)

    # --- Lifecycle signals (forwarded / derived from model) ---
    project_loaded = Signal()
    project_load_failed = Signal(str)
    project_saved = Signal(str)     # result message
    project_closed = Signal()

    def __init__(self, model, parent=None):
        super().__init__(parent)
        self._model = model
        self._is_project_loaded = False
        self._project_name = ""

        # Forward model signals
        self._model.project_loaded.connect(self._on_model_project_loaded)
        self._model.project_load_failed.connect(self._on_model_project_load_failed)

    # ------------------------------------------------------------------
    # Observable properties
    # ------------------------------------------------------------------
    @property
    def is_project_loaded(self) -> bool:
        return self._is_project_loaded

    @is_project_loaded.setter
    def is_project_loaded(self, value: bool):
        if self._is_project_loaded != value:
            self._is_project_loaded = value
            self.is_project_loaded_changed.emit(value)

    @property
    def project_name(self) -> str:
        return self._project_name

    @project_name.setter
    def project_name(self, value: str):
        if self._project_name != value:
            self._project_name = value
            self.project_name_changed.emit(value)

    # ------------------------------------------------------------------
    # Internal model signal handlers
    # ------------------------------------------------------------------
    def _on_model_project_loaded(self):
        self.is_project_loaded = True
        self.project_name = self._model.project_name
        self.project_loaded.emit()

    def _on_model_project_load_failed(self, error_msg):
        self.is_project_loaded = False
        self.project_name = ""
        self.project_load_failed.emit(error_msg)

    # ------------------------------------------------------------------
    # Public commands (called by Views / AppViewModel)
    # ------------------------------------------------------------------
    def load_project(self, mmtl_path: str, temp_dir: str):
        """Delegates to the model. Signals are forwarded via _on_model_project_loaded."""
        self._model.load_project(mmtl_path, temp_dir)

    def save_project(self):
        """Saves via model and emits the result message."""
        result_message = self._model.save_project()
        self.project_saved.emit(result_message)

    def save_project_as(self, file_path: str):
        """Updates the model path and saves."""
        self._model.set_mmtl_path(file_path)
        self.save_project()

    def close_project(self):
        """
        Cleans up the temporary directory, resets model state,
        and resets VM properties.
        """
        temp_dir = getattr(self._model, 'temp_dir', None)
        if temp_dir and os.path.exists(temp_dir):
            try:
                print(f"Cleaning up temporary directory: {temp_dir}")
                shutil.rmtree(temp_dir)
            except Exception as e:
                print(f"Warning: Could not remove temporary directory {temp_dir}: {e}")

        self._model._initialize_state()
        self.is_project_loaded = False
        self.project_name = ""
        self.project_closed.emit()
