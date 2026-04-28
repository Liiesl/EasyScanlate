# app/ui/components/vertical_toolbar.py

from PySide6.QtWidgets import QWidget, QVBoxLayout, QPushButton
from PySide6.QtCore import Signal, Qt
from PySide6.QtGui import QIcon
import qtawesome as qta

from app.ui.widgets.menus import Menu, ToggleButton
from assets import UNIVERSAL_STYLES


class VerticalToolbar(QWidget):
    """
    VS Code-style vertical toolbar on the far left.
    Owns all toolbar buttons and binds bidirectionally to ImageAreaViewModel.
    """

    settings_requested = Signal()

    def __init__(self, image_area_vm, parent=None):
        super().__init__(parent)
        self._vm = image_area_vm

        self.setObjectName("VerticalToolBar")
        self.setFixedWidth(50)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(5, 10, 5, 10)
        layout.setSpacing(10)
        layout.setAlignment(Qt.AlignTop)

        # --- Settings ---
        self.btn_settings = QPushButton(qta.icon("fa5s.cog", color="white"), "")
        self.btn_settings.setFixedSize(40, 40)
        self.btn_settings.setToolTip("Settings")
        self.btn_settings.clicked.connect(self.settings_requested.emit)
        layout.addWidget(self.btn_settings)

        layout.addSpacing(10)

        # --- Manual OCR ---
        self.btn_manual_ocr = QPushButton(QIcon("assets/icons/manual_ocr.svg"), "")
        self.btn_manual_ocr.setFixedSize(40, 40)
        self.btn_manual_ocr.setToolTip("Manual OCR Mode")
        self.btn_manual_ocr.setCheckable(True)
        self.btn_manual_ocr.toggled.connect(self._on_manual_ocr_toggled)
        self.btn_manual_ocr.setEnabled(False)
        layout.addWidget(self.btn_manual_ocr)

        # --- Toggle Text Visibility ---
        self.btn_toggle_text = ToggleButton(
            off_text="",
            on_text="",
            off_icon=qta.icon("fa5s.eye", color="white"),
            on_icon=qta.icon("fa5s.eye-slash", color="white"),
        )
        self.btn_toggle_text.setFixedSize(40, 40)
        self.btn_toggle_text.setToolTip("Toggle Text Visibility")
        self.btn_toggle_text.clicked.connect(self._vm.toggle_text_visibility)
        self.btn_toggle_text.setState(False)
        layout.addWidget(self.btn_toggle_text)

        # --- Context Fill Menu ---
        self.btn_context_fill_menu = QPushButton(qta.icon("fa5s.fill-drip", color="white"), "")
        self.btn_context_fill_menu.setFixedSize(40, 40)
        self.btn_context_fill_menu.setToolTip("Context Fill Options")
        self.btn_context_fill_menu.clicked.connect(self._show_context_fill_menu)
        layout.addWidget(self.btn_context_fill_menu)

        # --- Split ---
        self.btn_split = QPushButton(QIcon("assets/icons/split.svg"), "")
        self.btn_split.setFixedSize(40, 40)
        self.btn_split.setToolTip("Split Images")
        self.btn_split.clicked.connect(lambda: self._vm.start_action_mode("split"))
        layout.addWidget(self.btn_split)

        # --- Stitch ---
        self.btn_stitch = QPushButton(QIcon("assets/icons/stitch.svg"), "")
        self.btn_stitch.setFixedSize(40, 40)
        self.btn_stitch.setToolTip("Stitch Images")
        self.btn_stitch.clicked.connect(lambda: self._vm.start_action_mode("stitch"))
        layout.addWidget(self.btn_stitch)

        layout.addStretch()

        # --- VM signal bindings ---
        self._vm.text_visible_changed.connect(self._on_text_visibility_changed)
        self._vm.manual_ocr_mode_active_changed.connect(self._on_manual_ocr_mode_active_changed)
        self._vm.images_changed.connect(self._on_images_changed)

        self.setStyleSheet(UNIVERSAL_STYLES)

    # ------------------------------------------------------------------
    # Internal slots
    # ------------------------------------------------------------------
    def _on_manual_ocr_toggled(self, checked: bool):
        if checked:
            self._vm.start_action_mode("manual_ocr")
        else:
            self._vm.cancel_action_mode()

    def _on_manual_ocr_mode_active_changed(self, active: bool):
        """VM-driven sync: update toolbar button without re-emitting toggled."""
        self.btn_manual_ocr.blockSignals(True)
        self.btn_manual_ocr.setChecked(active)
        self.btn_manual_ocr.blockSignals(False)

    def _on_text_visibility_changed(self, visible: bool):
        """VM-driven sync: checked = hidden (eye-slash)."""
        self.btn_toggle_text.setChecked(not visible)

    def _on_images_changed(self, images: list):
        has_images = bool(images)
        self.btn_manual_ocr.setEnabled(has_images)

    def _show_context_fill_menu(self):
        """Creates, populates, and shows the Context Fill menu."""
        menu = Menu(self)

        btn_context_fill_mode = QPushButton(
            qta.icon("fa5s.fill-drip", color="white"), " Context Fill Mode"
        )
        btn_context_fill_mode.clicked.connect(
            lambda: self._vm.start_action_mode("inpaint")
        )
        menu.addButton(btn_context_fill_mode)

        btn_edit_context_fill = ToggleButton(
            off_text=" Edit Context Fills",
            on_text=" Edit Context Fills",
            off_icon=qta.icon("fa5s.paint-brush", color="white"),
            on_icon=qta.icon("fa5s.paint-brush", color="white"),
        )
        btn_edit_context_fill.setToolTip("Toggle Edit Context Fills")
        btn_edit_context_fill.setState(self._vm.inpaint_edit_mode_active)
        btn_edit_context_fill.toggled.connect(self._vm.toggle_inpaint_edit_mode)
        menu.addButton(btn_edit_context_fill, close_on_click=False)

        btn_toggle_fill_visibility = ToggleButton(
            off_text=" Show Fills",
            on_text=" Hide Fills",
            off_icon=qta.icon("fa5s.eye", color="white"),
            on_icon=qta.icon("fa5s.eye-slash", color="white"),
        )
        btn_toggle_fill_visibility.setToolTip("Toggle Fill Visibility")
        btn_toggle_fill_visibility.setState(self._vm.inpaints_visible)
        btn_toggle_fill_visibility.clicked.connect(self._vm.toggle_inpaint_visibility)
        menu.addButton(btn_toggle_fill_visibility, close_on_click=False)

        menu.set_position_and_show(self.btn_context_fill_menu, "right")
