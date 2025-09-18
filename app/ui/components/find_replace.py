# --- START OF FILE app/find_replace.py ---

from PySide6.QtWidgets import (QWidget, QHBoxLayout, QVBoxLayout, QLineEdit, QPushButton, QLabel, QTextEdit, QCheckBox, QSizePolicy, QAbstractItemView, QFrame) # Added QFrame
from PySide6.QtCore import Qt, Signal, QTimer
from PySide6.QtGui import QTextCursor, QTextCharFormat, QColor, QSyntaxHighlighter, QFont, QTextDocument
import qtawesome as qta
import re
from assets import FIND_REPLACE_STYLESHEET

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

        # Filter states (add more as needed)
        self._match_case = False
        self._match_whole_word = False # Placeholder
        self._use_regex = False        # Placeholder

        self._init_ui()
        self.hide()
        self.setStyleSheet(FIND_REPLACE_STYLESHEET)

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
        self.btn_prev.setStyleSheet(FIND_REPLACE_STYLESHEET)
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
        self.clear_highlights()
        self.matches = []
        self.current_match_index = -1

        # Update highlighters with current pattern and case sensitivity
        case_sensitive = self._match_case # Use internal state
        for highlighter in self._active_highlighters.values():
            highlighter.setPattern(search_term, case_sensitive)

        if not search_term:
            self.update_match_count_label(); return

        flags = re.NOFLAG if case_sensitive else re.IGNORECASE
        visible_results = [res for res in self.main_window.ocr_results if not res.get('is_deleted', False)]

        try:
            # --- Prepare search pattern based on filters ---
            pattern_to_search = search_term
            if not self._use_regex: # If not regex, escape it
                pattern_to_search = re.escape(pattern_to_search)
            if self._match_whole_word: # If whole word, add boundaries (even if regex)
                 # Basic word boundary - might need refinement for complex regex cases
                pattern_to_search = r"\b" + pattern_to_search + r"\b"

            # --- Find matches ---
            for result in visible_results:
                text = result.get('text', '')
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
            self.highlight_match(0) # Highlight first, don't focus

    def update_match_count_label(self):
        has_matches = bool(self.matches)
        count = len(self.matches)
        current_text = self.match_count_label.text() # Get current text to avoid override error message
        if "Err" not in current_text: # Don't overwrite error messages
            if not has_matches: self.match_count_label.setText("No results")
            else: current = self.current_match_index + 1; self.match_count_label.setText(f"{current} of {count}")

        self.btn_prev.setEnabled(count > 1); self.btn_next.setEnabled(count > 1)
        replace_visible = self.replace_row_widget.isVisible() # Check container widget visibility
        self.btn_replace.setEnabled(has_matches and replace_visible); self.btn_replace_all.setEnabled(has_matches and replace_visible)

    def _get_or_create_highlighter(self, document: QTextDocument) -> SearchHighlighter:
        if document not in self._active_highlighters: self._active_highlighters[document] = SearchHighlighter(document)
        return self._active_highlighters[document]

    # MODIFIED: Updated to access UI elements via self.main_window.results_widget
    def _find_widget_for_match(self, index):
        if not (0 <= index < len(self.matches)): return None, None, None
        match_info = self.matches[index]; row_number = match_info['row_number']
        
        # Access the modular widget that holds the results UI
        results_widget = self.main_window.results_widget
        
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
            if not hasattr(results_widget, 'simple_scroll_layout'): return None, None, None
            layout = results_widget.simple_scroll_layout
            for i in range(layout.count()):
                item_layout = layout.itemAt(i); container_widget = item_layout.widget() if item_layout else None
                if container_widget:
                    try:
                         widget_row_number = container_widget.property("ocr_row_number")
                         if widget_row_number is not None and float(widget_row_number) == float(row_number):
                             text_edit_found = container_widget.findChild(QTextEdit)
                             if text_edit_found: return 'simple_text_edit', text_edit_found, container_widget
                    except (ValueError, TypeError): continue
        return None, None, None

    # MODIFIED: Updated to access UI elements via self.main_window.results_widget
    def highlight_match(self, index):
        widget_type, target_widget, container = self._find_widget_for_match(index)
        if widget_type == 'table':
            table_item = target_widget; table = container; visible_row_index = table_item.row()
            table.clearSelection(); table.selectRow(visible_row_index)
            table.scrollToItem(table_item, QAbstractItemView.ScrollHint.EnsureVisible)
        elif widget_type == 'simple_text_edit':
            target_text_edit = target_widget; container_widget = container
            match_info = self.matches[index]; start = match_info['start']; end = match_info['end']
            highlighter = self._get_or_create_highlighter(target_text_edit.document())
            highlighter.setPattern(self.find_input.text(), self._match_case) # Ensure pattern set
            cursor = target_text_edit.textCursor(); cursor.setPosition(start); cursor.setPosition(end, QTextCursor.MoveMode.KeepAnchor)
            target_text_edit.setTextCursor(cursor)
            # Access the scroll area within the results_widget
            self.main_window.results_widget.simple_scroll.ensureWidgetVisible(container_widget)
        self.update_match_count_label()

    def focus_current_match(self):
        widget_type, target_widget, container = self._find_widget_for_match(self.current_match_index)
        if widget_type == 'table': container.setFocus()
        elif widget_type == 'simple_text_edit': target_widget.setFocus()
        else: self.find_input.setFocus()

    # MODIFIED: Updated to access UI elements via self.main_window.results_widget
    def clear_highlights(self):
        # Access the results_table within the results_widget
        self.main_window.results_widget.results_table.clearSelection()
        for highlighter in self._active_highlighters.values(): highlighter.setPattern("", self._match_case)

    def find_next(self):
        if not self.matches: return
        self.current_match_index = (self.current_match_index + 1) % len(self.matches)
        self.highlight_match(self.current_match_index); self.focus_current_match()

    def find_previous(self):
        if not self.matches: return
        self.current_match_index = (self.current_match_index - 1 + len(self.matches)) % len(self.matches)
        self.highlight_match(self.current_match_index); self.focus_current_match()


    # --- Replace Methods - Update to use filter states ---
    def replace_current(self):
        # Basic replace, doesn't handle regex groups etc.
        if not (0 <= self.current_match_index < len(self.matches)) or not self.replace_row_widget.isVisible(): return
        match_info = self.matches[self.current_match_index]; row_number = match_info['row_number']
        start = match_info['start']; end = match_info['end']
        result_to_update, _ = self.main_window._find_result_by_row_number(row_number)
        if not result_to_update or result_to_update.get('is_deleted', False): self.find_text(); return
        current_text = result_to_update['text']; search_term_len = end - start; replace_term = self.replace_input.text()
        if start < len(current_text) and start + search_term_len <= len(current_text):
            new_text = current_text[:start] + replace_term + current_text[start + search_term_len:]
        else: self.find_text(); return
        result_to_update['text'] = new_text
        self._update_ui_text(row_number, new_text)
        self.find_text() # Re-run find

    def replace_all(self):
        if not self.matches or not self.replace_row_widget.isVisible(): return # Check container widget

        search_term = self.find_input.text()
        if not search_term: return # Don't replace empty string

        replace_term = self.replace_input.text()
        case_sensitive = self._match_case # Use internal state
        flags = re.NOFLAG if case_sensitive else re.IGNORECASE

        replaced_count = 0; rows_updated = set()
        visible_results_indices = [idx for idx, res in enumerate(self.main_window.ocr_results) if not res.get('is_deleted', False)]

        try:
            # --- Prepare search pattern based on filters ---
            pattern_to_search = search_term
            if not self._use_regex: pattern_to_search = re.escape(pattern_to_search)
            if self._match_whole_word: pattern_to_search = r"\b" + pattern_to_search + r"\b"

            # --- Perform replacement ---
            for data_index in visible_results_indices:
                 result_to_update = self.main_window.ocr_results[data_index]
                 original_text = result_to_update['text']; row_number = result_to_update['row_number']

                 # Use re.subn which counts replacements
                 new_text, num_subs = re.subn(pattern_to_search, replace_term, original_text, flags=flags)

                 if num_subs > 0:
                     result_to_update['text'] = new_text; replaced_count += num_subs; rows_updated.add(row_number)
                     self._update_ui_text(row_number, new_text) # Update UI per row

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