"""Vertical exposure sheet. One column is one level. The left column is the back.

Rows are frames, numbered from 1. A cell holds one drawing id or is empty.
The playhead is the filmstrip frame. This is the same sheet, turned upright.
"""

from PySide6.QtCore import QRect, QSize, Qt, Signal
from PySide6.QtGui import QColor, QFont, QPainter
from PySide6.QtWidgets import QMenu, QSizePolicy, QWidget

CELL_W = 44
CELL_H = 16
HEAD = 18
FRAME_W = 32
INK = "#ffffff"
QUIET = "#3a3a3a"
RULE = "#2a2a2a"
FIELD = "#000000"


def run_command(sheet, name, start, count, arg):
    """One cell command. False means the sheet was left as it was."""
    if name == "reverse":
        return bool(sheet.reverse(start, count))
    if name == "swing":
        return bool(sheet.swing(start, count))
    if name == "step":
        return bool(sheet.step(start, count, arg))
    if name == "each":
        return bool(sheet.each(start, count, arg))
    if name == "reset":
        return bool(sheet.reset_step(start, count))
    if name == "increase":
        return bool(sheet.increase_step(start, count))
    if name == "decrease":
        return bool(sheet.decrease_step(start, count))
    if name == "autoexpose":
        return bool(sheet.autoexpose(start, count))
    if name == "reframe":
        return bool(sheet.reframe(start, count, arg))
    if name == "repeat":
        return bool(sheet.repeat(start, count, arg))
    if name == "insert":
        return bool(sheet.insert_empty(start, count))
    if name == "clear":
        return bool(sheet.clear_cells(start, count))
    if name == "cut":
        return bool(sheet.remove_cells(start, count))
    return False


class XSheet(QWidget):
    """Columns across, frames down. A command names the selected cells."""

    command = Signal(int, int, int, str, int)

    def __init__(self):
        super().__init__()
        self.columns = []
        self.playhead = 0
        self._sel = None
        self.setFocusPolicy(Qt.ClickFocus)
        self.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
        self._fit()

    def set_columns(self, columns):
        """columns is (level name, slots). Slots are drawing ids or blanks."""
        self.columns = [(str(name), list(slots or [])) for name, slots in columns]
        if self._sel is not None:
            col, _start, end = self._sel
            if col >= len(self.columns) or end >= len(self.columns[col][1]):
                self._sel = None
        self._fit()
        self.update()

    def set_playhead(self, frame):
        self.playhead = max(0, int(frame))
        self._fit()
        self.update()

    def row_count(self):
        longest = max((len(slots) for _name, slots in self.columns), default=0)
        return max(longest, self.playhead + 1, 1)

    def cell_text(self, col, row):
        if col < 0 or col >= len(self.columns):
            return ""
        slots = self.columns[col][1]
        if row < 0 or row >= len(slots):
            return ""
        return slots[row]

    def frame_label(self, row):
        return "%d" % (int(row) + 1)

    def sizeHint(self):
        width = FRAME_W + CELL_W * max(1, len(self.columns))
        return QSize(width, HEAD + CELL_H * self.row_count())

    def _fit(self):
        hint = self.sizeHint()
        self.setMinimumSize(hint)
        self.resize(hint)

    def _hit(self, pos):
        if pos.y() < HEAD or pos.x() < FRAME_W:
            return None
        col = (pos.x() - FRAME_W) // CELL_W
        row = (pos.y() - HEAD) // CELL_H
        if col < 0 or col >= len(self.columns) or row < 0 or row >= len(self.columns[col][1]):
            return None
        return int(col), int(row)

    def _select(self, col, row, extend):
        if extend and self._sel is not None and self._sel[0] == col:
            _col, start, end = self._sel
            self._sel = (col, min(start, row), max(end, row))
        else:
            self._sel = (col, row, row)
        self.update()

    def mousePressEvent(self, event):
        if event.button() != Qt.LeftButton:
            return
        hit = self._hit(event.position().toPoint())
        if hit is None:
            return
        self._select(hit[0], hit[1], bool(event.modifiers() & Qt.ShiftModifier))

    def contextMenuEvent(self, event):
        hit = self._hit(event.pos())
        if hit is not None:
            col, row = hit
            inside = (
                self._sel is not None
                and self._sel[0] == col
                and self._sel[1] <= row <= self._sel[2]
            )
            if not inside:
                self._select(col, row, False)
        if self._sel is None:
            return
        menu = QMenu(self)
        menu.addAction("Reverse", lambda: self._emit("reverse", 0))
        menu.addAction("Swing", lambda: self._emit("swing", 0))
        step = menu.addMenu("Step")
        for number in (2, 3, 4):
            step.addAction("Step %d" % number, lambda n=number: self._emit("step", n))
        each = menu.addMenu("Each")
        for number in (2, 3, 4):
            each.addAction("Each %d" % number, lambda n=number: self._emit("each", n))
        menu.addAction("Reset Step", lambda: self._emit("reset", 0))
        menu.addAction("Increase Step", lambda: self._emit("increase", 0))
        menu.addAction("Decrease Step", lambda: self._emit("decrease", 0))
        menu.addAction("Autoexpose", lambda: self._emit("autoexpose", 0))
        reframe = menu.addMenu("Reframe")
        for number in (1, 2, 3, 4):
            reframe.addAction("Reframe on %d's" % number, lambda n=number: self._emit("reframe", n))
        menu.addAction("Repeat", lambda: self._emit("repeat", 1))
        menu.addAction("Insert", lambda: self._emit("insert", 0))
        menu.addAction("Clear", lambda: self._emit("clear", 0))
        menu.addAction("Cut", lambda: self._emit("cut", 0))
        menu.exec(event.globalPos())

    def _emit(self, name, arg):
        if self._sel is None:
            return
        col, start, end = self._sel
        self.command.emit(col, start, end - start + 1, name, int(arg))

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), QColor(FIELD))
        font = QFont("monospace", 9)
        font.setStyleHint(QFont.Monospace)
        painter.setFont(font)
        painter.setPen(QColor(RULE))
        painter.drawLine(0, HEAD - 1, self.width(), HEAD - 1)
        painter.drawLine(FRAME_W - 1, 0, FRAME_W - 1, self.height())
        for col, (name, _slots) in enumerate(self.columns):
            left = FRAME_W + col * CELL_W
            painter.setPen(QColor(QUIET))
            painter.drawText(QRect(left, 0, CELL_W - 2, HEAD), Qt.AlignCenter, name[:6])
            painter.setPen(QColor(RULE))
            painter.drawLine(left + CELL_W - 1, 0, left + CELL_W - 1, self.height())
        rows = self.row_count()
        for row in range(rows):
            top = HEAD + row * CELL_H
            current = row == self.playhead
            painter.setPen(QColor(INK if current else QUIET))
            painter.drawText(QRect(0, top, FRAME_W - 2, CELL_H), Qt.AlignCenter, self.frame_label(row))
            painter.setPen(QColor(RULE))
            painter.drawLine(0, top + CELL_H - 1, self.width(), top + CELL_H - 1)
            for col, (_name, slots) in enumerate(self.columns):
                if row >= len(slots) or not slots[row]:
                    continue
                left = FRAME_W + col * CELL_W
                painter.setPen(QColor(INK if current else QUIET))
                painter.drawText(QRect(left, top, CELL_W - 2, CELL_H), Qt.AlignCenter, slots[row])
        if self._sel is not None:
            col, start, end = self._sel
            rect = QRect(
                FRAME_W + col * CELL_W,
                HEAD + start * CELL_H,
                CELL_W - 1,
                (end - start + 1) * CELL_H - 1,
            )
            painter.setPen(QColor(INK))
            painter.drawRect(rect)
        painter.end()
