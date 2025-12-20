# Move these to app/styles.py
# styles.py - QSS-based styling system with preprocessor
"""
This module provides a centralized styling system that uses QSS files with variables.
At runtime, QSS files are preprocessed to replace variables with actual values.
For distribution, preprocessed QSS files can be generated during build.
"""

import os
from .qss_variables_preprocessor import load_and_preprocess_qss

# Get the directory of this file
ASSETS_DIR = os.path.dirname(os.path.abspath(__file__))

# QSS file paths
VARIABLES_FILE = os.path.join(ASSETS_DIR, "variables.qssvars")
MAIN_QSS_FILE = os.path.join(ASSETS_DIR, "main.qss")
FIND_REPLACE_QSS_FILE = os.path.join(ASSETS_DIR, "find_replace.qss")

# Cache for processed stylesheets
_styles_cache = {}

def _load_stylesheet(qss_filename: str) -> str:
    """
    Load and preprocess a QSS file. Uses caching to avoid reprocessing.

    Args:
        qss_filename: Name of the QSS file (without path)

    Returns:
        Processed QSS content as string
    """
    if qss_filename in _styles_cache:
        return _styles_cache[qss_filename]

    qss_path = os.path.join(ASSETS_DIR, qss_filename)
    if not os.path.exists(qss_path):
        print(f"Warning: QSS file not found: {qss_path}")
        return ""

    processed = load_and_preprocess_qss(qss_path, VARIABLES_FILE)
    _styles_cache[qss_filename] = processed
    return processed

# Legacy compatibility - these can be removed once all references are updated
# For now, they return the processed QSS content

# Keep COLORS for backward compatibility (some code might still reference it)
COLORS = {
    "background": "#1E1E1E",
    "surface": "#2D2D2D",
    "primary": "#3A3A3A",     # Use for some backgrounds/buttons
    "secondary": "#4A4A4A",   # Use for hover/checked states
    "accent": "#007ACC",
    "text": "#FFFFFF",        # Main text/icons
    "text_secondary": "#E0E0E0", # Slightly brighter secondary text
    "border": "#606060",      # Slightly more visible border
    "error": "#c42b1c",
    "warning": "#DAA520",
    "input_bg": "#303030",    # Slightly lighter input background than surface
    "button_hover_bg": "#454545", # Clear hover background
    "button_checked_bg": "#505050", # Clear checked background, distinct from hover
    "button_checked_border": "#0090FF", # Brighter blue accent for checked border
    "button_pressed": "#252525", # Darker press
    "icon_color": "#FFFFFF",     # Make icons white by default for max contrast
    "icon_disabled_color": "#777777",
}

# Legacy stylesheets - these will be removed once all setStyleSheet calls are updated
# For now, they return the processed QSS content

ADVANCED_CHECK_STYLES = _load_stylesheet("advanced_check.qss")

HOME_STYLES = _load_stylesheet("home.qss")

HOME_LEFT_LAYOUT_STYLES = _load_stylesheet("home_left_layout.qss")

NEW_PROJECT_STYLES = _load_stylesheet("new_project.qss")

WFWF_STYLES = _load_stylesheet("wfwf.qss")

IMPORT_EXPORT_STYLES = _load_stylesheet("import_export.qss")

SIMPLE_VIEW_STYLES = _load_stylesheet("simple_view.qss")

from PySide6.QtGui import QColor

DEFAULT_GRADIENT = {
    'color1': QColor(255, 255, 255).name(QColor.HexArgb), # White
    'color2': QColor(200, 200, 200).name(QColor.HexArgb), # Lighter Gray
    'direction': 0, # Horizontal
    'midpoint': 0.5         # Example: 50%
}

DEFAULT_TEXT_STYLE = {
    # Shape
    'bubble_type': 1,  # Rounded Rectangle
    'corner_radius': 50,
    # Fill
    'fill_type': 'solid', # 'solid' or 'linear_gradient'
    'bg_color': QColor(255, 255, 255).name(QColor.HexArgb), # White solid fill
    'bg_gradient': DEFAULT_GRADIENT.copy(), # Default gradient fill (used if fill_type='linear_gradient')
    # Border
    'border_color': QColor(0, 0, 0, 0).name(QColor.HexArgb),     # Transparent Black (effectively no border)
    'border_width': 0,
    # Text
    'text_color_type': 'solid', # 'solid' or 'linear_gradient'
    'text_color': QColor(0, 0, 0).name(QColor.HexArgb),       # Black solid text
    'text_gradient': DEFAULT_GRADIENT.copy(), # Default gradient text (used if text_color_type='linear_gradient')
    # Font
    'font_family': "Anime Ace", # Default from TextBoxItem init, adjust if needed
    'font_style': "Regular",
    'font_size': 22, # Increased default size
    'font_bold': False,
    'font_italic': False,
    'text_alignment': 1,  # Center
    'auto_font_size': True,
}

def get_style_diff(current_style, default_style):
    """
    Returns a dictionary containing only the styles that differ from the default.
    Handles nested dictionaries like 'bg_gradient' and 'text_gradient'.
    """
    diff = {}
    all_keys = set(current_style.keys()) | set(default_style.keys())

    for key in all_keys:
        current_value = current_style.get(key)
        default_value = default_style.get(key)

        # Handle nested dictionaries (gradients)
        if isinstance(current_value, dict) and isinstance(default_value, dict):
            nested_diff = get_style_diff(current_value, default_value)
            if nested_diff: # Only add if there's a difference within the nested dict
                diff[key] = nested_diff
        # Handle regular value comparison
        elif current_value != default_value:
            # Convert QColor to string before storing in diff, if necessary
            # (Assuming input current_style might have QColor objects)
            if isinstance(current_value, QColor):
                diff[key] = current_value.name(QColor.HexArgb)
            else:
                diff[key] = current_value

    return diff

PROGRESS_STYLES = """QProgressBar {...}"""

RIGHT_PANEL_STYLES = _load_stylesheet("right_panel.qss")

UNIVERSAL_STYLES = _load_stylesheet("universal.qss")

STYLE_PANEL_STYLES = _load_stylesheet("style_panel.qss")
