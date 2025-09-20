# translations.py
import re
from google import genai
from PySide6.QtCore import QThread, Signal

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
# ... (This function is unchanged)
    """
    Generates Markdown content in for-translate format from OCR results,
    using text from the specified source profile.
    """
    content = "<!-- type: for-translate -->\n\n"
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

    for idx, (filename, texts_with_rows) in enumerate(grouped_results.items()):
        if idx > 0: content += "\n\n"
        content += f"<!-- file: {filename} -->\n\n"

        sorted_texts_with_rows = sorted(texts_with_rows, key=lambda x: float(x[1]))
        for text, row_number in sorted_texts_with_rows:
            content += f"{text}\n"
            content += f"-/{str(row_number)}\\-\n\n"

    return content.rstrip() + "\n"

def generate_retranslate_content(ocr_results, source_profile_name, selected_items, context_size=3):
# ... (This function is unchanged)
    """
    Generates Markdown content for re-translation in a structured format.
    For each selected item, it includes context from surrounding text within
    [CONTEXT] blocks and the text to be translated in a [TRANSLATE] block.
    'selected_items' is a list of (filename, row_number_str) tuples.
    """
    content = "<!-- type: for-translate -->\n\n"
    
    # Filter out deleted results and group all results by filename for quick lookup
    all_results_by_file = {}
    for res in ocr_results:
        if not res.get('is_deleted', False):
            filename = res.get('filename')
            if filename not in all_results_by_file:
                all_results_by_file[filename] = []
            all_results_by_file[filename].append(res)
    
    # Sort results within each file by row number
    for filename in all_results_by_file:
        all_results_by_file[filename].sort(key=lambda x: float(x.get('row_number', 0)))

    # Group the selected items by filename to process them in batches
    selected_by_file = {}
    for filename, row_number_str in selected_items:
        if filename not in selected_by_file:
            selected_by_file[filename] = []
        selected_by_file[filename].append(row_number_str)

    # Build the content string
    for file_idx, (filename, selected_rows) in enumerate(selected_by_file.items()):
        if file_idx > 0:
            content += "\n\n"
        content += f"<!-- file: {filename} -->\n\n"
        
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

            # Assemble the block using the new [CONTEXT] and [TRANSLATE] tags
            block_parts = []
            if context_before:
                block_parts.append(f"[CONTEXT]\n" + "\n".join(context_before) + "\n[/CONTEXT]")
            
            block_parts.append(f"[TRANSLATE]\n{text_to_retranslate}\n[/TRANSLATE]")

            if context_after:
                block_parts.append(f"[CONTEXT]\n" + "\n".join(context_after) + "\n[/CONTEXT]")

            final_block = "\n".join(block_parts)
            
            content += f"{final_block}\n-/{row_number_str}\\-\n\n"
            
    return content.rstrip() + "\n"


def import_translation_file_content(content):
# ... (This function is unchanged)
    """ Parses translated content and returns a dictionary.
    This function is robust and can handle two formats:
    1. Simple format for initial translation (text followed by delimiter).
    2. Structured format for re-translation (containing [TRANSLATE] tags).
    Returns: {filename: {row_number_str: translated_text}} """
    if '<!-- type: for-translate -->' not in content:
        raise ValueError("Unsupported MD format - missing type comment.")

    translations = {}
    current_file = None
    buffer_text = ""
    
    lines = content.split('\n')
    i = 0
    while i < len(lines):
        line = lines[i] # Keep original line with whitespace for the buffer

        stripped_line = line.strip()

        if stripped_line.startswith('<!-- file:') and stripped_line.endswith('-->'):
            current_file = stripped_line[10:-3].strip()
            if current_file not in translations:
                translations[current_file] = {}
            i += 1
            continue

        elif stripped_line.startswith('-/') and stripped_line.endswith('\\-') and current_file:
            row_number_str = stripped_line[2:-2].strip()
            
            if buffer_text:
                translated_text = ""
                # Check for the structured re-translate format first
                match = re.search(r'\[TRANSLATE\](.*?)\[/TRANSLATE\]', buffer_text, re.DOTALL)
                
                if match:
                    # If found, extract only the text within the [TRANSLATE] block
                    translated_text = match.group(1).strip()
                else:
                    # Otherwise, fall back to the simple format (the whole buffer is the translation)
                    translated_text = buffer_text.strip()
                
                translations[current_file][row_number_str] = translated_text
                buffer_text = ""

            i += 1
            continue

        elif current_file is not None:
            # Accumulate lines into the buffer for the current file block
            if buffer_text:
                buffer_text += "\n" + line
            else:
                buffer_text = line
        
        i += 1

    return translations