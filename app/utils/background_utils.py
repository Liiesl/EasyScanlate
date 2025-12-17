
import math
from PySide6.QtGui import QColor

# --- Shared Math for Palette Generation ---
def generate_aurora_palette(main_color, count, is_dark_mode, schema_index=0):
    """
    Calculates the colors for the blobs based on a specific Color Theory Schema.
    """
    blobs = []
    
    h = main_color.hue()
    if h == -1: h = 0
    s = main_color.saturation()
    v = main_color.value()

    # --- 1. Position Logic (Canvas layout) ---
    positions = []
    if count == 1:
        positions = [(0.5, 0.5)]
    elif count == 2:
        positions = [(0.0, 0.0), (1.0, 1.0)]
    elif count == 3:
        positions = [(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)]
    elif count == 4:
        positions = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
    else:
        for i in range(count):
            ang = (2 * math.pi * i) / count
            positions.append((0.5 + 0.4*math.cos(ang), 0.5 + 0.4*math.sin(ang)))

    # --- 2. Color Schema Logic (Hue Offsets) ---
    # We calculate a hue offset for every blob index
    offsets = []
    
    # Schema 0: Vibrant (Original) - Moderate shifts
    if schema_index == 0:
        step = 40
        for i in range(count):
            shift = (math.ceil(i/2) * step)
            if i % 2 == 0: shift = -shift
            if i == 0: shift = 0
            offsets.append(shift)
            
    # Schema 1: Analogous - Tight, subtle shifts, calming
    elif schema_index == 1:
        step = 20
        for i in range(count):
            # 0, +20, -20, +40, -40
            shift = (math.ceil(i/2) * step)
            if i % 2 == 0: shift = -shift
            if i == 0: shift = 0
            offsets.append(shift)

    # Schema 2: High Contrast (Complementary / Triadic / Square / Pentadic)
    elif schema_index == 2:
        if count == 2:
            offsets = [0, 180] # Direct Complementary
        elif count == 3:
            offsets = [0, 120, 240] # Triadic
        elif count == 4:
            offsets = [0, 90, 180, 270] # Square Harmony
        else:
            # Pentadic Harmony (72 degree steps)
            offsets = []
            for i in range(count):
                offsets.append(i * 72)

    # Schema 3: Neon / Wild - Large steps, not necessarily opposite
    elif schema_index == 3:
        for i in range(count):
            offsets.append(i * 70) # 0, 70, 140, 210...

    # --- 3. Construct Blobs ---
    for i, pos in enumerate(positions):
        # Apply Hue Shift
        shift = offsets[i] if i < len(offsets) else 0
        new_h = (h + shift) % 360
        
        # Mode Adjustment (Brightness/Saturation clamping)
        if is_dark_mode:
            # Dark Mode Constraints
            base_v = v + 20 if i > 0 else v
            new_v = min(115, base_v) 
            new_s = s
        else:
            # Light Mode Constraints
            new_v = min(230, v) 
            new_s = max(s, 100) 
            
            # For "Neon" schema in light mode, bump saturation slightly
            if schema_index == 3 and i > 0:
                new_s = min(255, new_s + 30)

        color = QColor.fromHsv(int(new_h), int(new_s), int(new_v))
        
        blobs.append({
            "color": color,
            "h": new_h, "s": new_s, "v": new_v,
            "x_pct": pos[0],
            "y_pct": pos[1]
        })
    return blobs
