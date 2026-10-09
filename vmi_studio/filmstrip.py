"""A horizontal frame ruler for a Clip Studio animation folder.

Each row is one animation folder. A cel is one block from the frame where it
was specified until the next specification. The last cel holds through the
end of the cut. One border wraps that whole block. A longer cel has no
vertical line inside it. The Clip Studio frame number is white, on the first
frame of the cel. That number is the cel's place in timeline order, starting
at 1. It is not the global frame, and it is not the layer name in the
outliner. Empty space, including past the cut and to the right of
the playhead, is one #333333 grid. A cel covers the columns it holds with
its solid block. The block's border is #666666 and 2px, one pixel thicker
than the grid, so a single frame stays visible.
A child that was never specified stays off the frames.
"""

from PySide6.QtCore import QPoint, QRect, QSize, Qt, Signal
from PySide6.QtGui import QColor, QFont, QFontMetrics, QPainter, QPen
from PySide6.QtWidgets import (
    QHBoxLayout,
    QPushButton,
    QSizePolicy,
    QSplitterHandle,
    QToolTip,
    QWidget,
)

from vmi_studio import theme
from vmi_studio.document import animation_folders, cel_at, find_node, frame_map, spare_cels

GUTTER = 132
CELL = 14
HEAD = 16
ROW = 18
INK = "#ffffff"
QUIET = "#3a3a3a"
RULE = "#2a2a2a"
FIELD = "#000000"
# A cel is one solid block. Empty space keeps the same #333333 grid.
GRID = "#040404"
FRAME_LINE = "#333333"
EMPTY_LINE = "#333333"
# Lighter than the grid, and one pixel thicker than its 1px lines.
CELL_EDGE = "#666666"
BORDER = 2
# The cel's frame number is INK. NUMBER stays for a theme that still names it.
NUMBER = "#a0a0a0"
# Red is the playhead. Green is reserved for a later record state and is not shown.
PLAYHEAD_RED = "#ff0000"
PLAYHEAD_GREEN = "#00ff00"

EMPTY = (
    "No animation folder. Clip Studio marks one with Animation folder. "
    "A cel is a child of that folder, specified on a frame."
)


class TimelineGutter(QSplitterHandle):
    """The picture/timeline handle. The line and the Timeline button are this widget.

    Dragging either side of the button moves the split. The button rides on the
    line because it is painted here, not on a second widget. A click on the
    word hides the strip.
    """

    clicked = Signal()
    dragged = Signal()
    LABEL = "Timeline"
    BAND = 18
    BAR = 7
    REST = "#141414"
    HOVER = "#3a3a3a"
    HELD = "#ffffff"

    def __init__(self, orientation, splitter):
        super().__init__(orientation, splitter)
        self.setObjectName("timelineGutter")
        self._hover = False
        self._bar_hover = False
        self._down = False
        self._press_y = None
        self._press_pos = None
        self._moved = False
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.NoFocus)
        self.setCursor(Qt.SplitVCursor)
        self.setAutoFillBackground(False)
        self.setAttribute(Qt.WA_StyledBackground, False)

    def button_rect(self):
        width = QFontMetrics(self.font()).horizontalAdvance(self.LABEL) + 16
        height = min(self.BAND, self.height() or self.BAND)
        top = max(0, (self.height() - height) // 2)
        return QRect((self.width() - width) // 2, top, width, height)

    def _bar_rect(self):
        thickness = min(self.BAR, self.height() or self.BAR)
        center = self.button_rect().center().y()
        top = max(0, min(center - thickness // 2, max(0, self.height() - thickness)))
        return QRect(0, top, max(0, self.width()), thickness)

    def _shift(self, delta):
        """Move this handle by delta pixels from where the press began."""
        if self._press_pos is None:
            return
        if not self._moved and int(delta) != 0:
            self._moved = True
            self.dragged.emit()
        self.moveSplitter(int(self._press_pos) + int(delta))
        if self._moved:
            self.dragged.emit()

    def mouseMoveEvent(self, event):
        if self._press_y is not None and (event.buttons() & Qt.LeftButton):
            self._shift(int(event.globalPosition().y() - self._press_y))
            event.accept()
            return
        point = event.position().toPoint()
        on_button = self.button_rect().contains(point)
        if on_button != self._hover or (not on_button) != self._bar_hover:
            self._hover = on_button
            self._bar_hover = not on_button
            self.setCursor(Qt.PointingHandCursor if on_button else Qt.SplitVCursor)
            self.update()
        event.accept()

    def leaveEvent(self, event):
        self._hover = False
        self._bar_hover = False
        if self._press_y is None:
            self.setCursor(Qt.SplitVCursor)
        self.update()
        super().leaveEvent(event)

    def mousePressEvent(self, event):
        point = event.position().toPoint()
        if event.button() == Qt.LeftButton and self.button_rect().contains(point):
            self._press_y = None
            self._press_pos = None
            self._down = False
            self.clicked.emit()
            event.accept()
            return
        if event.button() == Qt.LeftButton:
            self._press_y = event.globalPosition().y()
            self._press_pos = self.y()
            self._moved = False
            self._down = True
            self._bar_hover = False
            self.grabMouse()
            self.update()
            event.accept()
            return
        super().mousePressEvent(event)

    def mouseReleaseEvent(self, event):
        moved = self._moved
        self._press_y = None
        self._press_pos = None
        self._moved = False
        if self._down:
            self._down = False
            self.update()
        if QWidget.mouseGrabber() is self:
            self.releaseMouse()
        if moved:
            self.dragged.emit()
        event.accept()

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), QColor(FIELD))
        if self._down:
            tone = self.HELD
        elif self._bar_hover:
            tone = self.HOVER
        else:
            tone = self.REST
        painter.fillRect(self._bar_rect(), QColor(tone))
        box = self.button_rect()
        painter.fillRect(box, QColor(FIELD))
        painter.setPen(QColor(RULE))
        painter.drawRect(box.adjusted(0, 0, -1, -1))
        painter.setPen(QColor(INK if self._hover else QUIET))
        painter.drawText(box, Qt.AlignCenter, self.LABEL)
        painter.end()

    def sizeHint(self):
        return QSize(120, self.BAND)


class Transport(QWidget):
    """Play, stop, step, and loop. The strip can be hidden and this stays."""

    back_frame = Signal()
    forward_frame = Signal()
    play = Signal()
    loop = Signal()

    def __init__(self):
        super().__init__()
        self.setObjectName("transport")
        self.setFixedHeight(22)
        row = QHBoxLayout(self)
        row.setContentsMargins(4, 2, 4, 2)
        row.setSpacing(4)
        self.back_button = self._button("Back", "Previous frame", self.back_frame)
        self.play_button = self._button("Play", "Play or stop", self.play)
        self.forward_button = self._button("Forward", "Next frame", self.forward_frame)
        self.loop_button = self._button("Loop", "Loop playback", self.loop)
        for button in (self.back_button, self.play_button, self.forward_button, self.loop_button):
            row.addWidget(button)
        row.addStretch(1)
        self.setStyleSheet(theme.transport_sheet())

    def _button(self, label, tip, signal):
        button = QPushButton(label)
        button.setFocusPolicy(Qt.NoFocus)
        button.setFixedHeight(18)
        button.setCursor(Qt.PointingHandCursor)
        button.setToolTip(tip)
        button.clicked.connect(signal.emit)
        return button

    def set_playing(self, playing):
        self.play_button.setText("Stop" if playing else "Play")
        self._mark(self.play_button, playing)

    def set_looping(self, looping):
        self._mark(self.loop_button, looping)

    def _mark(self, button, on):
        button.setProperty("on", True if on else False)
        button.style().unpolish(button)
        button.style().polish(button)
        button.update()

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), QColor(FIELD))
        painter.setPen(QColor(RULE))
        painter.drawLine(0, 0, self.width(), 0)
        painter.end()


class Filmstrip(QWidget):
    """The timeline under the picture. The old list stays hidden."""

    cel_chosen = Signal(str)
    moved = Signal(str, str, int)
    scrubbed = Signal(int)

    def __init__(self):
        super().__init__()
        self.art = None
        self.selected = ""
        self.playhead = 0
        self._playhead_tone = "red"
        self._scroll = 0
        self._drag = None
        self._scrub = False
        self._hover = None
        self.setMouseTracking(True)
        self.setFocusPolicy(Qt.ClickFocus)
        self.setMinimumHeight(HEAD + ROW + 6)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)

    def set_project(self, art, selected=""):
        changed = art is not self.art
        self.art = art
        self.selected = selected or ""
        if changed:
            info = getattr(art, "clip_time", None) if art is not None else None
            self.playhead = int(info.get("current") or 0) if isinstance(info, dict) else 0
            self._scroll = 0
            self._drag = None
            self._scrub = False
            self._reveal_playhead()
        self._fit_height()
        self.update()

    def set_playhead_tone(self, tone):
        """Red is the line in use. Green is kept for a later record state."""
        self._playhead_tone = "green" if tone == "green" else "red"
        self.update()

    def playhead_color(self):
        if self._playhead_tone == "green":
            return PLAYHEAD_GREEN
        return PLAYHEAD_RED

    def set_playhead(self, frame):
        """Move the line. A change scrolls that frame into view."""
        frame = int(frame)
        if frame == int(self.playhead):
            return
        self.playhead = frame
        self._reveal_playhead()
        self.update()

    def rows(self):
        if self.art is None:
            return []
        return animation_folders(self.art.layers)

    def cut_range(self):
        """The frames playback and the step keys use. A cel drag can reach past this."""
        start, end = 0, 0
        info = getattr(self.art, "clip_time", None) if self.art is not None else None
        if isinstance(info, dict) and info.get("end") is not None:
            start = int(info.get("start") or 0)
            end = max(start, int(info.get("end") or 0))
            return start, end
        for folder in self.rows():
            for frame, _cel in frame_map(folder):
                end = max(end, int(frame))
        return start, end

    def go_frame(self, frame):
        """Move the playhead inside the cut and tell the picture. Same frame does nothing."""
        start, end = self.cut_range()
        frame = max(start, min(end, int(frame)))
        if frame == int(self.playhead):
            return
        self.playhead = frame
        self._reveal_playhead()
        self.update()
        self.scrubbed.emit(int(frame))

    def span(self):
        start, end = 0, 16
        info = getattr(self.art, "clip_time", None) if self.art is not None else None
        if isinstance(info, dict):
            start = int(info.get("start") or 0)
            end = max(end, int(info.get("end") or 0))
        for folder in self.rows():
            for frame, _cel in frame_map(folder):
                start = min(start, int(frame))
                end = max(end, int(frame))
            if getattr(self, "_drag", None) and self._drag.get("folder") == folder.id:
                end = max(end, int(self._drag.get("frame") or 0))
        if self.playhead > end:
            end = self.playhead
        return start, max(end, start)

    def blocks(self, folder):
        """(start frame, hold length, cel). A drag previews the new start."""
        pairs = list(frame_map(folder))
        if not pairs:
            return []
        info = getattr(self.art, "clip_time", None) if self.art is not None else None
        cut = int(info.get("end") or 0) if isinstance(info, dict) else 0
        end = max(cut, max(int(frame) for frame, _cel in pairs))
        drag = self._drag or {}
        blocks = []
        for index, (frame, cel) in enumerate(pairs):
            nxt = pairs[index + 1][0] if index + 1 < len(pairs) else None
            length = max(1, (int(nxt) if nxt is not None else end + 1) - int(frame))
            at = int(frame)
            if drag.get("cel") == cel.id and drag.get("frame") is not None:
                at = int(drag["frame"])
            blocks.append((at, length, cel))
        blocks.sort(key=lambda item: (item[0], item[2].id))
        return blocks

    def choose(self, folder_id, frame):
        """Select the cel holding this frame. Tests call this directly."""
        folder = find_node(self.art.layers, folder_id) if self.art is not None else None
        if folder is None:
            return None
        self.playhead = int(frame)
        cel = cel_at(folder, self.playhead)
        self.update()
        if cel is not None:
            self.cel_chosen.emit(cel.id)
        return cel

    def _preferred_height(self):
        """Eight folders stay on screen until the bar is dragged taller."""
        count = max(1, len(self.rows()))
        return min(HEAD + 8 * ROW + 8, max(HEAD + ROW + 6, HEAD + count * ROW + 8))

    def _fit_height(self):
        self.updateGeometry()

    def sizeHint(self):
        return QSize(max(self.width(), 320), self._preferred_height())

    def minimumSizeHint(self):
        return QSize(160, HEAD + ROW + 6)

    def _frame_at(self, x):
        start, end = self.span()
        local = int(x) - GUTTER + self._scroll
        if local < 0:
            return None
        frame = start + int(local // CELL)
        if frame > end + 8:
            return None
        return frame

    def _row_at(self, y):
        folders = self.rows()
        index = int((int(y) - HEAD) // ROW)
        if index < 0 or index >= len(folders):
            return None
        return folders[index]

    def _key_of(self, folder, cel):
        for frame, item in frame_map(folder):
            if item.id == cel.id:
                return int(frame)
        return None

    def _content_width(self):
        start, end = self.span()
        return (end - start + 1) * CELL

    def _clamp_scroll(self):
        room = max(0, self.width() - GUTTER)
        self._scroll = max(0, min(self._scroll, max(0, self._content_width() - room)))

    def _x_of(self, frame):
        start, _end = self.span()
        return GUTTER + (int(frame) - start) * CELL - self._scroll

    def playhead_x(self):
        """Center of the playhead cell. The line is drawn here, over the blocks."""
        return self._x_of(self.playhead) + CELL // 2

    def _reveal_playhead(self):
        """Scroll so the playhead cell is inside the frame area. Content x ignores scroll."""
        start, _end = self.span()
        content = (int(self.playhead) - start) * CELL
        room = max(0, self.width() - GUTTER)
        if room <= 0:
            return
        if content < self._scroll:
            self._scroll = content
        elif content + CELL > self._scroll + room:
            self._scroll = content + CELL - room
        self._clamp_scroll()

    def _scrub_frame(self, x):
        """Frame under x, clamped to the cut. A cel drag may use the extra pad; a scrub does not."""
        start, end = self.span()
        local = int(x) - GUTTER + self._scroll
        frame = start if local < 0 else start + int(local // CELL)
        return max(start, min(end, frame))

    def _scrub_to(self, frame, reveal=False):
        start, end = self.span()
        frame = max(start, min(end, int(frame)))
        if frame == int(self.playhead):
            return
        self.playhead = frame
        if reveal:
            self._reveal_playhead()
        self.update()
        self.scrubbed.emit(int(frame))

    def _on_playhead(self, x):
        center = self.playhead_x()
        if center < GUTTER or center > self.width():
            return False
        return abs(int(x) - center) <= 4

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing, False)
        painter.fillRect(self.rect(), QColor(FIELD))
        painter.setPen(QPen(QColor(RULE)))
        painter.drawLine(0, 0, self.width(), 0)
        folders = self.rows()
        if not folders:
            painter.setPen(QColor(QUIET))
            painter.drawText(self.rect().adjusted(12, 8, -12, -8), Qt.AlignVCenter | Qt.TextWordWrap, EMPTY)
            painter.end()
            return
        self._paint_grid(painter)
        self._paint_ruler(painter)
        for index, folder in enumerate(folders):
            self._paint_row(painter, folder, HEAD + index * ROW)
        self._paint_playhead(painter)
        painter.end()

    def frame_numbers(self, folder, frame, length):
        """The cel's place on this timeline, starting at 1.

        Clip Studio keeps that order on the track. It is not the global frame
        where the cel sits, and it is not the layer name or the outliner stack.
        """
        if folder is None or int(length) <= 0:
            return []
        for index, (at, _span, _cel) in enumerate(self.blocks(folder)):
            if int(at) == int(frame):
                return [index + 1]
        return []

    def _column_xs(self):
        """Frame boundaries from the gutter through the right edge."""
        start, _end = self.span()
        origin = self._x_of(start)
        x = origin
        if x < GUTTER:
            x += ((GUTTER - origin + CELL - 1) // CELL) * CELL
        limit = self.width()
        while x < limit:
            yield int(x)
            x += CELL

    def _paint_grid(self, painter):
        """One grid across the strip. A cel covers the columns it holds."""
        bottom = self.height()
        rule = QColor(FRAME_LINE)
        empty = QColor(EMPTY_LINE)
        folders = self.rows()
        width = max(0, self.width() - GUTTER)
        y = HEAD
        while y <= bottom:
            painter.fillRect(GUTTER, y, width, 1, rule)
            y += ROW
        self._paint_columns(painter, 0, HEAD, empty, [])
        for index, folder in enumerate(folders):
            row_top = HEAD + index * ROW
            if row_top >= bottom:
                break
            row_bottom = min(bottom, row_top + ROW)
            covered = []
            for frame, length, _cel in self.blocks(folder):
                left = self._x_of(int(frame))
                covered.append((left, left + int(length) * CELL))
            self._paint_columns(painter, row_top, row_bottom, empty, covered)
        tail = HEAD + max(1, len(folders)) * ROW
        if tail < bottom:
            self._paint_columns(painter, tail, bottom, empty, [])

    def _paint_columns(self, painter, top, bottom, line, covered):
        height = max(0, bottom - top)
        if height <= 0:
            return
        for x in self._column_xs():
            if any(left <= x < right for left, right in covered):
                continue
            painter.fillRect(x, top, 1, height, line)

    def _paint_ruler(self, painter):
        start, end = self.span()
        info = getattr(self.art, "clip_time", None)
        label = "TIMELINE"
        if isinstance(info, dict) and info.get("name"):
            label = "%s  %d fps" % (info.get("name"), int(info.get("fps") or 0))
        painter.setPen(QColor(QUIET))
        painter.drawText(QRect(4, 0, GUTTER - 8, HEAD), Qt.AlignVCenter | Qt.AlignLeft, label)
        for frame in range(start, end + 1):
            x = self._x_of(frame)
            if x < GUTTER - CELL or x > self.width():
                continue
            if (int(frame) - start) % 5 != 0 and frame != int(self.playhead):
                continue
            painter.setPen(QColor(self.playhead_color() if frame == int(self.playhead) else QUIET))
            painter.drawText(QRect(x + 2, 0, 36, HEAD - 1), Qt.AlignLeft | Qt.AlignVCenter, str(frame))

    def _paint_playhead(self, painter):
        """One pixel, and a downward triangle in the ruler. The tone is red unless set to green."""
        center = int(self.playhead_x())
        if center < GUTTER or center > self.width():
            return
        color = QColor(self.playhead_color())
        painter.save()
        painter.setClipRect(QRect(GUTTER, 0, max(0, self.width() - GUTTER), self.height()))
        for step in range(7):
            half = 4 - (step * 4) // 6
            painter.fillRect(center - half, step, half * 2 + 1, 1, color)
        painter.fillRect(center, 0, 1, self.height(), color)
        painter.restore()

    def _paint_row(self, painter, folder, top):
        """One border around a cel. A longer cel is one fill, with its frame number on the first frame."""
        selected = self._row_lit(folder)
        painter.setPen(QColor(INK if selected else QUIET))
        name = folder.name or "Animation"
        metrics = QFontMetrics(painter.font())
        painter.drawText(
            QRect(4, top, GUTTER - 8, ROW),
            Qt.AlignLeft | Qt.AlignVCenter,
            metrics.elidedText(name, Qt.ElideRight, GUTTER - 8),
        )
        painter.save()
        painter.setClipRect(QRect(GUTTER, top, max(0, self.width() - GUTTER), ROW))
        column = QColor(GRID)
        for frame, length, cel in self.blocks(folder):
            left = self._x_of(int(frame))
            span = int(length) * CELL
            if span <= 0 or left + span < GUTTER or left > self.width():
                continue
            if span > BORDER * 2 and ROW > BORDER * 2:
                painter.fillRect(
                    left + BORDER, top + BORDER, span - BORDER * 2, ROW - BORDER * 2, column
                )
            ink = INK if cel.id == self.selected else CELL_EDGE
            self._paint_frame_numbers(painter, folder, int(frame), int(length), top, INK)
            self._stroke_cell(painter, QRect(left, top, span, ROW), ink)
        painter.restore()
        if not frame_map(folder):
            painter.setPen(QColor(RULE))
            painter.drawText(
                QRect(GUTTER + 4, top, max(0, self.width() - GUTTER - 8), ROW),
                Qt.AlignVCenter,
                "No cels specified",
            )

    def _paint_frame_numbers(self, painter, folder, frame, length, top, color):
        """Timeline order, on the first frame of the cel. The ruler keeps the global frame."""
        marks = self.frame_numbers(folder, frame, length)
        if not marks:
            return
        left = self._x_of(int(frame))
        span = int(length) * CELL
        room = span - BORDER * 2
        if room < 4:
            return
        painter.save()
        painter.setClipRect(
            QRect(left + BORDER, top + BORDER, max(0, room), max(0, ROW - BORDER * 2)),
            Qt.IntersectClip,
        )
        painter.setRenderHint(QPainter.TextAntialiasing, False)
        font = QFont(painter.font())
        font.setStyleStrategy(
            QFont.StyleStrategy.NoAntialias | QFont.StyleStrategy.NoSubpixelAntialias
        )
        font.setHintingPreference(QFont.HintingPreference.PreferNoHinting)
        painter.setFont(font)
        painter.setPen(QColor(color))
        painter.drawText(
            QRect(left + BORDER, top + BORDER, max(1, room), ROW - BORDER * 2),
            Qt.AlignLeft | Qt.AlignVCenter,
            str(marks[0]),
        )
        painter.restore()

    def _stroke_cell(self, painter, cell, color):
        """A 2px edge, lighter than the grid. White when the cel is selected."""
        visible = cell.intersected(QRect(GUTTER, cell.top(), max(0, self.width() - GUTTER), cell.height()))
        if visible.isEmpty():
            return
        ink = QColor(color)
        thick = BORDER
        if visible.top() == cell.top():
            painter.fillRect(visible.left(), cell.top(), visible.width(), thick, ink)
        if visible.bottom() == cell.bottom():
            painter.fillRect(visible.left(), cell.bottom() - (thick - 1), visible.width(), thick, ink)
        if visible.left() == cell.left():
            painter.fillRect(cell.left(), visible.top(), thick, visible.height(), ink)
        if visible.right() == cell.right():
            painter.fillRect(cell.right() - (thick - 1), visible.top(), thick, visible.height(), ink)

    def _row_lit(self, folder):
        if not self.selected:
            return False
        if folder.id == self.selected:
            return True
        cel = cel_at(folder, self.playhead)
        if cel is not None and cel.id == self.selected:
            return True
        for _frame, item in frame_map(folder):
            if item.id == self.selected:
                return True
            if item.kind == "group" and find_node(item.children, self.selected):
                return True
        return False

    def _tip(self, folder, frame):
        cel = cel_at(folder, frame)
        if cel is None:
            return "%s  frame %s  no cel" % (folder.name, frame)
        key = self._key_of(folder, cel)
        return "%s  frame %s  %s from %s" % (folder.name, frame, cel.name, key)

    def _spare_tip(self, folder):
        spares = spare_cels(folder)
        if not spares:
            return ""
        return ", ".join(cel.name for cel in spares) + " not specified"

    def _show_tip(self, event, tip):
        if tip == self._hover:
            return
        self._hover = tip
        if not tip:
            QToolTip.hideText()
            return
        point = event.globalPosition().toPoint() if hasattr(event, "globalPosition") else QPoint(0, 0)
        QToolTip.showText(point, tip, self)

    def _release_grab(self):
        if QWidget.mouseGrabber() is self:
            self.releaseMouse()

    def mousePressEvent(self, event):
        if event.button() != Qt.LeftButton:
            super().mousePressEvent(event)
            return
        point = event.position()
        x = point.x()
        y = point.y()
        # The ruler and the line scrub. The line wins over a cel, so a drag there is not a key move.
        if (y < HEAD and x >= GUTTER) or self._on_playhead(x):
            self._drag = None
            self._scrub = True
            self.grabMouse()
            self._scrub_to(self._scrub_frame(x), reveal=False)
            event.accept()
            return
        folder = self._row_at(y)
        frame = self._frame_at(x)
        if folder is None or frame is None:
            super().mousePressEvent(event)
            return
        previous = int(self.playhead)
        self.playhead = int(frame)
        cel = cel_at(folder, frame)
        if cel is not None:
            key = self._key_of(folder, cel)
            self._drag = {
                "folder": folder.id,
                "cel": cel.id,
                "origin": key if key is not None else int(frame),
                "press": int(frame),
                "frame": key if key is not None else int(frame),
                "moved": False,
            }
            self.cel_chosen.emit(cel.id)
        elif int(frame) != previous:
            self.scrubbed.emit(int(frame))
        self.update()
        event.accept()

    def mouseMoveEvent(self, event):
        if self._scrub:
            self._scrub_to(self._scrub_frame(event.position().x()), reveal=False)
            event.accept()
            return
        point = event.position()
        folder = self._row_at(point.y())
        frame = self._frame_at(point.x())
        if folder is not None and point.x() < GUTTER:
            self._show_tip(event, self._spare_tip(folder))
        elif folder is not None and frame is not None:
            self._show_tip(event, self._tip(folder, frame))
        if self._drag and frame is not None:
            origin = int(self._drag["origin"])
            press = int(self._drag["press"])
            nxt = max(0, origin + int(frame) - press)
            if nxt != self._drag.get("frame"):
                self._drag["frame"] = nxt
                self._drag["moved"] = True
                self.playhead = int(frame)
                self.update()
        event.accept()

    def mouseReleaseEvent(self, event):
        if self._scrub:
            self._scrub = False
            self._release_grab()
            self.update()
            event.accept()
            return
        drag = self._drag
        self._drag = None
        if drag and drag.get("moved") and int(drag.get("frame")) != int(drag.get("origin")):
            self.moved.emit(drag["folder"], drag["cel"], int(drag["frame"]))
        self.update()
        event.accept()

    def wheelEvent(self, event):
        delta = event.angleDelta().x() or event.angleDelta().y()
        step = CELL * 3
        if delta > 0:
            self._scroll -= step
        elif delta < 0:
            self._scroll += step
        self._clamp_scroll()
        self.update()
        event.accept()

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._clamp_scroll()
