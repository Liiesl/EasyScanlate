import os
from PySide6.QtWidgets import (QWidget, QVBoxLayout, QLabel, QComboBox, QSpinBox, 
                             QHBoxLayout, QPushButton, QFrame, QButtonGroup, QCheckBox)
from PySide6.QtCore import Signal, Qt, QSize
from PySide6.QtGui import QFontDatabase, QColor, QIcon
import qtawesome as qta
from assets import TYPOGRAPHY_PANEL_STYLE

class TypographyStylePanel(QWidget):
    """
    A widget panel for managing the typography styles of a text box, including
    font properties and text color (solid or gradient).
    """
    style_changed = Signal()

    def __init__(self, color_chooser_fn, parent=None):
        """
        Initializes the typography style panel.
        
        Args:
            color_chooser_fn (function): A function to be called to open a color dialog.
            parent (QWidget, optional): The parent widget. Defaults to None.
        """
        super().__init__(parent)
        self.setObjectName("TypographyStylePanel")
        self._color_chooser_fn = color_chooser_fn
        self._updating_controls = False
        self.font_styles = {} # { "Family Name": ["Style1", "Style2", ...] }
        self.init_ui()
        self.load_custom_fonts()
        self._update_font_style_combo()

    def init_ui(self):
        """Initializes the user interface for the typography panel."""
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(10, 10, 10, 10)
        main_layout.setSpacing(12)
        main_layout.setAlignment(Qt.AlignTop)

        # --- Main Title ---
        typo_label = QLabel("Typography")
        typo_label.setObjectName("mainLabel")
        main_layout.addWidget(typo_label)

        # --- Color Type Dropdown ---
        self.combo_text_color_type = QComboBox()
        self.combo_text_color_type.setObjectName("styleCombo")
        self.combo_text_color_type.addItems(["Solid", "Linear Gradient"])
        self.combo_text_color_type.currentIndexChanged.connect(self._toggle_text_gradient_controls)
        self.combo_text_color_type.currentIndexChanged.connect(self._on_style_changed)
        main_layout.addWidget(self.combo_text_color_type)

        # --- Solid Color Controls ---
        self.solid_controls_widget = QWidget()
        solid_controls_layout = QHBoxLayout(self.solid_controls_widget)
        solid_controls_layout.setContentsMargins(0, 8, 0, 8)
        solid_controls_layout.setSpacing(10)
        
        color_layout = QVBoxLayout()
        color_layout.setSpacing(2)
        color_label = QLabel("Color")
        color_label.setObjectName("tinyLabel")
        color_layout.addWidget(color_label)
        self.btn_text_color = QPushButton("")
        self.btn_text_color.setObjectName("colorButton")
        self.btn_text_color.setFixedSize(48, 28)
        self.btn_text_color.clicked.connect(lambda: self._color_chooser_fn(self.btn_text_color))
        color_layout.addWidget(self.btn_text_color)
        solid_controls_layout.addLayout(color_layout)
        solid_controls_layout.addStretch()
        main_layout.addWidget(self.solid_controls_widget)

        # --- Gradient Color Controls (Initially Hidden) ---
        self.gradient_text_group = QFrame()
        self.gradient_text_group.setObjectName("gradientGroup")
        gradient_text_layout = QVBoxLayout(self.gradient_text_group)
        gradient_text_layout.setContentsMargins(0, 10, 0, 0)
        gradient_text_layout.setSpacing(8)

        text_grad_col1_layout = QHBoxLayout()
        text_grad_col1_label = QLabel("  Start:")
        text_grad_col1_layout.addWidget(text_grad_col1_label, 1)
        self.btn_text_gradient_color1 = QPushButton("")
        self.btn_text_gradient_color1.setObjectName("colorButton")
        self.btn_text_gradient_color1.setFixedSize(32, 24)
        self.btn_text_gradient_color1.clicked.connect(lambda: self._color_chooser_fn(self.btn_text_gradient_color1))
        text_grad_col1_layout.addWidget(self.btn_text_gradient_color1, 2)
        gradient_text_layout.addLayout(text_grad_col1_layout)

        text_grad_col2_layout = QHBoxLayout()
        text_grad_col2_label = QLabel("  End:")
        text_grad_col2_layout.addWidget(text_grad_col2_label, 1)
        self.btn_text_gradient_color2 = QPushButton("")
        self.btn_text_gradient_color2.setObjectName("colorButton")
        self.btn_text_gradient_color2.setFixedSize(32, 24)
        self.btn_text_gradient_color2.clicked.connect(lambda: self._color_chooser_fn(self.btn_text_gradient_color2))
        text_grad_col2_layout.addWidget(self.btn_text_gradient_color2, 2)
        gradient_text_layout.addLayout(text_grad_col2_layout)

        text_grad_mid_layout = QHBoxLayout()
        text_grad_mid_label = QLabel("  Midpoint (%):")
        text_grad_mid_layout.addWidget(text_grad_mid_label, 1)
        self.spin_text_gradient_midpoint = QSpinBox()
        self.spin_text_gradient_midpoint.setObjectName("gradientMidpointSpinBox")
        self.spin_text_gradient_midpoint.setRange(0, 100)
        self.spin_text_gradient_midpoint.setValue(50)
        self.spin_text_gradient_midpoint.setSuffix("%")
        self.spin_text_gradient_midpoint.valueChanged.connect(self._on_style_changed)
        text_grad_mid_layout.addWidget(self.spin_text_gradient_midpoint, 2)
        gradient_text_layout.addLayout(text_grad_mid_layout)

        text_grad_dir_layout = QHBoxLayout()
        text_grad_dir_label = QLabel("  Direction:")
        text_grad_dir_layout.addWidget(text_grad_dir_label, 1)
        self.combo_text_gradient_direction = QComboBox()
        self.combo_text_gradient_direction.setObjectName("styleCombo")
        self.combo_text_gradient_direction.addItems(["Horizontal (L>R)", "Vertical (T>B)", "Diagonal (TL>BR)", "Diagonal (BL>TR)"])
        self.combo_text_gradient_direction.currentIndexChanged.connect(self._on_style_changed)
        text_grad_dir_layout.addWidget(self.combo_text_gradient_direction, 2)
        gradient_text_layout.addLayout(text_grad_dir_layout)
        main_layout.addWidget(self.gradient_text_group)

        # --- Font Family Dropdown ---
        self.combo_font_family = QComboBox()
        self.combo_font_family.setObjectName("styleCombo")
        self.combo_font_family.currentIndexChanged.connect(self._update_font_style_combo)
        self.combo_font_family.currentIndexChanged.connect(self._on_style_changed)
        main_layout.addWidget(self.combo_font_family)

        # --- Alignment and Style Toggles ---
        props_layout1 = QHBoxLayout()
        props_layout1.setSpacing(5)
        
        icon_size = QSize(16, 16)
        icon_color = "#EAEAEA"
        align_left_pixmap = qta.icon('fa5s.align-left', color=icon_color).pixmap(icon_size)
        align_center_pixmap = qta.icon('fa5s.align-center', color=icon_color).pixmap(icon_size)
        align_right_pixmap = qta.icon('fa5s.align-right', color=icon_color).pixmap(icon_size)
        bold_pixmap = qta.icon('fa5s.bold', color=icon_color).pixmap(icon_size)
        italic_pixmap = qta.icon('fa5s.italic', color=icon_color).pixmap(icon_size)

        alignment_buttons_layout = QHBoxLayout()
        alignment_buttons_layout.setSpacing(0)
        self.alignment_group = QButtonGroup(self)
        self.alignment_group.setExclusive(True)

        self.btn_align_left = QPushButton("")
        self.btn_align_left.setCheckable(True)
        self.btn_align_left.setObjectName("alignButton")
        self.btn_align_left.setToolTip("Align Left")
        self.btn_align_left.setIcon(QIcon(align_left_pixmap))
        self.alignment_group.addButton(self.btn_align_left, 0)
        alignment_buttons_layout.addWidget(self.btn_align_left)
        
        self.btn_align_center = QPushButton("")
        self.btn_align_center.setCheckable(True)
        self.btn_align_center.setObjectName("alignButton")
        self.btn_align_center.setToolTip("Align Center")
        self.btn_align_center.setChecked(True)
        self.btn_align_center.setIcon(QIcon(align_center_pixmap))
        self.alignment_group.addButton(self.btn_align_center, 1)
        alignment_buttons_layout.addWidget(self.btn_align_center)

        self.btn_align_right = QPushButton("")
        self.btn_align_right.setCheckable(True)
        self.btn_align_right.setObjectName("alignButton")
        self.btn_align_right.setToolTip("Align Right")
        self.btn_align_right.setIcon(QIcon(align_right_pixmap))
        self.alignment_group.addButton(self.btn_align_right, 2)
        alignment_buttons_layout.addWidget(self.btn_align_right)
        
        self.alignment_group.idClicked.connect(self.set_alignment)
        props_layout1.addLayout(alignment_buttons_layout)
        props_layout1.addSpacing(10)

        self.btn_font_bold = QPushButton("")
        self.btn_font_bold.setCheckable(True)
        self.btn_font_bold.setObjectName("styleToggleButton")
        self.btn_font_bold.setToolTip("Bold")
        self.btn_font_bold.setIcon(QIcon(bold_pixmap))
        self.btn_font_bold.toggled.connect(self._on_style_changed)
        props_layout1.addWidget(self.btn_font_bold)

        self.btn_font_italic = QPushButton("")
        self.btn_font_italic.setCheckable(True)
        self.btn_font_italic.setObjectName("styleToggleButton")
        self.btn_font_italic.setToolTip("Italic")
        self.btn_font_italic.setIcon(QIcon(italic_pixmap))
        self.btn_font_italic.toggled.connect(self._on_style_changed)
        props_layout1.addWidget(self.btn_font_italic)
        props_layout1.addStretch()
        main_layout.addLayout(props_layout1)
        
        # --- Font Size and Auto-Size ---
        props_layout2 = QHBoxLayout()
        props_layout2.setContentsMargins(0, 8, 0, 0)
        props_layout2.setSpacing(10)

        size_layout = QVBoxLayout()
        size_layout.setSpacing(2)
        size_label = QLabel("Size")
        size_label.setObjectName("tinyLabel")
        size_layout.addWidget(size_label)
        self.spin_font_size = QSpinBox()
        self.spin_font_size.setObjectName("styleSpinBox")
        self.spin_font_size.setRange(6, 200)
        self.spin_font_size.setValue(24)
        self.spin_font_size.setFixedHeight(28)
        self.spin_font_size.valueChanged.connect(self._on_style_changed)
        size_layout.addWidget(self.spin_font_size)
        props_layout2.addLayout(size_layout, 1)

        auto_size_layout = QHBoxLayout()
        auto_size_layout.setContentsMargins(0, 15, 0, 0)
        auto_label = QLabel("Auto")
        auto_label.setObjectName("tinyLabel")
        auto_size_layout.addWidget(auto_label)
        # Use a standard QCheckBox as requested
        self.chk_auto_font_size = QCheckBox("")
        self.chk_auto_font_size.setChecked(True)
        self.chk_auto_font_size.stateChanged.connect(self._on_style_changed)
        auto_size_layout.addWidget(self.chk_auto_font_size)
        props_layout2.addLayout(auto_size_layout, 1)
        props_layout2.addStretch(1)
        main_layout.addLayout(props_layout2)

        # Hidden controls for state management
        self.combo_text_alignment = QComboBox()
        self.combo_text_alignment.addItems(["Left", "Center", "Right"])
        self.combo_text_alignment.setCurrentIndex(1)
        self.combo_text_alignment.setVisible(False)
        main_layout.addWidget(self.combo_text_alignment)
        
        self.font_style_widget = QWidget() # Now just a container for logic
        self.font_style_widget.setVisible(False)
        self.combo_font_style = QComboBox()
        self.combo_font_style.setParent(self.font_style_widget)


        main_layout.addStretch()
        self._toggle_text_gradient_controls()
        
        self.setStyleSheet(TYPOGRAPHY_PANEL_STYLE)

    def _on_style_changed(self):
        if not self._updating_controls:
            self.style_changed.emit()

    def _get_color_from_button(self, button):
        style = button.styleSheet()
        try:
            start = style.find("background-color:") + len("background-color:")
            end = style.find(";", start)
            if end == -1: end = len(style)
            color_str = style[start:end].strip()
            if QColor(color_str).isValid():
                return QColor(color_str)
        except:
            pass
        return QColor("#000000")
    
    def set_button_color(self, button, color_str):
        color = QColor(color_str)
        if not color.isValid():
            color = QColor(255, 255, 255)
        button.setStyleSheet(f"background-color: {color.name(QColor.HexArgb)}; border: 1px solid #60666E; border-radius: 3px;")

    def _toggle_text_gradient_controls(self):
        is_gradient = self.combo_text_color_type.currentIndex() == 1
        self.gradient_text_group.setVisible(is_gradient)
        self.solid_controls_widget.setVisible(not is_gradient)

    def set_alignment(self, index):
        """
        Called by the alignment button group when a button is clicked.
        Updates the state and emits a signal.
        """
        if self._updating_controls: return
        self.combo_text_alignment.setCurrentIndex(index)
        self._on_style_changed()

    def load_custom_fonts(self):
        self.font_styles.clear()
        self.combo_font_family.clear()
        self.combo_font_family.addItem("Default (System Font)")
        
        fonts_dir = "assets/fonts"
        if not os.path.exists(fonts_dir):
            print(f"Font directory not found: {fonts_dir}")
            return

        db = QFontDatabase()
        loaded_families = set()

        for file in os.listdir(fonts_dir):
            if file.lower().endswith(('.ttf', '.otf')):
                font_path = os.path.join(fonts_dir, file)
                font_id = db.addApplicationFont(font_path)
                if font_id != -1:
                    families = db.applicationFontFamilies(font_id)
                    for family in families:
                        loaded_families.add(family)
                else:
                    print(f"Warning: Could not load font: {font_path}")

        for family in sorted(list(loaded_families)):
            styles = db.styles(family)
            filtered_styles = [s for s in styles if "bold" not in s.lower() and "italic" not in s.lower() and "oblique" not in s.lower()]
            if not filtered_styles and "Regular" in styles:
                filtered_styles.append("Regular")
            elif not filtered_styles: # Case where only bold/italic styles exist
                filtered_styles.append(styles[0] if styles else "Regular")


            if filtered_styles:
                self.font_styles[family] = sorted(filtered_styles)
                self.combo_font_family.addItem(family)

    def _update_font_style_combo(self):
        if self._updating_controls: return
        
        current_family = self.combo_font_family.currentText()
        self.combo_font_style.clear()

        if current_family in self.font_styles and self.font_styles[current_family]:
            styles = self.font_styles[current_family]
            self.combo_font_style.addItems(styles)
            if "Regular" in styles:
                self.combo_font_style.setCurrentText("Regular")
            self.font_style_widget.setVisible(True)
        else:
            self.font_style_widget.setVisible(False)
        self._on_style_changed()


    def get_style(self, default_font_family, default_font_style):
        selected_family_text = self.combo_font_family.currentText()

        if selected_family_text == "Default (System Font)":
            font_family = default_font_family
            font_style = default_font_style
        else:
            font_family = selected_family_text
            if self.font_style_widget.isVisible() and self.combo_font_style.count() > 0:
                font_style = self.combo_font_style.currentText()
            else:
                # Fallback if no specific style is available or widget is hidden
                font_style = self.font_styles.get(font_family, ["Regular"])[0]

        style = {
            'text_color_type': 'linear_gradient' if self.combo_text_color_type.currentIndex() == 1 else 'solid',
            'text_color': self._get_color_from_button(self.btn_text_color).name(QColor.HexArgb),
            'text_gradient': {
                'color1': self._get_color_from_button(self.btn_text_gradient_color1).name(QColor.HexArgb),
                'color2': self._get_color_from_button(self.btn_text_gradient_color2).name(QColor.HexArgb),
                'direction': self.combo_text_gradient_direction.currentIndex(),
                'midpoint': self.spin_text_gradient_midpoint.value(),
            },
            'font_family': font_family,
            'font_style': font_style,
            'font_size': self.spin_font_size.value(),
            'font_bold': self.btn_font_bold.isChecked(),
            'font_italic': self.btn_font_italic.isChecked(),
            'text_alignment': self.combo_text_alignment.currentIndex(),
            'auto_font_size': self.chk_auto_font_size.isChecked(),
        }
        return style

    def set_style(self, style_dict, default_gradient):
        self._updating_controls = True
        
        text_color_type = style_dict.get('text_color_type', 'solid')
        self.combo_text_color_type.setCurrentIndex(1 if text_color_type == 'linear_gradient' else 0)
        self.set_button_color(self.btn_text_color, style_dict.get('text_color', '#ff000000'))
        
        text_gradient = style_dict.get('text_gradient', default_gradient)
        if text_gradient:
            self.set_button_color(self.btn_text_gradient_color1, text_gradient.get('color1'))
            self.set_button_color(self.btn_text_gradient_color2, text_gradient.get('color2'))
            self.combo_text_gradient_direction.setCurrentIndex(text_gradient.get('direction', 0))
            self.spin_text_gradient_midpoint.setValue(int(text_gradient.get('midpoint', 50)))

        font_family = style_dict.get('font_family', "Arial")
        font_style = style_dict.get('font_style', 'Regular')

        index = self.combo_font_family.findText(font_family)
        if index != -1:
            self.combo_font_family.setCurrentIndex(index)
        else:
            self.combo_font_family.setCurrentIndex(0) 
            print(f"Warning: Font family '{font_family}' not found, using default.")

        # This part needs to be called after setting the family to populate the styles
        self._update_font_style_combo() 

        if self.font_style_widget.isVisible():
            style_index = self.combo_font_style.findText(font_style)
            if style_index != -1:
                self.combo_font_style.setCurrentIndex(style_index)
            elif self.combo_font_style.count() > 0:
                self.combo_font_style.setCurrentIndex(0)
                print(f"Warning: Font style '{font_style}' not found, using first available.")

        self.spin_font_size.setValue(style_dict.get('font_size', 12))
        self.btn_font_bold.setChecked(style_dict.get('font_bold', False))
        self.btn_font_italic.setChecked(style_dict.get('font_italic', False))
        
        alignment_index = style_dict.get('text_alignment', 1)
        self.combo_text_alignment.setCurrentIndex(alignment_index)
        # Programmatically check the correct button in the group
        btn_to_check = self.alignment_group.button(alignment_index)
        if btn_to_check:
            btn_to_check.setChecked(True)

        self.chk_auto_font_size.setChecked(style_dict.get('auto_font_size', True))
        
        self._toggle_text_gradient_controls()
        self._updating_controls = False