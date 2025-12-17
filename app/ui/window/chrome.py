import sys
import ctypes
from PySide6.QtWidgets import QWidget, QHBoxLayout, QLabel, QPushButton
from PySide6.QtCore import Qt, QPoint, QObject, QEvent, QRect
from PySide6.QtGui import QCursor, QPixmap
import qtawesome

from app.ui.widgets.menu_bar import MenuBar, TitleBarState

# OS DETECTION & WINDOWS API SETUP
IS_WINDOWS = sys.platform == "win32"

if IS_WINDOWS:
    from ctypes import wintypes
    user32 = ctypes.windll.user32
    dwmapi = ctypes.windll.dwmapi

    # Windows Constants
    WM_NCCALCSIZE = 0x0083
    WM_NCHITTEST = 0x0084
    WM_NCLBUTTONDOWN = 0x00A1  # <--- NEW
    WM_NCLBUTTONUP = 0x00A2    # <--- NEW

    GWL_STYLE = -16
    WS_CAPTION = 0x00C00000
    WS_THICKFRAME = 0x00040000
    WS_MAXIMIZEBOX = 0x00010000
    WS_MINIMIZEBOX = 0x00020000
    
    HTNOWHERE = 0
    HTCLIENT = 1
    HTCAPTION = 2
    HTMAXBUTTON = 9            # <--- NEW
    HTLEFT = 10
    HTRIGHT = 11
    HTTOP = 12
    HTTOPLEFT = 13
    HTTOPRIGHT = 14
    HTBOTTOM = 15
    HTBOTTOMLEFT = 16
    HTBOTTOMRIGHT = 17

    class RECT(ctypes.Structure):
        _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long), 
                    ("right", ctypes.c_long), ("bottom", ctypes.c_long)]

# UPDATED CUSTOM TITLE BAR
class CustomTitleBar(QWidget):
    def __init__(self, parent):
        super().__init__(parent)
        self.setObjectName("CustomTitleBar")
        self.parent = parent
        self.setFixedHeight(35)
        self.layout = QHBoxLayout(self)
        self.layout.setContentsMargins(0, 0, 0, 0)
        self.layout.setSpacing(0)

        self.icon_label = QLabel()
        self.icon_label.setPixmap(QPixmap("assets/app_icon.ico").scaled(20, 20, Qt.KeepAspectRatio, Qt.SmoothTransformation))
        self.icon_label.setStyleSheet("padding-left: 10px; margin-right: 5px;")
        
        self.title = QLabel("EasyScanlate")
        self.title.setStyleSheet("background-color: #1E1E1E; color: white; font-weight: bold;")

        self.menu_bar = None
        self.btn_close = QPushButton()
        self.btn_maximize = QPushButton()
        self.btn_minimize = QPushButton()

        # Icons
        icon_color, icon_hover = "#AAAAAA", "white"
        self.icon_close = qtawesome.icon('mdi.close', color=icon_color, color_active=icon_hover)
        self.icon_minimize = qtawesome.icon('msc.chrome-minimize', color=icon_color, color_active=icon_hover)
        self.icon_maximize = qtawesome.icon('msc.chrome-maximize', color=icon_color, color_active=icon_hover)
        self.icon_restore = qtawesome.icon('msc.chrome-restore', color=icon_color, color_active=icon_hover)
        
        self.btn_close.setIcon(self.icon_close)
        self.btn_minimize.setIcon(self.icon_minimize)
        self.btn_maximize.setIcon(self.icon_maximize)

        # Logic
        self.btn_close.clicked.connect(self.parent.close)
        self.btn_maximize.clicked.connect(self.toggle_max)
        self.btn_minimize.clicked.connect(self.parent.showMinimized)

        for btn in [self.btn_close, self.btn_maximize, self.btn_minimize]:
            btn.setFixedSize(35, 35)
            # Initial Style
            btn.setStyleSheet("QPushButton { background: transparent; border: none; } QPushButton:hover { background: #3E3E3E; }")
            
        self.btn_close.setStyleSheet(self.btn_close.styleSheet().replace("#3E3E3E", "#E81123"))

        # FIX: Make max button transparent to mouse so Native events catch it easily
        if IS_WINDOWS:
            self.btn_maximize.setAttribute(Qt.WA_TransparentForMouseEvents)

        self.layout.addStretch()
        self.layout.addWidget(self.icon_label)
        self.layout.addWidget(self.title)
        self.layout.addStretch() 
        self.layout.addWidget(self.btn_minimize)
        self.layout.addWidget(self.btn_maximize)
        self.layout.addWidget(self.btn_close)

        self.setState(TitleBarState.HOME)
        self.parent.installEventFilter(self)

        # Software dragging for Linux/Mac
        self.pressing = False
        self.start_pos = QPoint(0,0)

    def setState(self, state):
        if self.menu_bar:
            self.layout.removeWidget(self.menu_bar)
            self.menu_bar.deleteLater()
            self.menu_bar = None
        if state != TitleBarState.NON_MAIN:
            self.menu_bar = MenuBar(self.parent, state)
            self.layout.insertWidget(0, self.menu_bar)

    def toggle_max(self):
        # FIX: Check both Maximized and FullScreen flags
        if self.parent.windowState() & (Qt.WindowMaximized | Qt.WindowFullScreen):
            self.parent.showNormal()
        else:
            self.parent.showMaximized()

    def update_maximize_icon(self):
        is_max = self.parent.windowState() & (Qt.WindowMaximized | Qt.WindowFullScreen)
        self.btn_maximize.setIcon(self.icon_restore if is_max else self.icon_maximize)
    
    # --- NEW: Helper to manually style the button since Qt events are bypassed ---
    def set_max_btn_style(self, state):
        if state == 'hover':
            self.btn_maximize.setStyleSheet("QPushButton { background: #3E3E3E; border: none; }")
        elif state == 'pressed':
            self.btn_maximize.setStyleSheet("QPushButton { background: #505050; border: none; }")
        else: # normal
            self.btn_maximize.setStyleSheet("QPushButton { background: transparent; border: none; }")

    def eventFilter(self, obj, event):
        if obj == self.parent and event.type() == QEvent.WindowStateChange:
            self.update_maximize_icon()
        return super().eventFilter(obj, event)

    # --- Linux/Mac Software Dragging ---
    def mousePressEvent(self, event):
        if IS_WINDOWS: return
        child = self.childAt(event.pos())
        if event.button() == Qt.LeftButton and (not child or child in [self.title, self.icon_label]):
            self.pressing = True
            self.start_pos = event.globalPosition().toPoint() - self.parent.frameGeometry().topLeft()

    def mouseMoveEvent(self, event):
        if not IS_WINDOWS and self.pressing:
            self.parent.move(event.globalPosition().toPoint() - self.start_pos)

    def mouseReleaseEvent(self, event):
        self.pressing = False

# INTEGRATED WINDOW RESIZER (Cross-Platform)
class WindowResizer(QObject):
    def __init__(self, window):
        super().__init__(window)
        self.window = window
        self.margin = 5
        self._max_hovering = False # Track hover state for max button
        
        # 1. Initialize Window Styles
        self.window.setWindowFlags(Qt.Window | Qt.FramelessWindowHint | Qt.WindowMinMaxButtonsHint)
        
        if IS_WINDOWS:
            # Enable native Windows features (Shadows, Snapping)
            hwnd = int(self.window.winId())
            style = user32.GetWindowLongW(hwnd, GWL_STYLE)
            user32.SetWindowLongW(hwnd, GWL_STYLE, style | WS_CAPTION | WS_THICKFRAME | WS_MAXIMIZEBOX | WS_MINIMIZEBOX)
            
            # Apply Windows 11 Rounded Corners
            try:
                if sys.getwindowsversion().build >= 22000:
                    pref = ctypes.c_int(2) # DWMWCP_ROUND
                    dwmapi.DwmSetWindowAttribute(wintypes.HWND(hwnd), 33, ctypes.byref(pref), 4)
            except: pass
        else:
            # 2. Initialize Software Resizing for Linux/Mac
            self.window.setMouseTracking(True)
            self.window.installEventFilter(self)
            self.resizing = False
            self.resize_edge = None

    def handle_windows_native(self, message):
        """
        Logic for MainWindow's nativeEvent. 
        Returns (handled: bool, result: int)
        """
        if not IS_WINDOWS: return False, 0
        
        msg = wintypes.MSG.from_address(int(message))
        
        if msg.message == WM_NCCALCSIZE:
            return True, 0 # Removes standard titlebar area

        # --- 1. HANDLE BUTTON CLICK SEQUENCE (Manually) ---
        # Find Title Bar instance
        tb = self.window.findChild(CustomTitleBar)
        
        if msg.message == WM_NCLBUTTONDOWN:
            if msg.wParam == HTMAXBUTTON and tb:
                tb.set_max_btn_style('pressed')
                return True, 0

        if msg.message == WM_NCLBUTTONUP:
            if msg.wParam == HTMAXBUTTON and tb:
                tb.set_max_btn_style('hover')
                tb.toggle_max()
                return True, 0

        if msg.message == WM_NCHITTEST:
            x_phys = msg.lParam & 0xFFFF
            y_phys = (msg.lParam >> 16) & 0xFFFF
            if x_phys > 32767: x_phys -= 65536
            if y_phys > 32767: y_phys -= 65536
            
            # Get Window Rect
            rect = RECT()
            user32.GetWindowRect(int(self.window.winId()), ctypes.byref(rect))
            
            # Local coordinates relative to window
            lx = x_phys - rect.left
            ly = y_phys - rect.top
            w = rect.right - rect.left
            h = rect.bottom - rect.top
            
            # FIX: Robust check for maximized state (Maximized OR FullScreen)
            is_max = self.window.windowState() & (Qt.WindowMaximized | Qt.WindowFullScreen)

            # --- 2. CHECK MAXIMIZE BUTTON (SNAP LAYOUT SUPPORT) ---
            if tb and tb.btn_maximize.isVisible():
                btn = tb.btn_maximize
                btn_local_pos = btn.mapTo(self.window, QPoint(0, 0))
                dpr = self.window.devicePixelRatio()
                
                b_x = int(btn_local_pos.x() * dpr)
                b_y = int(btn_local_pos.y() * dpr)
                b_w = int(btn.width() * dpr)
                b_h = int(btn.height() * dpr)

                if (b_x <= lx < b_x + b_w) and (b_y <= ly < b_y + b_h):
                    if not self._max_hovering:
                        self._max_hovering = True
                        tb.set_max_btn_style('hover')
                    return True, HTMAXBUTTON
                else:
                    if self._max_hovering:
                        self._max_hovering = False
                        tb.set_max_btn_style('normal')

            # --- 3. STANDARD RESIZE BORDERS ---
            if not is_max:
                if ly < self.margin:
                    if lx < self.margin: return True, HTTOPLEFT
                    if lx > w - self.margin: return True, HTTOPRIGHT
                    return True, HTTOP
                if ly > h - self.margin:
                    if lx < self.margin: return True, HTBOTTOMLEFT
                    if lx > w - self.margin: return True, HTBOTTOMRIGHT
                    return True, HTBOTTOM
                if lx < self.margin: return True, HTLEFT
                if lx > w - self.margin: return True, HTRIGHT

            # --- 4. TITLE BAR DRAGGING ---
            if tb and tb.geometry().contains(int(lx / self.window.devicePixelRatio()), int(ly / self.window.devicePixelRatio())):
                # If maximized, return False so Qt handles drag-to-restore
                if is_max:
                    return False, 0
                    
                local_pos = tb.mapFromGlobal(QCursor.pos())
                child = tb.childAt(local_pos)
                
                # If clicking other buttons (Close/Min) or MenuBar
                if isinstance(child, QPushButton) or (tb.menu_bar and tb.menu_bar.geometry().contains(local_pos)):
                    return True, HTCLIENT 
                
                return True, HTCAPTION 
                
        return False, 0

    # --- Software Resizing for Linux / Mac ---
    def eventFilter(self, obj, event):
        if IS_WINDOWS or obj is not self.window: return False
        
        if event.type() == QEvent.MouseButtonPress and event.button() == Qt.LeftButton:
            edge = self._get_edge(event.pos())
            if edge:
                self.resizing = True
                self.resize_edge = edge
                self.start_pos = event.globalPosition().toPoint()
                self.start_geo = self.window.geometry()
                return True
        elif event.type() == QEvent.MouseMove:
            if self.resizing:
                self._do_resize(event.globalPosition().toPoint())
                return True
            else:
                self._set_cursor(self._get_edge(event.pos()))
        elif event.type() == QEvent.MouseButtonRelease:
            self.resizing = False
        return False

    def _get_edge(self, pos):
        if self.window.isMaximized(): return None
        x, y = pos.x(), pos.y()
        w, h = self.window.width(), self.window.height()
        m = self.margin
        if x < m and y < m: return "tl"
        if x > w-m and y < m: return "tr"
        if x < m and y > h-m: return "bl"
        if x > w-m and y > h-m: return "br"
        if x < m: return "l"
        if x > w-m: return "r"
        if y < m: return "t"
        if y > h-m: return "b"
        return None

    def _set_cursor(self, edge):
        cursors = {"tl": Qt.SizeFDiagCursor, "br": Qt.SizeFDiagCursor,
                   "tr": Qt.SizeBDiagCursor, "bl": Qt.SizeBDiagCursor,
                   "l": Qt.SizeHorCursor, "r": Qt.SizeHorCursor,
                   "t": Qt.SizeVerCursor, "b": Qt.SizeVerCursor}
        self.window.setCursor(cursors.get(edge, Qt.ArrowCursor))

    def _do_resize(self, global_pos):
        geo = QRect(self.start_geo)
        diff = global_pos - self.start_pos
        if "l" in self.resize_edge: geo.setLeft(self.start_geo.left() + diff.x())
        if "r" in self.resize_edge: geo.setRight(self.start_geo.right() + diff.x())
        if "t" in self.resize_edge: geo.setTop(self.start_geo.top() + diff.y())
        if "b" in self.resize_edge: geo.setBottom(self.start_geo.bottom() + diff.y())
        if geo.width() >= self.window.minimumWidth() and geo.height() >= self.window.minimumHeight():
            self.window.setGeometry(geo)