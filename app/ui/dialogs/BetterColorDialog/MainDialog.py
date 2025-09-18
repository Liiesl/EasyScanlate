import sys
from PySide6.QtWidgets import (QDialog, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit, QPushButton, QWidget,
                             QDialogButtonBox, QGridLayout, QStyle, QTabWidget, QInputDialog, QFrame, QTabBar) # Added QInputDialog, QFrame
from PySide6.QtCore import (Qt, Signal, Slot, QSize, QPoint, QRegularExpression, QSettings)
from PySide6.QtGui import (QColor, QPixmap, QPainter, QRegularExpressionValidator, QIcon, QRegion)
# Import the new modular widgets from the helper file
from app.ui.dialogs.BetterColorDialog.Helper1 import HueRing, ColorSquare, CheckerboardWidget, EyedropperHelper, ColorSlidersWidget
from app.ui.dialogs.BetterColorDialog.ColorDialogStyles import CUSTOM_COLOR_DIALOG_V2_STYLESHEET

class CustomColorDialog(QDialog):
    """
    An enhanced, stylable color picker dialog with revised layout:
    Picker on left, controls (tabs, hex) below picker, previews/swatches on right.
    Swatches are now tabbed, with recent colors displayed below them.
    """
    colorSelected = Signal(QColor)

    # Constants for settings
    SETTINGS_GROUP = "CustomColorDialogRingV2_LayoutB"
    SETTINGS_RECENT_COLORS = "RecentColors"
    SETTINGS_SWATCHES_DATA = "SwatchesDataV2" # New key for tabbed swatches
    MAX_RECENT_COLORS = 12
    MAX_SWATCHES_PER_TAB = 24 # Swatches are now per-tab

    def __init__(self, initial_color=QColor(255, 255, 255), parent=None):
        super().__init__(parent)
        self._initial_color = QColor(initial_color)
        if not self._initial_color.isValid():
             self._initial_color = QColor(255, 255, 255) # Ensure valid start
        self._current_color = QColor(self._initial_color)
        self._updating_controls = False
        self._settings = QSettings("YourCompany", "MangaOCRTool")
        self._eyedropper_helper = None

        self.setWindowTitle("Select Color")
        self.setModal(True)
        self.setStyleSheet(CUSTOM_COLOR_DIALOG_V2_STYLESHEET)

        self._recent_colors = self._load_colors(self.SETTINGS_RECENT_COLORS, self.MAX_RECENT_COLORS)
        self._swatches_data = self._load_swatches_data() # New method for loading tabbed swatches

        # Widgets will be created in init_ui
        self.color_square = None
        self.hue_ring = None
        self.hex_edit = None
        self.color_sliders_widget = None # The new modular widget for sliders
        self.swatch_tabs = None # For Swatches
        self.eyedropper_button = None

        self.init_ui() # Builds the UI elements
        self._set_initial_control_values(self._current_color) # Set initial state

        # --- Connect signals AFTER initial state is set ---
        if self.color_square:
            self.color_square.svChanged.connect(self._square_sv_changed)
        if self.hue_ring:
            self.hue_ring.hueChanged.connect(self._hue_ring_changed)
        if self.hex_edit:
            self.hex_edit.textEdited.connect(self.hex_changed) # Connect hex input
        if self.color_sliders_widget:
            self.color_sliders_widget.colorChanged.connect(self._slider_widget_color_changed)


    def init_ui(self):
        # Overall structure: Horizontal layout
        # Left side: Vertical layout (Picker Area + Controls Area)
        # Right side: Vertical layout (Previews + Swatches/Recent + Buttons)
        main_h_layout = QHBoxLayout(self)
        main_h_layout.setSpacing(15)
        main_h_layout.setContentsMargins(15, 15, 15, 15)

        # --- Left Pane: Picker + Controls Below ---
        left_pane_v_layout = QVBoxLayout()
        left_pane_v_layout.setSpacing(15)

        # --- Picker Area (Ring + Square) ---
        picker_widget = QWidget()
        picker_widget.setObjectName("PickerContainer")
        picker_layout = QGridLayout(picker_widget)
        picker_layout.setContentsMargins(0, 0, 0, 0); picker_layout.setSpacing(0)

        self.hue_ring = HueRing()
        self.color_square = ColorSquare()

        ring_outer_dim = 300
        ring_width = self.hue_ring._ring_width
        ring_padding = self.hue_ring._padding
        inner_diameter = ring_outer_dim - 2 * (ring_width + ring_padding)
        square_dim = int(inner_diameter * 0.65)

        self.hue_ring.setFixedSize(ring_outer_dim, ring_outer_dim)
        self.color_square.setFixedSize(square_dim, square_dim)

        picker_layout.addWidget(self.hue_ring, 0, 0, Qt.AlignCenter)
        picker_layout.addWidget(self.color_square, 0, 0, Qt.AlignCenter)
        picker_widget.setFixedSize(ring_outer_dim, ring_outer_dim)

        left_pane_v_layout.addWidget(picker_widget, 0, Qt.AlignCenter)

        # --- Controls Area (Sliders + Hex/Eyedropper) ---
        controls_v_layout = QVBoxLayout()
        controls_v_layout.setSpacing(8)

        self.color_sliders_widget = ColorSlidersWidget()
        controls_v_layout.addWidget(self.color_sliders_widget)

        hex_eyedropper_widget = QWidget()
        hex_eyedropper_widget.setObjectName("HexEyedropperContainer")
        hex_eyedropper_layout = QHBoxLayout(hex_eyedropper_widget)
        hex_eyedropper_layout.setContentsMargins(10, 5, 10, 5)
        hex_eyedropper_layout.setSpacing(8)

        hex_label = QLabel("Hex:")
        self.hex_edit = QLineEdit()
        regex = QRegularExpression(r"^#[0-9A-Fa-f]{0,8}$")
        validator = QRegularExpressionValidator(regex)
        self.hex_edit.setValidator(validator)
        self.hex_edit.setMaxLength(9)
        self.hex_edit.setPlaceholderText("#RRGGBBAA")

        hex_eyedropper_layout.addWidget(hex_label)
        hex_eyedropper_layout.addWidget(self.hex_edit, 1)

        self.eyedropper_button = QPushButton()
        self.eyedropper_button.setObjectName("EyedropperButton")
        try:
            icon = QIcon.fromTheme("color-picker", QIcon.fromTheme("applications-graphics"))
            if icon.isNull(): raise ValueError("Theme icon not found")
        except Exception:
            icon = self.style().standardIcon(QStyle.SP_ComputerIcon)
        self.eyedropper_button.setIcon(icon)
        self.eyedropper_button.setToolTip("Pick color from screen")
        self.eyedropper_button.setFixedSize(30, 30)
        self.eyedropper_button.clicked.connect(self._activate_eyedropper)
        hex_eyedropper_layout.addWidget(self.eyedropper_button)

        controls_v_layout.addWidget(hex_eyedropper_widget)
        left_pane_v_layout.addLayout(controls_v_layout)
        left_pane_v_layout.addStretch(1)
        main_h_layout.addLayout(left_pane_v_layout)

        # --- Right Pane: Previews, Swatches/Recent, Buttons ---
        right_pane_v_layout = QVBoxLayout()
        right_pane_v_layout.setSpacing(10)

        # Preview Area (Comparison)
        preview_layout = QHBoxLayout(); preview_layout.setSpacing(5)
        self.preview_current_bg = CheckerboardWidget(); self.preview_new_bg = CheckerboardWidget()
        preview_current_layout = QVBoxLayout(self.preview_current_bg); preview_current_layout.setContentsMargins(0,0,0,0)
        self.preview_current_widget = QWidget(); self.preview_current_widget.setObjectName("ColorPreviewCurrent"); self.preview_current_widget.setAutoFillBackground(True)
        preview_current_layout.addWidget(self.preview_current_widget)
        preview_new_layout = QVBoxLayout(self.preview_new_bg); preview_new_layout.setContentsMargins(0,0,0,0)
        self.preview_new_widget = QWidget(); self.preview_new_widget.setObjectName("ColorPreviewNew"); self.preview_new_widget.setAutoFillBackground(True)
        preview_new_layout.addWidget(self.preview_new_widget)
        preview_layout.addWidget(self.preview_current_bg, 1); preview_layout.addWidget(self.preview_new_bg, 1)
        preview_area = QWidget(); preview_area_layout = QVBoxLayout(preview_area); preview_area_layout.setContentsMargins(0,0,0,0); preview_area_layout.setSpacing(2)
        preview_area_layout.addLayout(preview_layout)
        label_layout = QHBoxLayout(); label_layout.addWidget(QLabel("Original"), 1, Qt.AlignCenter); label_layout.addWidget(QLabel("New"), 1, Qt.AlignCenter)
        preview_area_layout.addLayout(label_layout)
        right_pane_v_layout.addWidget(preview_area)

        # Area for Tabbed Swatches and Recent Colors
        swatches_and_recent_area = self._create_swatches_and_recent_area()
        right_pane_v_layout.addWidget(swatches_and_recent_area, 1) # Allow vertical expansion

        # Buttons (OK/Cancel/Reset)
        button_layout = QHBoxLayout()
        self.reset_button = QPushButton("Reset"); self.reset_button.setObjectName("ResetButton")
        self.reset_button.clicked.connect(self._reset_color)
        button_layout.addWidget(self.reset_button)
        button_layout.addStretch(1)
        self.button_box = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        self.button_box.accepted.connect(self.accept_color)
        self.button_box.rejected.connect(self.reject)
        for button in self.button_box.buttons():
             button.setDefault(False); button.setAutoDefault(False)
        self.button_box.button(QDialogButtonBox.Ok).setDefault(True)

        button_layout.addWidget(self.button_box)
        right_pane_v_layout.addLayout(button_layout)

        right_pane_min_width = max(180, self.button_box.sizeHint().width() + self.reset_button.sizeHint().width() + button_layout.spacing() * 2)
        right_pane_widget = QWidget()
        right_pane_widget.setLayout(right_pane_v_layout)
        right_pane_widget.setMinimumWidth(right_pane_min_width)
        right_pane_widget.setMaximumWidth(right_pane_min_width + 60)

        main_h_layout.addWidget(right_pane_widget)


    def _create_swatches_and_recent_area(self):
        """Creates the container widget for tabbed swatches and recent colors below."""
        container = QWidget()
        layout = QVBoxLayout(container)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(10)

        # --- Tabbed Swatches Area ---
        swatches_label = QLabel("Swatches")
        swatches_label.setProperty("cssClass", "SubheadingLabel") # For styling

        self.swatch_tabs = QTabWidget()
        self.swatch_tabs.setObjectName("SwatchTabWidget") # For styling
        self.swatch_tabs.setTabsClosable(True)
        self.swatch_tabs.setMovable(True)
        self.swatch_tabs.tabCloseRequested.connect(self._close_swatch_tab)
        self.swatch_tabs.currentChanged.connect(self._handle_swatch_tab_change)
        self.swatch_tabs.tabBar().setElideMode(Qt.ElideRight)

        self._populate_all_swatch_tabs() # Create all tabs from data
        self._add_plus_tab() # Add the special "+" tab

        layout.addWidget(swatches_label)
        layout.addWidget(self.swatch_tabs)

        # --- Separator ---
        separator = QFrame()
        separator.setFrameShape(QFrame.HLine); separator.setFrameShadow(QFrame.Sunken)
        layout.addWidget(separator)

        # --- Recent Colors Area ---
        recent_label = QLabel("Recent")
        recent_label.setProperty("cssClass", "SubheadingLabel")
        recent_widget = QWidget()
        recent_layout = QVBoxLayout(recent_widget)
        recent_layout.setContentsMargins(0, 5, 0, 0); recent_layout.setSpacing(8)
        self.recent_grid_layout = QGridLayout(); self.recent_grid_layout.setSpacing(5)
        recent_layout.addLayout(self.recent_grid_layout)
        recent_layout.addStretch(1) # Push grid to the top
        self._populate_recent_grid(cols=5)

        layout.addWidget(recent_label)
        layout.addWidget(recent_widget)

        return container

    # --- Swatch/Recent Methods ---
    def _populate_all_swatch_tabs(self):
        """Clears and repopulates all swatch tabs from the self._swatches_data model."""
        self.swatch_tabs.blockSignals(True)
        while self.swatch_tabs.count() > 0:
            self.swatch_tabs.removeTab(0)

        for swatch_group in self._swatches_data:
            self._add_new_swatch_tab(swatch_group, set_as_current=False)
        self.swatch_tabs.blockSignals(False)

    def _add_new_swatch_tab(self, swatch_group, set_as_current=True, at_index=-1):
        """Adds a single swatch tab page based on a swatch_group dictionary."""
        tab_page = QWidget()
        page_layout = QVBoxLayout(tab_page)
        page_layout.setContentsMargins(5, 5, 5, 5); page_layout.setSpacing(8)

        # Grid for color buttons
        grid_layout = QGridLayout(); grid_layout.setSpacing(5)
        self._populate_grid(grid_layout, swatch_group['colors'], self.MAX_SWATCHES_PER_TAB, cols=5)
        page_layout.addLayout(grid_layout)

        # "Add Swatch" button at the bottom of the tab page
        add_button_layout = QHBoxLayout()
        add_button_layout.addStretch(1)
        add_swatch_button = QPushButton()
        add_swatch_button.setObjectName("AddSwatchButton")
        try:
            icon = QIcon.fromTheme("add", QIcon.fromTheme("list-add", self.style().standardIcon(QStyle.SP_DialogSaveButton)))
        except:
            icon = self.style().standardIcon(QStyle.SP_DialogSaveButton)
        add_swatch_button.setIcon(icon)
        add_swatch_button.setToolTip(f"Add current color to '{swatch_group['name']}'")
        add_swatch_button.setFixedSize(28, 28)
        # Connect to a method that knows which swatch group to modify
        add_swatch_button.clicked.connect(lambda chk=False, sg=swatch_group: self._add_color_to_swatch_group(sg))
        add_button_layout.addWidget(add_swatch_button)
        add_button_layout.addStretch(1)
        page_layout.addLayout(add_button_layout)

        page_layout.addStretch(1) # Push grid and button up

        # Insert the tab at the specified index or add to the end
        if at_index == -1:
            index = self.swatch_tabs.addTab(tab_page, swatch_group['name'])
        else:
            index = self.swatch_tabs.insertTab(at_index, tab_page, swatch_group['name'])

        if set_as_current:
            self.swatch_tabs.setCurrentIndex(index)
        return index

    def _add_plus_tab(self):
        """Adds the special '+' tab to the end of the swatch tabs."""
        plus_button_tab = QWidget() # Placeholder, content is irrelevant
        index = self.swatch_tabs.addTab(plus_button_tab, "+")
        self.swatch_tabs.setTabToolTip(index, "Add a new swatch set")
        # The '+' tab should not have a close button
        self.swatch_tabs.tabBar().setTabButton(index, QTabBar.RightSide, None)

    @Slot(int)
    def _handle_swatch_tab_change(self, index):
        """Handles clicks on the swatch tabs, specifically activating the '+' tab."""
        if index < 0: return

        plus_tab_index = self.swatch_tabs.count() - 1
        if index == plus_tab_index and self.swatch_tabs.tabText(index) == "+":
            # Temporarily switch focus away from the '+' tab
            self.swatch_tabs.setCurrentIndex(max(0, plus_tab_index - 1))

            text, ok = QInputDialog.getText(self, 'New Swatch Set', 'Enter name for the new set:')
            if ok and text.strip():
                new_swatch_group = {'name': text.strip(), 'colors': []}
                self._swatches_data.append(new_swatch_group)
                # Add the new tab before the '+' tab
                self._add_new_swatch_tab(new_swatch_group, set_as_current=True, at_index=plus_tab_index)
                self._save_swatches_data()

    @Slot(int)
    def _close_swatch_tab(self, index):
        """Removes the swatch tab at the given index from the model and UI."""
        # Prevent closing if it's the last real tab
        if self.swatch_tabs.count() <= 2: # 1 real tab + '+' tab
            return

        # Remove from data model first
        del self._swatches_data[index]
        # Then remove from widget
        self.swatch_tabs.removeTab(index)
        self._save_swatches_data()

    def _create_swatch_button(self, color):
        button = QPushButton()
        button.setObjectName("SwatchButton")
        button.setProperty("cssClass", "SwatchButton")
        button.setToolTip(color.name(QColor.HexArgb).upper())

        checker_bg = CheckerboardWidget(); checker_bg.setFixedSize(30, 30)
        pm_checker = QPixmap(checker_bg.size()); pm_checker.fill(Qt.transparent)
        checker_bg.render(pm_checker, QPoint(), QRegion(checker_bg.rect()))

        pm_final = QPixmap(pm_checker.size()); pm_final.fill(Qt.transparent)
        p = QPainter(pm_final)
        p.drawPixmap(0, 0, pm_checker)
        p.fillRect(pm_final.rect(), color)
        p.end()

        button.setIcon(QIcon(pm_final))
        button.setIconSize(QSize(28, 28))
        button.clicked.connect(lambda checked=False, c=QColor(color): self._swatch_clicked(c))
        return button

    @Slot(QColor)
    def _swatch_clicked(self, color):
        if color.isValid():
            self.update_controls(color, source="swatch")

    def _populate_grid(self, grid_layout, colors, max_items, cols=6):
        while grid_layout.count():
            item = grid_layout.takeAt(0); widget = item.widget()
            if widget: widget.deleteLater()
        row, col = 0, 0
        for i, color_hex in enumerate(colors):
            if i >= max_items: break
            color = QColor(color_hex)
            if color.isValid():
                swatch_button = self._create_swatch_button(color)
                grid_layout.addWidget(swatch_button, row, col)
                col += 1
                if col >= cols: col = 0; row += 1

    def _populate_recent_grid(self, cols=6):
        self._populate_grid(self.recent_grid_layout, self._recent_colors, self.MAX_RECENT_COLORS, cols=cols)

    def _load_swatches_data(self):
        """Loads tabbed swatch data, with backward compatibility for old format."""
        self._settings.beginGroup(self.SETTINGS_GROUP)
        data = self._settings.value(self.SETTINGS_SWATCHES_DATA, None)
        if data is None: # Check for old format for backward compatibility
            old_data = self._settings.value("Swatches", None)
            if old_data is not None:
                valid_colors = [c for c in old_data if isinstance(c, str) and QColor.isValidColor(c)][:self.MAX_SWATCHES_PER_TAB]
                data = [{'name': 'Imported', 'colors': valid_colors}]
        self._settings.endGroup()

        if data is None: # Provide a default if no settings exist at all
            return [{'name': 'Default', 'colors': []}]

        # Validate the loaded data structure
        if isinstance(data, list) and all(isinstance(i, dict) and 'name' in i and 'colors' in i for i in data):
            for item in data:
                item['colors'] = [c for c in item.get('colors', []) if isinstance(c, str) and QColor.isValidColor(c)][:self.MAX_SWATCHES_PER_TAB]
            return data

        return [{'name': 'Default', 'colors': []}] # Return default if data is corrupt

    def _load_colors(self, key, max_items):
        self._settings.beginGroup(self.SETTINGS_GROUP)
        colors = self._settings.value(key, [], type=list)
        self._settings.endGroup()
        valid_colors = [c for c in colors if isinstance(c, str) and QColor.isValidColor(c)]
        return valid_colors[:max_items]

    def _save_colors(self, key, colors):
        self._settings.beginGroup(self.SETTINGS_GROUP); self._settings.setValue(key, colors); self._settings.endGroup()

    def _save_swatches_data(self):
        self._save_colors(self.SETTINGS_SWATCHES_DATA, self._swatches_data)

    def _add_color_to_swatch_group(self, swatch_group):
        """Adds the current color to the specified swatch group's color list."""
        if not self._current_color.isValid(): return
        color_list = swatch_group['colors']
        hex_color = self._current_color.name(QColor.HexArgb).upper()

        if hex_color in color_list: color_list.remove(hex_color)
        color_list.insert(0, hex_color)
        swatch_group['colors'] = color_list[:self.MAX_SWATCHES_PER_TAB] # Trim list
        self._save_swatches_data()

        # Find the tab associated with this swatch_group and refresh its grid
        for i in range(len(self._swatches_data)):
            if self._swatches_data[i] is swatch_group:
                tab_page = self.swatch_tabs.widget(i)
                grid_layout = tab_page.findChild(QGridLayout)
                if grid_layout:
                    self._populate_grid(grid_layout, swatch_group['colors'], self.MAX_SWATCHES_PER_TAB, cols=5)
                break

    def _add_color_to_recent(self, color):
        if not color.isValid(): return
        hex_color = color.name(QColor.HexArgb).upper()
        if hex_color in self._recent_colors: self._recent_colors.remove(hex_color)
        self._recent_colors.insert(0, hex_color)
        self._recent_colors = self._recent_colors[:self.MAX_RECENT_COLORS]
        self._save_colors(self.SETTINGS_RECENT_COLORS, self._recent_colors)
        self._populate_recent_grid(cols=self.recent_grid_layout.columnCount() or 5)

    # --- Control Update Logic (Unchanged from previous refactor) ---
    def _get_current_hue(self):
        return self.hue_ring.getHue() if self.hue_ring else 0
    def _get_current_sv(self):
        return (self.color_square._saturation, self.color_square._value) if self.color_square else (0, 255)
    def _get_current_alpha(self):
        return self.color_sliders_widget.get_alpha() if self.color_sliders_widget else 255
    def _set_initial_control_values(self, color):
        if not all([self.color_square, self.hue_ring, self.color_sliders_widget, self.hex_edit]): return
        self._updating_controls = True
        if hasattr(self.hue_ring, 'setHue'): self.hue_ring.setHue(color.hueF() * 359)
        if hasattr(self.color_square, 'setColor'): self.color_square.setColor(color)
        self.color_sliders_widget.setColor(color)
        if self.hex_edit: self.hex_edit.setText(color.name(QColor.HexArgb).upper())
        self.update_preview(color, self._initial_color)
        self._updating_controls = False
    def update_controls(self, color, source=None):
        if self._updating_controls or not isinstance(color, QColor) or not color.isValid(): return
        if self._current_color and self._current_color.isValid() and color.rgba() == self._current_color.rgba(): return
        self._updating_controls = True
        self._current_color = QColor(color)
        h, s, v, _ = self._current_color.getHsv()
        current_picker_hue = self._get_current_hue()
        display_hue = h if h != -1 else current_picker_hue
        if source != "hue_ring" and hasattr(self.hue_ring, 'setHue'): self.hue_ring.setHue(display_hue)
        if source != "square":
             if hasattr(self.color_square, 'setHue') and (h != -1 or (hasattr(self.color_square,'_hue') and self.color_square._hue != display_hue)):
                 self.color_square.setHue(display_hue)
             if hasattr(self.color_square, 'setSaturationValue'): self.color_square.setSaturationValue(s, v)
        self.update_preview(self._current_color)
        if source != "sliders_widget" and self.color_sliders_widget: self.color_sliders_widget.setColor(self._current_color)
        if source != "hex" and self.hex_edit:
            hex_val = self._current_color.name(QColor.HexArgb).upper()
            if self.hex_edit.text().upper() != hex_val: self.hex_edit.setText(hex_val)
        self._updating_controls = False
    def update_preview(self, new_color, initial_color=None):
        if not hasattr(self, 'preview_current_widget') or not hasattr(self, 'preview_new_widget'): return
        if initial_color is not None and initial_color.isValid():
            self.preview_current_widget.setStyleSheet(f"#ColorPreviewCurrent {{ background-color: {initial_color.name(QColor.HexArgb)}; }}")
            self.preview_current_bg.update()
        if new_color and new_color.isValid():
            self.preview_new_widget.setStyleSheet(f"#ColorPreviewNew {{ background-color: {new_color.name(QColor.HexArgb)}; }}")
            self.preview_new_bg.update()

    # --- Signal Handlers (Unchanged from previous refactor) ---
    @Slot(int, int)
    def _square_sv_changed(self, saturation, value):
        if self._updating_controls: return
        new_color = QColor.fromHsv(self._get_current_hue(), saturation, value, self._get_current_alpha())
        self.update_controls(new_color, source="square")
    @Slot(int)
    def _hue_ring_changed(self, hue):
        if self._updating_controls: return
        if hasattr(self.color_square, 'setHue'): self.color_square.setHue(hue)
        s, v = self._get_current_sv()
        new_color = QColor.fromHsv(hue, s, v, self._get_current_alpha())
        self.update_controls(new_color, source="hue_ring")
    @Slot(QColor)
    def _slider_widget_color_changed(self, color):
        if self._updating_controls: return
        self.update_controls(color, source="sliders_widget")
    @Slot(str)
    def hex_changed(self, text):
        if self._updating_controls or not self.hex_edit: return
        processed_text = text.strip().upper()
        needs_hash = False
        if not processed_text.startswith('#') and len(processed_text) in [3, 4, 6, 8] and all(c in '0123456789ABCDEF' for c in processed_text):
            needs_hash = True
        candidate_text = ('#' + processed_text) if needs_hash else processed_text
        temp_color = QColor(candidate_text)
        if temp_color.isValid():
            if needs_hash:
                self.hex_edit.blockSignals(True)
                cursor_pos = self.hex_edit.cursorPosition()
                self.hex_edit.setText(candidate_text)
                self.hex_edit.setCursorPosition(min(cursor_pos + 1, len(candidate_text)))
                self.hex_edit.blockSignals(False)
            if len(candidate_text[1:]) in [3, 6]:
                if temp_color.alpha() != self._get_current_alpha(): temp_color.setAlpha(self._get_current_alpha())
            self.update_controls(temp_color, source="hex")
    @Slot()
    def _reset_color(self):
        self.update_controls(self._initial_color, source="reset")

    # --- Eyedropper Methods (Unchanged) ---
    @Slot()
    def _activate_eyedropper(self):
        if self._eyedropper_helper and self._eyedropper_helper._active:
            self._eyedropper_helper.stop(); self._eyedropper_helper.deleteLater(); self._eyedropper_helper = None
        self._eyedropper_helper = EyedropperHelper(self)
        self._eyedropper_helper.colorSelected.connect(self._handle_eyedropper_result)
        self._eyedropper_helper.cancelled.connect(self._handle_eyedropper_result)
        self._eyedropper_helper.start()
    @Slot(QColor)
    @Slot()
    def _handle_eyedropper_result(self, color=None):
        sender_helper = self.sender()
        if sender_helper and isinstance(sender_helper, EyedropperHelper):
            sender_helper.stop(); sender_helper.deleteLater()
            if self._eyedropper_helper == sender_helper: self._eyedropper_helper = None
        if color is not None and isinstance(color, QColor) and color.isValid():
            color.setAlpha(self._get_current_alpha())
            self.update_controls(color, source="eyedropper")

    # --- Dialog Actions (Unchanged) ---
    def accept_color(self):
        if self._current_color.isValid():
            self._add_color_to_recent(self._current_color)
            self.colorSelected.emit(self._current_color)
            self.accept()
        else:
             print("Warning: Tried to accept an invalid color.", file=sys.stderr)
             self.reject()
    def selected_color(self):
        return self._current_color if self._current_color and self._current_color.isValid() else QColor()

    # --- Static Method (Unchanged) ---
    @staticmethod
    def getColor(initial_color=QColor(255, 255, 255), parent=None):
        dialog = CustomColorDialog(initial_color, parent)
        result = dialog.exec_()
        selected = dialog.selected_color()
        if result == QDialog.Accepted and selected.isValid():
            return selected
        else:
            return dialog._initial_color