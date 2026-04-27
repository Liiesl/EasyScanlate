# project_model.py

import os, json, traceback, zipfile, math, sys, uuid
from PySide6.QtCore import QObject, Signal, QBuffer
from PySide6.QtGui import QPixmap

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
    # Emitted when the list of profiles changes (new profile added).
    profiles_updated = Signal()
    # Emitted when a profile is created for a user edit (not programmatic changes like find/replace).
    profile_created_for_user_edit = Signal()
    # Emitted when the image list changes structurally (load, stitch, split).
    image_list_changed = Signal()

    # --- Granular data-change signals ---
    # Each carries a list of affected filenames (empty list means "all").
    text_updated = Signal(list)          # text/translation edits, combine rows
    style_updated = Signal(list)         # custom style diff applied
    rows_deleted = Signal(list)          # row soft-deleted
    ocr_results_added = Signal(list)     # new OCR results added or standard results cleared
    inpaint_updated = Signal(list)       # inpaint record added/removed
    structural_updated = Signal(list)    # import, stitch, split (bulk structural changes)

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
        # --- NEW: Add inpaint data to the model's state ---
        self.inpaint_data: list[dict] = []
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
                
            # --- NEW: Load inpaint.json ---
            inpaint_path = os.path.join(temp_dir, 'inpaint.json')
            if os.path.exists(inpaint_path):
                with open(inpaint_path, 'r', encoding='utf-8') as f:
                    self.inpaint_data = json.load(f)
                    print(f"Loaded {len(self.inpaint_data)} inpaint records.")

            print(f"Project '{self.project_name}' loaded successfully into model.")
            # Emit image_list_changed BEFORE project_loaded so reactive ViewModels
            # (e.g. ImageAreaViewModel) have synced their state before Views react.
            self.image_list_changed.emit()
            self.project_loaded.emit()

        except Exception as e:
            error_msg = f"Failed to load project: {e}"
            print(error_msg)
            traceback.print_exc()
            self.project_load_failed.emit(error_msg)

    # ... ( _load_master_json and _load_meta_json remain unchanged ) ...
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
                
            # --- NEW: Save inpaint data ---
            if self.inpaint_data:
                inpaint_path = os.path.join(self.temp_dir, 'inpaint.json')
                with open(inpaint_path, 'w', encoding='utf-8') as f:
                    json.dump(self.inpaint_data, f, indent=2, ensure_ascii=False)

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

    def add_inpaint_record(self, record: dict, patch_pixmap: QPixmap):
        """
        Adds a new inpaint record to the model and saves the patch image.
        """
        try:
            inpaint_dir = os.path.join(self.temp_dir, 'inpaint')
            os.makedirs(inpaint_dir, exist_ok=True)
            
            patch_filename = record.get("patch_filename")
            if not patch_filename:
                raise ValueError("Inpaint record is missing 'patch_filename'.")

            patch_save_path = os.path.join(inpaint_dir, patch_filename)
            if not patch_pixmap.save(patch_save_path, "PNG"):
                raise IOError(f"Failed to save inpaint patch to {patch_save_path}")

            self.inpaint_data.append(record)
            print(f"Added and saved inpaint record for '{record['target_image']}'.")

            # Signal that inpaint data changed, affecting one specific image.
            self.inpaint_updated.emit([record['target_image']])
            return True, None
        except Exception as e:
            error_msg = f"Failed to add inpaint record: {e}"
            print(error_msg)
            traceback.print_exc()
            return False, error_msg
            
    def remove_inpaint_record(self, record_id: str):
        """
        Finds an inpaint record by its ID, deletes its associated patch file,
        and removes it from the model's data.
        """
        record_to_remove = None
        for record in self.inpaint_data:
            if record.get("id") == record_id:
                record_to_remove = record
                break

        if not record_to_remove:
            print(f"Warning: Could not find inpaint record with ID '{record_id}' to remove.")
            return False, "Record not found."

        try:
            # 1. Delete the patch file from the temporary directory
            inpaint_dir = os.path.join(self.temp_dir, 'inpaint')
            patch_path = os.path.join(inpaint_dir, record_to_remove['patch_filename'])
            if os.path.exists(patch_path):
                os.remove(patch_path)
                print(f"Deleted inpaint patch file: {patch_path}")
            else:
                print(f"Warning: Inpaint patch file not found for deletion: {patch_path}")

            # 2. Remove the record from the list in memory
            self.inpaint_data.remove(record_to_remove)
            print(f"Removed inpaint record ID '{record_id}' from model.")

            # 3. Signal that inpaint data changed, affecting the target image
            # This will trigger a UI refresh, causing the patch to disappear.
            target_image = record_to_remove['target_image']
            self.inpaint_updated.emit([target_image])
            return True, None
        except Exception as e:
            error_msg = f"Failed to remove inpaint record: {e}"
            print(error_msg)
            traceback.print_exc()
            return False, error_msg

    # --- NEW: Method to get all inpaint records for a specific image ---
    def get_inpaint_records_for_image(self, filename: str) -> list[dict]:
        """
        Retrieves all inpaint records associated with a specific image filename.
        """
        if not filename:
            return []
        return [record for record in self.inpaint_data if record.get('target_image') == filename]

    # --- NEW: Method to load a specific inpaint patch as a QPixmap ---
    def get_inpaint_patch_pixmap(self, patch_filename: str) -> QPixmap | None:
        """
        Loads an inpaint patch image file into a QPixmap.
        """
        if not patch_filename or not self.temp_dir:
            return None
        
        patch_path = os.path.join(self.temp_dir, 'inpaint', patch_filename)
        
        if os.path.exists(patch_path):
            return QPixmap(patch_path)
        else:
            print(f"Warning: Inpaint patch pixmap not found at {patch_path}")
            return None

    def _redistribute_ocr_for_split(self, source_filename: str, new_image_data: list[dict], split_y_coords: list[int]):
        """
        Reassigns OCR results from a source image to newly created split images.
        """
        y_starts = [0] + split_y_coords
        source_path_to_remove = next((p for p in self.image_paths if os.path.basename(p) == source_filename), None)

        affected_filenames = {source_filename}
        for result in self.ocr_results:
            if result.get('filename') == source_filename:
                try:
                    coords = result.get('coordinates', [])
                    if not coords: continue
                    
                    top_y = min(p[1] for p in coords)
                    segment_index = -1
                    for i in range(len(y_starts)):
                        start_boundary = y_starts[i]
                        end_boundary = y_starts[i+1] if i + 1 < len(y_starts) else float('inf')
                        
                        if start_boundary <= top_y < end_boundary:
                            segment_index = i
                            break
                    
                    if segment_index != -1:
                        new_data = new_image_data[segment_index]
                        new_filename = new_data['filename']
                        y_offset = y_starts[segment_index]
                        
                        result['filename'] = new_filename
                        result['coordinates'] = [[p[0], p[1] - y_offset] for p in coords]
                        affected_filenames.add(new_filename)
                    else:
                        print(f"Warning: Could not place OCR result (row {result.get('row_number')}) into a split segment.")

                except Exception as e:
                    print(f"Error processing result for split: {e} - Result: {result}")
        
        if source_path_to_remove and source_path_to_remove in self.image_paths:
            self.image_paths.remove(source_path_to_remove)
        
        for data in new_image_data:
            self.image_paths.append(data['path'])

    def _redistribute_inpaint_for_split(self, source_filename: str, new_image_data: list[dict], split_y_coords: list[int]):
        """
        Reassigns inpaint records from a source image to newly created split images.
        """
        y_starts = [0] + sorted(split_y_coords)
        
        for record in self.inpaint_data:
            if record.get('target_image') == source_filename:
                try:
                    coords = record.get('coordinates', [])
                    if not coords or len(coords) != 4: continue
                    
                    record_y = coords[1]
                    segment_index = -1
                    
                    for i in range(len(y_starts)):
                        start_boundary = y_starts[i]
                        end_boundary = y_starts[i+1] if i + 1 < len(y_starts) else float('inf')
                        
                        if start_boundary <= record_y < end_boundary:
                            segment_index = i
                            break
                    
                    if segment_index != -1:
                        new_data = new_image_data[segment_index]
                        new_filename = new_data['filename']
                        y_offset = y_starts[segment_index]
                        
                        record['target_image'] = new_filename
                        record['coordinates'][1] -= y_offset
                    else:
                        print(f"Warning: Could not place inpaint record (id {record.get('id')}) into a split segment.")

                except Exception as e:
                    print(f"Error processing inpaint record for split: {e} - Record: {record}")

    # --- Rich, self-notifying model methods ---

    def set_active_profile(self, name: str) -> bool:
        """Sets the active profile if it exists. Returns True if changed."""
        if name in self.profiles and name != self.active_profile_name:
            self.active_profile_name = name
            return True
        return False

    def set_next_global_row_number(self, value: int):
        """Sets the next global row number."""
        self.next_global_row_number = value

    def set_mmtl_path(self, path: str):
        """Sets the project's .mmtl file path."""
        self.mmtl_path = path

    def import_master_data(self, new_ocr_results: list[dict]):
        """
        Replaces OCR results with imported master data.
        Rebuilds profiles, recalculates next_global_row_number,
        resets active profile if invalid, and emits all necessary signals.
        """
        self.ocr_results = new_ocr_results

        loaded_profiles = set(["Original"])
        max_row_num = -1

        for res in self.ocr_results:
            if 'row_number' in res:
                try:
                    max_row_num = max(max_row_num, int(float(res['row_number'])))
                except (ValueError, TypeError):
                    pass
            if 'translations' in res and isinstance(res['translations'], dict):
                for profile_name in res['translations']:
                    loaded_profiles.add(profile_name)

        if max_row_num >= 0:
            self.next_global_row_number = max_row_num + 1

        self.profiles = {name: {} for name in loaded_profiles}

        if self.active_profile_name not in self.profiles:
            print(f"Warning: Active profile '{self.active_profile_name}' not found in imported data. Defaulting to 'Original'.")
            self.active_profile_name = "Original"

        self.profiles_updated.emit()
        self.structural_updated.emit([])
        self.image_list_changed.emit()

    def stitch_images_update(self, filenames: list[str], new_filename: str, offsets: dict[str, int]):
        """
        Reassigns OCR results and inpaint data for stitched images.
        Removes old images from image_paths, sorts results, and emits signals.
        """
        # Update OCR results
        for result in self.ocr_results:
            old_filename = result.get('filename')
            if old_filename in offsets:
                result['filename'] = new_filename
                height_offset = offsets[old_filename]
                if height_offset > 0:
                    coords = result.get('coordinates', [])
                    if coords:
                        result['coordinates'] = [[p[0], p[1] + height_offset] for p in coords]

        # Update inpaint data
        for record in self.inpaint_data:
            old_filename = record.get('target_image')
            if old_filename in offsets:
                record['target_image'] = new_filename
                height_offset = offsets[old_filename]
                if height_offset > 0:
                    coords = record.get('coordinates', [])
                    if coords and len(coords) == 4:
                        record['coordinates'][1] += height_offset

        # Remove old images from image_paths
        images_dir = os.path.join(self.temp_dir, 'images')
        filenames_to_remove = [f for f in filenames if f != new_filename]
        for fname in filenames_to_remove:
            path = next((p for p in self.image_paths if os.path.basename(p) == fname), None)
            if path and path in self.image_paths:
                self.image_paths.remove(path)
            full_path = os.path.join(images_dir, fname)
            try:
                if os.path.exists(full_path):
                    os.remove(full_path)
            except Exception as e:
                print(f"Warning: Could not delete old image file {full_path}. Error: {e}")

        self._sort_ocr_results()
        self.structural_updated.emit([])
        self.image_list_changed.emit()

    def split_image_update(self, source_filename: str, new_image_data: list[dict], split_y_coords: list[int]) -> list[str]:
        """
        Reassigns OCR results and inpaint data for a split image.
        Replaces the source image path with new split image paths at the correct index.
        Sorts results and emits signals.
        Returns the list of new filenames.
        """
        # Record original position before mutation
        original_index = None
        for i, p in enumerate(self.image_paths):
            if os.path.basename(p) == source_filename:
                original_index = i
                break

        self._redistribute_inpaint_for_split(source_filename, new_image_data, split_y_coords)
        self._redistribute_ocr_for_split(source_filename, new_image_data, split_y_coords)

        # Reorder image_paths: remove newly appended paths and insert at original index
        new_paths = [data['path'] for data in new_image_data]
        for np in new_paths:
            if np in self.image_paths:
                self.image_paths.remove(np)

        insert_at = original_index if original_index is not None else len(self.image_paths)
        for i, np in enumerate(new_paths):
            self.image_paths.insert(insert_at + i, np)

        affected_filenames = {source_filename}
        for data in new_image_data:
            affected_filenames.add(data['filename'])

        self._sort_ocr_results()
        self.structural_updated.emit(list(filter(None, affected_filenames)))
        self.image_list_changed.emit()

        return [data['filename'] for data in new_image_data]

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

    def update_style(self, row_number, style_diff):
        """Updates the custom_style for a given row and emits style_updated."""
        target_result, _ = self._find_result_by_row_number(row_number)
        if not target_result:
            return False
        if target_result.get('is_deleted', False):
            return False
        if style_diff:
            target_result['custom_style'] = style_diff
        elif 'custom_style' in target_result:
            del target_result['custom_style']
        self.style_updated.emit([target_result.get('filename')])
        return True

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
        translations = result.get('translations', {})
        original_text = result.get('text', '')
        
        if self.active_profile_name != "Original":
            edited_text = translations.get(self.active_profile_name)
            if edited_text is not None:
                return edited_text
        
        return original_text
        
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
        # Notify views so image labels refresh (batch OCR clear was leaving stale text boxes).
        self.ocr_results_added.emit([])


    def add_new_ocr_results(self, new_results: list[dict]):
        """Adds results from a completed OCR process to the model."""
        if not new_results:
            return
        
        self.ocr_results.extend(new_results)
        self._sort_ocr_results()

        affected_filename = new_results[0].get('filename')
        self.ocr_results_added.emit([affected_filename] if affected_filename else [])

    def _find_existing_user_edit_profile(self):
        """Finds the first existing user edit profile, or None if none exists.
        
        Returns:
            str or None: The name of the first user edit profile (e.g., "User Edit 1"), or None
        """
        # Sort profile names to find the lowest numbered user edit profile
        user_edit_profiles = []
        for profile_name in self.profiles.keys():
            if profile_name.startswith("User Edit "):
                try:
                    # Extract the number after "User Edit "
                    num = int(profile_name.split("User Edit ")[1])
                    user_edit_profiles.append((num, profile_name))
                except (ValueError, IndexError):
                    continue
        
        if user_edit_profiles:
            # Sort by number and return the first one
            user_edit_profiles.sort(key=lambda x: x[0])
            return user_edit_profiles[0][1]
        return None

    def update_text(self, row_number, new_text: str, is_user_edit: bool = True):
        """Updates the text for a given row in the active profile.
        
        Args:
            row_number: The row number to update
            new_text: The new text to set
            is_user_edit: If True, this is a direct user edit (will show profile creation message).
                         If False, this is a programmatic change (e.g., find/replace) and should not show messages.
        """
        target_result, _ = self._find_result_by_row_number(row_number)
        if not target_result or target_result.get('is_deleted', False):
            return "Result not found or is deleted.", False

        profile_created = False
        if self.active_profile_name == "Original":
            existing_user_edit = self._find_existing_user_edit_profile()
            if existing_user_edit:
                self.active_profile_name = existing_user_edit
            else:
                self.active_profile_name = "User Edit 1"
                if self.active_profile_name not in self.profiles:
                    self.profiles[self.active_profile_name] = {}
                    if is_user_edit:
                        self.profiles_updated.emit()
                    profile_created = True

        if 'translations' not in target_result:
            target_result['translations'] = {}

        original_text = target_result.get('text', '')
        
        was_routing_from_original = False
        existing_user_edit_text = None
        if self.active_profile_name != "Original" and self.active_profile_name in target_result['translations']:
            existing_user_edit_text = target_result['translations'][self.active_profile_name]
            if new_text != existing_user_edit_text:
                if (new_text.startswith(original_text) or original_text.startswith(new_text) or 
                    new_text == original_text or 
                    abs(len(new_text) - len(original_text)) < abs(len(new_text) - len(existing_user_edit_text))):
                    was_routing_from_original = True
        
        if new_text == original_text:
            if self.active_profile_name != "Original" and self.active_profile_name in target_result['translations']:
                existing_translation = target_result['translations'][self.active_profile_name]
                if existing_translation != original_text:
                    del target_result['translations'][self.active_profile_name]
        elif was_routing_from_original and existing_user_edit_text:
            user_edit_len = len(existing_user_edit_text)
            original_len = len(original_text)
            new_text_len = len(new_text)
            
            # Check if restoring deleted content (works for all character types)
            is_restoring = False
            if user_edit_len < original_len and new_text_len > user_edit_len:
                if new_text_len <= original_len:
                    is_restoring = True
                elif new_text_len > original_len:
                    deleted_region_size = original_len - user_edit_len
                    if (new_text_len - original_len) < deleted_region_size * 0.5:
                        is_restoring = True
            
            if is_restoring:
                merged_text = existing_user_edit_text
            elif new_text.startswith(original_text):
                merged_text = existing_user_edit_text + new_text[len(original_text):]
            elif original_text.startswith(new_text) and len(existing_user_edit_text) < len(original_text):
                merged_text = existing_user_edit_text
            else:
                merged_text = existing_user_edit_text
            
            target_result['translations'][self.active_profile_name] = merged_text
        else:
            target_result['translations'][self.active_profile_name] = new_text

        self.text_updated.emit([target_result.get('filename')])
        return None, True, profile_created, profile_created and is_user_edit

    def delete_row(self, row_number_to_delete):
        """Marks a row as deleted."""
        target_result, target_index = self._find_result_by_row_number(row_number_to_delete)
        if target_index == -1 or target_result.get('is_deleted', False):
            return

        self.ocr_results[target_index]['is_deleted'] = True
        print(f"Marked row {row_number_to_delete} as deleted in model.")

        affected_filename = target_result.get('filename')
        self.rows_deleted.emit([affected_filename] if affected_filename else [])

    def combine_rows(self, first_row_number, combined_text, min_confidence, rows_to_delete):
        """Combines multiple rows into a single entry."""
        first_result, first_result_index = self._find_result_by_row_number(first_row_number)
        if first_result_index == -1:
            return "Could not find first row to update in data model.", False
        
        if self.active_profile_name == "Original":
            # First, check if there's already a user edit profile
            existing_user_edit = self._find_existing_user_edit_profile()
            if existing_user_edit:
                # Route to existing user edit profile
                self.active_profile_name = existing_user_edit
            else:
                # Create a new user edit profile
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

        self.text_updated.emit(list(filter(None, affected_filenames)))
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
        self.text_updated.emit([])