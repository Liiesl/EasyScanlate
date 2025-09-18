# Move these to app/styles.py
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

# --- REVISED: Find/Replace Stylesheet (Larger Fonts, Higher Contrast) ---
FIND_REPLACE_STYLESHEET = f"""
    #FindReplaceWidget {{
        background-color: {COLORS['surface']};
        border-bottom: 1px solid {COLORS['border']};
        padding: 4px 8px; /* Increased padding */
    }}
    /* Input fields */
    #FindReplaceWidget QLineEdit {{
        background-color: {COLORS['input_bg']};
        color: {COLORS['text']};
        border: 1px solid {COLORS['border']}; /* More visible border */
        border-radius: 3px; /* Match button radius */
        padding: 6px 8px; /* Increased padding */
        font-size: 15px; /* SIGNIFICANTLY Increased font size */
        min-height: 28px; /* Ensure height accommodates font + padding */
    }}
    /* Base style for all icon buttons within the widget */
    #FindReplaceWidget QPushButton {{
        background-color: transparent;
        color: {COLORS['icon_color']}; /* White icons */
        border: 1px solid transparent;
        border-radius: 3px;
        padding: 5px; /* Increased padding */
        /* Adjust size based on new font/padding */
        min-width: 30px; max-width: 30px;
        min-height: 30px; max-height: 30px;
    }}
    #FindReplaceWidget QPushButton:hover {{
        background-color: {COLORS['button_hover_bg']};
        border: 1px solid {COLORS['border']}; /* Add border on hover */
    }}
    #FindReplaceWidget QPushButton:pressed {{
        background-color: {COLORS['button_pressed']};
    }}
    #FindReplaceWidget QPushButton:disabled {{
        color: {COLORS['icon_disabled_color']};
        background-color: transparent;
        border-color: transparent;
    }}
    /* Style for checkable filter buttons when checked */
    #FindReplaceWidget QPushButton:checked {{
        background-color: {COLORS['button_checked_bg']}; /* Distinct checked bg */
        border: 1px solid {COLORS['button_checked_border']}; /* Brighter accent border */
        color: {COLORS['text']}; /* Keep icon white */
    }}
     /* Keep checked style on hover */
    #FindReplaceWidget QPushButton:checked:hover {{
        background-color: {COLORS['button_checked_bg']}; /* Keep same background */
        border: 1px solid {COLORS['button_checked_border']}; /* Keep same border */
    }}
    /* Specific override for Close button hover */
    #FindReplaceWidget QPushButton#CloseButton:hover {{
        background-color: {COLORS['error']};
        color: {COLORS['text']};
        border: 1px solid {COLORS['error']};
    }}
    /* Match count label */
    #FindReplaceWidget QLabel#MatchCountLabel {{
        color: {COLORS['text_secondary']}; /* Brighter secondary text */
        font-size: 14px; /* Increased label size */
        padding: 0 10px; /* More horizontal padding */
        min-width: 80px; /* Ensure enough space */
        background-color: transparent;
        /* border: 1px solid red; /* Debug */
    }}
    /* Container for replace row */
    #FindReplaceWidget QWidget#ReplaceRowWidget {{
         background-color: transparent;
         padding-top: 5px; /* More space */
    }}
"""

MAIN_STYLESHEET = """
            /* General background color */
            QMainWindow, QWidget {
                background-color: #1A1A1A;
                color: #FFFFFF;
                font-size: 20px;
            }
            /* Buttons style */
            QPushButton {
                background-color: #3A3A3A;
                color: #FFFFFF;
                border: none;
                padding: 10px;
                border-radius: 20px; 
            }
            QPushButton:hover {
                background-color: #4A4A4A;
            }
            QPushButton:pressed {
                background-color: #2A2A2A;
            }
                           
            /* Table styling */
            QTableWidget {
                background-color: #2D2D2D;
                gridline-color: "#3A3A3A";
                border-radius: 50px;
            }
            QHeaderView::section {
                background-color: "#3A3A3A";
                padding: 12px;
                border: none;
            }
            QTableWidget::item {
                padding: 2px;
            }
            QTableWidget::item.column-6 {  /* Target the 7th column (index 6) */
                background-color: transparent;
                border: none;
            }
            
            /* Ensure table buttons use the same style */
            QTableWidget QPushButton {
                background-color: #3A3A3A;
                color: #FFFFFF;
                border: none;
                padding: 10px;
                border-radius: 20px;
            }
            QTableWidget QPushButton:hover {
                background-color: #4A4A4A;
            }
            QTableWidget QPushButton:pressed {
                background-color: #2A2A2A;
            }
                           
            /* Scroll area style */
            QScrollArea {
                border: 20px solid #2A2A2A; /* Solid color border */
                border-top-right-radius: 50px; /* Rounded top-right corner */
                background-color: #2A2A2A; /* Background color */
            }
                           
            QScrollArea > QWidget > QWidget {  /* Target the viewport */
                background: transparent;
            }
                           
            /* Progress bar style */
            QProgressBar {
                background-color: #2A2A2A;
                color: #FFFFFF;
                border: 1px solid #3A3A3A;
                border-radius: 5px;
                text-align: center;
            }
            QProgressBar::chunk {
                background-color: #4A4A4A;
                width: 10px;
            }
            
            /* Tab Widget style */
            QTabWidget {
                background-color: #1A1A1A;
                border: none;
            }
            QTabWidget::pane {
                border: 1px solid #3A3A3A;
                border-radius: 10px;
                background-color: #2D2D2D;
            }
            QTabBar::tab {
                background-color: #3A3A3A;
                color: #FFFFFF;
                padding: 10px;
                border: none;
                border-radius: 5px;
            }
            QTabBar::tab:selected {
                background-color: #4A4A4A;
                margin-bottom: -1px; /* Ensure selected tab appears above the pane border */
            }
            QTabBar::tab:!selected {
                margin-top: 2px; /* Add some spacing between unselected tabs and the pane */
            }

            QSplitter {
                width: 30px;
            }
            
            QTableWidget {
                background-color: #1A1A1A;
            }
            /* Scroll Bar Styling */
            QScrollBar:vertical {
                background-color: #1A1A1A;
                width: 15px;
                margin: 0px;
                border-radius: 7px;
            }

            QScrollBar::handle:vertical {
                background-color: #4A4A4A;
                min-height: 30px;
                border-radius: 7px;
            }

            QScrollBar::handle:vertical:hover {
                background-color: #5A5A5A;
            }

            QScrollBar::handle:vertical:pressed {
                background-color: #007ACC;
            }

            QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {
                height: 0px;
            }

            QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical {
                background: none;
            }

            /* Horizontal Scroll Bar */
            QScrollBar:horizontal {
                background-color: #1A1A1A;
                height: 15px;
                margin: 0px;
                border-radius: 7px;
            }

            QScrollBar::handle:horizontal {
                background-color: #4A4A4A;
                min-width: 30px;
                border-radius: 7px;
            }

            QScrollBar::handle:horizontal:hover {
                background-color: #5A5A5A;
            }

            QScrollBar::handle:horizontal:pressed {
                background-color: #007ACC;
            }

            QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal {
                width: 0px;
            }

            QScrollBar::add-page:horizontal, QScrollBar::sub-page:horizontal {
                background: none;
            }
        """

IV_BUTTON_STYLES = """
            QPushButton {
                background-color: #3A3A3A;
                border: none;
                border-radius: 25px; 
            }
            QPushButton:hover {
                background-color: #4A4A4A;
            }
            QPushButton:pressed {
                background-color: #2A2A2A;
            }
        """

ADVANCED_CHECK_STYLES = """
            QCheckBox {
                color: #FFFFFF;
                font-size: 16px;
                spacing: 12px;
                padding: 4px;
            }
            QCheckBox::indicator {
                width: 22px;
                height: 22px;
                border-radius: 4px;
            }
            QCheckBox::indicator:unchecked {
                border: 2px solid #3A3A3A;
                background-color: #2A2A2A;
            }
            QCheckBox::indicator:unchecked:hover {
                border: 2px solid #4A4A4A;
                background-color: #333333;
            }
            QCheckBox::indicator:checked {
                border: 2px solid #007ACC;
                background-color: qlineargradient(x1:0, y1:0, x2:0, y2:1, stop:0 #0088EE, stop:1 #007ACC);
                image: url(data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxNCIgaGVpZ2h0PSIxNCIgdmlld0JveD0iMCAwIDI0IDI0IiBmaWxsPSJub25lIiBzdHJva2U9IiNGRkZGRkYiIHN0cm9rZS13aWR0aD0iMyIgc3Ryb2tlLWxpbmVjYXA9InJvdW5kIiBzdHJva2UtbGluZWpvaW49InJvdW5kIj48cG9seWxpbmUgcG9pbnRzPSIyMCA2IDkgMTcgNCAxMiI+PC9wb2x5bGluZT48L3N2Zz4=);
            }
            QCheckBox::indicator:checked:hover {
                border: 2px solid #0088EE;
                background-color: qlineargradient(x1:0, y1:0, x2:0, y2:1, stop:0 #0099FF, stop:1 #0088EE);
            }
            QCheckBox:disabled {
                color: #666666;
            }
            QCheckBox::indicator:disabled {
                border: 2px solid #444444;
                background-color: #333333;
            }
        """

RIGHT_WIDGET_STYLES = """
            #RightWidget {
                background-color: #2A2A2A;
                border: none;
                border-top-left-radius: 50px; /* Rounded top-left corner */
            }
            QWidget {
                background-color: #2A2A2A;
                border: none;
            }
            /* Buttons style */
            QPushButton {
                background-color: #3A3A3A;
                color: #FFFFFF;
                border: none;
                padding: 10px;
                border-radius: 20px; 
            }
            QPushButton:hover {
                background-color: #4A4A4A;
            }
            QPushButton:pressed {
                background-color: #2A2A2A;
            }
        """

SIMPLE_VIEW_STYLES = """
                QFrame {
                    background-color: #3A3A3A;
                    border-radius: 35px;
                    padding: 10px;
                    margin: 0;
                }
                QTextEdit {
                    color: white;
                    font-size: 20px;
                    background-color: transparent;
                    border: 1px solid #4A4A4A;
                    border-radius: 25px;
                    padding: 5px;
                }
                QTextEdit:hover {
                    border: 1px solid #007ACC;
                }
                QTextEdit:focus {
                    border: 2px solid #007ACC;
                }
                QPushButton {
                    background-color: #3A3A3A;
                    border: none;
                    border-radius: 34px;
                }
                QPushButton:hover {
                    background-color: #4A4A4A;
                }
                QPushButton:pressed {
                    background-color: #2A2A2A;
                }
            """

DELETE_ROW_STYLES = f"""
                QMessageBox {{
                    background-color: {COLORS['background']};
                    color: {COLORS['text']};
                    font-size: 16px;
                }}
                QLabel {{
                    color: {COLORS['text']};
                }}
                QCheckBox {{
                    color: {COLORS['text']};
                    spacing: 8px;
                }}
                QCheckBox::indicator {{
                    width: 18px;
                    height: 18px;
                }}
                QPushButton {{
                    background-color: {COLORS['primary']};
                    color: {COLORS['text']};
                    min-width: 80px;
                    padding: 8px;
                    border-radius: 4px;
                }}
                QPushButton:hover {{
                    background-color: {COLORS['secondary']};
                }}
            """

HOME_STYLES = """
            QMainWindow, QWidget {
                background-color: #1a1a1a;
                color: white;
            }
            QPushButton {
                background-color: #3E3E3E;
                color: white;
                border-radius: 15px;
                padding: 10px 15px;
                border: none;
                font-size: 14px;
            }
            QPushButton:hover {
                background-color: #4f4f4f;
            }
            QScrollArea {
                background-color: transparent;
                border: none;
            }
            QScrollBar:vertical {
                background: #1a1a1a;
                width: 10px;
                margin: 0px;
            }
            QScrollBar::handle:vertical {
                background: #444444;
                min-height: 20px;
                border-radius: 5px;
            }
            QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {
                height: 0px;
            }
        """

HOME_LEFT_LAYOUT_STYLES = """
                            QWidget {
                                background-color: #2B2B2B;
                                padding: 20px;
                                border: none;
                                border-top-right-radius: 20px;
                            }
                            QPushButton {
                                background-color: #3E3E3E;
                                color: white;
                                border-radius: 15px;
                                padding: 10px 15px;
                                border: none;
                                font-size: 14px;
                                min-height: 40px;
                            }
                            QPushButton:hover {
                                background-color: #4f4f4f;
                            }
                        """

NEW_PROJECT_STYLES = """
            QDialog {
                background-color: #2D2D2D;
                color: #FFFFFF;
                font-size: 14px;
            }
            QLineEdit {
                background-color: #3A3A3A;
                padding: 8px;
                border: 1px solid #4A4A4A;
                border-radius: 4px;
                margin: 5px 0;
            }
            QPushButton {
                background-color: #4A4A4A;
                color: #FFFFFF;
                padding: 8px 16px;
                border: none;
                border-radius: 4px;
                min-width: 80px;
            }
            QPushButton:hover {
                background-color: #5A5A5A;
            }
            QLabel {
                margin-top: 10px;
                color: #CCCCCC;
                background-color: none;
            }
            QComboBox {
                background-color: #3A3A3A;
                color: #FFFFFF;
                padding: 5px;
                border: 1px solid #4A4A4A;
                border-radius: 4px;
            }
            QComboBox::drop-down {
                border: none;
            }
            QComboBox QAbstractItemView {
                background-color: #3A3A3A;
                color: #FFFFFF;
                selection-background-color: #5A5A5A;
            }
        """

WFWF_STYLES = """
            QDialog {
                background-color: #2D2D2D;
                color: #FFFFFF;
                font-size: 14px;
            }
            QLineEdit {
                background-color: #3A3A3A;
                padding: 8px;
                border: 1px solid #4A4A4A;
                border-radius: 4px;
                margin: 5px 0;
            }
            QLabel {
                color: #CCCCCC;
            }
            QListWidget {
                background-color: #3A3A3A;
                color: #FFFFFF;
                border-radius: 4px;
                padding: 5px;
            }
            QPushButton {
                background-color: #4A4A4A;
                color: #FFFFFF;
                padding: 8px 16px;
                border: none;
                border-radius: 4px;
                min-width: 80px;
            }
            QPushButton:hover {
                background-color: #5A5A5A;
            }
            QPushButton:disabled {
                background-color: #3A3A3A;
                color: #999999;
            }
            QProgressBar {
                border: 1px solid #4A4A4A;
                border-radius: 4px;
                text-align: center;
                height: 20px;
            }
            QProgressBar::chunk {
                background-color: #5294e2;
                width: 1px;
            }
            QStatusBar {
                color: #CCCCCC;
            }
        """

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