# translations.py
import re
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
                
        except Exception as e:
            self.translation_failed.emit(f"Gemini API Error: {str(e)}")

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
    Generates XML-like content for re-translation.
    For each selected item, it includes context and the text to be translated
    within <context> and <translate> tags respectively.
    """
    content = "<translations>\n"
    
    all_results_by_file = {}
    for res in ocr_results:
        if not res.get('is_deleted', False):
            filename = res.get('filename')
            if filename not in all_results_by_file:
                all_results_by_file[filename] = []
            all_results_by_file[filename].append(res)
    
    for filename in all_results_by_file:
        all_results_by_file[filename].sort(key=lambda x: float(x.get('row_number', 0)))

    selected_by_file = {}
    for filename, row_number_str in selected_items:
        if filename not in selected_by_file:
            selected_by_file[filename] = []
        selected_by_file[filename].append(row_number_str)

    for filename, selected_rows in selected_by_file.items():
        content += f"<{escape(filename)}>\n"
        
        file_results = all_results_by_file.get(filename, [])
        if not file_results:
            continue

        for row_number_str in selected_rows:
            target_idx = -1
            for i, res in enumerate(file_results):
                if str(res.get('row_number')) == row_number_str:
                    target_idx = i
                    break
            
            if target_idx == -1:
                continue

            start_idx = max(0, target_idx - context_size)
            end_idx = min(len(file_results), target_idx + context_size + 1)
            context_slice = file_results[start_idx:end_idx]
            
            context_before, text_to_retranslate, context_after = [], "", []
            target_row_float = float(file_results[target_idx].get('row_number', -1))

            for res in context_slice:
                text = _get_text_for_profile_static(res, source_profile_name)
                res_row_float = float(res.get('row_number', 0))

                if res_row_float < target_row_float:
                    context_before.append(text)
                elif res_row_float == target_row_float:
                    text_to_retranslate = text
                else:
                    context_after.append(text)

            # Assemble the block using <context> and <translate> tags
            block = f"<{row_number_str}>\n"
            if context_before:
                block += f"<context>{escape(chr(10).join(context_before))}</context>\n"
            
            block += f"<translate>{escape(text_to_retranslate)}</translate>\n"

            if context_after:
                block += f"<context>{escape(chr(10).join(context_after))}</context>\n"
            
            block += f"</{row_number_str}>\n"
            content += block
            
        content += f"</{escape(filename)}>\n"
            
    return content + "</translations>\n"


def import_translation_file_content(content):
    """
    Parses translated XML-like content robustly and returns a dictionary.
    Handles malformed data such as missing closing tags, the optional
    presence of <translate> tags, and row data that appears outside
    of a file tag (assigning it to the most recently seen filename).

    Returns: {filename: {row_number_str: translated_text}}
    """
    translations = {}
    
    # Regex to identify start tags. Heuristics are used to differentiate
    # file tags from row tags.
    file_tag_pattern = re.compile(r'<(?P<name>[^/][^>]+)>')
    row_tag_pattern = re.compile(r'<(?P<rownum>\d+)>')
    
    # Regex for content extraction, tolerant of missing closing tags.
    translate_pattern = re.compile(r'<translate>(.*?)(?:</translate>)?', re.DOTALL | re.IGNORECASE)

    current_filename = None

    for line in content.splitlines():
        line = line.strip()
        if not line:
            continue

        row_match = row_tag_pattern.match(line)
        file_match = file_tag_pattern.match(line)

        # Check for a new file tag. A tag is considered a file tag if it's
        # not a purely numeric tag (a row tag).
        if file_match and not file_match.group('name').isdigit():
            potential_filename = file_match.group('name')
            if potential_filename.lower() not in ['translations', 'translate', 'context']:
                current_filename = unescape(potential_filename)
                if current_filename not in translations:
                    translations[current_filename] = {}
                continue
        
        # Check for a row tag. This check is independent of the file tag check
        # to allow rows to be associated with the last known file.
        if row_match and current_filename:
            row_number = row_match.group('rownum')
            
            # Extract content from the row tag.
            # Start after the opening tag.
            content_start = row_match.end()
            # End before the closing tag, if it exists.
            closing_tag = f'</{row_number}>'
            content_end = line.rfind(closing_tag)
            
            if content_end != -1:
                line_content = line[content_start:content_end]
            else:
                line_content = line[content_start:]
            
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
    
    return translations