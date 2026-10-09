"""A small window that stays up while a drawing is read.

The picture is three greys. Liquid noise is warped, then ordered-dithered.
The file name is on the left of the top line. LOADING is on the right, with
dots stepping after the word. Those are white. The studio behind the window
is dimmed. The studio cannot be used until the window closes.
"""

import math

from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QColor, QFont, QFontMetrics, QImage, QPainter
from PySide6.QtWidgets import (
    QApplication,
    QDialog,
    QHBoxLayout,
    QLabel,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

# Black veil over the editor. 158/255 leaves the studio visible and clearly darker.
DIM = 158
WHITE_TEXT = "background-color: #000000; color: #ffffff; border: none;"
QUIET_TEXT = "background-color: #000000; color: #3a3a3a; border: none;"
SOFT_TEXT = "background-color: #000000; color: #a0a0a0; border: none;"

BAYER = (
    (0, 8, 2, 10),
    (12, 4, 14, 6),
    (3, 11, 1, 9),
    (15, 7, 13, 5),
)
WIDTH = 96
HEIGHT = 54
SCALE = 3
# Black, a dark middle grey, and the pale tone that used to be white.
TONE_HEX = ("#000000", "#444444", "#CCCCCC")

# One glyph at a time, in this order, at the head of the filling bar.
GLYPHS = ("#", "0", "o", ",", "_", ",", "*", "%")
BAR_CAP = 0.92
BAR_STEP = 0.004
BAR_MS = 40
# The filled track. The animated cell in front of these stays white.
BAR_HASH = "#BBBBBB"
DOTS = (".", "..", "...")
DOT_MS = 300


def _hash(ix, iy, iz):
    n = (int(ix) * 374761393 + int(iy) * 668265263 + int(iz) * 1440673973) & 0xFFFFFFFF
    n ^= n >> 13
    n = (n * 1274126177) & 0xFFFFFFFF
    return (n & 255) / 255.0


def _fade(value):
    return value * value * (3.0 - 2.0 * value)


def _noise(x, y, z):
    x *= 0.13
    y *= 0.13
    ix, iy, iz = math.floor(x), math.floor(y), math.floor(z)
    fx, fy, fz = _fade(x - ix), _fade(y - iy), _fade(z - iz)

    def at(dx, dy, dz):
        return _hash(ix + dx, iy + dy, iz + dz)

    def lerp(a, b, t):
        return a + (b - a) * t

    front = lerp(lerp(at(0, 0, 0), at(1, 0, 0), fx), lerp(at(0, 1, 0), at(1, 1, 0), fx), fy)
    back = lerp(lerp(at(0, 0, 1), at(1, 0, 1), fx), lerp(at(0, 1, 1), at(1, 1, 1), fx), fy)
    return lerp(front, back, fz)


def _level_at(tick, x, y):
    """One dithered cell. 0 is black, 1 is #444444, 2 is #CCCCCC."""
    steps = len(TONE_HEX) - 1
    t = float(tick) * 0.17
    wave_y = 10.0 * math.sin(y * 0.11 + t)
    u = x + wave_y
    v = y + 8.0 * math.sin(x * 0.09 - t * 1.2)
    sample = _noise(u, v, t * 2.0)
    ix = int(math.floor(x))
    iy = int(math.floor(y))
    limit = (BAYER[iy & 3][ix & 3] + 0.5) / 16.0
    scaled = sample * steps
    level = int(math.floor(scaled))
    if scaled - level > limit:
        level += 1
    if level < 0:
        level = 0
    elif level > steps:
        level = steps
    return level


def liquid_bits(tick, width=WIDTH, height=HEIGHT):
    """One dithered frame. Each cell is 0 black, 1 #444444, or 2 #CCCCCC."""
    rows = []
    for y in range(height):
        rows.append([_level_at(tick, x, y) for x in range(width)])
    return rows


def _tone_rgb(level):
    value = int(TONE_HEX[level][1:], 16)
    return (value >> 16) & 255, (value >> 8) & 255, value & 255


def liquid_image(tick, width=WIDTH, height=HEIGHT):
    rows = liquid_bits(tick, width, height)
    image = QImage(width, height, QImage.Format.Format_Indexed8)
    image.setColorCount(len(TONE_HEX))
    for index, hex_color in enumerate(TONE_HEX):
        image.setColor(index, QColor(hex_color).rgb())
    image.fill(0)
    for y, row in enumerate(rows):
        for x, level in enumerate(row):
            if level:
                image.setPixel(x, y, level)
    return image


class DitherView(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.tick = 0
        self.image = liquid_image(0)
        self.setFixedSize(WIDTH * SCALE, HEIGHT * SCALE)
        self._timer = QTimer(self)
        self._timer.setInterval(70)
        self._timer.timeout.connect(self.step)

    def step(self):
        self.tick += 1
        self.image = liquid_image(self.tick)
        self.update()

    def showEvent(self, event):
        super().showEvent(event)
        self._timer.start()

    def hideEvent(self, event):
        self._timer.stop()
        super().hideEvent(event)

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
        painter.drawImage(self.rect(), self.image)


def stage_fraction(text):
    """How full the bar may jump when a read step is named. Never the end."""
    lower = (text or "").lower()
    if "picture" in lower or "composite" in lower or "painting" in lower:
        return 0.84
    if "layer" in lower:
        return 0.7
    if "convert" in lower:
        return 0.36
    if "reading" in lower or "blueprint" in lower:
        return 0.5
    return 0.16


class AsciiBar(QWidget):
    """A filling track. The head cell cycles glyphs until the picture is ready."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.tick = 0
        self.fraction = 0.0
        self.target = 0.08
        self._font = QFont(self.font())
        self._font.setPixelSize(12)
        self._font.setStyleStrategy(
            QFont.StyleStrategy.NoAntialias | QFont.StyleStrategy.NoSubpixelAntialias
        )
        self.setFont(self._font)
        metrics = QFontMetrics(self._font)
        self._cell = max(metrics.horizontalAdvance(ch) for ch in GLYPHS + ("[", "]", ".", "#"))
        self._cell = max(self._cell, 8)
        self.setFixedHeight(metrics.height() + 4)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        self._timer = QTimer(self)
        self._timer.setInterval(BAR_MS)
        self._timer.timeout.connect(self.step)

    def reset(self):
        self.tick = 0
        self.fraction = 0.0
        self.target = 0.08
        self.update()

    def glyph(self):
        return GLYPHS[self.tick % len(GLYPHS)]

    def set_target(self, value):
        value = max(0.0, min(BAR_CAP, float(value)))
        if value > self.target:
            self.target = value
        if self.fraction < 1.0 and self.target > self.fraction:
            self.fraction = self.target
            self.update()

    def complete(self):
        self.fraction = 1.0
        self.update()

    def step(self):
        self.tick += 1
        if self.fraction < 1.0:
            nxt = min(BAR_CAP, self.fraction + BAR_STEP)
            if self.target > nxt:
                nxt = min(BAR_CAP, self.target)
            self.fraction = nxt
        self.update()

    def cells(self):
        """Bar text, and the index of the animated cell. None when the bar is full."""
        count = self._count()
        filled = int(self.fraction * count + 1e-6)
        filled = max(0, min(count, filled))
        chars = ["."] * count
        head = None
        if self.fraction >= 1.0:
            chars = ["#"] * count
        else:
            for index in range(filled):
                chars[index] = "#"
            at = filled if filled < count else count - 1
            chars[at] = self.glyph()
            head = at + 1
        return "[" + "".join(chars) + "]", head

    def render(self):
        text, _head = self.cells()
        return text

    def _count(self):
        inner = self.width() - (self._cell * 2)
        if inner < self._cell * 8:
            return 24
        return max(8, inner // self._cell)

    def showEvent(self, event):
        super().showEvent(event)
        self._timer.start()

    def hideEvent(self, event):
        self._timer.stop()
        super().hideEvent(event)

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, False)
        painter.setFont(self._font)
        text, head = self.cells()
        cell = self._cell
        quiet = QColor("#3a3a3a")
        white = QColor("#ffffff")
        filled = QColor(BAR_HASH)
        for index, ch in enumerate(text):
            if index == head:
                painter.setPen(white)
            elif ch == "#":
                painter.setPen(filled)
            elif ch in ".[]":
                painter.setPen(quiet)
            else:
                painter.setPen(white)
            painter.drawText(index * cell, 0, cell, self.height(), Qt.AlignmentFlag.AlignCenter, ch)


class EditorShade(QWidget):
    """Translucent window over the studio. A child of QMainWindow cannot dim it."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.Tool
            | Qt.WindowType.WindowDoesNotAcceptFocus
            | Qt.WindowType.WindowTransparentForInput
        )
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating, True)
        self.setAttribute(Qt.WidgetAttribute.WA_NoSystemBackground, True)
        self.setAutoFillBackground(False)
        self.hide()

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.setCompositionMode(QPainter.CompositionMode.CompositionMode_Source)
        painter.fillRect(self.rect(), QColor(0, 0, 0, DIM))


def _line(text, sheet, wrap=False):
    label = QLabel(text)
    label.setStyleSheet(sheet)
    font = QFont(label.font())
    font.setStyleStrategy(
        QFont.StyleStrategy.NoAntialias | QFont.StyleStrategy.NoSubpixelAntialias
    )
    label.setFont(font)
    if wrap:
        label.setWordWrap(True)
    return label


class LoadGate(QDialog):
    """Application-modal. Input to the studio waits until hide()."""

    def __init__(self, file_name="", parent=None):
        super().__init__(parent)
        self.setWindowTitle("VMI STUDIO")
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.Tool
        )
        self.setWindowModality(Qt.WindowModality.ApplicationModal)
        self.setStyleSheet(
            "QDialog { background: #000000; color: #ffffff; border: 1px solid #2a2a2a; }"
            "QLabel { background: #000000; border: none; }"
        )
        layout = QVBoxLayout(self)
        layout.setContentsMargins(16, 16, 16, 16)
        layout.setSpacing(8)
        self._file_name = file_name or ""
        self._catchup = False
        header = QWidget()
        header.setObjectName("loadHeader")
        self._header = header
        row = QHBoxLayout(header)
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(0)
        self.name = _line(self._file_name, WHITE_TEXT)
        self.name.setAlignment(Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter)
        self.name.setMinimumWidth(0)
        self.name.setSizePolicy(QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Preferred)
        self.loading = _line("LOADING", WHITE_TEXT)
        self.loading.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        self.loading.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Preferred)
        self.dots = _line(DOTS[0], WHITE_TEXT)
        self.dots.setObjectName("loadDots")
        self.dots.setAlignment(Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter)
        self.dots.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Preferred)
        dot_width = QFontMetrics(self.dots.font()).horizontalAdvance(DOTS[-1])
        self.dots.setFixedWidth(max(dot_width, 1))
        row.addWidget(self.name, 1)
        row.addSpacing(12)
        row.addWidget(self.loading, 0)
        row.addWidget(self.dots, 0)
        layout.addWidget(header)
        self._dot_index = 0
        self._dot_timer = QTimer(self)
        self._dot_timer.setInterval(DOT_MS)
        self._dot_timer.timeout.connect(self._step_dots)
        self.view = DitherView()
        layout.addWidget(self.view, 0, Qt.AlignmentFlag.AlignHCenter)
        self._quiet = _line(
            "The studio cannot be used until this file is loaded.",
            QUIET_TEXT,
            wrap=True,
        )
        layout.addWidget(self._quiet)
        self._soft = _line("thank you for your patience", SOFT_TEXT)
        layout.addWidget(self._soft)
        self.bar = AsciiBar()
        layout.addWidget(self.bar)
        self.setFixedWidth(340)
        header_labels = {self.name, self.loading, self.dots}
        for label in self.findChildren(QLabel):
            if label in header_labels:
                continue
            label.setFixedWidth(340 - 32)
        self.ensurePolished()
        layout.activate()
        self.setFixedHeight(layout.sizeHint().height())
        self._elide_name()

    def set_catchup(self, catchup):
        """Graphic and bar only. The name line and the two notes stay hidden."""
        self._catchup = bool(catchup)
        self._header.setVisible(not self._catchup)
        self._quiet.setVisible(not self._catchup)
        self._soft.setVisible(not self._catchup)
        if self._catchup:
            self._dot_timer.stop()
        elif self.isVisible():
            self._dot_index = 0
            self.dots.setText(DOTS[0])
            self._dot_timer.start()
            self._elide_name()
        layout = self.layout()
        layout.invalidate()
        layout.activate()
        self.setFixedHeight(layout.sizeHint().height())

    def set_name(self, file_name):
        self._file_name = file_name or ""
        self.name.setText(self._file_name)
        self._elide_name()

    def _elide_name(self):
        full = self._file_name
        width = self.name.width()
        if width < 24:
            if self.name.text() != full:
                self.name.setText(full)
            return
        shown = QFontMetrics(self.name.font()).elidedText(
            full, Qt.TextElideMode.ElideRight, max(1, width)
        )
        if self.name.text() != shown:
            self.name.setText(shown)

    def _step_dots(self):
        self._dot_index = (self._dot_index + 1) % len(DOTS)
        self.dots.setText(DOTS[self._dot_index])

    def hear(self, text):
        self.bar.set_target(stage_fraction(text))

    def showEvent(self, event):
        super().showEvent(event)
        if not self._catchup:
            self._dot_index = 0
            self.dots.setText(DOTS[0])
            self._dot_timer.start()
            self._elide_name()
        screen = self.screen() or QApplication.primaryScreen()
        if screen is None:
            return
        geo = screen.availableGeometry()
        frame = self.frameGeometry()
        frame.moveCenter(geo.center())
        self.move(frame.topLeft())

    def hideEvent(self, event):
        self._dot_timer.stop()
        super().hideEvent(event)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._elide_name()
