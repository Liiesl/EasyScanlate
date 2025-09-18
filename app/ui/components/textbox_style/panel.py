import os
import json
import functools
from PySide6.QtWidgets import (QWidget, QVBoxLayout, QLabel, QPushButton, QFrame,
                             QGroupBox, QHBoxLayout, QScrollArea)
from PySide6.QtCore import Qt, Signal, QSettings
from PySide6.QtGui import QColor
import qtawesome as qta
from app.ui.components import PresetButton
# TEXT_BOX_STYLE_PANEL_STYLESHEET is no longer needed here
from assets import DEFAULT_GRADIENT, TEXT_BOX_STYLE_PANEL_STYLE
from app.ui.dialogs import CustomColorDialog
from .shape_panel import ShapeStylePanel
from .typography_panel import TypographyStylePanel

def get_style_diff(style_dict, base_style_dict):
    """Compares a style dict to a base and returns only the changed values."""
    diff = {}
    for key, value in style_dict.items():
        if key not in base_style_dict or base_style_dict[key] != value:
            if isinstance(value, dict) and key in base_style_dict and isinstance(base_style_dict[key], dict):
                nested_diff = get_style_diff(value, base_style_dict[key])
                if nested_diff:
                    diff[key] = nested_diff
            else:
                diff[key] = value
    return diff

class TextBoxStylePanel(QWidget):
    """
    An orchestrator panel that combines ShapeStylePanel and TypographyStylePanel
    to customize the appearance of a TextBoxItem. It manages style state,
    presets, and communication with the main application.
    """
    style_changed = Signal(dict)

    def __init__(self, parent=None, default_style=None):
        super().__init__(parent)
        self.setObjectName("TextBoxStylePanel")
        self.setMinimumWidth(400)
        self.settings = QSettings("YourCompany", "MangaOCRTool")
        self.presets = []
        self._original_default_style = default_style if default_style else {}
        self._default_style = self._ensure_gradient_defaults(self._original_default_style)
        self._updating_controls = False
        self.selected_style_info = None
        
        self.init_ui()
        self.update_style_panel(self._default_style)
        self._load_presets()

    def _ensure_gradient_defaults(self, style_dict):
        """Ensures a style dictionary has default gradient fields."""
        style = style_dict.copy() if style_dict else {}
        # Fill defaults
        if 'fill_type' not in style: style['fill_type'] = 'solid'
        if 'bg_color' not in style: style['bg_color'] = '#ffffffff'
        if 'bg_gradient' not in style: style['bg_gradient'] = {}
        style['bg_gradient'] = {**DEFAULT_GRADIENT, **style['bg_gradient']}
        # Text defaults
        if 'text_color_type' not in style: style['text_color_type'] = 'solid'
        if 'text_color' not in style: style['text_color'] = '#ff000000'
        if 'text_gradient' not in style: style['text_gradient'] = {}
        style['text_gradient'] = {**DEFAULT_GRADIENT, **style['text_gradient']}
        # Font style default
        if 'font_style' not in style: style['font_style'] = 'Regular'
        return style

    def init_ui(self):
        """Initializes the main panel UI, creating and arranging sub-panels."""
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(15, 15, 15, 15)
        main_layout.setSpacing(10)
        main_layout.setAlignment(Qt.AlignTop)

        # --- Header ---
        header_layout = QHBoxLayout()
        title_label = QLabel("Text Box Styles")
        title_label.setObjectName("panelTitle")
        header_layout.addWidget(title_label)
        main_layout.addLayout(header_layout)
        header_divider = QFrame()
        header_divider.setObjectName("headerDivider")
        header_divider.setFrameShape(QFrame.HLine)
        header_divider.setFrameShadow(QFrame.Plain)
        main_layout.addWidget(header_divider)

        # --- Scroll Area Setup ---
        scroll_area = QScrollArea()
        scroll_area.setObjectName("styleScrollArea")
        scroll_area.setWidgetResizable(True)
        scroll_area.setFrameShape(QFrame.NoFrame)
        scroll_content = QWidget()
        scroll_layout = QVBoxLayout(scroll_content)
        scroll_layout.setContentsMargins(0, 5, 5, 5)
        scroll_layout.setSpacing(12)

        # --- Instantiate and Add Sub-Panels ---
        self.shape_panel = ShapeStylePanel(color_chooser_fn=self.choose_color)
        self.typography_panel = TypographyStylePanel(color_chooser_fn=self.choose_color)
        
        self.shape_panel.style_changed.connect(self.style_changed_handler)
        self.typography_panel.style_changed.connect(self.style_changed_handler)
        
        scroll_layout.addWidget(self.shape_panel)
        scroll_layout.addWidget(self.typography_panel)
        # --- Presets Group (Inside Scroll Area) ---
        presets_group = QGroupBox("Presets")
        presets_group.setObjectName("styleGroup")
        presets_main_layout = QHBoxLayout(presets_group)
        presets_main_layout.setContentsMargins(10, 15, 10, 10)
        preset_scroll_area = QScrollArea()
        preset_scroll_area.setWidgetResizable(True)
        preset_scroll_area.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        preset_scroll_area.setHorizontalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        preset_scroll_area.setFrameShape(QFrame.NoFrame)
        preset_scroll_area.setStyleSheet("background-color: transparent;")
        self.preset_buttons_container = QWidget()
        self.presets_buttons_layout = QHBoxLayout(self.preset_buttons_container)
        self.presets_buttons_layout.setContentsMargins(0, 0, 0, 0)
        self.presets_buttons_layout.setSpacing(6)
        preset_scroll_area.setWidget(self.preset_buttons_container)
        presets_main_layout.addWidget(preset_scroll_area)
        self.btn_add_preset = QPushButton(qta.icon('fa5s.plus'), "")
        self.btn_add_preset.setToolTip("Save current style as a new preset")
        self.btn_add_preset.setFixedSize(48, 48)
        self.btn_add_preset.setObjectName("addPresetButton")
        self.btn_add_preset.clicked.connect(self._add_preset)
        presets_main_layout.addWidget(self.btn_add_preset)
        scroll_layout.addWidget(presets_group)

        # --- Finish Scroll Area ---
        scroll_layout.addStretch()
        scroll_area.setWidget(scroll_content)
        main_layout.addWidget(scroll_area)

        # --- Button Bar ---
        button_container = QWidget()
        button_container.setObjectName("buttonContainer")
        button_layout = QHBoxLayout(button_container)
        button_layout.setContentsMargins(0, 10, 0, 0)
        self.btn_reset = QPushButton("Reset")
        self.btn_reset.setObjectName("resetButton")
        self.btn_reset.clicked.connect(self.reset_style)
        button_layout.addWidget(self.btn_reset)
        button_layout.addSpacing(10)
        self.btn_apply = QPushButton("Apply")
        self.btn_apply.setObjectName("applyButton")
        self.btn_apply.clicked.connect(self.apply_style)
        button_layout.addWidget(self.btn_apply)

        # The large stylesheet is removed from here. Only panel-specific styles remain.
        self.setStyleSheet(TEXT_BOX_STYLE_PANEL_STYLE)

    def get_current_style(self):
        """
        Aggregates style dictionaries from sub-panels into a single dictionary.
        
        Returns:
            dict: The complete, current style dictionary.
        """
        shape_style = self.shape_panel.get_style()
        
        default_font = self._default_style.get('font_family', "Arial")
        default_font_style = self._default_style.get('font_style', "Regular")
        typography_style = self.typography_panel.get_style(default_font, default_font_style)
        
        # Merge the dictionaries
        full_style = {**shape_style, **typography_style}
        return full_style

    def update_style_panel(self, style_dict_in):
        """
        Populates all UI controls from a single style dictionary by delegating
        to the appropriate sub-panels.
        
        Args:
            style_dict_in (dict): The style dictionary to apply.
        """
        if not style_dict_in:
            style_dict = self._default_style
        else:
            style_dict = self._ensure_gradient_defaults(style_dict_in)
            
        self._updating_controls = True
        self.selected_style_info = style_dict

        self.shape_panel.set_style(style_dict, DEFAULT_GRADIENT)
        self.typography_panel.set_style(style_dict, DEFAULT_GRADIENT)

        self._updating_controls = False
        if style_dict_in:
            self.show()

    def style_changed_handler(self):
        """Handles the style_changed signal from sub-panels."""
        if not self._updating_controls:
            current_style = self.get_current_style()
            self.style_changed.emit(current_style)

    def apply_style(self):
        """Emits the current style to be applied."""
        self.style_changed.emit(self.get_current_style())

    def reset_style(self):
        """Resets the style to the initial default and updates the UI."""
        self.update_style_panel(self._default_style)
        self.style_changed_handler()

    def choose_color(self, button):
        """
        Opens a custom color dialog and applies the chosen color to the button.
        This method is passed to sub-panels to handle their color buttons.
        """
        style = button.styleSheet()
        try:
            start = style.find("background-color:") + len("background-color:")
            end = style.find(";", start)
            current_color = QColor(style[start:end].strip())
        except:
            current_color = QColor(0, 0, 0)
            
        color = CustomColorDialog.getColor(initial_color=current_color, parent=self)
        
        if color is not None and color.isValid():
            button.setStyleSheet(f"background-color: {color.name(QColor.HexArgb)}; border: 1px solid #60666E; border-radius: 3px;")
            self.style_changed_handler()

    def clear_and_hide(self):
        self.selected_style_info = None
        self.hide()

    # --- PRESET MANAGEMENT (Unchanged from original) ---
    def _rebuild_preset_ui(self):
        while self.presets_buttons_layout.count():
            child = self.presets_buttons_layout.takeAt(0)
            if child.widget():
                child.widget().deleteLater()
        
        self.presets_buttons_layout.addStretch() # Keep add button to the right
        for i, style in reversed(list(enumerate(self.presets))):
            preset_button = PresetButton(i, self)
            preset_button.set_style(style)
            preset_button.clicked.connect(functools.partial(self._on_preset_clicked, i))
            preset_button.overwrite_requested.connect(self._overwrite_preset)
            preset_button.delete_requested.connect(self._delete_preset)
            self.presets_buttons_layout.insertWidget(0, preset_button)

    def _load_presets(self):
        self.presets = []
        self.settings.beginGroup("style_presets")
        i = 0
        while True:
            key = f"preset_{i}"
            preset_str = self.settings.value(key, None)
            if preset_str is None:
                break
            try:
                self.presets.append(json.loads(preset_str))
            except (json.JSONDecodeError, TypeError):
                print(f"Warning: Could not load preset at index {i}.")
            i += 1
        self.settings.endGroup()
        self._rebuild_preset_ui()

    def _save_presets(self):
        self.settings.beginGroup("style_presets")
        self.settings.remove("") 
        for i, preset in enumerate(self.presets):
            if preset:
                self.settings.setValue(f"preset_{i}", json.dumps(preset))
        self.settings.endGroup()
        self.settings.sync()

    def _on_preset_clicked(self, index):
        if not (0 <= index < len(self.presets)): return
        style_diff = self.presets[index]
        if style_diff is None: return
        full_style = json.loads(json.dumps(self._default_style))
        for key, value in style_diff.items():
            if isinstance(value, dict) and key in full_style:
                full_style[key].update(value)
            else:
                full_style[key] = value
        self.update_style_panel(full_style)
        self.style_changed_handler()

    def _add_preset(self):
        current_style = self.get_current_style()
        style_diff = get_style_diff(current_style, self._default_style)
        self.presets.append(style_diff)
        self._save_presets()
        self._rebuild_preset_ui()

    def _overwrite_preset(self, index):
        if not (0 <= index < len(self.presets)): return
        current_style = self.get_current_style()
        style_diff = get_style_diff(current_style, self._default_style)
        self.presets[index] = style_diff
        self._save_presets()
        self._rebuild_preset_ui()

    def _delete_preset(self, index):
        if not (0 <= index < len(self.presets)): return
        self.presets.pop(index)
        self._save_presets()
        self._rebuild_preset_ui()