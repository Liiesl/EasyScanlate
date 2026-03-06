
from PySide6.QtWidgets import QWidget
from PySide6.QtGui import QPainter, QRadialGradient, QColor, QBrush
from PySide6.QtCore import Qt, QSettings
from app.utils.background_utils import generate_aurora_palette

# --- The Smart Gradient Canvas ---
class AuroraCanvas(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.settings = QSettings("Liiesl", "EasyScanlate")
        self.load_settings()
        self.recalculate_blobs()

    def load_settings(self):
        # Color
        color_val = self.settings.value("aurora_color", "#3b0600")
        self.main_color = QColor(color_val)
        
        # Blobs
        self.blob_count = int(self.settings.value("aurora_blob_count", 2))
        
        # Mode
        mode_val = self.settings.value("aurora_dark_mode", "true")
        self.is_dark_mode = (str(mode_val).lower() == "true")
        
        # Schema
        self.schema_index = int(self.settings.value("aurora_schema_index", 1))

    def set_main_color(self, color):
        self.main_color = color
        self.settings.setValue("aurora_color", color.name())
        self.recalculate_blobs()
        self.update()

    def set_blob_count(self, count):
        self.blob_count = count
        self.settings.setValue("aurora_blob_count", count)
        self.recalculate_blobs()
        self.update()

    def set_theme_mode(self, is_dark):
        self.is_dark_mode = is_dark
        self.settings.setValue("aurora_dark_mode", "true" if is_dark else "false")
        self.recalculate_blobs()
        self.update()
        
    def set_schema_index(self, index):
        self.schema_index = index
        self.settings.setValue("aurora_schema_index", index)
        self.recalculate_blobs()
        self.update()

    def recalculate_blobs(self):
        # Pass schema index to generator
        self.blobs = generate_aurora_palette(self.main_color, self.blob_count, self.is_dark_mode, self.schema_index)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        w = self.width()
        h = self.height()

        base_c = QColor(self.main_color)
        if self.is_dark_mode:
            base_c.setHsv(base_c.hue(), base_c.saturation(), int(base_c.value() * 0.2))
        else:
            base_c.setHsv(base_c.hue(), int(base_c.saturation() * 0.1), 250)
            
        painter.fillRect(self.rect(), base_c)

        if self.blob_count == 1:
            overlay = QColor(self.main_color)
            overlay.setAlpha(100 if self.is_dark_mode else 50)
            painter.fillRect(self.rect(), overlay)
            return

        radius = max(w, h) * 0.85

        for blob in self.blobs:
            bx = blob["x_pct"] * w
            by = blob["y_pct"] * h
            grad = QRadialGradient(bx, by, radius)
            
            c = blob["color"]
            c_fade = QColor(c)
            
            if self.is_dark_mode:
                c.setAlpha(180)
                c_fade.setAlpha(0)
            else:
                c.setAlpha(120) 
                c_fade.setAlpha(0)
            
            grad.setColorAt(0.0, c)
            grad.setColorAt(1.0, c_fade)
            
            painter.setBrush(QBrush(grad))
            painter.setPen(Qt.NoPen)
            painter.drawRect(self.rect())
