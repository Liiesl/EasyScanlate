# app/viewmodels/translation_viewmodel.py

from PySide6.QtCore import Signal
from app.viewmodels.base_viewmodel import BaseViewModel
from app.core.translations import (
    TranslationThread,
    generate_for_translate_content,
    import_translation_file_content,
)


class TranslationViewModel(BaseViewModel):
    """
    Manages translation panel state: profiles, text editing, and translation orchestration.

    TODO(Phase 3 dirty flag): The 'is_dirty' observable is deferred to a follow-up.
    Currently text is written to ProjectModel immediately on every keystroke.
    Add is_dirty tracking when we implement batched/committed edits.
    """

    # -- Profile / results signals --
    profiles_changed = Signal(list)                # list[str]
    active_profile_changed = Signal(str)           # profile name
    ocr_results_changed = Signal(list)             # list[dict]
    row_text_updated = Signal(int, str)            # row_number, actual_saved_text
    current_text_changed = Signal(str)             # text for selected row

    # -- Translation thread signals --
    is_translating_changed = Signal(bool)
    translation_error_occurred = Signal(str, str)  # title, message
    translation_complete_message = Signal(str)     # success message for dialog

    # -- Notification signals (consumed by MainWindow for dialogs) --
    profile_created_for_user_edit = Signal()

    def __init__(self, model, editor_viewmodel, get_settings=None, app_viewmodel=None, parent=None):
        super().__init__(parent)
        self._model = model
        self._editor_vm = editor_viewmodel
        self._get_settings = get_settings
        self._app_vm = app_viewmodel

        self._profiles = []
        self._active_profile = ""
        self._ocr_results = []
        self._previous_visible_rows = set()
        self._selected_row = None
        self._current_text = ""
        self._is_translating = False
        self._is_dirty = False  # TODO: Deferred – see class docstring.

        self._translation_thread = None
        self._pending_retranslate_row = None
        self._batch_target_lang = ""

        # Model signals
        self._model.model_updated.connect(self._on_model_updated)
        self._model.profiles_updated.connect(self._on_profiles_updated)
        self._model.project_loaded.connect(self._on_project_loaded)
        self._model.profile_created_for_user_edit.connect(self.profile_created_for_user_edit)

        # Editor VM signals
        if self._editor_vm:
            self._editor_vm.selected_row_changed.connect(self._on_selected_row_changed)

        # App VM signals
        if self._app_vm:
            self._app_vm.profile_switched.connect(self._on_app_profile_switched)

        self._sync_from_model()

    # ------------------------------------------------------------------
    # Properties
    # ------------------------------------------------------------------
    @property
    def profiles(self):
        return self._profiles

    @profiles.setter
    def profiles(self, value):
        if self._profiles != value:
            self._profiles = value
            self.profiles_changed.emit(value)

    @property
    def active_profile(self):
        return self._active_profile

    @active_profile.setter
    def active_profile(self, value):
        if self._active_profile != value:
            self._active_profile = value
            self.active_profile_changed.emit(value)

    @property
    def ocr_results(self):
        return self._ocr_results

    @ocr_results.setter
    def ocr_results(self, value):
        # Use 'is not' because the model mutates dicts in-place; element-wise
        # equality can falsely match against our cached list.
        if self._ocr_results is not value:
            self._ocr_results = value
            self.ocr_results_changed.emit(value)

    @property
    def current_text(self):
        return self._current_text

    @current_text.setter
    def current_text(self, value):
        if self._current_text != value:
            self._current_text = value
            self.current_text_changed.emit(value)

    @property
    def is_translating(self):
        return self._is_translating

    @is_translating.setter
    def is_translating(self, value):
        if self._is_translating != value:
            self._is_translating = value
            self.is_translating_changed.emit(value)

    @property
    def is_dirty(self):
        # TODO(Phase 3 dirty flag): Deferred. Wire this up when we switch from
        # immediate-per-keystroke writes to batched/committed edits.
        return self._is_dirty

    # ------------------------------------------------------------------
    # Internal sync
    # ------------------------------------------------------------------
    def _sync_from_model(self):
        self.profiles = list(self._model.profiles.keys())
        self.active_profile = self._model.active_profile_name
        self._rebuild_ocr_results()
        self._refresh_current_text()

    def _rebuild_ocr_results(self):
        """Emit ocr_results_changed only when the visible row set changes."""
        new_visible_rows = {
            r.get("row_number") for r in self._model.ocr_results if not r.get("is_deleted", False)
        }
        if self._previous_visible_rows != new_visible_rows:
            self._previous_visible_rows = new_visible_rows.copy()
            self.ocr_results = list(self._model.ocr_results)

    def _refresh_current_text(self):
        if self._selected_row is None:
            self.current_text = ""
            return
        result, _ = self._model._find_result_by_row_number(self._selected_row)
        if result:
            self.current_text = self._model.get_display_text(result)
        else:
            self.current_text = ""

    def _on_model_updated(self, affected_filenames):
        # Structural changes (delete, combine, new OCR) change visible row numbers;
        # text edits do not. _rebuild_ocr_results handles the distinction.
        self._rebuild_ocr_results()
        self._refresh_current_text()

    def _on_profiles_updated(self):
        self.profiles = list(self._model.profiles.keys())
        self.active_profile = self._model.active_profile_name

    def _on_project_loaded(self):
        self._sync_from_model()

    def _on_selected_row_changed(self, row_number):
        self._selected_row = row_number
        self._refresh_current_text()

    def _on_app_profile_switched(self, name):
        self.active_profile = name
        self._refresh_current_text()
        # Texts for all rows changed; notify panel to repopulate.
        self.ocr_results_changed.emit(self._ocr_results)

    # ------------------------------------------------------------------
    # Public API: display helper
    # ------------------------------------------------------------------
    def get_display_text(self, result):
        return self._model.get_display_text(result)

    # ------------------------------------------------------------------
    # Public API: Profile
    # ------------------------------------------------------------------
    def set_active_profile(self, name):
        """Called by the panel when the user switches profile via dropdown."""
        if self._app_vm:
            self._app_vm.switch_profile(name)
        elif name in self._model.profiles and name != self._model.active_profile_name:
            self._model.active_profile_name = name
            self.active_profile = name
            self._refresh_current_text()
            self.ocr_results_changed.emit(self._ocr_results)

    # ------------------------------------------------------------------
    # Public API: Text editing
    # ------------------------------------------------------------------
    def update_text(self, row_number, new_text):
        """
        Replaces MainWindow.update_ocr_text. Delegates to the model and
        syncs the actual saved text back to the panel.
        """
        old_profile = self._model.active_profile_name
        result = self._model.update_text(row_number, new_text, is_user_edit=True)

        # Parse return tuple: (error, success, profile_created, should_show_message)
        profile_created = False
        if len(result) >= 3:
            if len(result) == 4:
                _, _, _, should_show_message = result
                profile_created = should_show_message
            else:
                _, _, profile_created = result

        # Sync actual saved text back to the panel
        result_data, _ = self._model._find_result_by_row_number(row_number)
        if result_data:
            actual_text = self._model.get_display_text(result_data)
            self.row_text_updated.emit(row_number, actual_text)
        else:
            self.row_text_updated.emit(row_number, new_text)

        if profile_created:
            self.profile_created_for_user_edit.emit()
            self.profiles = list(self._model.profiles.keys())
            self.active_profile = self._model.active_profile_name
        elif self._model.active_profile_name != old_profile:
            # Routed from Original to an existing user-edit profile
            self.active_profile = self._model.active_profile_name

    # ------------------------------------------------------------------
    # Public API: Translation orchestration
    # ------------------------------------------------------------------
    def start_single_translation(self, row_number, provider, model_name, target_lang):
        settings = self._get_settings() if self._get_settings else None
        if not settings:
            self.translation_error_occurred.emit("Settings Error", "Settings not available.")
            return

        api_key = self._get_api_key(settings, provider)
        if not api_key:
            self.translation_error_occurred.emit(
                "API Key Missing", f"Please set your {provider} API key in Settings."
            )
            return

        result = None
        for r in self._model.ocr_results:
            if int(r.get("row_number", 0)) == row_number:
                result = r
                break
        if not result:
            return

        source_text = result.get("text", "")
        prompt = (
            f"Translate the following text to {target_lang}. "
            f"Respond ONLY with the translation, no explanation.\n\nText: {source_text}"
        )

        self._pending_retranslate_row = row_number
        self._start_translation_thread(api_key, prompt, model_name, provider, is_single=True)

    def start_batch_translation(self, provider, model_name, target_lang, source_language):
        settings = self._get_settings() if self._get_settings else None
        if not settings:
            self.translation_error_occurred.emit("Settings Error", "Settings not available.")
            return

        api_key = self._get_api_key(settings, provider)
        if not api_key:
            self.translation_error_occurred.emit(
                "API Key Missing", f"Please set your {provider} API key in Settings."
            )
            return

        if not self._model.ocr_results:
            self.translation_error_occurred.emit("No Data", "There are no OCR results to translate.")
            return

        user_prompt = (
            f"Translate the {source_language} text to {target_lang}, "
            f"keep everything else. Respond only with the file."
        )

        try:
            content = generate_for_translate_content(self._model.ocr_results, "Original")
            if not content.strip() or "<translations>" not in content:
                self.translation_error_occurred.emit("No Content", "There is no text content to translate.")
                return

            full_prompt = f"{user_prompt}\n\n{content}"
            self._batch_target_lang = target_lang
            self._start_translation_thread(api_key, full_prompt, model_name, provider, is_single=False)
        except Exception as e:
            self.translation_error_occurred.emit("Error", f"Failed to prepare translation: {str(e)}")

    def _get_api_key(self, settings, provider):
        if provider == "Mistral":
            return settings.value("mistral_api_key", "")
        return settings.value("gemini_api_key", "")

    def _start_translation_thread(self, api_key, prompt, model_name, provider, is_single):
        self.is_translating = True

        if self._translation_thread and self._translation_thread.isRunning():
            try:
                self._translation_thread.translation_finished.disconnect()
                self._translation_thread.translation_failed.disconnect()
            except RuntimeError:
                pass
            self._translation_thread.stop()
            self._translation_thread.wait(1000)

        self._translation_thread = TranslationThread(
            api_key, prompt, model_name, provider=provider, parent=self
        )
        self._translation_thread.translation_finished.connect(
            lambda text: self._on_translation_finished(text, provider, is_single)
        )
        self._translation_thread.translation_failed.connect(self._on_translation_failed)
        self._translation_thread.start()

    def _on_translation_finished(self, full_text, provider, is_single):
        self.is_translating = False

        try:
            if is_single and self._pending_retranslate_row is not None:
                translated_text = full_text.strip()
                if translated_text.startswith('"') and translated_text.endswith('"'):
                    translated_text = translated_text[1:-1]
                if translated_text.startswith("'") and translated_text.endswith("'"):
                    translated_text = translated_text[1:-1]

                self.update_text(self._pending_retranslate_row, translated_text)
                self._pending_retranslate_row = None
            else:
                parsed = import_translation_file_content(full_text)
                profile_name = f"{provider} Translation ({self._batch_target_lang})"
                self._model.add_profile(profile_name, parsed)
                self.translation_complete_message.emit(
                    f"Translation successfully applied to profile:\n'{profile_name}'"
                )
        except Exception as e:
            self._on_translation_failed(f"Failed to parse translation: {str(e)}")

    def _on_translation_failed(self, error_message):
        self.is_translating = False
        self.translation_error_occurred.emit("Translation Error", error_message)
        self._pending_retranslate_row = None

    def stop_translation(self):
        if self._translation_thread and self._translation_thread.isRunning():
            self._translation_thread.stop()

    def cleanup(self):
        self.stop_translation()
        if self._translation_thread:
            self._translation_thread.wait(500)
