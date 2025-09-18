from PySide6.QtWidgets import (QWidget, QVBoxLayout, QLabel, QComboBox, QSpinBox, 
                             QHBoxLayout, QPushButton, QFrame)
from PySide6.QtCore import Signal, Qt, QSize, QPoint
from PySide6.QtGui import QColor, QIcon, QPixmap, QPainter, QBrush, QPolygon
from assets import SHAPE_PANEL_STYLE

class ShapeStylePanel(QWidget):
    """
    A widget panel for managing the shape, fill, and stroke styles of a text box.
    It emits a signal when any of its managed style properties change.
    """
    style_changed = Signal()

    def __init__(self, color_chooser_fn, parent=None):
        """
        Initializes the shape style panel.
        
        Args:
            color_chooser_fn (function): A function to be called to open a color dialog.
                                         This function should accept a QPushButton as an argument.
            parent (QWidget, optional): The parent widget. Defaults to None.
        """
        super().__init__(parent)
        self.setObjectName("ShapeStylePanel")
        self._color_chooser_fn = color_chooser_fn
        self._updating_controls = False
        self.init_ui()

    def _create_shape_icons(self):
        """Creates QIcon objects for shape selection buttons."""
        icon_size = QSize(32, 32)
        base_color = QColor("#DDDDDD") # A light color for the icon shape

        # --- Icon 1: Rectangle ---
        pixmap_rect = QPixmap(icon_size)
        pixmap_rect.fill(Qt.transparent)
        p1 = QPainter(pixmap_rect)
        p1.setRenderHint(QPainter.Antialiasing)
        p1.setBrush(QBrush(base_color))
        p1.setPen(Qt.NoPen)
        p1.drawRect(5, 8, 22, 16) # x, y, width, height
        p1.end()
        self.icon_rect = QIcon(pixmap_rect) 

        # --- Icon 2: Rounded Rectangle ---
        pixmap_rounded = QPixmap(icon_size)
        pixmap_rounded.fill(Qt.transparent)
        p2 = QPainter(pixmap_rounded)
        p2.setRenderHint(QPainter.Antialiasing)
        p2.setBrush(QBrush(base_color))
        p2.setPen(Qt.NoPen)
        p2.drawRoundedRect(5, 8, 22, 16, 5, 5) # x, y, w, h, x-radius, y-radius
        p2.end()
        self.icon_rounded = QIcon(pixmap_rounded)

        # --- Icon 3: Polygon ---
        pixmap_poly = QPixmap(icon_size)
        pixmap_poly.fill(Qt.transparent)
        p3 = QPainter(pixmap_poly)
        p3.setRenderHint(QPainter.Antialiasing)
        p3.setBrush(QBrush(base_color))
        p3.setPen(Qt.NoPen)
        
        points = QPolygon([
            QPoint(6, 8), QPoint(26, 8), QPoint(26, 24),
            QPoint(16, 24), QPoint(12, 28), QPoint(12, 24), QPoint(6, 24)
        ])
        
        p3.drawPolygon(points)
        p3.end()
        self.icon_poly = QIcon(pixmap_poly)

    def init_ui(self):
        """Initializes the user interface for the shape panel."""
        self._create_shape_icons()

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(10, 10, 10, 10)
        main_layout.setSpacing(12)
        main_layout.setAlignment(Qt.AlignTop)

        shape_label = QLabel("Shape")
        shape_label.setObjectName("mainLabel")
        main_layout.addWidget(shape_label)

        self.combo_fill_type = QComboBox()
        self.combo_fill_type.setObjectName("styleCombo")
        self.combo_fill_type.addItems(["Solid", "Linear Gradient"])
        self.combo_fill_type.currentIndexChanged.connect(self._toggle_fill_gradient_controls)
        self.combo_fill_type.currentIndexChanged.connect(self._on_style_changed)
        main_layout.addWidget(self.combo_fill_type)
        
        self.solid_controls_widget = QWidget()
        solid_controls_layout = QHBoxLayout(self.solid_controls_widget)
        solid_controls_layout.setContentsMargins(0, 8, 0, 8)
        solid_controls_layout.setSpacing(10)

        fill_layout = QVBoxLayout()
        fill_layout.setSpacing(2)
        fill_label = QLabel("Fill")
        fill_label.setObjectName("tinyLabel")
        fill_layout.addWidget(fill_label)
        self.btn_bg_color = QPushButton("")
        self.btn_bg_color.setObjectName("colorButton")
        self.btn_bg_color.setFixedSize(48, 28)
        self.btn_bg_color.clicked.connect(lambda: self._handle_color_choice(self.btn_bg_color))
        fill_layout.addWidget(self.btn_bg_color)
        solid_controls_layout.addLayout(fill_layout, 1)

        stroke_width_layout = QVBoxLayout()
        stroke_width_layout.setSpacing(2)
        stroke_width_label = QLabel("Width")
        stroke_width_label.setObjectName("tinyLabel")
        stroke_width_layout.addWidget(stroke_width_label)
        self.spin_border_width = QSpinBox()
        self.spin_border_width.setObjectName("borderWidthSpinner")
        self.spin_border_width.setRange(0, 99)
        self.spin_border_width.setFixedHeight(28)
        self.spin_border_width.valueChanged.connect(self._on_style_changed)
        self.spin_border_width.valueChanged.connect(self._update_stroke_button_visuals)
        stroke_width_layout.addWidget(self.spin_border_width)
        solid_controls_layout.addLayout(stroke_width_layout, 1)

        stroke_color_layout = QVBoxLayout()
        stroke_color_layout.setSpacing(2)
        stroke_color_label = QLabel("Stroke")
        stroke_color_label.setObjectName("tinyLabel")
        stroke_color_layout.addWidget(stroke_color_label)
        self.btn_border_color = QPushButton("")
        self.btn_border_color.setObjectName("colorButton")
        self.btn_border_color.setFixedSize(48, 28)
        self.btn_border_color.clicked.connect(lambda: self._handle_color_choice(self.btn_border_color))
        stroke_color_layout.addWidget(self.btn_border_color)
        solid_controls_layout.addLayout(stroke_color_layout, 1)
        main_layout.addWidget(self.solid_controls_widget)

        shape_details_layout = QHBoxLayout()
        shape_details_layout.setSpacing(10)
        
        self.combo_bubble_type = QComboBox()
        self.combo_bubble_type.setObjectName("styleCombo")
        self.combo_bubble_type.setIconSize(QSize(28, 28))
        self.combo_bubble_type.addItems(["Rectangle", "Rounded Rectangle", "Ellipse", "Speech Bubble"])
        self.combo_bubble_type.currentIndexChanged.connect(self._on_style_changed)
        shape_details_layout.addWidget(self.combo_bubble_type, 1)

        radius_layout = QHBoxLayout()
        radius_label = QLabel("Radius:")
        radius_label.setObjectName("tinyLabel")
        radius_layout.addWidget(radius_label)
        self.spin_corner_radius = QSpinBox()
        self.spin_corner_radius.setObjectName("styleSpinBox")
        self.spin_corner_radius.setRange(0, 100)
        self.spin_corner_radius.valueChanged.connect(self._on_style_changed)
        radius_layout.addWidget(self.spin_corner_radius)
        shape_details_layout.addLayout(radius_layout, 1)
        main_layout.addLayout(shape_details_layout)

        self.gradient_fill_group = QFrame()
        self.gradient_fill_group.setObjectName("gradientGroup")
        gradient_fill_layout = QVBoxLayout(self.gradient_fill_group)
        gradient_fill_layout.setContentsMargins(0, 10, 0, 0)
        gradient_fill_layout.setSpacing(8)

        bg_grad_col1_layout = QHBoxLayout()
        bg_grad_col1_label = QLabel("  Start:")
        bg_grad_col1_layout.addWidget(bg_grad_col1_label, 1)
        self.btn_bg_gradient_color1 = QPushButton("")
        self.btn_bg_gradient_color1.setObjectName("colorButton")
        self.btn_bg_gradient_color1.setFixedSize(32, 24)
        self.btn_bg_gradient_color1.clicked.connect(lambda: self._handle_color_choice(self.btn_bg_gradient_color1))
        bg_grad_col1_layout.addWidget(self.btn_bg_gradient_color1, 2)
        gradient_fill_layout.addLayout(bg_grad_col1_layout)

        bg_grad_col2_layout = QHBoxLayout()
        bg_grad_col2_label = QLabel("  End:")
        bg_grad_col2_layout.addWidget(bg_grad_col2_label, 1)
        self.btn_bg_gradient_color2 = QPushButton("")
        self.btn_bg_gradient_color2.setObjectName("colorButton")
        self.btn_bg_gradient_color2.setFixedSize(32, 24)
        self.btn_bg_gradient_color2.clicked.connect(lambda: self._handle_color_choice(self.btn_bg_gradient_color2))
        bg_grad_col2_layout.addWidget(self.btn_bg_gradient_color2, 2)
        gradient_fill_layout.addLayout(bg_grad_col2_layout)

        bg_grad_mid_layout = QHBoxLayout()
        bg_grad_mid_label = QLabel("  Midpoint (%):")
        bg_grad_mid_layout.addWidget(bg_grad_mid_label, 1)
        self.spin_bg_gradient_midpoint = QSpinBox()
        self.spin_bg_gradient_midpoint.setObjectName("gradientMidpointSpinBox")
        self.spin_bg_gradient_midpoint.setRange(0, 100)
        self.spin_bg_gradient_midpoint.setValue(50)
        self.spin_bg_gradient_midpoint.setSuffix("%")
        self.spin_bg_gradient_midpoint.valueChanged.connect(self._on_style_changed)
        bg_grad_mid_layout.addWidget(self.spin_bg_gradient_midpoint, 2)
        gradient_fill_layout.addLayout(bg_grad_mid_layout)

        bg_grad_dir_layout = QHBoxLayout()
        bg_grad_dir_label = QLabel("  Direction:")
        bg_grad_dir_layout.addWidget(bg_grad_dir_label, 1)
        self.combo_bg_gradient_direction = QComboBox()
        self.combo_bg_gradient_direction.setObjectName("styleCombo")
        self.combo_bg_gradient_direction.addItems(["Horizontal (L>R)", "Vertical (T>B)", "Diagonal (TL>BR)", "Diagonal (BL>TR)"])
        self.combo_bg_gradient_direction.currentIndexChanged.connect(self._on_style_changed)
        bg_grad_dir_layout.addWidget(self.combo_bg_gradient_direction, 2)
        gradient_fill_layout.addLayout(bg_grad_dir_layout)
        
        main_layout.addWidget(self.gradient_fill_group)
        main_layout.addStretch()
        self._toggle_fill_gradient_controls()
        
        self.setStyleSheet(SHAPE_PANEL_STYLE)

    def _handle_color_choice(self, button):
        """
        Generic handler for all color buttons. It uses the external chooser
        and ensures the internal state ("colorValue" property) is updated correctly.
        """
        current_color = self._get_color_from_button(button)

        # Temporarily set a simple background for the generic color picker to read.
        button.setStyleSheet(f"background-color: {current_color.name(QColor.HexArgb)};")

        # Call the modal color chooser function passed from the parent.
        # This function will modify the button's stylesheet directly.
        self._color_chooser_fn(button)

        # After the chooser closes, parse the new color it has set on the stylesheet.
        style = button.styleSheet()
        new_color_str = current_color.name(QColor.HexArgb) # Default to old color on failure
        try:
            start = style.find("background-color:") + len("background-color:")
            end = style.find(";", start)
            if end == -1: end = len(style)
            parsed_color = style[start:end].strip()
            if QColor(parsed_color).isValid():
                new_color_str = parsed_color
        except:
            pass # On error, new_color_str remains the old color

        # Set the new color in our model. This also restores the correct visual preview
        self.set_button_color(button, new_color_str)
        
        # If the color actually changed, emit the signal to the parent.
        if QColor(new_color_str) != current_color:
            self._on_style_changed()

    def _on_style_changed(self):
        """Emits the style_changed signal if controls are not being updated programmatically."""
        if not self._updating_controls:
            self.style_changed.emit()

    def _get_color_from_button(self, button):
        """Extracts the QColor from a button's custom property."""
        color_str = button.property("colorValue")
        if color_str and QColor(color_str).isValid():
            return QColor(color_str)
        return QColor("#000000")

    def _toggle_fill_gradient_controls(self):
        """Shows or hides the gradient controls based on the fill type selection."""
        is_gradient = self.combo_fill_type.currentIndex() == 1
        self.gradient_fill_group.setVisible(is_gradient)
        self.solid_controls_widget.setVisible(not is_gradient)

    def _update_stroke_button_visuals(self):
        """
        Updates the visual style of the stroke button to preview the
        color and width. The width in the preview is capped for clarity.
        """
        if not hasattr(self, 'btn_border_color'): return

        width = self.spin_border_width.value()
        preview_width = min(width, 4) # Cap visual width at 4px
        color = self._get_color_from_button(self.btn_border_color)
        
        hover_color = color.lighter(130)

        self.btn_border_color.setStyleSheet(f"""
            QPushButton#colorButton {{
                background-color: transparent;
                border: {preview_width}px solid {color.name(QColor.HexArgb)};
                border-radius: 3px;
            }}
            QPushButton#colorButton:hover {{
                border-color: {hover_color.name(QColor.HexArgb)};
            }}
        """)

    def set_button_color(self, button, color_str):
        """Sets the color for a button, storing it in a property and updating visuals."""
        color = QColor(color_str)
        if not color.isValid():
            color = QColor("#ffffff") # Default to white if invalid
        
        button.setProperty("colorValue", color.name(QColor.HexArgb))

        if button is self.btn_border_color:
            self._update_stroke_button_visuals()
        else:
            # For all other buttons, just set the background color
            button.setStyleSheet(f"background-color: {color.name(QColor.HexArgb)};")

    def get_style(self):
        """Retrieves the current style settings from the UI controls."""
        style = {
            'bubble_type': self.combo_bubble_type.currentIndex(),
            'corner_radius': self.spin_corner_radius.value(),
            'border_width': self.spin_border_width.value(),
            'border_color': self._get_color_from_button(self.btn_border_color).name(QColor.HexArgb),
            'fill_type': 'linear_gradient' if self.combo_fill_type.currentIndex() == 1 else 'solid',
            'bg_color': self._get_color_from_button(self.btn_bg_color).name(QColor.HexArgb),
            'bg_gradient': {
                'color1': self._get_color_from_button(self.btn_bg_gradient_color1).name(QColor.HexArgb),
                'color2': self._get_color_from_button(self.btn_bg_gradient_color2).name(QColor.HexArgb),
                'direction': self.combo_bg_gradient_direction.currentIndex(),
                'midpoint': self.spin_bg_gradient_midpoint.value(),
            },
        }
        return style

    def set_style(self, style_dict, default_gradient):
        """Updates the UI controls with the values from a given style dictionary."""
        self._updating_controls = True
        
        self.combo_bubble_type.setCurrentIndex(style_dict.get('bubble_type', 1))
        self.spin_corner_radius.setValue(style_dict.get('corner_radius', 10))
        self.spin_border_width.setValue(style_dict.get('border_width', 1))
        
        # This will set the property and call _update_stroke_button_visuals
        self.set_button_color(self.btn_border_color, style_dict.get('border_color', '#ff000000'))

        fill_type = style_dict.get('fill_type', 'solid')
        self.combo_fill_type.setCurrentIndex(1 if fill_type == 'linear_gradient' else 0)
        self.set_button_color(self.btn_bg_color, style_dict.get('bg_color', '#ffffffff'))

        bg_gradient = style_dict.get('bg_gradient', default_gradient)
        if bg_gradient:
            self.set_button_color(self.btn_bg_gradient_color1, bg_gradient.get('color1'))
            self.set_button_color(self.btn_bg_gradient_color2, bg_gradient.get('color2'))
            self.combo_bg_gradient_direction.setCurrentIndex(bg_gradient.get('direction', 0))
            self.spin_bg_gradient_midpoint.setValue(int(bg_gradient.get('midpoint', 50)))

        self._toggle_fill_gradient_controls()
        self._updating_controls = False
        # Final visual update just in case
        self._update_stroke_button_visuals()