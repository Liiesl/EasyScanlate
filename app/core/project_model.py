import os, json, traceback, zipfile, math, sys
from PySide6.QtCore import QObject, Signal

class ProjectModel(QObject):
    """
    A central data management class for the Easy Scanlate project.
    It holds all project data, provides methods to manipulate it,
    and emits signals when the data changes.
    Inherits from QObject to support Qt's signal/slot mechanism.
    """
    # --- Signals ---
    # Emitted when a project is successfully loaded.
    project_loaded = Signal()
    # Emitted with an error message if project loading fails.
    project_load_failed = Signal(str)
    # Emitted after any data change (e.g., text edit, deletion, new OCR results).
    # The payload is a list of affected filenames for targeted UI updates.
    model_updated = Signal(list)
    # Emitted when the list of profiles changes (new profile added).
    profiles_updated = Signal()

    def __init__(self):
        super().__init__()
        self._initialize_state()

    def _initialize_state(self):
        """Resets all project data to its default, empty state."""
        self.mmtl_path: str = ""
        self.temp_dir: str = ""
        self.project_name: str = ""
        self.image_paths: list[str] = []
        self.ocr_results: list[dict] = []
        self.profiles: dict = {"Original": {}}
        self.original_language: str = "Korean"
        self.active_profile_name: str = "Original"
        self.next_global_row_number: int = 0

    def load_project(self, mmtl_path: str, temp_dir: str):
        """
        Loads a project from a directory, populates the model's state,
        and emits signals indicating success or failure.
        """
        try:
            self._initialize_state()
            self.mmtl_path = mmtl_path
            self.temp_dir = temp_dir
            self.project_name = os.path.splitext(os.path.basename(mmtl_path))[0]

            # 1. Load image paths
            image_dir = os.path.join(temp_dir, 'images')
            if not os.path.exists(image_dir):
                raise FileNotFoundError("The 'images' directory is missing in the project file.")
            
            self.image_paths = sorted([
                os.path.join(image_dir, f)
                for f in os.listdir(image_dir)
                if f.lower().endswith(('png', 'jpg', 'jpeg'))
            ])
            
            if not self.image_paths:
                 print("Warning: No images found in the project's images directory.")

            # 2. Load master.json (OCR results)
            master_path = os.path.join(temp_dir, 'master.json')
            if os.path.exists(master_path):
                self._load_master_json(master_path)
            
            # 3. Load meta.json (project metadata)
            meta_path = os.path.join(temp_dir, 'meta.json')
            if os.path.exists(meta_path):
                self._load_meta_json(meta_path)

            print(f"Project '{self.project_name}' loaded successfully into model.")
            self.project_loaded.emit()

        except Exception as e:
            error_msg = f"Failed to load project: {e}"
            print(error_msg)
            traceback.print_exc()
            self.project_load_failed.emit(error_msg)

    def _load_master_json(self, path: str):
        """Loads and processes the master.json file."""
        max_row_num = -1
        loaded_profiles = {"Original"}
        
        with open(path, 'r', encoding='utf-8') as f:
            loaded_data = json.load(f)

        self.ocr_results = []
        for res in loaded_data:
            if all(k in res for k in ['row_number', 'filename', 'coordinates', 'text']):
                 if 'row_number' in res:
                     max_row_num = max(max_row_num, int(float(res['row_number'])))
                 if 'translations' in res and isinstance(res['translations'], dict):
                     for profile_name in res['translations']:
                         loaded_profiles.add(profile_name)
                 self.ocr_results.append(res)
        
        self.next_global_row_number = max_row_num + 1
        self.profiles = {name: {} for name in loaded_profiles}

    def _load_meta_json(self, path: str):
        """Loads and processes the meta.json file."""
        with open(path, 'r', encoding='utf-8') as f:
            meta = json.load(f)
        
        self.original_language = meta.get('original_language', 'Korean')
        
        # If the saved active profile exists, use it. Otherwise, default to "Original".
        saved_profile = meta.get('active_profile_name', 'Original')
        if saved_profile in self.profiles:
            self.active_profile_name = saved_profile
        else:
            self.active_profile_name = "Original"
            print(f"Warning: Saved active profile '{saved_profile}' not found. Defaulting to 'Original'.")
    
    def save_project(self):
        """Saves the current project state back to its .mmtl file."""
        if not self.mmtl_path or not self.temp_dir:
            return "No project loaded or temporary directory missing. Cannot save."
        
        try:
            # Save master JSON file
            master_path = os.path.join(self.temp_dir, 'master.json')
            self._sort_ocr_results()
            with open(master_path, 'w', encoding='utf-8') as f:
                json.dump(self.ocr_results, f, indent=2, ensure_ascii=False)

            # Save metadata
            meta_path = os.path.join(self.temp_dir, 'meta.json')
            meta_data = {
                'original_language': self.original_language,
                'active_profile_name': self.active_profile_name
            }
            with open(meta_path, 'w', encoding='utf-8') as f:
                json.dump(meta_data, f, indent=2, ensure_ascii=False)

            # Create the final zip archive
            with zipfile.ZipFile(self.mmtl_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
                for root, _, files in os.walk(self.temp_dir):
                    for file in files:
                        full_path = os.path.join(root, file)
                        rel_path = os.path.relpath(full_path, self.temp_dir).replace(os.sep, '/')
                        zipf.write(full_path, rel_path)
            
            return f"Project saved successfully to\n{self.mmtl_path}"

        except Exception as e:
             print(f"Save Error: {e}")
             traceback.print_exc()
             return f"Failed to save project: {e}"

    def _find_result_by_row_number(self, row_number_to_find):
        """Internal helper to find an OCR result and its index by its row number."""
        try:
            target_rn_float = float(row_number_to_find)
        except (ValueError, TypeError):
            return None, -1
        for index, result in enumerate(self.ocr_results):
            try:
                current_rn_float = float(result.get('row_number', float('nan')))
                if not math.isnan(current_rn_float) and math.isclose(current_rn_float, target_rn_float):
                    return result, index
            except (ValueError, TypeError):
                continue
        return None, -1

    def _sort_ocr_results(self):
        """Sorts OCR results primarily by filename, then by row number."""
        try:
            def sort_key(item):
                try:
                    row_num = float(item.get('row_number', float('inf')))
                except (ValueError, TypeError):
                    row_num = float('inf')
                return (item.get('filename', ''), row_num)
            self.ocr_results.sort(key=sort_key)
        except Exception as e:
            print(f"Error during sorting OCR results: {e}. Check row_number values.")
            traceback.print_exc(file=sys.stdout)

    def get_display_text(self, result: dict) -> str:
        """Gets the text to display for a result based on the active profile."""
        if self.active_profile_name != "Original":
            edited_text = result.get('translations', {}).get(self.active_profile_name)
            if edited_text is not None:
                return edited_text
        return result.get('text', '')
        
    def clear_standard_results(self):
        """Removes all non-manual OCR results before a new run."""
        results_to_keep = [res for res in self.ocr_results if res.get('is_manual', False)]
        self.ocr_results = results_to_keep
        
        max_existing_base = -1
        if results_to_keep:
            for res in results_to_keep:
                try: max_existing_base = max(max_existing_base, math.floor(float(res.get('row_number', -1))))
                except: pass
        self.next_global_row_number = max_existing_base + 1
        print(f"Standard OCR results cleared. Next global row number will start from: {self.next_global_row_number}")


    def add_new_ocr_results(self, new_results: list[dict]):
        """Adds results from a completed OCR process to the model."""
        if not new_results:
            return
        
        self.ocr_results.extend(new_results)
        self._sort_ocr_results()
        
        affected_filename = new_results[0].get('filename')
        self.model_updated.emit([affected_filename] if affected_filename else [])

    def update_text(self, row_number, new_text: str):
        """Updates the text for a given row in the active profile."""
        target_result, _ = self._find_result_by_row_number(row_number)
        if not target_result or target_result.get('is_deleted', False):
            return "Result not found or is deleted.", False

        # If user is editing while in "Original", create a new profile.
        if self.active_profile_name == "Original":
            self.active_profile_name = "User Edit 1"
            if self.active_profile_name not in self.profiles:
                self.profiles[self.active_profile_name] = {}
                self.profiles_updated.emit() # Signal that the profile list has changed
                # The view will handle showing the message.

        if 'translations' not in target_result:
            target_result['translations'] = {}

        original_text = target_result.get('text', '')
        if new_text == original_text:
            if self.active_profile_name in target_result['translations']:
                del target_result['translations'][self.active_profile_name]
        else:
            target_result['translations'][self.active_profile_name] = new_text

        self.model_updated.emit([target_result.get('filename')])
        return None, True

    def delete_row(self, row_number_to_delete):
        """Marks a row as deleted."""
        target_result, target_index = self._find_result_by_row_number(row_number_to_delete)
        if target_index == -1 or target_result.get('is_deleted', False):
            return

        self.ocr_results[target_index]['is_deleted'] = True
        print(f"Marked row {row_number_to_delete} as deleted in model.")
        
        affected_filename = target_result.get('filename')
        self.model_updated.emit([affected_filename] if affected_filename else [])

    def combine_rows(self, first_row_number, combined_text, min_confidence, rows_to_delete):
        """Combines multiple rows into a single entry."""
        first_result, first_result_index = self._find_result_by_row_number(first_row_number)
        if first_result_index == -1:
            return "Could not find first row to update in data model.", False
        
        if self.active_profile_name == "Original":
            self.active_profile_name = "User Edit 1"
            if self.active_profile_name not in self.profiles:
                self.profiles[self.active_profile_name] = {}
                self.profiles_updated.emit()

        # Update confidence on the original record, but store combined text in the profile
        self.ocr_results[first_result_index]['confidence'] = min_confidence
        if 'translations' not in self.ocr_results[first_result_index]:
            self.ocr_results[first_result_index]['translations'] = {}
        self.ocr_results[first_result_index]['translations'][self.active_profile_name] = combined_text

        affected_filenames = {self.ocr_results[first_result_index].get('filename')}
        
        for rn_to_delete in rows_to_delete:
            result_to_delete, delete_index = self._find_result_by_row_number(rn_to_delete)
            if delete_index != -1:
                self.ocr_results[delete_index]['is_deleted'] = True
                affected_filenames.add(result_to_delete.get('filename'))

        self.model_updated.emit(list(filter(None, affected_filenames)))
        return f"Combined rows into row {first_row_number} in profile '{self.active_profile_name}'", True

    def add_profile(self, profile_name, translation_data=None):
        """Adds a new profile and optionally populates it with data."""
        if profile_name in self.profiles:
            print(f"Warning: Overwriting existing profile '{profile_name}'.")
        
        self.profiles[profile_name] = {}
        applied_count = 0

        if translation_data:
            for result in self.ocr_results:
                if result.get('is_deleted', False): continue
                
                filename = result.get('filename')
                row_number_str = str(result.get('row_number'))

                if filename in translation_data and row_number_str in translation_data[filename]:
                    translated_text = translation_data[filename][row_number_str]
                    if 'translations' not in result:
                        result['translations'] = {}
                    result['translations'][profile_name] = translated_text
                    applied_count += 1
        
        print(f"Added profile '{profile_name}'. Applied {applied_count} translations.")
        self.active_profile_name = profile_name
        self.profiles_updated.emit()
        self.model_updated.emit([]) # Use an empty list for a full refresh