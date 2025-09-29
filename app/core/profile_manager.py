# app/core/profile_manager.py

from PySide6.QtCore import QObject, Signal
import copy

class ProfileManager(QObject):
    """
    Manages text profiles (Original, User Edits, Translations) for a project.
    This class is the single source of truth for which text to display and where
    to save edits.
    """
    profiles_updated = Signal()
    active_profile_changed = Signal()
    edit_profile_created = Signal(str) # Emits the name of the new profile

    def __init__(self, model):
        super().__init__()
        self.model = model # The main ProjectModel instance
        self.profiles = {}
        self.active_profile_name = "Original"

    def load_from_results(self, ocr_results):
        """
        Initializes profiles from a list of OCR result dictionaries.
        It parses the 'translations' dictionary within each result.
        """
        self.profiles = {"Original": {}}
        self.active_profile_name = "Original"

        for result in ocr_results:
            filename = result.get('filename')
            row_number = str(result.get('row_number'))
            if not filename or not row_number: continue

            if filename not in self.profiles["Original"]:
                self.profiles["Original"][filename] = {}
            self.profiles["Original"][filename][row_number] = result.get('text', '')
            
            # Populate other profiles from the 'translations' dictionary
            for profile_name, text in result.get('translations', {}).items():
                if profile_name not in self.profiles:
                    self.profiles[profile_name] = {}
                if filename not in self.profiles[profile_name]:
                    self.profiles[profile_name][filename] = {}
                self.profiles[profile_name][filename][row_number] = text

        self.profiles_updated.emit()
        print(f"ProfileManager loaded profiles: {list(self.profiles.keys())}")

    def add_profile(self, profile_name, data):
        """
        Adds a new profile, typically from a translation or import.
        `data` is a dict like: { "filename1": { "row1": "text", "row2": "text"}, ... }
        """
        base_name = profile_name
        counter = 1
        while profile_name in self.profiles:
            profile_name = f"{base_name} ({counter})"
            counter += 1

        self.profiles[profile_name] = data
        self.profiles_updated.emit()
        print(f"Added new profile: {profile_name}")

    def switch_active_profile(self, profile_name):
        """Switches the currently active profile."""
        if profile_name in self.profiles and profile_name != self.active_profile_name:
            self.active_profile_name = profile_name
            self.active_profile_changed.emit()
            print(f"Active profile switched to: {profile_name}")

    def get_active_profile_name(self):
        """Returns the name of the active profile."""
        return self.active_profile_name

    def get_all_profile_names(self):
        """Returns a list of all available profile names."""
        return list(self.profiles.keys())

    def get_display_text(self, result):
        """
        Gets the text for a given result dictionary based on the active profile.
        Falls back to Original text if the active profile has no entry for this result.
        """
        filename = result.get('filename')
        row_number = str(result.get('row_number'))

        profile_data = self.profiles.get(self.active_profile_name, {})
        text = profile_data.get(filename, {}).get(row_number)
        
        if text is not None:
            return text
        
        # Fallback to Original if text is missing in the active profile
        return self.profiles.get("Original", {}).get(filename, {}).get(row_number, "")

    def _ensure_user_edit_profile_exists(self):
        """
        If the current profile is "Original", this creates a new "User Edit" profile,
        copies the "Original" data to it, switches to it, and returns its name.
        If the current profile is not "Original", it does nothing and returns the current profile name.
        """
        if self.active_profile_name != "Original":
            return self.active_profile_name

        # Find a unique name for the new edit profile
        i = 1
        while f"User Edit {i}" in self.profiles:
            i += 1
        new_profile_name = f"User Edit {i}"

        # Deep copy of the Original profile data
        self.profiles[new_profile_name] = copy.deepcopy(self.profiles["Original"])
        
        self.switch_active_profile(new_profile_name)
        self.profiles_updated.emit()
        self.edit_profile_created.emit(new_profile_name)

        return new_profile_name

    def update_text(self, row_number, new_text):
        """Updates the text for a given row in the correct profile."""
        target_profile_name = self._ensure_user_edit_profile_exists()
        
        result_to_update, _ = self.model._find_result_by_row_number(row_number)
        if not result_to_update:
            return

        filename = result_to_update.get('filename')
        row_str = str(result_to_update.get('row_number'))
        
        if filename in self.profiles[target_profile_name]:
            self.profiles[target_profile_name][filename][row_str] = new_text
        else:
            print(f"Warning: Filename {filename} not found in profile {target_profile_name} for update.")

    def combine_rows(self, first_row_number, combined_text):
        """
        Handles the text profile update part of combining rows.
        The main model is responsible for deleting the other rows.
        """
        target_profile_name = self._ensure_user_edit_profile_exists()
        
        result_to_update, _ = self.model._find_result_by_row_number(first_row_number)
        if not result_to_update:
            return

        filename = result_to_update.get('filename')
        row_str = str(result_to_update.get('row_number'))

        if filename in self.profiles[target_profile_name]:
            self.profiles[target_profile_name][filename][row_str] = combined_text
        else:
             print(f"Warning: Filename {filename} not found in profile {target_profile_name} for combine.")

    def get_translations_for_save(self):
        """
        Reconstructs the 'translations' dictionary for each ocr_result
        based on the current state of all profiles. This is used by the
        ProjectModel when saving the project file.
        """
        translations_by_key = {} # (filename, row_number): { "profile1": "text1", ...}

        for profile_name, profile_data in self.profiles.items():
            if profile_name == "Original":
                continue
            for filename, rows in profile_data.items():
                for row_number, text in rows.items():
                    key = (filename, str(row_number))
                    if key not in translations_by_key:
                        translations_by_key[key] = {}
                    translations_by_key[key][profile_name] = text
        
        return translations_by_key