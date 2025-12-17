
import math
from PySide6.QtWidgets import (QWidget, QVBoxLayout, QHBoxLayout, QFrame, QLabel, QPushButton)
from PySide6.QtGui import (QPainter, QRadialGradient, QConicalGradient, QColor, 
                           QBrush, QPen, QMouseEvent)
from PySide6.QtCore import Qt, Signal, QPointF
from app.utils.background_utils import generate_aurora_palette
from app.ui.components.background import AuroraCanvas

# --- The Corrected Square Color Picker ---
class AuroraColorWheel(QWidget):
    colorChanged = Signal(QColor)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedSize(180, 180) 
        
        self.hue = 0.0          
        self.saturation = 0.0   
        self.value = 0.9 
        
        self.margin = 5
        self.dragging = False
        
        # Visualizer config
        self.blob_count = 1
        self.is_dark_mode = True
        self.schema_index = 1 # Default: Analogous
        
        self.dm_min_val = 0.06
        self.dm_max_val = 0.45

    def set_blob_config(self, count, is_dark, schema_idx):
        self.blob_count = count
        self.is_dark_mode = is_dark
        self.schema_index = schema_idx
        
        if self.is_dark_mode:
             self.value = min(self.dm_max_val, max(self.dm_min_val, self.value))
        else:
             self.value = 1.0
        self.update()

    def set_current_color(self, color: QColor):
        h, s, v, _ = color.getHsvF()
        if h < 0: h = 0.0 # -1 means undefined (grayscale)
        self.hue = h
        self.saturation = s
        # For value, we might want to respect the clamped logic or just trust the input
        # Trusting input is safer for exact restore
        self.value = v
        self.update()

    def get_color(self):
        return QColor.fromHsvF(self.hue, self.saturation, self.value)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)

        rect = self.rect().adjusted(self.margin, self.margin, -self.margin, -self.margin)
        center = rect.center()
        max_radius = math.sqrt((rect.width()/2)**2 + (rect.height()/2)**2)

        # 1. Base Spectrum
        v_base = self.dm_max_val if self.is_dark_mode else 1.0
        hue_gradient = QConicalGradient(center, 90)
        for i in range(361):
            hue_val = (360.0 - i) / 360.0
            hue_gradient.setColorAt(i/360.0, QColor.fromHsvF(hue_val, 1.0, v_base))
        
        painter.setPen(Qt.NoPen)
        painter.setBrush(QBrush(hue_gradient))
        painter.drawRect(rect)

        # 2. Overlay
        overlay_gradient = QRadialGradient(center, max_radius)
        if self.is_dark_mode:
            grey_val = int(255 * v_base)
            center_color = QColor(grey_val, grey_val, grey_val, 255)
            transparent_color = QColor(grey_val, grey_val, grey_val, 0)
            min_grey = int(255 * self.dm_min_val)
            
            overlay_gradient.setColorAt(0.0, center_color) 
            overlay_gradient.setColorAt(0.5, transparent_color)   
            overlay_gradient.setColorAt(0.5001, QColor(0, 0, 0, 0))         
            overlay_gradient.setColorAt(1.0, QColor(min_grey, min_grey, min_grey, 255)) 
        else:
            overlay_gradient.setColorAt(0.0, QColor(255, 255, 255, 255))
            overlay_gradient.setColorAt(0.7, QColor(255, 255, 255, 50)) 
            overlay_gradient.setColorAt(1.0, QColor(255, 255, 255, 0))  

        painter.setBrush(QBrush(overlay_gradient))
        painter.drawRect(rect)

        # 3. Draw Handles
        current_col = self.get_color()
        # PASS SCHEMA INDEX HERE - Modified to use generate_aurora_palette
        blobs = generate_aurora_palette(current_col, self.blob_count, self.is_dark_mode, self.schema_index)
        
        # Ghosts
        for i in range(1, len(blobs)):
            b = blobs[i]
            self.draw_handle(painter, center, max_radius, rect, 
                             b['h']/360.0, b['s']/255.0, b['v']/255.0, 
                             is_ghost=True, color=b['color'])

        # Main Handle
        preview_col = QColor(current_col)
        if not self.is_dark_mode:
            h = current_col.hue()
            if h == -1: h = 0
            s = current_col.saturation()
            v = current_col.value()
            preview_col = QColor.fromHsv(h, max(s, 100), min(230, v))
        
        self.draw_handle(painter, center, max_radius, rect, 
                         self.hue, self.saturation, self.value, 
                         is_ghost=False, color=preview_col)

    def draw_handle(self, painter, center, max_radius, rect, h, s, v, is_ghost, color):
        dist_pct = 0.0
        if self.is_dark_mode:
            if v >= (self.dm_max_val - 0.01):
                dist_pct = s * 0.5
            else:
                val_range = self.dm_max_val - self.dm_min_val
                if val_range == 0: val_range = 1
                darkness_ratio = (self.dm_max_val - v) / val_range
                dist_pct = 0.5 + (darkness_ratio * 0.5)
        else:
            dist_pct = s

        angle = (h * 2 * math.pi) - (math.pi/2)
        dist_px = dist_pct * max_radius
        
        hx = center.x() + dist_px * math.cos(angle)
        hy = center.y() + dist_px * math.sin(angle)

        # Clamp
        hx = max(rect.left(), min(rect.right(), hx))
        hy = max(rect.top(), min(rect.bottom(), hy))

        if is_ghost:
            painter.setPen(QPen(QColor(255, 255, 255, 150), 1, Qt.DotLine))
            painter.drawLine(center, QPointF(hx, hy))
            painter.setPen(QPen(Qt.white, 1))
            painter.setBrush(color)
            painter.drawEllipse(QPointF(hx, hy), 6, 6)
        else:
            painter.setPen(QPen(Qt.white, 2))
            if color.value() > 180 and color.saturation() < 80:
                 painter.setPen(QPen(Qt.black, 2))
            
            painter.setBrush(color)
            painter.drawEllipse(QPointF(hx, hy), 8, 8)

    def mousePressEvent(self, event: QMouseEvent):
        self.dragging = True
        self.update_color(event.position())

    def mouseMoveEvent(self, event: QMouseEvent):
        if self.dragging:
            self.update_color(event.position())

    def mouseReleaseEvent(self, event):
        self.dragging = False

    def update_color(self, local_pos):
        rect = self.rect().adjusted(self.margin, self.margin, -self.margin, -self.margin)
        center = rect.center()
        max_radius = math.sqrt((rect.width()/2)**2 + (rect.height()/2)**2)
        
        dx = local_pos.x() - center.x()
        dy = local_pos.y() - center.y()
        dist = math.sqrt(dx*dx + dy*dy)
        
        angle = math.atan2(dy, dx) + math.pi/2
        hue = angle / (2 * math.pi)
        if hue < 0: hue += 1
        if hue > 1: hue -= 1
        self.hue = hue

        dist_pct = min(1.0, dist / max_radius)
        
        if self.is_dark_mode:
            if dist_pct < 0.5:
                self.saturation = dist_pct * 2.0
                self.value = self.dm_max_val
            else:
                self.saturation = 1.0
                outer_prog = (dist_pct - 0.5) * 2.0 
                self.value = self.dm_max_val - (outer_prog * (self.dm_max_val - self.dm_min_val))
        else:
            self.saturation = dist_pct
            self.value = 1.0

        self.update()
        self.colorChanged.emit(self.get_color())

# --- Aurora Editor Panel ---
class AuroraEditorPanel(QFrame):
    def __init__(self, canvas: AuroraCanvas, parent=None):
        super().__init__(parent)
        self.canvas = canvas
        
        # Sync initial state from canvas if available
        if self.canvas:
            self.count = self.canvas.blob_count
            self.is_dark = self.canvas.is_dark_mode
            self.schema_idx = self.canvas.schema_index
        else:
            self.count = 1
            self.is_dark = True
            self.schema_idx = 1 # Default: Analogous
        
        # Map Index to Name for Label
        self.schema_names = {
            0: "Vibrant",
            1: "Analogous",
            2: "Contrast",
            3: "Neon"
        }
        
        self.setFixedWidth(240)
        
        # Use a style sheet that works well on top of the Aurora background or inside the settings
        # Providing a fallback background if not over Aurora
        self.setStyleSheet("""
            QFrame {
                background-color: rgba(20, 20, 20, 220);
                border: 1px solid rgba(255, 255, 255, 40);
                border-radius: 20px;
            }
            QLabel { color: white; font-weight: bold; font-size: 13px; border: none; background: transparent;}
            QPushButton {
                background-color: rgba(255,255,255,0.15);
                color: white; border: none;
                font-size: 14px; font-weight: bold;
            }
            QPushButton:hover { background-color: rgba(255,255,255,0.3); }
        """)

        layout = QVBoxLayout(self)
        layout.setAlignment(Qt.AlignCenter)

        # --- MODE TOGGLE ---
        mode_layout = QHBoxLayout()
        mode_layout.setSpacing(0)
        
        self.btn_light = QPushButton("Light")
        self.btn_light.setFixedHeight(32)
        self.btn_light.clicked.connect(lambda: self.set_mode(False))
        
        self.btn_dark = QPushButton("Dark")
        self.btn_dark.setFixedHeight(32)
        self.btn_dark.clicked.connect(lambda: self.set_mode(True))
        
        mode_layout.addWidget(self.btn_light)
        mode_layout.addWidget(self.btn_dark)
        
        layout.addLayout(mode_layout)
        layout.addSpacing(15)
        
        # Label
        layout.addWidget(QLabel("Primary Color"), 0, Qt.AlignCenter)
        layout.addSpacing(10)

        # Wheel
        self.wheel = AuroraColorWheel()
        # Connect to canvas safely if it exists
        if self.canvas:
             self.wheel.set_current_color(self.canvas.main_color) # Sync color
             self.wheel.colorChanged.connect(self.canvas.set_main_color)
        layout.addWidget(self.wheel, 0, Qt.AlignCenter)
        
        layout.addSpacing(20)

        # Controls Row
        row = QHBoxLayout()
        self.btn_minus = QPushButton("−")
        self.btn_minus.setFixedSize(30,30)
        self.btn_minus.setStyleSheet("border-radius: 15px;")
        self.btn_minus.clicked.connect(self.dec)
        
        self.lbl_count = QLabel("Solid")
        self.lbl_count.setFixedWidth(70) 
        self.lbl_count.setAlignment(Qt.AlignCenter)
        
        self.btn_plus = QPushButton("+")
        self.btn_plus.setFixedSize(30,30)
        self.btn_plus.setStyleSheet("border-radius: 15px;")
        self.btn_plus.clicked.connect(self.inc)

        # Switch Schema Button (Right of Plus)
        self.btn_schema = QPushButton("⟳")
        self.btn_schema.setFixedSize(30, 30)
        self.btn_schema.setStyleSheet("border-radius: 15px; font-size: 16px;")
        self.btn_schema.setToolTip("Switch Color Schema")
        self.btn_schema.clicked.connect(self.toggle_schema)

        row.addStretch()
        row.addWidget(self.btn_minus)
        row.addWidget(self.lbl_count)
        row.addWidget(self.btn_plus)
        row.addSpacing(10)
        row.addWidget(self.btn_schema) 
        row.addStretch()
        
        layout.addLayout(row)
        layout.addSpacing(10)

        # Init Mode UI with synced state
        self.set_mode(self.is_dark)
        
        # Init Label Text
        self.update_count_ui()

    def set_mode(self, is_dark):
        self.is_dark = is_dark
        if self.canvas:
             self.canvas.set_theme_mode(is_dark)
        self.update_config()
        
        # Styles
        style_base_light = "border-top-left-radius: 15px; border-bottom-left-radius: 15px;"
        style_base_dark = "border-top-right-radius: 15px; border-bottom-right-radius: 15px;"
        
        style_active = "background-color: white; color: black;"
        style_inactive = "background-color: rgba(255,255,255,0.15); color: white;"

        if is_dark:
            self.btn_dark.setStyleSheet(f"{style_base_dark} {style_active}")
            self.btn_light.setStyleSheet(f"{style_base_light} {style_inactive}")
        else:
            self.btn_light.setStyleSheet(f"{style_base_light} {style_active}")
            self.btn_dark.setStyleSheet(f"{style_base_dark} {style_inactive}")

    def inc(self):
        if self.count < 5:
            self.count += 1
            self.update_count_ui()

    def dec(self):
        if self.count > 1:
            self.count -= 1
            self.update_count_ui()

    def toggle_schema(self):
        # Cycle 0 -> 1 -> 2 -> 3 -> 0
        self.schema_idx = (self.schema_idx + 1) % 4
        if self.canvas:
             self.canvas.set_schema_index(self.schema_idx)
        self.update_count_ui()

    def update_config(self):
        # Update Wheel and Canvas state
        self.wheel.set_blob_config(self.count, self.is_dark, self.schema_idx)
        if self.canvas:
             self.canvas.set_blob_count(self.count) 

    def update_count_ui(self):
        # We display the count AND the current schema name if count > 1
        if self.count == 1:
            self.lbl_count.setText("Solid")
            self.btn_schema.setEnabled(False)
            self.btn_schema.setAlpha = 0.5
        else:
            # e.g. "3 | Vibrant"
            # Shorten names for label fitting
            s_name = self.schema_names[self.schema_idx]
            self.lbl_count.setText(f"{self.count} | {s_name}")
            self.btn_schema.setEnabled(True)
        
        self.update_config()
