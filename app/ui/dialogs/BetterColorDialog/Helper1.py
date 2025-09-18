import math
import sys
from PySide6.QtWidgets import (QWidget, QSizePolicy, QApplication, QTabWidget, QGridLayout, QVBoxLayout,
                             QLabel, QSlider, QSpinBox, QAbstractSpinBox, QHBoxLayout, QLineEdit)
from PySide6.QtCore import (Qt, Signal, QSize, QPoint, QPointF, QObject)
from PySide6.QtGui import (QColor, QPainter, QConicalGradient, QCursor, QBrush, QPen, QLinearGradient, QPainterPath) # Added QPainterPath
# --- NEW IMPORTS for Global Eyedropper ---
# You must run: pip install pynput
try:
    from pynput import mouse, keyboard
except ImportError:
    print("WARNING!!")
    print("pynput is not installed. eyedropper won't be working propperly.")
    print("Please run 'pip install pynput' for the eyedropper to work.")
    mouse = None
    keyboard = None
# --- END NEW IMPORTS ---

class CheckerboardWidget(QWidget):
    """A widget that draws a checkerboard pattern as its background."""
    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing, False) # Sharp squares

        tileSize = 10 # Size of each checkerboard square
        bgColor1 = QColor(200, 200, 200) # Light gray
        bgColor2 = QColor(230, 230, 230) # Lighter gray

        for y in range(0, self.height(), tileSize):
            for x in range(0, self.width(), tileSize):
                if ((x // tileSize) % 2 == 0 and (y // tileSize) % 2 == 0) or \
                ((x // tileSize) % 2 != 0 and (y // tileSize) % 2 != 0):
                    painter.fillRect(x, y, tileSize, tileSize, bgColor1)
                else:
                    painter.fillRect(x, y, tileSize, tileSize, bgColor2)

# --- NEW: Helper Widget for Alpha Slider Background (Moved from MainDialog.py) ---
class AlphaSliderContainer(CheckerboardWidget):
    """ A CheckerboardWidget container specifically for the Alpha slider """
    def __init__(self, slider, parent=None):
        super().__init__(parent)
        self.setObjectName("AlphaSliderContainer")
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(slider)
        # Make the container visually blend with the slider groove
        # Slider groove height is 8px, margin is 2px top/bottom = 12px total
        # Add border (1px top/bottom) = 14px needed for container height
        self.setFixedHeight(slider.sizeHint().height() + 4) # Adjust based on style
        # Apply border/radius similar to slider groove
        self.setStyleSheet("#AlphaSliderContainer { background: transparent; border: 1px solid #3A3A3A; border-radius: 4px; }")

    def paintEvent(self, event):
        # Draw checkerboard first
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing, False) # Sharp squares
        tileSize = 6 # Smaller tiles for slider background
        bgColor1 = QColor(180, 180, 180) # Slightly darker grays
        bgColor2 = QColor(210, 210, 210)
        for y in range(0, self.height(), tileSize):
            for x in range(0, self.width(), tileSize):
                if ((x // tileSize) + (y // tileSize)) % 2 == 0:
                    painter.fillRect(x, y, tileSize, tileSize, bgColor1)
                else:
                    painter.fillRect(x, y, tileSize, tileSize, bgColor2)
        # No super().paintEvent() needed as the slider is a child widget
        # and will be painted automatically over the background.

# --- NEW: Modular Widget for Color Sliders ---
class ColorSlidersWidget(QWidget):
    """
    A self-contained widget with RGB/HSV tabs for color component selection.
    It emits a signal when the color is changed via its controls.
    """
    colorChanged = Signal(QColor)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._updating_controls = False
        self._current_color = QColor()

        # --- Initialize Widgets ---
        self.input_tab_widget = QTabWidget()
        self.input_tab_widget.setObjectName("InputTabWidget") # For styling
        self.rgb_sliders = {}
        self.hsv_sliders = {}
        self.rgb_spinboxes = {}
        self.hsv_spinboxes = {}
        self.alpha_slider_container = None
        self.hsv_alpha_display = None

        # --- Layout ---
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.addWidget(self.input_tab_widget)

        self._create_tabs()

    def _create_tabs(self):
        # --- RGB Tab ---
        rgb_widget = QWidget()
        rgb_layout = QGridLayout(rgb_widget); rgb_layout.setSpacing(8); rgb_layout.setContentsMargins(5, 5, 5, 5)
        for i, comp in enumerate(["Red", "Green", "Blue", "Alpha"]):
            label = QLabel(f"{comp[0]}:"); label.setMinimumWidth(15)
            slider = QSlider(Qt.Horizontal); slider.setRange(0, 255); slider.setObjectName(f"RgbSlider_{comp}")
            spinbox = QSpinBox(); spinbox.setRange(0, 255); spinbox.setButtonSymbols(QAbstractSpinBox.NoButtons)

            rgb_layout.addWidget(label, i, 0, Qt.AlignRight)
            if comp == "Alpha":
                self.alpha_slider_container = AlphaSliderContainer(slider)
                rgb_layout.addWidget(self.alpha_slider_container, i, 1)
            else:
                rgb_layout.addWidget(slider, i, 1)

            rgb_layout.addWidget(spinbox, i, 2)
            rgb_layout.setColumnStretch(1, 1)
            slider.valueChanged.connect(lambda val, s=comp.lower(): self._slider_changed("rgb", s, val))
            spinbox.valueChanged.connect(lambda val, s=comp.lower(): self._spinbox_changed("rgb", s, val))
            self.rgb_sliders[comp.lower()] = slider
            self.rgb_spinboxes[comp.lower()] = spinbox
        self.input_tab_widget.addTab(rgb_widget, "RGB(A)")

        # --- HSV Tab ---
        hsv_widget = QWidget()
        hsv_layout = QGridLayout(hsv_widget); hsv_layout.setSpacing(8); hsv_layout.setContentsMargins(5, 5, 5, 5)
        ranges = {"hue": 359, "saturation": 255, "value": 255}
        labels = {"hue": "H", "saturation": "S", "value": "V"}
        for i, comp in enumerate(["hue", "saturation", "value"]):
            label = QLabel(f"{labels[comp]}:"); label.setMinimumWidth(15)
            slider = QSlider(Qt.Horizontal); slider.setRange(0, ranges[comp]); slider.setObjectName(f"HsvSlider_{comp.capitalize()}")
            spinbox = QSpinBox(); spinbox.setRange(0, ranges[comp]); spinbox.setButtonSymbols(QAbstractSpinBox.NoButtons)
            hsv_layout.addWidget(label, i, 0, Qt.AlignRight)
            hsv_layout.addWidget(slider, i, 1)
            hsv_layout.addWidget(spinbox, i, 2)
            hsv_layout.setColumnStretch(1, 1)
            slider.valueChanged.connect(lambda val, s=comp: self._slider_changed("hsv", s, val))
            spinbox.valueChanged.connect(lambda val, s=comp: self._spinbox_changed("hsv", s, val))
            self.hsv_sliders[comp] = slider
            self.hsv_spinboxes[comp] = spinbox
        hsv_layout.addWidget(QLabel("A:"), 3, 0, Qt.AlignRight)
        self.hsv_alpha_display = QLineEdit(); self.hsv_alpha_display.setReadOnly(True)
        self.hsv_alpha_display.setMaximumWidth(spinbox.sizeHint().width())
        hsv_layout.addWidget(self.hsv_alpha_display, 3, 2)
        self.input_tab_widget.addTab(hsv_widget, "HSV")

    def get_alpha(self):
        """Public method to get the current alpha value."""
        return self.rgb_spinboxes.get('alpha', QWidget()).value()

    def setColor(self, color):
        """Public method to update all controls from a QColor without emitting signals."""
        if not color.isValid() or (self._current_color.isValid() and color.rgba() == self._current_color.rgba()):
            return

        self._updating_controls = True
        self._current_color = QColor(color)

        r, g, b, a = self._current_color.getRgb()
        h, s, v, _ = self._current_color.getHsv()
        # For grayscale, use hue from slider if available, else 0
        display_hue = h if h != -1 else (self.hsv_sliders['hue'].value() if 'hue' in self.hsv_sliders else 0)

        # Update RGB(A) Tab
        self.rgb_spinboxes['red'].setValue(r); self.rgb_sliders['red'].setValue(r)
        self.rgb_spinboxes['green'].setValue(g); self.rgb_sliders['green'].setValue(g)
        self.rgb_spinboxes['blue'].setValue(b); self.rgb_sliders['blue'].setValue(b)
        self.rgb_spinboxes['alpha'].setValue(a); self.rgb_sliders['alpha'].setValue(a)

        # Update HSV Tab
        self.hsv_spinboxes['hue'].setValue(display_hue); self.hsv_sliders['hue'].setValue(display_hue)
        self.hsv_spinboxes['saturation'].setValue(s); self.hsv_sliders['saturation'].setValue(s)
        self.hsv_spinboxes['value'].setValue(v); self.hsv_sliders['value'].setValue(v)
        self.hsv_alpha_display.setText(str(a))

        self._update_slider_gradients()
        self._updating_controls = False

    def _slider_changed(self, model, component, value):
        if self._updating_controls: return
        spinbox = getattr(self, f"{model}_spinboxes", {}).get(component)
        if spinbox:
            spinbox.blockSignals(True); spinbox.setValue(value); spinbox.blockSignals(False)
        if model == 'rgb' and component == 'alpha':
            self.hsv_alpha_display.setText(str(value))

        new_color = self._construct_color_from_inputs(model)
        if new_color.isValid():
            self._current_color = new_color # Update internal color state
            self._update_slider_gradients() # Update gradients based on the change
            self.colorChanged.emit(new_color) # Notify parent

    def _spinbox_changed(self, model, component, value):
        if self._updating_controls: return
        slider = getattr(self, f"{model}_sliders", {}).get(component)
        if slider:
            slider.blockSignals(True); slider.setValue(value); slider.blockSignals(False)
        if model == 'rgb' and component == 'alpha':
            self.hsv_alpha_display.setText(str(value))

        new_color = self._construct_color_from_inputs(model)
        if new_color.isValid():
            self._current_color = new_color # Update internal color state
            self._update_slider_gradients() # Update gradients based on the change
            self.colorChanged.emit(new_color) # Notify parent

    def _construct_color_from_inputs(self, model):
        try:
            alpha = self.get_alpha()
            if model == "rgb":
                r = self.rgb_spinboxes['red'].value(); g = self.rgb_spinboxes['green'].value(); b = self.rgb_spinboxes['blue'].value()
                return QColor(r, g, b, alpha)
            elif model == "hsv":
                h = self.hsv_spinboxes['hue'].value(); s = self.hsv_spinboxes['saturation'].value(); v = self.hsv_spinboxes['value'].value()
                return QColor.fromHsv(h, s, v, alpha)
        except Exception as e:
            print(f"Error constructing color from {model} inputs: {e}", file=sys.stderr)
        return QColor()

    def _update_slider_gradients(self):
        if not self._current_color.isValid(): return

        r, g, b, a = self._current_color.getRgb()
        h, s, v, _ = self._current_color.getHsv()
        current_hue = h if h != -1 else self.hsv_sliders['hue'].value()

        def format_rgba(qcolor): return f"rgba({qcolor.red()},{qcolor.green()},{qcolor.blue()},{max(0.0, min(1.0, qcolor.alphaF())):.3f})"
        base_handle_style = "QSlider::handle:horizontal { background:#E0E0E0; border:1px solid #666; width:16px; margin:-4px 0; border-radius:8px; } QSlider::handle:horizontal:hover { background:#FFFFFF; }"
        base_groove_style = "border: 1px solid #3A3A3A; height: 8px; margin: 2px 0; border-radius: 4px;"

        # RGB Sliders
        sliders_to_update = {'red':(QColor(0,g,b,a),QColor(255,g,b,a)),'green':(QColor(r,0,b,a),QColor(r,255,b,a)),'blue':(QColor(r,g,0,a),QColor(r,g,255,a))}
        for comp, (col0, col1) in sliders_to_update.items():
            slider = self.rgb_sliders.get(comp)
            if slider: slider.setStyleSheet(f"QSlider#{slider.objectName()}::groove:horizontal {{ {base_groove_style} background:qlineargradient(x1:0,y1:0,x2:1,y2:0,stop:0 {format_rgba(col0)},stop:1 {format_rgba(col1)}); }} {base_handle_style}")
        
        # Alpha Slider
        alpha_slider = self.rgb_sliders.get('alpha')
        if alpha_slider and self.alpha_slider_container:
            col_a0, col_a1 = QColor(r,g,b,0), QColor(r,g,b,255)
            alpha_slider.setStyleSheet(f"QSlider#{alpha_slider.objectName()}::groove:horizontal {{ background:qlineargradient(x1:0,y1:0,x2:1,y2:0,stop:0 {format_rgba(col_a0)}, stop:1 {format_rgba(col_a1)}); border:none; border-radius:4px; height:8px; margin:2px 0; }} QSlider#{alpha_slider.objectName()} {{ background:transparent; }} {base_handle_style}")
            self.alpha_slider_container.update()

        # HSV Sliders
        hue_slider = self.hsv_sliders.get('hue')
        if hue_slider:
            stops_h = [f"stop:{i/6.0:.3f} hsl({i*60},100%,50%)" for i in range(7)]
            if stops_h: stops_h[-1] = "stop:1.0 hsl(0,100%,50%)"
            hue_slider.setStyleSheet(f"QSlider#{hue_slider.objectName()}::groove:horizontal {{ {base_groove_style} background:qlineargradient(x1:0,y1:0,x2:1,y2:0, {', '.join(stops_h)}); }} {base_handle_style}")
        
        sat_slider = self.hsv_sliders.get('saturation')
        if sat_slider:
            col_s0, col_s1 = QColor.fromHsv(current_hue,0,v,a), QColor.fromHsv(current_hue,255,v,a)
            sat_slider.setStyleSheet(f"QSlider#{sat_slider.objectName()}::groove:horizontal {{ {base_groove_style} background:qlineargradient(x1:0,y1:0,x2:1,y2:0,stop:0 {format_rgba(col_s0)},stop:1 {format_rgba(col_s1)}); }} {base_handle_style}")

        val_slider = self.hsv_sliders.get('value')
        if val_slider:
            col_v0, col_v1 = QColor.fromHsv(current_hue,s,0,a), QColor.fromHsv(current_hue,s,255,a)
            val_slider.setStyleSheet(f"QSlider#{val_slider.objectName()}::groove:horizontal {{ {base_groove_style} background:qlineargradient(x1:0,y1:0,x2:1,y2:0,stop:0 {format_rgba(col_v0)},stop:1 {format_rgba(col_v1)}); }} {base_handle_style}")

# --- Color Square (Saturation/Value) - Unchanged Logic ---
class ColorSquare(QWidget):
    """ A widget displaying Saturation/Value for a given Hue. """
    svChanged = Signal(int, int) # Emits saturation (0-255), value (0-255)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._hue = 0            # Current Hue (0-359)
        self._saturation = 0     # Current Saturation (0-255)
        self._value = 255        # Current Value (0-255)
        self._indicator_pos = QPoint(0, 0)
        # Default size, can be overridden by layout/parent
        self._square_size = QSize(200, 200)

        # Set minimum size but allow it to be smaller if layout constrains it
        self.setMinimumSize(100, 100)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.setCursor(Qt.CrossCursor)
        # Don't call _update_indicator_pos here, wait for first resize or setColor

    def sizeHint(self):
        # Prefer the default size unless constrained smaller
        return self._square_size

    def setHue(self, hue):
        """ Sets the hue (0-359) used for the background gradient. """
        hue = hue % 360 # Ensure hue is within range (0-359)
        if self._hue != hue:
            self._hue = hue
            self.update() # Trigger repaint with the new hue gradient

    def setSaturationValue(self, saturation, value):
        """ Sets the saturation and value, updating the indicator position. """
        sat_changed = self._saturation != saturation
        val_changed = self._value != value

        if sat_changed or val_changed:
            self._saturation = max(0, min(255, saturation))
            self._value = max(0, min(255, value))
            # Update position immediately for internal consistency
            self._update_indicator_pos()
            self.update() # Repaint indicator

    def setColor(self, color):
        """ Sets the state based on a QColor. """
        if not isinstance(color, QColor) or not color.isValid(): return

        new_hue = color.hue()
        new_sat = color.saturation()
        new_val = color.value()

        # Keep current hue if color is grayscale (hue == -1)
        current_internal_hue = self._hue
        if new_hue == -1:
            # If the square already has a non-zero hue, keep it for grayscale.
            # Otherwise (e.g., initial state), default to 0.
            new_hue = current_internal_hue if current_internal_hue != 0 else 0

        hue_changed = self._hue != new_hue
        sv_changed = self._saturation != new_sat or self._value != new_val

        self._hue = new_hue
        self._saturation = new_sat
        self._value = new_val

        self._update_indicator_pos()

        if hue_changed or sv_changed:
            self.update() # Request repaint if anything changed

    def _update_indicator_pos(self):
        """ Calculate indicator pixel position from current S/V. """
        if self.width() <= 0 or self.height() <= 0:
             self._indicator_pos = QPoint(0,0)
             return

        # Normalize S and V to 0.0-1.0 range
        s_norm = self._saturation / 255.0
        v_norm = self._value / 255.0

        # Y corresponds to Value (inverted: 0=bottom, 1=top)
        # X corresponds to Saturation (0=left, 1=right)
        indicator_x = s_norm * (self.width() - 1)
        indicator_y = (1.0 - v_norm) * (self.height() - 1) # Invert Y

        # Clamp position strictly within widget bounds
        self._indicator_pos = QPoint(
            max(0, min(int(indicator_x), self.width() - 1)),
            max(0, min(int(indicator_y), self.height() - 1))
        )


    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        rect = self.rect()
        if not rect.isValid(): return # Don't draw if size is invalid

        # --- Draw Saturation Gradient (Left: White -> Right: Full Hue Color) ---
        right_color = QColor.fromHsv(self._hue, 255, 255) # S=255, V=255
        sat_gradient = QLinearGradient(rect.topLeft(), rect.topRight())
        sat_gradient.setColorAt(0, QColor(255, 255, 255)) # S=0, V=255 (pure white)
        sat_gradient.setColorAt(1, right_color)
        painter.fillRect(rect, sat_gradient)

        # --- Draw Value Gradient (Overlay) (Top: Transparent -> Bottom: Black) ---
        val_gradient = QLinearGradient(rect.topLeft(), rect.bottomLeft())
        val_gradient.setColorAt(0, Qt.transparent)      # V=max
        val_gradient.setColorAt(1, QColor(0, 0, 0))       # V=min
        painter.fillRect(rect, val_gradient)

        # --- Draw Indicator ---
        indicator_radius = 5 # Use radius for ellipse drawing
        indicator_center = self._indicator_pos

        # Choose contrasting color for indicator based on calculated color under it
        indicator_color = QColor.fromHsv(self._hue, self._saturation, self._value)
        indicator_outline_color = QColor(0,0,0) if indicator_color.value() > 128 else QColor(255,255,255)

        painter.setPen(QPen(indicator_outline_color, 1.5))
        painter.setBrush(Qt.NoBrush)
        painter.drawEllipse(indicator_center, indicator_radius, indicator_radius)


    def mousePressEvent(self, event):
        if event.button() == Qt.LeftButton:
            self._update_from_pos(event.pos())

    def mouseMoveEvent(self, event):
        if event.buttons() & Qt.LeftButton:
            self._update_from_pos(event.pos())

    def resizeEvent(self, event):
        # Recalculate indicator position when widget size changes
        self._update_indicator_pos()
        self.update() # Ensure repaint on resize
        super().resizeEvent(event)

    def _update_from_pos(self, pos):
        """ Calculate S/V from mouse position and emit signal. """
        w = self.width()
        h = self.height()
        if w <= 1 or h <= 1: return # Avoid division by zero or invalid calc

        # Clamp position within bounds
        x = max(0, min(pos.x(), w - 1))
        y = max(0, min(pos.y(), h - 1))

        # Calculate Saturation (0-255) based on X
        new_saturation = int((x / (w - 1)) * 255)

        # Calculate Value (0-255) based on Y (inverted)
        new_value = int(((h - 1 - y) / (h - 1)) * 255)

        # Only update and emit if values actually changed
        if new_saturation != self._saturation or new_value != self._value:
            self._saturation = new_saturation
            self._value = new_value
            self._indicator_pos = QPoint(x, y)
            self.update() # Repaint to show indicator move

            # Emit the changed S and V values
            self.svChanged.emit(self._saturation, self._value)


# --- NEW: Hue Ring ---
class HueRing(QWidget):
    """ A widget displaying a hue spectrum in a ring, allowing selection. """
    hueChanged = Signal(int) # Emits hue (0-359)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._hue = 0             # Current Hue (0-359)
        self._ring_width = 30     # Thickness of the color ring
        self._padding = 5         # Padding around the ring
        self._indicator_size = 15  # Diameter of the indicator circle
        self._mouse_pressed = False

        self.setMinimumSize(100, 100) # Ensure it's not too small
        self.setCursor(Qt.PointingHandCursor) # Indicate clickability

    def sizeHint(self):
        # Suggest a size that comfortably fits the ring + padding
        return QSize(200 + 2 * self._padding, 200 + 2 * self._padding)

    def setHue(self, hue):
        """ Sets the hue (0-359) and updates the indicator position. """
        # --- START OF FIX ---
        # Ensure hue is treated as an integer and is in the correct range
        try:
            # Explicitly cast to integer FIRST
            hue_int = int(hue)
            # Apply modulo to keep it within 0-359
            hue_int = hue_int % 360
        except (ValueError, TypeError):
            # Handle cases where input might not be convertible (e.g., None, string)
            print(f"[HueRing] Warning: Invalid hue value received ({hue}), defaulting to 0.")
            hue_int = 0

        # Only update if the integer hue value has changed
        if self._hue != hue_int:
            self._hue = hue_int # Store the integer value
            self.update() # Trigger repaint
        # --- END OF FIX ---

    def getHue(self):
        """ Returns the current hue (0-359). """
        return self._hue

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)

        rect = self.rect().adjusted(self._padding, self._padding, -self._padding, -self._padding)
        if rect.width() <= 2 * self._ring_width or rect.height() <= 2 * self._ring_width:
            return # Not enough space to draw

        center = rect.center()
        outer_radius = min(rect.width(), rect.height()) / 2.0
        inner_radius = outer_radius - self._ring_width

        if inner_radius <= 0: return # Not enough space

        # --- Draw Hue Ring using QConicalGradient ---
        gradient = QConicalGradient(center, 0) # Start angle at 0 degrees (right)
        # Standard HSV Hue stops (Angle corresponds to hue/360)
        stops = [
            (0.0, QColor.fromHsv(0, 255, 255)),    # Red
            (1/6, QColor.fromHsv(60, 255, 255)),   # Yellow
            (2/6, QColor.fromHsv(120, 255, 255)),  # Green
            (3/6, QColor.fromHsv(180, 255, 255)),  # Cyan
            (4/6, QColor.fromHsv(240, 255, 255)),  # Blue
            (5/6, QColor.fromHsv(300, 255, 255)),  # Magenta
            (1.0, QColor.fromHsv(359, 255, 255))   # Red again (close loop)
        ]
        for stop, color in stops:
            gradient.setColorAt(stop, color)

        # Create a path for the ring (annulus)
        ring_path = QPainterPath()
        ring_path.addEllipse(center, outer_radius, outer_radius)
        ring_path.addEllipse(center, inner_radius, inner_radius)
        # Use EvenOdd fill rule so the inner circle creates a hole

        painter.setBrush(QBrush(gradient))
        painter.setPen(Qt.NoPen) # Don't draw outline for the gradient area
        painter.drawPath(ring_path)

        # --- Draw Indicator ---
        indicator_radius = self._indicator_size / 2.0
        # Angle needs adjustment: QConicalGradient 0 is right, math 0 is right, but we want 0=Red up? No, keep 0=Red right for consistency with HSV.
        angle_rad = math.radians(self._hue) # Hue 0..359 maps to angle 0..359 degrees
        # Calculate position on the centerline of the ring
        indicator_dist = inner_radius + self._ring_width / 2.0
        indicator_x = center.x() + indicator_dist * math.cos(angle_rad)
        indicator_y = center.y() + indicator_dist * math.sin(angle_rad)
        indicator_center = QPointF(indicator_x, indicator_y)

        # Draw indicator circle (e.g., white with black outline)
        painter.setPen(QPen(Qt.black, 1.5))
        painter.setBrush(Qt.NoBrush)
        painter.drawEllipse(indicator_center, indicator_radius, indicator_radius)

    def mousePressEvent(self, event):
        if event.button() == Qt.LeftButton:
            if self._update_hue_from_pos(event.pos()):
                self._mouse_pressed = True

    def mouseMoveEvent(self, event):
        if self._mouse_pressed and (event.buttons() & Qt.LeftButton):
            self._update_hue_from_pos(event.pos())

    def mouseReleaseEvent(self, event):
        if event.button() == Qt.LeftButton:
            self._mouse_pressed = False
            # Optional: one final update on release
            # self._update_hue_from_pos(event.pos())

    def _update_hue_from_pos(self, pos):
        """ Calculate Hue from mouse position and emit signal if changed. """
        rect = self.rect().adjusted(self._padding, self._padding, -self._padding, -self._padding)
        center = rect.center()
        outer_radius = min(rect.width(), rect.height()) / 2.0
        inner_radius = outer_radius - self._ring_width

        dx = pos.x() - center.x()
        dy = pos.y() - center.y()
        distance_sq = dx*dx + dy*dy

        # Check if click is within the ring bounds
        if inner_radius*inner_radius <= distance_sq <= outer_radius*outer_radius:
            # Calculate angle using atan2, result is in radians (-pi to pi)
            angle_rad = math.atan2(dy, dx)
            # Convert to degrees (0 to 360)
            angle_deg = math.degrees(angle_rad)
            hue = int(angle_deg + 360) % 360 # Normalize to 0-359 range

            if self._hue != hue:
                self._hue = hue
                self.update() # Repaint indicator
                self.hueChanged.emit(self._hue)
                return True # Hue was updated
        return False # Click was outside ring or hue didn't change

class EyedropperHelper(QObject):
    """
    A robust, global eyedropper using pynput to capture mouse and keyboard
    events anywhere on the screen. This avoids the need to hide the main dialog.
    Requires `pip install pynput`.
    """
    colorSelected = Signal(QColor)
    cancelled = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self._mouse_listener = None
        self._key_listener = None
        self._active = False
        if mouse is None or keyboard is None:
            print("pynput is not available. Eyedropper will not function.")

    def start(self):
        if self._active or mouse is None:
            return
        self._active = True
        # Set cursor for the entire application, which persists
        QApplication.setOverrideCursor(Qt.CrossCursor)

        # pynput listeners run in a separate thread
        self._mouse_listener = mouse.Listener(on_click=self._on_click)
        self._key_listener = keyboard.Listener(on_press=self._on_press)
        self._mouse_listener.start()
        self._key_listener.start()

    def stop(self):
        if not self._active:
            return
        
        QApplication.restoreOverrideCursor()
        
        if self._mouse_listener:
            self._mouse_listener.stop()
        if self._key_listener:
            self._key_listener.stop()

        self._mouse_listener = None
        self._key_listener = None
        self._active = False

    def _on_click(self, x, y, button, pressed):
        # We only care about the left mouse button press
        if pressed and button == mouse.Button.left:
            # The same screen grabbing logic as before
            pos = QCursor.pos()
            screen = QApplication.screenAt(pos)
            color = QColor()
            if screen:
                pixmap = screen.grabWindow(0, pos.x(), pos.y(), 1, 1)
                if not pixmap.isNull():
                    image = pixmap.toImage()
                    if not image.isNull():
                        color.setRgb(image.pixel(0, 0))
            
            # Emit signal and stop listeners
            self.colorSelected.emit(color)
            return False # Returning False stops the listener thread

    def _on_press(self, key):
        # Check for the Escape key to cancel
        if key == keyboard.Key.esc:
            self.cancelled.emit()
            return False # Returning False stops the listener thread