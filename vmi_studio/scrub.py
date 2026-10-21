"""Drag a number or a menu the way the softness control drags.

Up raises the value. Down lowers it. A mouse, a finger, and a pen all use
the same motion. One gesture is one change, from the value where it started.
"""

from PySide6.QtCore import QEvent, QObject, QPoint, QPointF, Qt
from PySide6.QtWidgets import QAbstractSpinBox, QApplication, QComboBox, QStyle, QStyleOptionSpinBox

_SCRUB = False
_SCRUB_ID = 0


def scrubbing():
    """True while a drag is changing a control. Undo keeps one step for it."""
    return _SCRUB


def scrub_id():
    """Changes each time a drag starts, so the next drag is a new undo step."""
    return _SCRUB_ID


def _begin_scrub():
    global _SCRUB, _SCRUB_ID
    _SCRUB = True
    _SCRUB_ID += 1


def _end_scrub():
    global _SCRUB
    _SCRUB = False


def pixels_per_step(span, step):
    """How far the pointer travels for one step of this range."""
    step = abs(float(step)) or 1.0
    span = abs(float(span))
    if span <= 0:
        return 4.0
    quarter = span / 4.0
    if quarter <= step:
        return 4.0
    return min(4.0, max(0.35, 300.0 * step / quarter))


def scrub_value(start, dy, low, high, step):
    """The value after a vertical drag. dy is positive when the pointer moves up."""
    low = float(low)
    high = float(high)
    if high < low:
        low, high = high, low
    step = abs(float(step)) or 1.0
    rate = pixels_per_step(high - low, step)
    raw = float(start) + float(dy) / rate * step
    steps = round((raw - low) / step)
    value = low + steps * step
    if value < low:
        value = low
    if value > high:
        value = high
    return value


def scrub_index(start, dy, count):
    """A menu row. About 14 pixels moves one entry."""
    count = int(count)
    if count <= 1:
        return 0
    shift = int(round(float(dy) / 14.0))
    return max(0, min(count - 1, int(start) + shift))


class _Click:
    """The piece of a mouse event the softness control and the curve already read."""

    def __init__(self, pos, button, buttons):
        self._pos = pos
        self._button = button
        self._buttons = buttons

    def position(self):
        return self._pos

    def button(self):
        return self._button

    def buttons(self):
        return self._buttons

    def accept(self):
        return None

    def ignore(self):
        return None


def take_pointer(widget, event):
    """Turn a pen or finger event into the widget's mouse press, move, and release.

    A mouse event returns False so the widget's own mouse path still runs.
    """
    kind = event.type()
    tablet = {
        QEvent.Type.TabletPress: "press",
        QEvent.Type.TabletMove: "move",
        QEvent.Type.TabletRelease: "release",
    }
    if kind in tablet:
        button = event.button()
        if button == Qt.NoButton:
            button = Qt.LeftButton
        buttons = event.buttons()
        if tablet[kind] != "release":
            buttons = buttons | Qt.LeftButton
        _dispatch(widget, tablet[kind], event.position(), button, buttons)
        return True
    if kind in (
        QEvent.Type.TouchBegin,
        QEvent.Type.TouchUpdate,
        QEvent.Type.TouchEnd,
        QEvent.Type.TouchCancel,
    ):
        if kind == QEvent.Type.TouchCancel:
            _dispatch(widget, "release", QPointF(), Qt.LeftButton, Qt.NoButton)
            return True
        points = event.points()
        if not points:
            return True
        point = points[0]
        if kind == QEvent.Type.TouchBegin or point.state() == QEventPoint_Pressed():
            mode = "press"
            buttons = Qt.LeftButton
        elif kind == QEvent.Type.TouchEnd or point.state() == QEventPoint_Released():
            mode = "release"
            buttons = Qt.NoButton
        else:
            mode = "move"
            buttons = Qt.LeftButton
        _dispatch(widget, mode, point.position(), Qt.LeftButton, buttons)
        return True
    return False


def QEventPoint_Pressed():
    return QEventPointState("Pressed")


def QEventPoint_Released():
    return QEventPointState("Released")


def QEventPointState(name):
    state = getattr(Qt, "TouchPoint" + name, None)
    if state is None:
        from PySide6.QtGui import QEventPoint

        state = getattr(QEventPoint.State, name)
    return state


def _dispatch(widget, mode, pos, button, buttons):
    click = _Click(pos, button, buttons)
    if mode == "press":
        widget.mousePressEvent(click)
    elif mode == "move":
        widget.mouseMoveEvent(click)
    else:
        widget.mouseReleaseEvent(click)


def _synthesized(event):
    source = event.source() if hasattr(event, "source") else None
    plain = getattr(Qt, "MouseEventNotSynthesized", None)
    if plain is None:
        plain = getattr(getattr(Qt, "MouseEventSource", None), "MouseEventNotSynthesized", None)
    return source not in (None, plain)


def _pointer_kind(event):
    kind = event.type()
    if kind in (QEvent.Type.MouseButtonPress, QEvent.Type.MouseMove, QEvent.Type.MouseButtonRelease):
        return "mouse"
    if kind in (QEvent.Type.TabletPress, QEvent.Type.TabletMove, QEvent.Type.TabletRelease):
        return "pen"
    if kind in (
        QEvent.Type.TouchBegin,
        QEvent.Type.TouchUpdate,
        QEvent.Type.TouchEnd,
        QEvent.Type.TouchCancel,
    ):
        return "touch"
    return None


def _global(event):
    if hasattr(event, "globalPosition"):
        try:
            point = event.globalPosition()
            return float(point.x()), float(point.y())
        except Exception:
            pass
    if hasattr(event, "points"):
        try:
            points = event.points()
        except Exception:
            points = None
        if points:
            point = points[0].globalPosition()
            return float(point.x()), float(point.y())
    return None


def _climb(widget):
    while widget is not None:
        if isinstance(widget, (QAbstractSpinBox, QComboBox)):
            return widget
        parent = widget.parentWidget() if hasattr(widget, "parentWidget") else None
        widget = parent
    return None


def _control(watched, pos):
    host = _climb(watched)
    if host is None and pos is not None:
        found = QApplication.widgetAt(QPoint(int(pos[0]), int(pos[1])))
        host = _climb(found)
    if host is None or not host.isEnabled():
        return None
    if isinstance(host, QAbstractSpinBox) and host.isReadOnly():
        return None
    return host


def _on_stepper(spin, pos):
    local = spin.mapFromGlobal(QPoint(int(pos[0]), int(pos[1])))
    option = QStyleOptionSpinBox()
    spin.initStyleOption(option)
    style = spin.style()
    for sub in (QStyle.SubControl.SC_SpinBoxUp, QStyle.SubControl.SC_SpinBoxDown):
        rect = style.subControlRect(QStyle.ComplexControl.CC_SpinBox, option, sub, spin)
        if rect.contains(local):
            return True
    return False


def _read(host):
    if isinstance(host, QComboBox):
        return host.currentIndex()
    return host.value()


def _write(host, origin, dy):
    if isinstance(host, QComboBox):
        index = scrub_index(origin, dy, host.count())
        if index != host.currentIndex():
            host.setCurrentIndex(index)
        return
    value = scrub_value(origin, dy, host.minimum(), host.maximum(), host.singleStep())
    decimals = int(host.decimals()) if hasattr(host, "decimals") else 0
    if decimals <= 0:
        value = int(round(value))
    else:
        value = round(float(value), decimals)
    if value != host.value():
        host.setValue(value)


def _mark_tip(host):
    if host.property("scrubTip"):
        return
    host.setProperty("scrubTip", True)
    tip = host.toolTip()
    extra = "Drag up or down."
    host.setToolTip((tip + "    " + extra).strip() if tip else extra)


def _drag_cursor(host, on):
    if on:
        host.setCursor(Qt.SizeVerCursor)
    else:
        host.unsetCursor()
    line = host.lineEdit() if hasattr(host, "lineEdit") else None
    if line is None:
        return
    if on:
        line.setCursor(Qt.SizeVerCursor)
        line.deselect()
    else:
        line.unsetCursor()


class ScrubFilter(QObject):
    """Application filter. Number fields and menus scrub under a drag."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self._clear()

    def _clear(self):
        self._host = None
        self._kind = ""
        self._x = 0.0
        self._y = 0.0
        self._origin = 0

    def eventFilter(self, watched, event):
        pointer = _pointer_kind(event)
        if pointer is None:
            return False
        kind = event.type()
        if kind in (QEvent.Type.MouseButtonPress, QEvent.Type.TabletPress, QEvent.Type.TouchBegin):
            return self._press(watched, event, pointer)
        if kind in (QEvent.Type.MouseMove, QEvent.Type.TabletMove, QEvent.Type.TouchUpdate):
            return self._move(event, pointer)
        if kind in (
            QEvent.Type.MouseButtonRelease,
            QEvent.Type.TabletRelease,
            QEvent.Type.TouchEnd,
            QEvent.Type.TouchCancel,
        ):
            return self._release(event, pointer)
        return False

    def _press(self, watched, event, pointer):
        if self._host is not None and pointer == "mouse" and _synthesized(event):
            return True
        if pointer == "mouse" and event.button() != Qt.LeftButton:
            return False
        if pointer == "touch":
            try:
                if len(event.points()) > 1:
                    return False
            except Exception:
                return False
        pos = _global(event)
        if pos is None:
            return False
        host = _control(watched, pos)
        if host is None:
            return False
        if isinstance(host, QComboBox) and host.view() is not None and host.view().isVisible():
            return False
        if isinstance(host, QAbstractSpinBox) and _on_stepper(host, pos):
            return False
        host.setAttribute(Qt.WidgetAttribute.WA_AcceptTouchEvents, True)
        _mark_tip(host)
        self._host = host
        self._kind = pointer
        self._x, self._y = pos
        self._origin = _read(host)
        return False

    def _move(self, event, pointer):
        if self._host is None:
            return False
        if pointer != self._kind:
            return pointer == "mouse" and _synthesized(event)
        if pointer == "mouse" and not (event.buttons() & Qt.LeftButton):
            return False
        if pointer == "touch":
            try:
                if len(event.points()) > 1:
                    self._cancel()
                    return False
            except Exception:
                return False
        pos = _global(event)
        if pos is None:
            return False
        dx = pos[0] - self._x
        dy = self._y - pos[1]
        if not scrubbing():
            if abs(dx) < 4.0 and abs(dy) < 4.0:
                return pointer != "mouse"
            if abs(dx) > abs(dy) + 2.0:
                self._clear()
                return False
            _begin_scrub()
            _drag_cursor(self._host, True)
            if isinstance(self._host, QComboBox):
                self._host.hidePopup()
        _write(self._host, self._origin, dy)
        return True

    def _release(self, event, pointer):
        if self._host is None:
            return False
        if pointer != self._kind and not (pointer == "mouse" and _synthesized(event)):
            return False
        moved = scrubbing()
        self._cancel()
        return moved or pointer != "mouse"

    def _cancel(self):
        host = self._host
        if host is not None and scrubbing():
            _drag_cursor(host, False)
        _end_scrub()
        self._clear()
