"""The picture tools. Pen, paint, eraser, and vector share this bar with the warp tools.

The bar is the place a new illustration tool shows up. Brush controls stay
hidden until the active tool uses a brush.
"""

from PySide6.QtCore import QPointF, QRectF, QSize, Qt, Signal
from PySide6.QtGui import QColor, QIcon, QImage, QPainter, QPen, QPixmap
from PySide6.QtWidgets import (
    QDialog,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from vmi_studio.paint import AA_STEPS, DEFAULT_WARP_COLOR, TOOLS, Brush, aa_label, parse_hex
from vmi_studio.scrub import take_pointer
from vmi_studio.toggles import show_toggle, watch


def _button(text):
    button = QPushButton(text)
    button.setCursor(Qt.PointingHandCursor)
    return button


def _tip(label, shortcut=""):
    """Name, then the key expression, the same gap the View menu uses."""
    if shortcut:
        return "%s    %s" % (label, shortcut)
    return label


# 16×16 pixel icons. '#' is the mark. They scale 2× nearest so the pixels stay square.
_ICONS = {
    "pen": (
        "................",
        "..............#.",
        ".............#..",
        "............#...",
        "...........#....",
        "..........#.....",
        ".........#......",
        "........#.......",
        ".......#........",
        "......#.........",
        ".....#..........",
        "....#...........",
        "...#............",
        "..##............",
        ".##.............",
        "................",
    ),
    "paint": (
        "................",
        "......####......",
        "....########....",
        "...##########...",
        "..############..",
        "..############..",
        "..############..",
        "..############..",
        "..############..",
        "...##########...",
        "....########....",
        "......####......",
        "................",
        "................",
        "................",
        "................",
    ),
    "eraser": (
        "................",
        "...##########...",
        "...##########...",
        "...##########...",
        "...##########...",
        "...##########...",
        "...##......##...",
        "...##......##...",
        "...##########...",
        "...##########...",
        "...##########...",
        "...##########...",
        "................",
        "................",
        "................",
        "................",
    ),
    "vector": (
        "................",
        "#...............",
        ".#..............",
        "..#.............",
        "...#............",
        "....#...........",
        ".....#..........",
        "......#.........",
        ".......#......#.",
        "........#....#..",
        ".........#..#...",
        "..........#.....",
        ".........#......",
        "........#.......",
        "................",
        "................",
    ),
    "line": (
        "................",
        "...............#",
        "..............#.",
        ".............#..",
        "............#...",
        "...........#....",
        "..........#.....",
        ".........#......",
        "........#.......",
        ".......#........",
        "......#.........",
        ".....#..........",
        "....#...........",
        "...#............",
        "..#.............",
        "#...............",
    ),
    "warp-target": (
        "................",
        ".......##.......",
        "......#..#......",
        ".....#....#.....",
        "....#......#....",
        "...#........#...",
        "...#........#...",
        "..#....##....#..",
        "..#....##....#..",
        "...#........#...",
        "...#........#...",
        "....#......#....",
        ".....#....#.....",
        "......#..#......",
        ".......##.......",
        "................",
    ),
    "warp-mask": (
        "................",
        ".##############.",
        ".#............#.",
        ".#............#.",
        ".#............#.",
        ".#............#.",
        ".#....####....#.",
        ".#....####....#.",
        ".#....####....#.",
        ".#............#.",
        ".#............#.",
        ".#............#.",
        ".#............#.",
        ".##############.",
        "................",
        "................",
    ),
}


def _tool_icon(ident, ink):
    """A 32×32 icon from the 16×16 mark. Nearest scale keeps each pixel square."""
    rows = _ICONS.get(ident) or _ICONS["pen"]
    image = QImage(16, 16, QImage.Format.Format_ARGB32)
    image.fill(QColor(0, 0, 0, 0))
    color = QColor(ink)
    for y, row in enumerate(rows):
        for x, mark in enumerate(row):
            if mark == "#":
                image.setPixelColor(x, y, color)
    pixmap = QPixmap.fromImage(image).scaled(
        32, 32, Qt.AspectRatioMode.IgnoreAspectRatio, Qt.TransformationMode.FastTransformation
    )
    return QIcon(pixmap)


class SoftKnob(QWidget):
    """Vertical drag. Up raises softness. The digits are 00 through FF."""

    changed = Signal(float)

    def __init__(self):
        super().__init__()
        self.value = 0.0
        self._y = None
        self.setFixedSize(22, 22)
        self.setCursor(Qt.SizeVerCursor)
        self.setAttribute(Qt.WidgetAttribute.WA_AcceptTouchEvents, True)
        self.setToolTip("Softness. Drag up to soften the edge.")

    def event(self, event):
        if take_pointer(self, event):
            event.accept()
            return True
        return super().event(event)

    def set_value(self, value):
        self.value = max(0.0, min(1.0, float(value)))
        self.update()

    def mousePressEvent(self, event):
        self._y = event.position().y()
        event.accept()

    def mouseMoveEvent(self, event):
        if self._y is None or not (event.buttons() & Qt.LeftButton):
            return
        delta = self._y - event.position().y()
        self._y = event.position().y()
        self.set_value(self.value + delta / 80.0)
        self.changed.emit(self.value)
        event.accept()

    def mouseReleaseEvent(self, event):
        self._y = None
        event.accept()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing, False)
        color = QColor("#ffffff" if self.value > 0 else "#3a3a3a")
        painter.setPen(QPen(color))
        painter.setBrush(Qt.NoBrush)
        painter.drawEllipse(self.rect().adjusted(1, 1, -2, -2))
        painter.drawText(self.rect(), Qt.AlignCenter, "%02X" % int(round(self.value * 255)))
        painter.end()


class CurvePlot(QWidget):
    """One pressure curve. The ends stay put. Double-click adds a point."""

    changed = Signal()

    def __init__(self, points):
        super().__init__()
        self.points = [list(point) for point in points]
        self._drag = None
        self.setMinimumSize(180, 100)
        self.setMouseTracking(True)
        self.setAttribute(Qt.WidgetAttribute.WA_AcceptTouchEvents, True)

    def event(self, event):
        if take_pointer(self, event):
            event.accept()
            return True
        return super().event(event)

    def _box(self):
        return QRectF(12, 12, self.width() - 24, self.height() - 24)

    def _to_widget(self, t, value):
        box = self._box()
        return QPointF(box.left() + t * box.width(), box.bottom() - value * box.height())

    def _from_widget(self, pos):
        box = self._box()
        if box.width() < 1 or box.height() < 1:
            return 0.0, 0.0
        t = (pos.x() - box.left()) / box.width()
        value = (box.bottom() - pos.y()) / box.height()
        return max(0.0, min(1.0, t)), max(0.0, min(1.0, value))

    def _hit(self, pos):
        for index, point in enumerate(self.points):
            widget = self._to_widget(point[0], point[1])
            if abs(widget.x() - pos.x()) <= 6 and abs(widget.y() - pos.y()) <= 6:
                return index
        return None

    def mousePressEvent(self, event):
        index = self._hit(event.position())
        if event.button() == Qt.RightButton and index not in (None, 0, len(self.points) - 1):
            self.points.pop(index)
            self.changed.emit()
            self.update()
            event.accept()
            return
        if event.button() == Qt.LeftButton and index is not None:
            self._drag = index
            event.accept()
            return
        super().mousePressEvent(event)

    def mouseDoubleClickEvent(self, event):
        if event.button() != Qt.LeftButton or len(self.points) >= 8:
            return
        t, value = self._from_widget(event.position())
        self.points.append([t, value])
        self.points.sort(key=lambda item: item[0])
        self.changed.emit()
        self.update()
        event.accept()

    def mouseMoveEvent(self, event):
        if self._drag is None or not (event.buttons() & Qt.LeftButton):
            return
        t, value = self._from_widget(event.position())
        point = self.points[self._drag]
        if self._drag in (0, len(self.points) - 1):
            point[1] = value
        else:
            previous = self.points[self._drag - 1][0] + 0.02
            nxt = self.points[self._drag + 1][0] - 0.02
            point[0] = min(max(t, previous), nxt)
            point[1] = value
        self.changed.emit()
        self.update()
        event.accept()

    def mouseReleaseEvent(self, event):
        self._drag = None
        event.accept()

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), QColor("#000000"))
        box = self._box()
        painter.setPen(QPen(QColor("#2a2a2a")))
        painter.drawRect(box)
        painter.setPen(QPen(QColor("#ffffff")))
        previous = None
        for point in self.points:
            widget = self._to_widget(point[0], point[1])
            if previous is not None:
                painter.drawLine(previous, widget)
            previous = widget
        painter.setBrush(QColor("#ffffff"))
        for point in self.points:
            widget = self._to_widget(point[0], point[1])
            painter.drawRect(int(widget.x()) - 2, int(widget.y()) - 2, 4, 4)
        painter.end()


class CurveDialog(QDialog):
    """Size and opacity curves. The toggles on the bar decide if a curve is used."""

    def __init__(self, brush, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Pressure")
        self.setModal(False)
        self.brush = brush
        layout = QVBoxLayout(self)
        layout.setContentsMargins(12, 12, 12, 12)
        layout.setSpacing(8)
        layout.addWidget(QLabel("Size"))
        self.size_plot = CurvePlot(brush.size_curve)
        self.opacity_plot = CurvePlot(brush.opacity_curve)
        self.size_plot.changed.connect(self._take)
        self.opacity_plot.changed.connect(self._take)
        layout.addWidget(self.size_plot)
        layout.addWidget(QLabel("Opacity"))
        layout.addWidget(self.opacity_plot)
        note = QLabel("Double-click adds a point. Right-click removes one. The ends stay.")
        note.setWordWrap(True)
        layout.addWidget(note)
        self.setStyleSheet(
            "QDialog, QLabel { background: #000000; color: #3a3a3a; }"
        )

    def _take(self):
        self.brush.size_curve = [tuple(point) for point in self.size_plot.points]
        self.brush.opacity_curve = [tuple(point) for point in self.opacity_plot.points]


class DrawBar(QWidget):
    """Pen, paint, eraser, vector, and the warp tools. The brush row follows the tool."""

    mode_requested = Signal(str)
    color_changed = Signal(str)

    def __init__(self):
        super().__init__()
        self.brush = Brush()
        self._mode = ""
        self._color = DEFAULT_WARP_COLOR
        self._back = "#000000"
        self._clear = False
        self._aa_index = 1
        self.brush.antialias = AA_STEPS[self._aa_index]
        self._curve = None
        self._guard = False
        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(4)
        tools = QHBoxLayout()
        tools.setSpacing(4)
        self.buttons = {}
        self.tool_column = QWidget()
        self.tool_column.setObjectName("toolColumn")
        self.tool_column.setFixedWidth(78)
        grid = QGridLayout(self.tool_column)
        grid.setContentsMargins(0, 0, 0, 0)
        grid.setSpacing(2)
        for index, tool in enumerate(TOOLS):
            button = _button("")
            button.setToolTip(_tip(tool.label, tool.shortcut))
            button.setFixedSize(36, 36)
            button.setIconSize(QSize(32, 32))
            button.clicked.connect(lambda _checked=False, ident=tool.id: self._click(ident))
            self.buttons[tool.id] = button
            grid.addWidget(button, index // 2, index % 2)
        self.fore_swatch = _button("")
        self.back_swatch = _button("")
        self.fore_swatch.setFixedSize(36, 36)
        self.back_swatch.setFixedSize(36, 36)
        self.fore_swatch.setToolTip(_tip("Ink", "X"))
        self.back_swatch.setToolTip(_tip("Other color", "X"))
        self.back_swatch.clicked.connect(self.swap_colors)
        colors = QHBoxLayout()
        colors.setSpacing(2)
        colors.addWidget(self.fore_swatch)
        colors.addWidget(self.back_swatch)
        grid.addLayout(colors, (len(TOOLS) + 1) // 2, 0, 1, 2)
        grid.setRowStretch((len(TOOLS) + 1) // 2 + 1, 1)
        self.hex_edit = QLineEdit(self._color)
        self.hex_edit.setFixedWidth(84)
        self.hex_edit.setToolTip(_tip("Ink color", "X"))
        self.hex_edit.editingFinished.connect(self._commit_color)
        tools.addWidget(self.hex_edit)
        self.white_swatch = _button("")
        self.black_swatch = _button("")
        for swatch, tip in ((self.white_swatch, _tip("White ink")), (self.black_swatch, _tip("Black ink"))):
            swatch.setFixedSize(18, 18)
            swatch.setToolTip(tip)
        self.white_swatch.clicked.connect(lambda: self.set_color("#FFFFFF"))
        self.black_swatch.clicked.connect(lambda: self.set_color("#000000"))
        tools.addWidget(self.white_swatch)
        tools.addWidget(self.black_swatch)
        self.clear_button = _button("Clear")
        self.clear_button.setCheckable(True)
        self.clear_button.setToolTip(_tip("Clear", "C"))
        self.clear_button.toggled.connect(self._clear_toggled)
        watch(self.clear_button)
        tools.addWidget(self.clear_button)
        self._mark_swatches()
        tools.addStretch(1)
        root.addLayout(tools)

        brush = QHBoxLayout()
        brush.setSpacing(4)
        self._brush_row = QWidget()
        brush_layout = QHBoxLayout(self._brush_row)
        brush_layout.setContentsMargins(0, 0, 0, 0)
        brush_layout.setSpacing(4)
        self.size_box = self._spin(1, 512, int(self.brush.size))
        self.size_box.setToolTip(_tip("Size", "[ ]"))
        self.min_box = self._spin(1, 512, int(self.brush.size_min))
        self.max_box = self._spin(1, 512, int(self.brush.size_max))
        self.soft = SoftKnob()
        self.aa_button = _button(aa_label(AA_STEPS[self._aa_index]))
        self.aa_button.setToolTip("Antialiasing. Click to cycle off, .5, 1, and 2.")
        self.size_pressure = _button("Pressure size")
        self.opacity_pressure = _button("Pressure opacity")
        for button in (self.size_pressure, self.opacity_pressure):
            button.setCheckable(True)
            button.setChecked(True)
            watch(button)
        self.curve_button = _button("Curve")
        self.curve_button.setToolTip("Pressure curves for size and opacity")
        for label, widget in (
            ("Size", self.size_box),
            ("Min", self.min_box),
            ("Max", self.max_box),
            ("Soft", self.soft),
        ):
            brush_layout.addWidget(QLabel(label))
            brush_layout.addWidget(widget)
        brush_layout.addWidget(self.aa_button)
        brush_layout.addWidget(self.size_pressure)
        brush_layout.addWidget(self.opacity_pressure)
        brush_layout.addWidget(self.curve_button)
        brush_layout.addStretch(1)
        root.addWidget(self._brush_row)
        self._brush_row.hide()
        self.size_box.valueChanged.connect(self._apply_brush)
        self.min_box.valueChanged.connect(self._apply_brush)
        self.max_box.valueChanged.connect(self._apply_brush)
        self.soft.changed.connect(self._soft_changed)
        self.aa_button.clicked.connect(self._cycle_aa)
        self.size_pressure.toggled.connect(self._apply_brush)
        self.opacity_pressure.toggled.connect(self._apply_brush)
        self.curve_button.clicked.connect(self._open_curve)
        self._apply_brush()
        self._mark_aa()
        self._style_tools()

    def _spin(self, low, high, value):
        box = QSpinBox()
        box.setRange(low, high)
        box.setValue(value)
        box.setFixedWidth(72)
        return box

    def color(self):
        return self._color

    def clears(self):
        """True while the current tool should stamp empty pixels."""
        return bool(self._clear)

    def swap_colors(self):
        """X. The two inks trade places. The tool stays put."""
        self._color, self._back = self._back, self._color
        self.hex_edit.setText(self._color)
        self._mark_swatches()
        self.color_changed.emit(self._color)

    def toggle_clear(self):
        """C. Empty pixels, not a switch over to the eraser tool."""
        self.clear_button.setChecked(not self.clear_button.isChecked())

    def nudge_size(self, step):
        """[ and ]. The spin stays inside its range."""
        self.size_box.setValue(int(self.size_box.value()) + int(step))

    def _clear_toggled(self, on):
        self._clear = bool(on)
        self._mark_swatches()

    def set_color(self, text):
        parsed = parse_hex(text)
        if parsed is None:
            return
        changed = parsed != self._color
        self._color = parsed
        self.hex_edit.setText(parsed)
        self._mark_swatches()
        if changed:
            self.color_changed.emit(parsed)

    def set_mode(self, mode):
        self._mode = mode or ""
        uses = False
        for tool in TOOLS:
            if tool.id == self._mode and tool.uses_brush:
                uses = True
        self._brush_row.setVisible(uses)
        self._style_tools()

    def _click(self, ident):
        self.mode_requested.emit("" if self._mode == ident else ident)

    def _commit_color(self):
        parsed = parse_hex(self.hex_edit.text())
        if parsed is None:
            self.hex_edit.setText(self._color)
            return
        self.set_color(parsed)

    def _mark_swatches(self):
        for button, color in ((self.white_swatch, "#FFFFFF"), (self.black_swatch, "#000000")):
            border = "#ffffff" if self._color == color else "#3a3a3a"
            button.setStyleSheet(
                "QPushButton { background: %s; color: %s; border: 1px solid %s; border-radius: 0; }"
                % (color, color, border)
            )
        fore = "#000000" if self._clear else self._color
        fore_ink = "#ffffff" if self._clear else fore
        self.fore_swatch.setText("A" if self._clear else "")
        self.fore_swatch.setStyleSheet(
            "QPushButton { background: %s; color: %s; border: 1px solid #ffffff; border-radius: 0; }"
            % (fore, fore_ink)
        )
        self.back_swatch.setStyleSheet(
            "QPushButton { background: %s; color: %s; border: 1px solid #3a3a3a; border-radius: 0; }"
            % (self._back, self._back)
        )
        show_toggle(self.clear_button, self._clear)

    def _cycle_aa(self):
        self._aa_index = (self._aa_index + 1) % len(AA_STEPS)
        self.aa_button.setText(aa_label(AA_STEPS[self._aa_index]))
        self._mark_aa()
        self._apply_brush()

    def _mark_aa(self):
        """Filled when antialiasing is on. Hollow when it is off."""
        show_toggle(self.aa_button, AA_STEPS[self._aa_index] != 0)

    def _soft_changed(self, value):
        self.brush.softness = float(value)

    def _apply_brush(self, *_args):
        if self._guard:
            return
        low = self.min_box.value()
        high = max(low, self.max_box.value())
        size = min(max(self.size_box.value(), low), high)
        self._guard = True
        if self.max_box.value() != high:
            self.max_box.setValue(high)
        if self.size_box.value() != size:
            self.size_box.setValue(size)
        self._guard = False
        self.brush.size = float(size)
        self.brush.size_min = float(low)
        self.brush.size_max = float(high)
        self.brush.antialias = AA_STEPS[self._aa_index]
        self.brush.use_size_pressure = self.size_pressure.isChecked()
        self.brush.use_opacity_pressure = self.opacity_pressure.isChecked()

    def _open_curve(self):
        if self._curve is None:
            self._curve = CurveDialog(self.brush, self)
        self._curve.show()
        self._curve.raise_()

    def _style_tools(self):
        for ident, button in self.buttons.items():
            on = ident == self._mode
            button.setIcon(_tool_icon(ident, "#000000" if on else "#ffffff"))
            button.setStyleSheet(
                "QPushButton { background: %s; color: %s; border: 1px solid %s; border-radius: 0; padding: 0; }"
                % (("#ffffff", "#000000", "#ffffff") if on else ("#000000", "#ffffff", "#2a2a2a"))
            )
