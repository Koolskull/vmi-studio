"""Numbers and menus drag up and down. A pen and a finger use that same drag."""

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QEvent, QPoint, QPointF, Qt
from PySide6.QtGui import QMouseEvent
from PySide6.QtWidgets import QApplication, QComboBox, QDoubleSpinBox, QSpinBox

from vmi_studio.drawbar import CurvePlot, SoftKnob
from vmi_studio.history import Edit, History
from vmi_studio.scrub import (
    ScrubFilter,
    pixels_per_step,
    scrub_index,
    scrub_value,
    scrubbing,
    _begin_scrub,
    _end_scrub,
)


def _app():
    app = QApplication.instance()
    if app is None:
        app = QApplication([])
    return app


class _Pointer:
    """A pen or finger event the filter can read without a real tablet device."""

    def __init__(self, kind, x, y, button=Qt.LeftButton, buttons=Qt.LeftButton, points=None):
        self._kind = kind
        self._x = float(x)
        self._y = float(y)
        self._button = button
        self._buttons = buttons
        self._points = points

    def type(self):
        return self._kind

    def button(self):
        return self._button

    def buttons(self):
        return self._buttons

    def position(self):
        return QPointF(self._x, self._y)

    def globalPosition(self):
        return QPointF(self._x, self._y)

    def source(self):
        return "synthesized"

    def points(self):
        if self._points is not None:
            return self._points
        return [_Pointer(self._kind, self._x, self._y)]

    def state(self):
        return Qt.TouchPointState.TouchPointPressed if hasattr(Qt, "TouchPointState") else 0

    def accept(self):
        return None

    def ignore(self):
        return None


def _field_point(spin):
    line = spin.lineEdit()
    if line is not None and line.width() > 4 and line.height() > 4:
        local = line.mapTo(spin, QPoint(line.width() // 2, line.height() // 2))
        return local
    return QPoint(8, max(4, spin.height() // 2))


def _send(app, widget, kind, local, global_pos, button, buttons):
    event = QMouseEvent(
        kind,
        QPointF(local),
        QPointF(global_pos),
        button,
        buttons,
        Qt.NoModifier,
    )
    app.sendEvent(widget, event)


def _drag(app, widget, ups):
    """ups are pixels upward from the press. One press, then each move, then release."""
    widget.show()
    app.processEvents()
    target = widget.lineEdit() if hasattr(widget, "lineEdit") and widget.lineEdit() is not None else widget
    local = _field_point(widget)
    start = widget.mapToGlobal(local)
    _send(app, target, QEvent.Type.MouseButtonPress, local, start, Qt.LeftButton, Qt.LeftButton)
    for up in ups:
        moved = QPoint(local.x(), local.y() - int(up))
        _send(
            app,
            target,
            QEvent.Type.MouseMove,
            moved,
            widget.mapToGlobal(moved),
            Qt.LeftButton,
            Qt.LeftButton,
        )
    end = widget.mapToGlobal(QPoint(local.x(), local.y() - int(ups[-1] if ups else 0)))
    _send(app, target, QEvent.Type.MouseButtonRelease, local, end, Qt.LeftButton, Qt.NoButton)
    app.processEvents()


class ScrubMathTests(unittest.TestCase):
    def test_up_raises_and_a_long_range_moves_faster(self):
        self.assertEqual(pixels_per_step(1.0, 0.01), 4.0)
        self.assertEqual(pixels_per_step(8191, 1), 0.35)
        self.assertEqual(scrub_value(50, 20, 0, 100, 1), 55)
        self.assertEqual(scrub_value(50, -20, 0, 100, 1), 45)
        self.assertEqual(scrub_value(2, 1000, 0, 10, 1), 10)
        self.assertEqual(scrub_value(2, -1000, 0, 10, 1), 0)
        self.assertEqual(scrub_index(1, 14, 3), 2)
        self.assertEqual(scrub_index(1, -14, 3), 0)
        self.assertEqual(scrub_index(0, 100, 1), 0)


class ScrubWidgetTests(unittest.TestCase):
    def setUp(self):
        self.app = _app()
        self.filter = ScrubFilter()
        self.app.installEventFilter(self.filter)

    def tearDown(self):
        self.app.removeEventFilter(self.filter)
        self.assertFalse(scrubbing())

    def test_a_spin_follows_the_drag_and_a_sideways_drag_does_not(self):
        spin = QSpinBox()
        spin.setRange(0, 100)
        spin.setValue(40)
        spin.setToolTip("Size")
        spin.resize(140, 28)
        _drag(self.app, spin, [24])
        self.assertEqual(spin.value(), 46)
        self.assertIn("Drag up or down.", spin.toolTip())
        _drag(self.app, spin, [-24])
        self.assertEqual(spin.value(), 40)
        before = spin.value()
        spin.show()
        self.app.processEvents()
        target = spin.lineEdit()
        local = _field_point(spin)
        start = spin.mapToGlobal(local)
        _send(self.app, target, QEvent.Type.MouseButtonPress, local, start, Qt.LeftButton, Qt.LeftButton)
        side = QPoint(local.x() + 40, local.y())
        _send(
            self.app,
            target,
            QEvent.Type.MouseMove,
            side,
            spin.mapToGlobal(side),
            Qt.LeftButton,
            Qt.LeftButton,
        )
        _send(self.app, target, QEvent.Type.MouseButtonRelease, side, spin.mapToGlobal(side), Qt.LeftButton, Qt.NoButton)
        self.assertEqual(spin.value(), before)
        spin.close()

    def test_a_fraction_and_a_menu_follow_the_same_drag(self):
        box = QDoubleSpinBox()
        box.setRange(0.0, 1.0)
        box.setDecimals(4)
        box.setSingleStep(0.01)
        box.setValue(0.20)
        box.resize(140, 28)
        _drag(self.app, box, [20])
        self.assertAlmostEqual(box.value(), 0.25, places=4)
        box.close()
        menu = QComboBox()
        menu.addItems(["pen", "paint", "eraser"])
        menu.resize(140, 28)
        _drag(self.app, menu, [28])
        self.assertEqual(menu.currentIndex(), 2)
        _drag(self.app, menu, [-28])
        self.assertEqual(menu.currentIndex(), 0)
        menu.close()

    def test_a_pen_raises_the_number_and_a_second_finger_lets_go(self):
        spin = QSpinBox()
        spin.setRange(0, 100)
        spin.setValue(40)
        spin.resize(140, 28)
        spin.show()
        self.app.processEvents()
        local = _field_point(spin)
        start = spin.mapToGlobal(local)
        self.filter.eventFilter(spin, _Pointer(QEvent.Type.TabletPress, start.x(), start.y()))
        up = spin.mapToGlobal(QPoint(local.x(), local.y() - 24))
        self.filter.eventFilter(spin, _Pointer(QEvent.Type.TabletMove, up.x(), up.y()))
        self.assertEqual(spin.value(), 46)
        held = spin.value()
        further = spin.mapToGlobal(QPoint(local.x(), local.y() - 80))
        swallowed = self.filter.eventFilter(
            spin, _Pointer(QEvent.Type.MouseMove, further.x(), further.y())
        )
        self.assertTrue(swallowed)
        self.assertEqual(spin.value(), held)
        self.filter.eventFilter(spin, _Pointer(QEvent.Type.TabletRelease, up.x(), up.y(), Qt.LeftButton, Qt.NoButton))
        self.assertFalse(scrubbing())
        begin = spin.mapToGlobal(local)
        self.filter.eventFilter(spin, _Pointer(QEvent.Type.TouchBegin, begin.x(), begin.y()))
        touch_up = spin.mapToGlobal(QPoint(local.x(), local.y() - 24))
        self.filter.eventFilter(spin, _Pointer(QEvent.Type.TouchUpdate, touch_up.x(), touch_up.y()))
        self.assertEqual(spin.value(), 52)
        extra = _Pointer(QEvent.Type.TouchUpdate, touch_up.x(), touch_up.y() - 40)
        extra._points = [
            _Pointer(QEvent.Type.TouchUpdate, touch_up.x(), touch_up.y()),
            _Pointer(QEvent.Type.TouchUpdate, touch_up.x() + 20, touch_up.y()),
        ]
        self.assertFalse(self.filter.eventFilter(spin, extra))
        self.assertEqual(spin.value(), 52)
        self.assertFalse(scrubbing())
        spin.close()

    def test_a_read_only_number_stays_put(self):
        spin = QSpinBox()
        spin.setRange(0, 100)
        spin.setValue(40)
        spin.setReadOnly(True)
        spin.resize(140, 28)
        _drag(self.app, spin, [24])
        self.assertEqual(spin.value(), 40)
        spin.close()


class PointerWidgetTests(unittest.TestCase):
    def test_the_softness_control_and_the_curve_take_a_pen(self):
        _app()
        knob = SoftKnob()
        self.assertTrue(knob.testAttribute(Qt.WidgetAttribute.WA_AcceptTouchEvents))
        press = _Pointer(QEvent.Type.TabletPress, 10, 40)
        move = _Pointer(QEvent.Type.TabletMove, 10, 20)
        self.assertTrue(knob.event(press))
        self.assertTrue(knob.event(move))
        self.assertAlmostEqual(knob.value, 0.25, places=4)
        mouse = QMouseEvent(
            QEvent.Type.MouseButtonPress,
            QPointF(10, 40),
            QPointF(10, 40),
            Qt.LeftButton,
            Qt.LeftButton,
            Qt.NoModifier,
        )
        mouse_move = QMouseEvent(
            QEvent.Type.MouseMove,
            QPointF(10, 20),
            QPointF(10, 20),
            Qt.LeftButton,
            Qt.LeftButton,
            Qt.NoModifier,
        )
        fresh = SoftKnob()
        self.assertFalse(take_mouse(fresh, mouse, mouse_move))
        self.assertAlmostEqual(fresh.value, 0.25, places=4)
        plot = CurvePlot([[0.0, 0.5], [1.0, 0.5]])
        plot.resize(180, 100)
        plot.show()
        _app().processEvents()
        pos = plot._to_widget(0.0, 0.5)
        self.assertTrue(plot.event(_Pointer(QEvent.Type.TabletPress, pos.x(), pos.y())))
        self.assertTrue(plot.event(_Pointer(QEvent.Type.TabletMove, pos.x(), pos.y() - 20)))
        self.assertGreater(plot.points[0][1], 0.5)
        plot.close()


def take_mouse(widget, press, move):
    """The mouse path still runs. take_pointer leaves a real mouse event alone."""
    from vmi_studio.scrub import take_pointer

    if take_pointer(widget, press):
        return True
    widget.event(press)
    widget.event(move)
    return False


class ScrubHistoryTests(unittest.TestCase):
    def tearDown(self):
        _end_scrub()

    def test_one_drag_is_one_undo_and_the_next_drag_is_another(self):
        history = History()
        _begin_scrub()
        history._push("the outline", "before", "mid")
        history._push("the outline", "ignored", "after")
        self.assertEqual(len(history.undo_stack), 1)
        self.assertEqual(history.undo_stack[0].before, "before")
        self.assertEqual(history.undo_stack[0].after, "after")
        _end_scrub()
        _begin_scrub()
        history._push("the outline", "second", "second-after")
        self.assertEqual(len(history.undo_stack), 2)
        self.assertEqual(history.undo_stack[0].after, "after")
        self.assertEqual(history.undo_stack[1].before, "second")
        _end_scrub()
        history._push("the outline", "plain", "plain-after")
        self.assertEqual(len(history.undo_stack), 3)
        self.assertIsInstance(history.undo_stack[-1], Edit)

    def test_the_desk_size_and_outline_drag_once(self):
        from tests.test_studio import ToolWindowTests

        app, win = ToolWindowTests()._open([])
        win.open_blank(32, 32)
        app.processEvents()
        win.set_draw_mode("pen")
        app.processEvents()
        size = win.draw_bar.size_box
        start = size.value()
        _drag(app, size, [24])
        self.assertGreater(size.value(), start)
        self.assertEqual(win.draw_bar.brush.size, size.value())
        win.outline_box.setChecked(True)
        app.processEvents()
        width = win.outline_px.value()
        steps = len(win.history.undo_stack)
        _drag(app, win.outline_px, [16, 40])
        self.assertGreater(win.outline_px.value(), width)
        self.assertEqual(len(win.history.undo_stack), steps + 1)
        self.assertEqual(win.history.undo_stack[-1].label, "the outline")
        win.undo()
        self.assertEqual(win.outline_px.value(), width)
        self.assertTrue(win.outline_box.isChecked())
        win.close()
