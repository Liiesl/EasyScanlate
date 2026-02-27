from PySide6.QtWidgets import (QWidget, QVBoxLayout, QHBoxLayout, QFrame, QScrollArea, QPushButton, QTextEdit, QScroller)
from PySide6.QtCore import Qt, QPropertyAnimation, QEasingCurve, Signal, QEvent, QTimer
from PySide6.QtGui import QFont, QWheelEvent
import qtawesome as qta
import random
from assets.styles import SIMPLE_VIEW_STYLES

class AutoResizingTextEdit(QTextEdit):
    focusReceived = Signal()
    def __init__(self, text, parent=None):
        super().__init__(text, parent)

        self.setObjectName("SimpleViewTextEdit")
        self.setAcceptRichText(False)
        self.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setTabChangesFocus(True) 
        self.textChanged.connect(self.adjust_height)
        
    def wheelEvent(self, event: QWheelEvent):
        # Ignore wheel events so they propagate to the SmoothScrollArea
        event.ignore()

    def adjust_height(self):
        doc = self.document()
        doc.setTextWidth(self.viewport().width())
        new_height = doc.size().height() + 2 
        self.setFixedHeight(int(new_height))
        self.verticalScrollBar().setValue(0)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self.adjust_height()

    def focusInEvent(self, event):
        super().focusInEvent(event)
        self.focusReceived.emit()

class SmoothScrollArea(QScrollArea):
    """A ScrollArea with animated smooth scrolling."""
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("SimpleViewScroll")
        self.setWidgetResizable(True)
        self._scroll_anim = QPropertyAnimation(self.verticalScrollBar(), b"value")
        self._scroll_anim.setDuration(250) # Duration in ms
        self._scroll_anim.setEasingCurve(QEasingCurve.OutCubic)

    def wheelEvent(self, event: QWheelEvent):
        # Calculate new scroll position
        delta = event.angleDelta().y()
        current_value = self.verticalScrollBar().value()
        target_value = current_value - (delta * 2) # Multiplier for speed

        # Clamp target value
        target_value = max(self.verticalScrollBar().minimum(), 
                           min(target_value, self.verticalScrollBar().maximum()))

        # Animate to new position
        self._scroll_anim.stop()
        self._scroll_anim.setStartValue(current_value)
        self._scroll_anim.setEndValue(target_value)
        self._scroll_anim.start()

class ResultRow(QFrame):
    rowDeleted = Signal(float)
    textChanged = Signal(float, str)
    rowSelected = Signal(float)
    
    def __init__(self, row_number, text, is_last=False):
        super().__init__()
        self.row_number = row_number
        self.setObjectName("ResultRow")
        self.setProperty("isLast", "true" if is_last else "false")
        
        layout = QHBoxLayout(self)
        layout.setContentsMargins(20, 18, 15, 18)
        layout.setSpacing(10)

        self.text_edit = AutoResizingTextEdit(text)
        # Block signals initially to prevent initial resize triggering update
        
        btn_layout = QHBoxLayout()
        btn_layout.setSpacing(8)

        self.delete_btn = QPushButton(qta.icon('fa5s.trash-alt', color='#FF453A'), "")
        self.delete_btn.setObjectName("ActionButton")
        self.delete_btn.setFixedSize(34, 34)
        self.delete_btn.clicked.connect(self._on_delete)
        
        self.sync_btn = QPushButton(qta.icon('fa5s.sync-alt', color='#FFFFFF'), "")
        self.sync_btn.setObjectName("ActionButton")
        self.sync_btn.setFixedSize(34, 34)

        btn_layout.addWidget(self.delete_btn)
        btn_layout.addWidget(self.sync_btn)

        layout.addWidget(self.text_edit, 1)
        layout.addLayout(btn_layout)
        
        self.text_edit.textChanged.connect(self._on_text_changed)
        self.text_edit.focusReceived.connect(self._on_focus_received)
    
    def _on_focus_received(self):
        self.rowSelected.emit(self.row_number)

    def _on_delete(self):
        self.rowDeleted.emit(self.row_number)

    def _on_text_changed(self):
        self.textChanged.emit(self.row_number, self.text_edit.toPlainText())

class SimpleView(QWidget):
    rowDeleted = Signal(float)
    textChanged = Signal(float, str)
    rowSelected = Signal(float)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("MainBackground")
        self.setStyleSheet(SIMPLE_VIEW_STYLES)
        
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)

        # Using our custom smooth scroll area
        self.scroll = SmoothScrollArea()
        
        # Also enable "Kinetic" flick scrolling
        QScroller.grabGesture(self.scroll.viewport(), QScroller.LeftMouseButtonGesture)
        
        self.card_container = QFrame()
        self.card_container.setObjectName("CardContainer")
        self.card_layout = QVBoxLayout(self.card_container)
        self.card_layout.setContentsMargins(0, 5, 0, 5)
        self.card_layout.setSpacing(0)
        
        self.card_layout.addStretch(1)
        
        self.scroll.setWidget(self.card_container)
        main_layout.addWidget(self.scroll)
        
        self.rows = {}

    def populate(self, results, get_display_text_func):
        # Clear existing rows
        self._clear_rows()
        
        valid_results = [r for r in results if not r.get('is_deleted', False)]
        
        for i, result in enumerate(valid_results):
            row_number = result['row_number']
            text = get_display_text_func(result)
            is_last = (i == len(valid_results) - 1)
            
            row = ResultRow(row_number, text, is_last)
            row.rowDeleted.connect(self.rowDeleted)
            row.textChanged.connect(self.textChanged)
            row.rowSelected.connect(self.rowSelected)
            
            # Insert before the stretch item
            self.card_layout.insertWidget(self.card_layout.count() - 1, row)
            self.rows[row_number] = row

    def update_text(self, row_number, new_text):
        if row_number in self.rows:
            row = self.rows[row_number]
            if row.text_edit.toPlainText() != new_text:
                row.text_edit.blockSignals(True)
                row.text_edit.setText(new_text)
                row.text_edit.blockSignals(False)
                row.text_edit.adjust_height()

    def scroll_to_row(self, row_number):
        if row_number in self.rows:
            row = self.rows[row_number]
            self.scroll.ensureWidgetVisible(row)

    def _clear_rows(self):
        while self.card_layout.count() > 1: # Keep the stretch
            item = self.card_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()
        self.rows.clear()
