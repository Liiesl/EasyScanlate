# app/viewmodels/app_viewmodel.py

from PySide6.QtCore import Signal
from app.viewmodels.base_viewmodel import BaseViewModel
from app.viewmodels.editor_viewmodel import EditorViewModel
from app.viewmodels.image_area_viewmodel import ImageAreaViewModel
from app.viewmodels.translation_viewmodel import TranslationViewModel
from app.viewmodels.style_viewmodel import StyleViewModel
from app.viewmodels.batch_ocr_viewmodel import BatchOCRViewModel
from app.services.ocr_service import OCRService


class AppViewModel(BaseViewModel):
    """
    Top-level coordinator ViewModel.
    Owns child VMs and exposes signals for the MainWindow to react to.
    """

    # --- Signals forwarded to MainWindow ---
    profile_switched = Signal(str)
    project_saved = Signal(str)          # result_message
    project_save_as_requested = Signal(str)  # file_path
    ocr_toggled = Signal()
    panel_layout_toggled = Signal(bool)
    find_widget_toggled = Signal()
    import_translation_requested = Signal()
    export_ocr_results_requested = Signal()

    def __init__(self, model, get_settings=None, parent=None):
        super().__init__(parent)
        self._model = model
        self.ocr_service = OCRService(model, self)
        self.editor_vm = EditorViewModel(model, self)
        self.image_area_vm = ImageAreaViewModel(model, self.ocr_service, get_settings, self)
        self.translation_vm = TranslationViewModel(model, self.editor_vm, get_settings, app_viewmodel=self, parent=self)
        self.style_vm = StyleViewModel(model, self.editor_vm, self)
        self.batch_ocr_vm = BatchOCRViewModel(model, self.ocr_service, get_settings, self)

    # ------------------------------------------------------------------
    # Project / File actions (called by MenuBar)
    # ------------------------------------------------------------------
    def save_project(self):
        result_message = self._model.save_project()
        self.project_saved.emit(result_message)

    def save_project_as(self, file_path):
        self._model.mmtl_path = file_path
        self.save_project()

    def switch_profile(self, profile_name):
        if profile_name and profile_name in self._model.profiles and profile_name != self._model.active_profile_name:
            print(f"Switching to active profile: {profile_name}")
            self._model.active_profile_name = profile_name
            self.profile_switched.emit(profile_name)

    # ------------------------------------------------------------------
    # UI actions (called by MenuBar)
    # ------------------------------------------------------------------
    def toggle_ocr(self):
        self.ocr_toggled.emit()

    def toggle_panel_layout(self, checked):
        self.panel_layout_toggled.emit(checked)

    def toggle_find_widget(self):
        self.find_widget_toggled.emit()

    def import_translation(self):
        self.import_translation_requested.emit()

    def export_ocr_results(self):
        self.export_ocr_results_requested.emit()

    # ------------------------------------------------------------------
    # Profile queries (called by MenuBar)
    # ------------------------------------------------------------------
    def get_profiles(self):
        return list(self._model.profiles.keys())

    def get_active_profile(self):
        return self._model.active_profile_name
