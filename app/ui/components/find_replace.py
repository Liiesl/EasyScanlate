# --- START OF FILE app/find_replace.py ---

from PySide6.QtWidgets import (QWidget, QHBoxLayout, QVBoxLayout, QLineEdit, QPushButton, QLabel, QTextEdit, QCheckBox, QSizePolicy, QAbstractItemView, QFrame) # Added QFrame
from PySide6.QtCore import Qt, Signal, QTimer
from PySide6.QtGui import QTextCursor, QTextCharFormat, QColor, QSyntaxHighlighter, QFont, QTextDocument
import qtawesome as qta
import re

# --- SearchHighlighter class remains the same ---
class SearchHighlighter(QSyntaxHighlighter):
    def __init__(self, parent_document: QTextDocument):
        super().__init__(parent_document)
        self._pattern = ""
        self._case_sensitive = False
        self.highlight_format = QTextCharFormat()
        self.highlight_format.setBackground(QColor("#DAA520")) # Goldenrod / VSCode search yellow
        self.highlight_format.setForeground(QColor("black"))
        self.highlight_format.setFontWeight(QFont.Bold)

    @property
    def pattern(self): return self._pattern

    def setPattern(self, pattern: str, case_sensitive: bool):
        if pattern != self._pattern or case_sensitive != self._case_sensitive:
            self._pattern = pattern; self._case_sensitive = case_sensitive
            self.rehighlight()

    def highlightBlock(self, text: str):
        if not self._pattern: return
        flags = re.NOFLAG if self._case_sensitive else re.IGNORECASE
        try:
            escaped_pattern = re.escape(self._pattern) # Basic search escapes
            # TODO: Add Regex option handling here if implementing regex filter
            for match in re.finditer(escaped_pattern, text, flags=flags):
                start, end = match.span()
                self.setFormat(start, end - start, self.highlight_format)
        except re.error as e: print(f"Highlighter: Regex error: {e}")


# --- FindReplaceWidget Class ---
class FindReplaceWidget(QWidget):
    closed = Signal()
    request_update_ocr_data = Signal(object, str)

    def __init__(self, main_window):
        super().__init__(main_window)
        self.main_window = main_window
        self.setObjectName("FindReplaceWidget")

        self.matches = []
        self.current_match_index = -1
        self._active_highlighters: dict[QTextDocument, SearchHighlighter] = {}
        self.search_timer = QTimer(self); self.search_timer.setSingleShot(True)
        self.search_timer.setInterval(300); self.search_timer.timeout.connect(self.find_text)
        self._is_highlighting = False  # Flag to prevent textChanged from triggering updates during highlighting
        self._highlighting_text_cache = {}  # Cache text during highlighting to detect false changes

        # Filter states (add more as needed)
        self._match_case = False
        self._match_whole_word = False # Placeholder
        self._use_regex = False        # Placeholder

        self._init_ui()
        self.hide()

    def _init_ui(self):
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0) # Remove margins from main layout
        main_layout.setSpacing(0) # No spacing between find/replace rows

        # --- Top Row (Find) ---
        find_row_layout = QHBoxLayout()
        find_row_layout.setContentsMargins(0, 0, 0, 0)
        find_row_layout.setSpacing(4) # Spacing between elements in the row

        # Toggle Replace Button
        self.btn_toggle_replace = QPushButton(qta.icon('fa5s.chevron-right', color='inherit'), "")
        self.btn_toggle_replace.setCheckable(True)
        self.btn_toggle_replace.setToolTip("Toggle Replace")
        self.btn_toggle_replace.clicked.connect(self.toggle_replace_visible)
        find_row_layout.addWidget(self.btn_toggle_replace)

        # Find Input
        self.find_input = QLineEdit()
        self.find_input.setPlaceholderText("Find")
        self.find_input.textChanged.connect(self.schedule_find)
        self.find_input.returnPressed.connect(self.find_next)
        self.find_input.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred) # Allow expansion
        find_row_layout.addWidget(self.find_input)

        # Match Count Label
        self.match_count_label = QLabel("No results")
        self.match_count_label.setObjectName("MatchCountLabel") # Assign ID for styling
        self.match_count_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        find_row_layout.addWidget(self.match_count_label)

        # --- Filter Buttons ---
        self.btn_match_case = QPushButton(qta.icon('mdi.format-letter-case', color='inherit'), "")
        self.btn_match_case.setCheckable(True)
        self.btn_match_case.setToolTip("Match Case")
        self.btn_match_case.clicked.connect(self._update_filters)
        find_row_layout.addWidget(self.btn_match_case)

        self.btn_whole_word = QPushButton(qta.icon('mdi.format-letter-matches', color='inherit'), "")
        self.btn_whole_word.setCheckable(True)
        self.btn_whole_word.setToolTip("Match Whole Word (Not Implemented)")
        self.btn_whole_word.setEnabled(False) # Disable for now
        # self.btn_whole_word.clicked.connect(self._update_filters)
        find_row_layout.addWidget(self.btn_whole_word)

        self.btn_regex = QPushButton(qta.icon('mdi.regex', color='inherit'), "")
        self.btn_regex.setCheckable(True)
        self.btn_regex.setToolTip("Use Regular Expression (Not Implemented)")
        self.btn_regex.setEnabled(False) # Disable for now
        # self.btn_regex.clicked.connect(self._update_filters)
        find_row_layout.addWidget(self.btn_regex)
        # --- End Filter Buttons ---

        # --- Navigation Buttons ---
        self.btn_prev = QPushButton(qta.icon('fa5s.arrow-up', color='inherit'), "")
        self.btn_prev.setToolTip("Previous Match (Shift+Enter)")
        self.btn_prev.clicked.connect(self.find_previous)
        self.btn_prev.setShortcut("Shift+Return")
        find_row_layout.addWidget(self.btn_prev)

        self.btn_next = QPushButton(qta.icon('fa5s.arrow-down', color='inherit'), "")
        self.btn_next.setToolTip("Next Match (Enter)")
        self.btn_next.clicked.connect(self.find_next)
        # Enter is handled by find_input.returnPressed
        find_row_layout.addWidget(self.btn_next)
        # --- End Navigation Buttons ---

        # Close Button
        self.btn_close = QPushButton(qta.icon('fa5s.times', color='inherit'), "")
        self.btn_close.setObjectName("CloseButton") # Assign ID for specific hover style
        self.btn_close.setToolTip("Close (Esc)")
        self.btn_close.clicked.connect(self.close_widget)
        self.btn_close.setShortcut(Qt.Key_Escape)
        find_row_layout.addWidget(self.btn_close)

        main_layout.addLayout(find_row_layout) # Add find row to main layout

        # --- Bottom Row (Replace) - Initially Hidden ---
        self.replace_row_widget = QWidget() # Use a container widget
        self.replace_row_widget.setObjectName("ReplaceRowWidget")
        replace_row_layout = QHBoxLayout(self.replace_row_widget)
        replace_row_layout.setContentsMargins(0, 0, 0, 0) # No margins inside replace row
        replace_row_layout.setSpacing(4) # Spacing similar to find row

        # Spacer to align replace input under find input (might need adjustment)
        replace_row_layout.addSpacing(self.btn_toggle_replace.sizeHint().width() + find_row_layout.spacing()) # Match toggle button width + spacing

        # Replace Input
        self.replace_input = QLineEdit()
        self.replace_input.setPlaceholderText("Replace")
        self.replace_input.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)
        replace_row_layout.addWidget(self.replace_input)

        # Replace Buttons
        self.btn_replace = QPushButton(qta.icon('mdi.find-replace', color='inherit'), "")
        self.btn_replace.setToolTip("Replace Current (Ctrl+H)")
        self.btn_replace.clicked.connect(self.replace_current)
        self.btn_replace.setShortcut("Ctrl+H")
        replace_row_layout.addWidget(self.btn_replace)

        self.btn_replace_all = QPushButton(qta.icon('mdi.file-replace-outline', color='inherit'), "")
        self.btn_replace_all.setToolTip("Replace All (Ctrl+Shift+H)")
        self.btn_replace_all.clicked.connect(self.replace_all)
        self.btn_replace_all.setShortcut("Ctrl+Shift+H")
        replace_row_layout.addWidget(self.btn_replace_all)

        # Add dummy widgets to align replace buttons under find navigation/close (approx)
        # Adjust spacing/widgets based on final look
        replace_row_layout.addSpacing(
             self.btn_match_case.sizeHint().width() * 3 + # Space for 3 filters
             self.btn_prev.sizeHint().width() * 2 +       # Space for 2 nav buttons
             self.btn_close.sizeHint().width() +          # Space for close
             find_row_layout.spacing() * 6                 # Approx spacing between those 6 elements
         )

        self.replace_row_widget.hide() # Hide the container
        main_layout.addWidget(self.replace_row_widget) # Add replace row container

    def _update_filters(self):
        """Reads filter button states and triggers a new search."""
        self._match_case = self.btn_match_case.isChecked()
        self._match_whole_word = self.btn_whole_word.isChecked() # Read placeholder state
        self._use_regex = self.btn_regex.isChecked()           # Read placeholder state

        # TODO: Add actual logic for whole word and regex in find_text/replace methods
        if self._match_whole_word or self._use_regex:
            print("Warning: Whole Word / Regex search not fully implemented yet.")

        print(f"Filters updated: Case={self._match_case}, Word={self._match_whole_word}, Regex={self._use_regex}") # Debug
        self.find_text() # Re-run search when filters change


    def schedule_find(self): self.search_timer.start()

    def find_text(self):
        search_term = self.find_input.text()
        
        # Log current profile
        active_profile = getattr(self.main_window.model, 'active_profile_name', 'Unknown')
        
        # Log current match info before clearing
        if self.current_match_index >= 0 and self.current_match_index < len(self.matches):
            current_match = self.matches[self.current_match_index]
        
        self.clear_highlights()
        self.matches = []
        self.current_match_index = -1

        # Update highlighters with current pattern and case sensitivity
        case_sensitive = self._match_case # Use internal state
        valid_highlighters = {}
        for doc, highlighter in self._active_highlighters.items():
            try:
                # Check if document is still valid
                if doc and doc.parent():
                    highlighter.setPattern(search_term, case_sensitive)
                    valid_highlighters[doc] = highlighter
            except (AttributeError, RuntimeError):
                # Document or parent widget has been deleted, skip this highlighter
                continue
        # Update active highlighters to only include valid ones
        self._active_highlighters = valid_highlighters

        if not search_term:
            self.update_match_count_label(); return

        flags = re.NOFLAG if case_sensitive else re.IGNORECASE
        visible_results = [res for res in self.main_window.model.ocr_results if not res.get('is_deleted', False)]

        try:
            # --- Prepare search pattern based on filters ---
            pattern_to_search = search_term
            if not self._use_regex: # If not regex, escape it
                pattern_to_search = re.escape(pattern_to_search)
            if self._match_whole_word: # If whole word, add boundaries (even if regex)
                 # Basic word boundary - might need refinement for complex regex cases
                pattern_to_search = r"\b" + pattern_to_search + r"\b"

            # --- Find matches ---
            # If we're in Original but there's a user edit profile, search in user edit text
            # to show matches in what will actually be replaced
            search_in_user_edit = False
            existing_user_edit = None
            if self.main_window.model.active_profile_name == "Original":
                existing_user_edit = self.main_window.model._find_existing_user_edit_profile()
                if existing_user_edit:
                    search_in_user_edit = True
            
            for result in visible_results:
                # Get text to search - use user edit if we're routing to it, otherwise use display text
                if search_in_user_edit and existing_user_edit:
                    translations = result.get('translations', {})
                    if existing_user_edit in translations:
                        text = translations[existing_user_edit]
                    else:
                        # This row doesn't have user edit yet, search in original
                        text = result.get('text', '')
                else:
                    text = self.main_window.get_display_text(result)
                
                row_number = result.get('row_number')
                filename = result.get('filename')
                if row_number is None: continue
                

                # Find all matches using the constructed pattern
                for match in re.finditer(pattern_to_search, text, flags=flags):
                    start, end = match.span()
                    self.matches.append({
                        'row_number': row_number, 'start': start, 'end': end,
                        'filename': filename, 'text': text
                    })

        except re.error as e:
            print(f"Regex error during find: {e}")
            # Provide visual feedback for invalid regex? (e.g., input border color)
            # For now, just clear matches and show error label
            self.matches = []
            self.match_count_label.setText("Regex Err")
            self.update_match_count_label() # Update button states etc.
            return # Stop processing on regex error


        self.update_match_count_label()
        if self.matches:
            self.current_match_index = 0
            first_match = self.matches[0]
            # Delay highlighting more to ensure widgets are fully created and rendered
            # This is especially important after profile changes when widgets are recreated
            QTimer.singleShot(200, lambda: self.highlight_match(0)) # Highlight first, don't focus

    def update_match_count_label(self):
        has_matches = bool(self.matches)
        count = len(self.matches)
        current_text = self.match_count_label.text() # Get current text to avoid override error message
        if "Err" not in current_text: # Don't overwrite error messages
            if not has_matches: 
                new_text = "No results"
                self.match_count_label.setText(new_text)
            else: 
                current = self.current_match_index + 1
                new_text = f"{current} of {count}"
                self.match_count_label.setText(new_text)

        self.btn_prev.setEnabled(count > 1); self.btn_next.setEnabled(count > 1)
        replace_visible = self.replace_row_widget.isVisible() # Check container widget visibility
        self.btn_replace.setEnabled(has_matches and replace_visible); self.btn_replace_all.setEnabled(has_matches and replace_visible)

    def _get_or_create_highlighter(self, document: QTextDocument) -> SearchHighlighter:
        if document not in self._active_highlighters: self._active_highlighters[document] = SearchHighlighter(document)
        return self._active_highlighters[document]

    # MODIFIED: Updated to access UI elements via self.main_window.results_widget
    def _find_widget_for_match(self, index):
        
        if not (0 <= index < len(self.matches)):
            return None, None, None
        
        match_info = self.matches[index]
        row_number = match_info['row_number']
        
        # Access the modular widget that holds the results UI
        results_widget = self.main_window.results_widget
        if not results_widget:
            return None, None, None
        
        if self.main_window.advanced_mode_check.isChecked():
            table = results_widget.results_table
            for r in range(table.rowCount()):
                 item = table.item(r, 0)
                 try:
                      item_rn = item.data(Qt.UserRole) if item else None
                      if item_rn is not None and float(item_rn) == float(row_number):
                          return 'table', item, table
                 except (ValueError, TypeError): continue
        else:
            # Check if simple_scroll_layout exists to avoid errors
            if not hasattr(results_widget, 'simple_scroll_layout'): 
                return None, None, None
            layout = results_widget.simple_scroll_layout
            for i in range(layout.count()):
                item_layout = layout.itemAt(i)
                container_widget = item_layout.widget() if item_layout else None
                if container_widget:
                    try:
                         widget_row_number = container_widget.property("ocr_row_number")
                         if widget_row_number is not None and float(widget_row_number) == float(row_number):
                             text_edit_found = container_widget.findChild(QTextEdit)
                             if text_edit_found:
                                 return 'simple_text_edit', text_edit_found, container_widget
                    except (ValueError, TypeError) as e:
                        continue
        
        return None, None, None

    # MODIFIED: Updated to access UI elements via self.main_window.results_widget
    def highlight_match(self, index):
        
        if index < 0 or index >= len(self.matches):
            return
        
        match_info = self.matches[index]
        
        widget_type, target_widget, container = self._find_widget_for_match(index)
        
        if widget_type == 'table':
            try:
                table_item = target_widget; table = container
                if table_item and table and table_item.row() >= 0:
                    visible_row_index = table_item.row()
                    table.clearSelection(); table.selectRow(visible_row_index)
                    table.scrollToItem(table_item, QAbstractItemView.ScrollHint.EnsureVisible)
            except (AttributeError, RuntimeError) as e:
                # Widget might have been deleted during profile change
                return
        elif widget_type == 'simple_text_edit':
            try:
                target_text_edit = target_widget; container_widget = container
                if not target_text_edit or not container_widget:
                    return
                
                match_info = self.matches[index]
                start = match_info['start']
                end = match_info['end']
                widget_text = target_text_edit.toPlainText()
                match_text = match_info.get('text', '')
                row_number = match_info.get('row_number')
                
                # CRITICAL: ALWAYS verify match position against widget text (handles race conditions after profile changes)
                # The widget text is the source of truth - it's what's actually displayed to the user
                search_term = self.find_input.text()
                if search_term:
                    flags = re.NOFLAG if self._match_case else re.IGNORECASE
                    pattern_to_search = re.escape(search_term) if not self._use_regex else search_term
                    if self._match_whole_word:
                        pattern_to_search = r"\b" + pattern_to_search + r"\b"
                    
                    # Check if stored position is valid for current widget text
                    stored_position_valid = False
                    if start >= 0 and end <= len(widget_text) and start <= end:
                        widget_at_position = widget_text[start:end]
                        # Check if the text at stored position matches search term
                        match_obj_check = re.search(pattern_to_search, widget_at_position, flags=flags)
                        stored_position_valid = match_obj_check is not None
                    
                    # If stored position is invalid OR widget text differs from match text, re-search
                    if not stored_position_valid or widget_text != match_text:
                        
                        # Re-search for the match in the widget's current text
                        match_obj = re.search(pattern_to_search, widget_text, flags=flags)
                        if match_obj:
                            start, end = match_obj.span()
                            # Update match info with new text and position
                            match_info['text'] = widget_text
                            match_info['start'] = start
                            match_info['end'] = end
                        else:
                            # No match found in widget text - this can happen if profile changed and search term doesn't exist in new text
                            return
                else:
                    return
                
                # Get current profile and check if widget text needs updating
                active_profile_name = None
                model_display_text = None
                try:
                    result_data, result_index = self.main_window.model._find_result_by_row_number(row_number)
                    if result_data:
                        active_profile_name = getattr(self.main_window.model, 'active_profile_name', 'Unknown')
                        # Get what the display text SHOULD be according to model
                        if result_index >= 0 and result_index < len(self.main_window.model.ocr_results):
                            model_result = self.main_window.model.ocr_results[result_index]
                            model_display_text = self.main_window.get_display_text(model_result)
                        else:
                            model_display_text = self.main_window.get_display_text(result_data)
                except Exception as e:
                    pass
                
                # If widget text doesn't match model display text, update widget (handles stale widgets after profile change)
                if model_display_text and widget_text != model_display_text:
                    target_text_edit.blockSignals(True)
                    try:
                        target_text_edit.setPlainText(model_display_text)
                        widget_text = model_display_text
                        # Re-search for match in updated text
                        if search_term:
                            match_obj = re.search(pattern_to_search, widget_text, flags=flags)
                            if match_obj:
                                start, end = match_obj.span()
                                match_info['text'] = widget_text
                                match_info['start'] = start
                                match_info['end'] = end
                            else:
                                target_text_edit.blockSignals(False)
                                return
                    finally:
                        target_text_edit.blockSignals(False)
                
                
                # Block signals during highlighting to prevent textChanged from triggering update_ocr_text
                self._is_highlighting = True
                # Store original text to detect if it actually changed
                original_text = widget_text
                target_text_edit.blockSignals(True)
                try:
                    doc = target_text_edit.document()
                    if doc and doc.parent():
                        highlighter = self._get_or_create_highlighter(doc)
                        highlighter.setPattern(self.find_input.text(), self._match_case) # Ensure pattern set
                        cursor = target_text_edit.textCursor()
                        cursor.setPosition(start)
                        cursor.setPosition(end, QTextCursor.MoveMode.KeepAnchor)
                        target_text_edit.setTextCursor(cursor)
                except Exception as e:
                    pass
                finally:
                    target_text_edit.blockSignals(False)
                    # Store the text that was there before highlighting to detect false changes
                    if not hasattr(self, '_highlighting_text_cache'):
                        self._highlighting_text_cache = {}
                    self._highlighting_text_cache[id(target_text_edit)] = original_text
                    # Use QTimer to reset flag after Qt processes any queued events (longer delay to catch queued textChanged)
                    QTimer.singleShot(200, lambda: self._clear_highlighting_flag(id(target_text_edit)))
                # Access the scroll area within the results_widget
                if hasattr(self.main_window, 'results_widget') and self.main_window.results_widget:
                    if hasattr(self.main_window.results_widget, 'simple_scroll'):
                        self.main_window.results_widget.simple_scroll.ensureWidgetVisible(container_widget)
            except (AttributeError, RuntimeError) as e:
                # Widget might have been deleted during profile change
                return
        
        self.update_match_count_label()

    def _clear_highlighting_flag(self, widget_id):
        """Clears the highlighting flag and cache for a specific widget."""
        if widget_id in self._highlighting_text_cache:
            del self._highlighting_text_cache[widget_id]
        self._is_highlighting = False
    
    def focus_current_match(self):
        widget_type, target_widget, container = self._find_widget_for_match(self.current_match_index)
        if widget_type == 'table': 
            container.setFocus()
        elif widget_type == 'simple_text_edit': 
            # Don't set focus on the text edit - keep focus on find input so Enter continues to work
            # Just scroll to the match without focusing it
            pass
        # Always keep focus on find input so Enter continues to trigger find_next
        self.find_input.setFocus()

    # MODIFIED: Updated to access UI elements via self.main_window.results_widget
    def clear_highlights(self):
        # Safely clear table selection if it exists
        try:
            if hasattr(self.main_window, 'results_widget') and self.main_window.results_widget:
                if hasattr(self.main_window.results_widget, 'results_table') and self.main_window.results_widget.results_table:
                    self.main_window.results_widget.results_table.clearSelection()
        except (AttributeError, RuntimeError):
            # Widget might be in the process of being deleted
            pass
        
        # Safely clear highlighters, filtering out those with invalid documents
        valid_highlighters = {}
        for doc, highlighter in self._active_highlighters.items():
            try:
                # Check if document is still valid by trying to access it
                if doc and doc.parent():
                    highlighter.setPattern("", self._match_case)
                    valid_highlighters[doc] = highlighter
            except (AttributeError, RuntimeError):
                # Document or parent widget has been deleted, skip this highlighter
                continue
        
        # Update active highlighters to only include valid ones
        self._active_highlighters = valid_highlighters

    def find_next(self):
        if not self.matches:
            return
        # Set flag before highlighting to prevent textChanged from triggering updates
        self._is_highlighting = True
        old_index = self.current_match_index
        self.current_match_index = (self.current_match_index + 1) % len(self.matches)
        if self.current_match_index < len(self.matches):
            match_info = self.matches[self.current_match_index]
        self.highlight_match(self.current_match_index)
        self.focus_current_match()

    def find_previous(self):
        if not self.matches:
            return
        # Set flag before highlighting to prevent textChanged from triggering updates
        self._is_highlighting = True
        old_index = self.current_match_index
        self.current_match_index = (self.current_match_index - 1 + len(self.matches)) % len(self.matches)
        if self.current_match_index < len(self.matches):
            match_info = self.matches[self.current_match_index]
        self.highlight_match(self.current_match_index)
        self.focus_current_match()


    # --- Replace Methods - Update to use filter states ---
    def replace_current(self):
        # Basic replace, doesn't handle regex groups etc.
        if not (0 <= self.current_match_index < len(self.matches)) or not self.replace_row_widget.isVisible(): return
        match_info = self.matches[self.current_match_index]; row_number = match_info['row_number']
        start = match_info['start']; end = match_info['end']
        result_to_update, _ = self.main_window._find_result_by_row_number(row_number)
        if not result_to_update or result_to_update.get('is_deleted', False): self.find_text(); return
        
        # Get the text we should be working with (user edit if exists, otherwise original)
        # We need to check if there's a user edit profile and use that text for replacement
        current_text = self.main_window.get_display_text(result_to_update)
        search_term = self.find_input.text()
        replace_term = self.replace_input.text()
        
        # Check if we're in Original but there's a user edit profile - we should use that text
        if self.main_window.model.active_profile_name == "Original":
            existing_user_edit = self.main_window.model._find_existing_user_edit_profile()
            if existing_user_edit:
                # Get the text from the user edit profile for this result
                translations = result_to_update.get('translations', {})
                if existing_user_edit in translations:
                    current_text = translations[existing_user_edit]
                    # Re-verify match position against the user edit text
                    # The match might be at a slightly different position due to previous edits
                    flags = re.NOFLAG if self._match_case else re.IGNORECASE
                    pattern_to_search = re.escape(search_term) if not self._use_regex else search_term
                    if self._match_whole_word:
                        pattern_to_search = r"\b" + pattern_to_search + r"\b"
                    
                    # Try to find the match in the user edit text
                    match_obj = re.search(pattern_to_search, current_text, flags=flags)
                    if match_obj:
                        start, end = match_obj.span()
                    else:
                        # Match not found in user edit text - might have been edited differently
                        # Try to find a similar match near the original position
                        # Check if the text at the stored position is close to the search term
                        if start < len(current_text) and end <= len(current_text):
                            text_at_position = current_text[start:end]
                            # If the text at position doesn't match, try searching nearby
                            search_start = max(0, start - len(search_term))
                            search_end = min(len(current_text), end + len(search_term))
                            nearby_text = current_text[search_start:search_end]
                            match_obj = re.search(pattern_to_search, nearby_text, flags=flags)
                            if match_obj:
                                start = search_start + match_obj.start()
                                end = search_start + match_obj.end()
                            else:
                                # Couldn't find match - refresh search and return
                                self.find_text()
                                return
        
        # Verify match position is still valid
        if start < 0 or end > len(current_text) or start > end:
            self.find_text()
            return
        
        # Perform the replacement
        search_term_len = end - start
        if start < len(current_text) and start + search_term_len <= len(current_text):
            new_text = current_text[:start] + replace_term + current_text[start + search_term_len:]
        else:
            self.find_text()
            return
        
        # Check if profile was created (for programmatic updates, we need to update selector but not show message)
        was_original = self.main_window.model.active_profile_name == "Original"
        result = self.main_window.model.update_text(row_number, new_text, is_user_edit=False)
        # If profile was created, update selector but don't show message
        # Format: (error, success, profile_created, should_show_message)
        if len(result) == 4 and result[2] and was_original:  # profile_created is True
            self.main_window.model.profiles_updated.emit()
        self.find_text() # Re-run find

    def replace_all(self):
        if not self.matches or not self.replace_row_widget.isVisible(): return # Check container widget

        search_term = self.find_input.text()
        if not search_term: return # Don't replace empty string

        replace_term = self.replace_input.text()
        case_sensitive = self._match_case # Use internal state
        flags = re.NOFLAG if case_sensitive else re.IGNORECASE

        replaced_count = 0; rows_updated = set()
        visible_results_indices = [idx for idx, res in enumerate(self.main_window.model.ocr_results) if not res.get('is_deleted', False)]

        try:
            # --- Prepare search pattern based on filters ---
            pattern_to_search = search_term
            if not self._use_regex: pattern_to_search = re.escape(pattern_to_search)
            if self._match_whole_word: pattern_to_search = r"\b" + pattern_to_search + r"\b"

            # --- Perform replacement ---
            was_original = self.main_window.model.active_profile_name == "Original"
            profile_created = False
            # Check if we're in Original but there's a user edit profile - we should use that text
            existing_user_edit = None
            if was_original:
                existing_user_edit = self.main_window.model._find_existing_user_edit_profile()
            
            for data_index in visible_results_indices:
                 result_to_update = self.main_window.model.ocr_results[data_index]
                 row_number = result_to_update['row_number']
                 
                 # Get the text to work with - use user edit if exists, otherwise display text
                 text_to_replace = self.main_window.get_display_text(result_to_update)
                 if was_original and existing_user_edit:
                     # Check if this result has a translation in the user edit profile
                     translations = result_to_update.get('translations', {})
                     if existing_user_edit in translations:
                         text_to_replace = translations[existing_user_edit]

                 # Use re.subn which counts replacements
                 new_text, num_subs = re.subn(pattern_to_search, replace_term, text_to_replace, flags=flags)

                 if num_subs > 0:
                     result = self.main_window.model.update_text(row_number, new_text, is_user_edit=False)
                     # Check if profile was created (only on first update if in Original)
                     # Format: (error, success, profile_created, should_show_message)
                     if len(result) == 4 and result[2] and was_original and not profile_created:  # profile_created is True
                         profile_created = True
                     replaced_count += num_subs; rows_updated.add(row_number)
            
            # If profile was created, update selector but don't show message
            if profile_created:
                self.main_window.model.profiles_updated.emit()

        except re.error as e:
            print(f"Replace All: Regex error: {e}")
            # self.main_window.show_error_message("Replace All Error", f"Invalid search pattern: {e}")
            # Indicate error in UI?
            self.match_count_label.setText("Regex Err")
            return

        print(f"Replace All: Replaced {replaced_count} occurrences in {len(rows_updated)} rows.")
        self.matches = []; self.current_match_index = -1
        self.clear_highlights(); self.update_match_count_label()

    # MODIFIED: Updated to access UI elements via self.main_window.results_widget
    def _update_ui_text(self, row_number, new_text):
        # Access the modular widget that holds the results UI
        results_widget = self.main_window.results_widget

        if self.main_window.advanced_mode_check.isChecked():
            table = results_widget.results_table
            for r in range(table.rowCount()):
                 item = table.item(r, 0)
                 try:
                      item_rn = item.data(Qt.UserRole) if item else None
                      if item_rn is not None and float(item_rn) == float(row_number):
                          table.blockSignals(True); item.setText(new_text); table.blockSignals(False); break
                 except (ValueError, TypeError): continue
        else:
            # Check if simple_scroll_layout exists to avoid errors
            if not hasattr(results_widget, 'simple_scroll_layout'): return
            layout = results_widget.simple_scroll_layout
            for i in range(layout.count()):
                item_layout = layout.itemAt(i); container_widget = item_layout.widget() if item_layout else None
                if container_widget:
                    try:
                         widget_row_number = container_widget.property("ocr_row_number")
                         if widget_row_number is not None and float(widget_row_number) == float(row_number):
                             target_text_edit = container_widget.findChild(QTextEdit)
                             if target_text_edit:
                                 target_text_edit.blockSignals(True); target_text_edit.setText(new_text); target_text_edit.blockSignals(False)
                             break
                    except (ValueError, TypeError): continue

    def toggle_replace_visible(self, checked):
        self.replace_row_widget.setVisible(checked) # Show/hide the container
        icon_name = 'fa5s.chevron-down' if checked else 'fa5s.chevron-right'
        self.btn_toggle_replace.setIcon(qta.icon(icon_name, color='inherit'))
        self.update_match_count_label() # Update replace button states

    def showEvent(self, event):
        super().showEvent(event); self.find_input.setFocus(); self.find_input.selectAll()
        self.find_text()

    def hideEvent(self, event):
        super().hideEvent(event)
        self.clear_highlights(); self.matches = []; self.current_match_index = -1
        self.update_match_count_label()

    def focus_find_input(self):
        self.show(); self.raise_()
        self.find_input.setFocus(); self.find_input.selectAll()

    def close_widget(self):
        self.hide(); self.closed.emit()

    def on_profile_changed(self):
        """Handles profile changes by completely resetting find/replace state.
        
        When profile changes, all text changes, so we need to:
        1. Clear all highlights and matches (they're invalid now)
        2. Clear all highlighters (widgets will be recreated)
        3. Let widgets be destroyed and recreated with new profile text
        4. Run find_text() again on the new widgets (handled by update_views)
        """
        # Get profile info before clearing
        # Note: The model.active_profile_name may already be changed, so we can't reliably get the old profile name here
        old_match_index = self.current_match_index
        old_matches_count = len(self.matches)
        old_match_row = None
        old_match_info = None
        translations_before = None
        if old_match_index >= 0 and old_match_index < len(self.matches):
            old_match_row = self.matches[old_match_index].get('row_number')
            old_match_info = f"index={old_match_index}, row={old_match_row}, start={self.matches[old_match_index].get('start')}, end={self.matches[old_match_index].get('end')}"
        
        # Check translations BEFORE profile change
        if old_match_row is not None:
            result_data, result_index = self.main_window.model._find_result_by_row_number(old_match_row)
            if result_data and result_index >= 0:
                translations_before = result_data.get('translations', {})
        
        # Clear all matches - they're invalid for the new profile
        self.matches = []
        self.current_match_index = -1
        
        # Clear all highlighters - widgets will be destroyed and recreated anyway
        for doc, highlighter in list(self._active_highlighters.items()):
            try:
                if highlighter:
                    highlighter.setPattern("", self._match_case)
            except (AttributeError, RuntimeError) as e:
                pass
        self._active_highlighters.clear()
        
        # Clear table selection if it exists
        try:
            if hasattr(self.main_window, 'results_widget') and self.main_window.results_widget:
                if hasattr(self.main_window.results_widget, 'results_table'):
                    self.main_window.results_widget.results_table.clearSelection()
        except (AttributeError, RuntimeError) as e:
            pass
        
        # Clear match count label
        self.match_count_label.setText("No results")
        
        
        # Note: find_text() will be called by results_widget.update_views() after widgets are recreated
        # This ensures we search the new profile text with the new widgets