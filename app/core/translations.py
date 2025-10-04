# translations.py
import re
import traceback
from google import genai
from PySide6.QtCore import QThread, Signal
from xml.sax.saxutils import escape, unescape
import xml.etree.ElementTree as ET

class TranslationThread(QThread):
    """
    Worker thread for performing the Gemini API call.
    Streams the translation back to the parent window.
    """
    translation_progress = Signal(str)
    translation_finished = Signal(str)
    translation_failed = Signal(str)

    def __init__(self, api_key, full_prompt, model_name, parent=None):
        super().__init__(parent)
        self.api_key = api_key
        self.full_prompt = full_prompt
        self.model_name = model_name
        self._is_running = True

    def run(self):
        try:
            client = genai.Client(api_key=self.api_key)
            client = genai.Client(api_key=self.api_key)
            
            response_stream = client.models.generate_content_stream(
                model=self.model_name,
                contents=self.full_prompt,
            )
            response_stream = client.models.generate_content_stream(
                model=self.model_name,
                contents=self.full_prompt,
            )
            full_response_text = ""
            
            for chunk in response_stream:
                if not self._is_running:
                    print("Translation thread stopped by user.")
                    break

                try:
                    text = chunk.text
                    if text: # Also check if the text is not an empty string
                        full_response_text += text
                        self.translation_progress.emit(text)
                except (ValueError, IndexError):
                    pass
            
            if self._is_running:
                self.translation_finished.emit(full_response_text)
                
        except Exception:
            error_details = traceback.format_exc()
            print(f"Gemini API Error:\n{error_details}") # Print full traceback to console
            self.translation_failed.emit(f"Gemini API Error:\n\n{error_details}")

    def stop(self):
        self._is_running = False

def _get_text_for_profile_static(result, profile_name):
    """Gets the text for a given result based on the specified profile."""
    if profile_name != "Original":
        edited_text = result.get('translations', {}).get(profile_name)
        if edited_text is not None:
            return edited_text
    return result.get('text', '')

def generate_for_translate_content(ocr_results, source_profile_name):
    """
    Generates XML-like content for translation from OCR results,
    using text from the specified source profile.
    """
    content = "<translations>\n"
    grouped_results = {}

    visible_results = [res for res in ocr_results if not res.get('is_deleted', False)]

    for result in visible_results:
        text = _get_text_for_profile_static(result, source_profile_name)
        filename = result.get('filename')
        row_number = result.get('row_number')

        if not all([filename, text, row_number is not None]) or text.isspace():
            continue

        if filename not in grouped_results:
            grouped_results[filename] = []
        grouped_results[filename].append((text, row_number))

    for filename, texts_with_rows in grouped_results.items():
        content += f"<{escape(filename)}>\n"
        sorted_texts_with_rows = sorted(texts_with_rows, key=lambda x: float(x[1]))
        for text, row_number in sorted_texts_with_rows:
            content += f"<{str(row_number)}>{escape(text)}</{str(row_number)}>\n"
        content += f"</{escape(filename)}>\n"

    return content + "</translations>\n"

def generate_retranslate_content(ocr_results, source_profile_name, selected_items, context_size=3):
    """
    Generates XML-like content for re-translation based on selected items.
    Groups selected rows by proximity into <re-translation> blocks and wraps
    them in their parent filename tags.
    """
    if not selected_items:
        return ""

    content = ""
    
    # Organize all valid results by filename
    all_results_by_file = {}
    for res in ocr_results:
        if not res.get('is_deleted', False):
            filename = res.get('filename')
            if filename not in all_results_by_file:
                all_results_by_file[filename] = []
            all_results_by_file[filename].append(res)
    
    # Sort results in each file by row number
    for filename in all_results_by_file:
        all_results_by_file[filename].sort(key=lambda x: float(x.get('row_number', 0)))

    # Organize selected items by filename
    selected_by_file = {}
    for filename, row_number_str in selected_items:
        if filename not in selected_by_file:
            selected_by_file[filename] = []
        selected_by_file[filename].append(row_number_str)

    # Process selections for each file
    for filename, selected_rows_str in selected_by_file.items():
        file_results = all_results_by_file.get(filename, [])
        if not file_results:
            continue

        # Map row numbers to their index in the sorted list for efficient lookup
        row_num_to_idx = {str(res.get('row_number')): i for i, res in enumerate(file_results)}
        
        selected_indices = sorted([row_num_to_idx[r] for r in selected_rows_str if r in row_num_to_idx])
        
        if not selected_indices:
            continue

        content += f"<{escape(filename)}>\n"
            
        # Group selected indices by proximity (overlapping context)
        groups = []
        if selected_indices:
            current_group = [selected_indices[0]]
            for i in range(1, len(selected_indices)):
                prev_idx = current_group[-1]
                current_idx = selected_indices[i]
                
                if (current_idx - context_size) <= (prev_idx + context_size):
                    current_group.append(current_idx)
                else:
                    groups.append(current_group)
                    current_group = [current_idx]
            groups.append(current_group)

        # Generate XML for each group
        for group in groups:
            content += "<re-translation>\n"
            
            min_idx_in_range = max(0, group[0] - context_size)
            max_idx_in_range = min(len(file_results) - 1, group[-1] + context_size)
            
            selected_indices_in_group = set(group)

            for idx in range(min_idx_in_range, max_idx_in_range + 1):
                result = file_results[idx]
                text = _get_text_for_profile_static(result, source_profile_name)
                row_number = str(result.get('row_number'))

                if idx in selected_indices_in_group:
                    content += f"<{row_number}>{escape(text)}</{row_number}>\n"
                else:
                    content += f"<context>{escape(text)}</context>\n"

            content += "</re-translation>\n"
        
        content += f"</{escape(filename)}>\n"
            
    return content

def import_translation_file_content(content):
    """
    Parses translated XML-like content robustly and returns a dictionary.
    Handles malformed data such as missing closing tags, the optional
    presence of <translate> tags, and row data that appears outside
    of a file tag (assigning it to the most recently seen filename).
    Handles both integer and decimal row number tags.

    Returns: {filename: {row_number_str: translated_text}}
    """
    translations = {}
    
    # Regex for file tags (general, non-closing tag)
    file_tag_pattern = re.compile(r'<(?P<name>[^/][^>]+)>')
    # Regex specifically for row tags, now including decimals.
    row_tag_pattern = re.compile(r'<(?P<rownum>\d+(\.\d+)?)>')
    
    # Regex for content extraction, tolerant of missing closing tags.
    translate_pattern = re.compile(r'<translate>(.*?)(?:</translate>)?', re.DOTALL | re.IGNORECASE)

    current_filename = None

    for line in content.splitlines():
        line = line.strip()
        if not line:
            continue

        row_match = row_tag_pattern.match(line)

        # Priority 1: Check if the line starts with a numeric/decimal row tag.
        if row_match and current_filename:
            row_number = row_match.group('rownum')
            
            # Extract content from within the row tag.
            content_start = row_match.end()
            closing_tag = f'</{row_number}>'
            content_end = line.rfind(closing_tag)
            
            line_content = line[content_start:content_end] if content_end != -1 else line[content_start:]
            
            # Within the row content, check for a <translate> tag.
            text_inside_translate = translate_pattern.search(line_content)
            if text_inside_translate:
                translated_text = text_inside_translate.group(1)
            else:
                # If no <translate> tag, use the entire content of the row tag.
                translated_text = line_content
            
            # Unescape and clean up the final text.
            final_text = unescape(translated_text).strip()
            if final_text:
                translations[current_filename][row_number] = final_text
            continue # Move to the next line

        file_match = file_tag_pattern.match(line)
        # Priority 2: If it's not a row tag, check if it's a new file tag.
        if file_match:
            potential_filename = file_match.group('name')
            # Exclude known structural tags from being considered filenames.
            if potential_filename.lower() not in ['translations', 'translate', 'context', 're-translation']:
                current_filename = unescape(potential_filename)
                if current_filename not in translations:
                    translations[current_filename] = {}
    
    return translations