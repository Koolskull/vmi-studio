"""The picture tools for a warp target and its mask.

The bar is the place a new illustration tool shows up. Brush controls stay
hidden until the active tool uses a brush.
"""

from PySide6.QtCore import QPointF, QRectF, Qt, Signal
from PySide6.QtGui import QColor, QPainter, QPen
from PySide6.QtWidgets import (
    QDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from vmi_studio.paint import DEFAULT_WARP_COLOR, TOOLS, Brush, parse_hex


def _button(text):
    button = QPushButton(text)
    button.setCursor(Qt.PointingHandCursor)
    return button


class SoftKnob(QWidget):
    """Vertical drag. Up raises softness. The digits are 00 through FF."""

    changed = Signal(float)

    def __init__(self):
        super().__init__()
        self.value = 0.0
        self._y = None
        self.setFixedSize(22, 22)
        self.setCursor(Qt.SizeVerCursor)
        self.setToolTip("Softness. Drag up to soften the edge.")

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
    """Create warp target, draw warp mask, and the brush for the mask."""

    mode_requested = Signal(str)
    color_changed = Signal(str)

    def __init__(self):
        super().__init__()
        self.brush = Brush()
        self._mode = ""
        self._color = DEFAULT_WARP_COLOR
        self._curve = None
        self._guard = False
        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(4)
        tools = QHBoxLayout()
        tools.setSpacing(4)
        self.buttons = {}
        for tool in TOOLS:
            button = _button(tool.label)
            button.clicked.connect(lambda _checked=False, ident=tool.id: self._click(ident))
            self.buttons[tool.id] = button
            tools.addWidget(button)
        self.hex_edit = QLineEdit(self._color)
        self.hex_edit.setFixedWidth(84)
        self.hex_edit.setToolTip("Warp color. Any hex is treated as a warp target.")
        self.hex_edit.editingFinished.connect(self._commit_color)
        tools.addWidget(self.hex_edit)
        tools.addStretch(1)
        root.addLayout(tools)

        brush = QHBoxLayout()
        brush.setSpacing(4)
        self._brush_row = QWidget()
        brush_layout = QHBoxLayout(self._brush_row)
        brush_layout.setContentsMargins(0, 0, 0, 0)
        brush_layout.setSpacing(4)
        self.size_box = self._spin(1, 512, int(self.brush.size))
        self.min_box = self._spin(1, 512, int(self.brush.size_min))
        self.max_box = self._spin(1, 512, int(self.brush.size_max))
        self.soft = SoftKnob()
        self.aa_button = _button("AA")
        self.aa_button.setCheckable(True)
        self.aa_button.setChecked(True)
        self.aa_button.setToolTip("Antialiasing")
        self.size_pressure = _button("Pressure size")
        self.opacity_pressure = _button("Pressure opacity")
        for button in (self.size_pressure, self.opacity_pressure):
            button.setCheckable(True)
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
        self.aa_button.toggled.connect(self._apply_brush)
        self.size_pressure.toggled.connect(self._apply_brush)
        self.opacity_pressure.toggled.connect(self._apply_brush)
        self.curve_button.clicked.connect(self._open_curve)
        self._style_tools()

    def _spin(self, low, high, value):
        box = QSpinBox()
        box.setRange(low, high)
        box.setValue(value)
        box.setFixedWidth(72)
        return box

    def color(self):
        return self._color

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
        self._color = parsed
        self.hex_edit.setText(parsed)
        self.color_changed.emit(parsed)

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
        self.brush.antialias = self.aa_button.isChecked()
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
            button.setStyleSheet(
                "QPushButton { background: %s; color: %s; border: 1px solid %s; border-radius: 0; padding: 4px 8px; }"
                % (("#ffffff", "#000000", "#ffffff") if on else ("#000000", "#3a3a3a", "#2a2a2a"))
            )
