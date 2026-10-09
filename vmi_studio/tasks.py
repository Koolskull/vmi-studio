"""Tasks for the drawing that is open.

The board is its own widget. The objects pane hosts it. Float lifts that
same widget above the studio, and dock puts it back. A later host can call
place() with another layout.

Open task text is the board's text color, white unless the gear changes it.
An executed task is struck through in dark grey. The list, the colors, and
the done marks are written into the blueprint.
"""

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor, QFont, QFontMetrics, QPainter
from PySide6.QtWidgets import (
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMenu,
    QPushButton,
    QFrame,
    QSizePolicy,
    QStyledItemDelegate,
    QStyle,
    QVBoxLayout,
    QWidget,
)

DONE_GREY = "#3A3A3A"
DEFAULT_BACKGROUND = "#000000"
DEFAULT_TEXT = "#FFFFFF"
TASK_ID = Qt.UserRole
TASK_DONE = Qt.UserRole + 1

# 7 by 7. A tooth, a ring, and a hole.
GEAR_BITS = (
    "#.#.#.#",
    "#######",
    "##.#.##",
    "#.###.#",
    "##.#.##",
    "#######",
    "#.#.#.#",
)

MENU_STYLE = (
    "QMenu { background: #000000; color: #a0a0a0; border: 1px solid #2a2a2a; }"
    "QMenu::item { padding: 4px 18px; background: #000000; }"
    "QMenu::item:selected { background: #111111; color: #ffffff; }"
    "QMenu::item:disabled { color: #2a2a2a; background: #000000; }"
)


def clean_hex(value, fallback=None):
    """#RGB or #RRGGBB. Anything else returns the fallback."""
    text = str(value or "").strip()
    if text.startswith("#"):
        text = text[1:]
    if len(text) == 3 and all(ch in "0123456789abcdefABCDEF" for ch in text):
        text = "".join(ch * 2 for ch in text)
    if len(text) == 6 and all(ch in "0123456789abcdefABCDEF" for ch in text):
        return "#" + text.upper()
    return fallback


def pack_tasks(tasks):
    """Blueprint rows. Blank text and repeated ids are dropped."""
    rows = []
    seen = set()
    for row in tasks or []:
        if not isinstance(row, dict):
            continue
        ident = str(row.get("id") or "").strip()
        text = str(row.get("text") or "").strip()
        if not ident or ident in seen or not text:
            continue
        seen.add(ident)
        rows.append({"id": ident, "text": text, "done": bool(row.get("done"))})
    return rows


def pack_colors(colors):
    colors = colors if isinstance(colors, dict) else {}
    return {
        "background": clean_hex(colors.get("background"), DEFAULT_BACKGROUND),
        "text": clean_hex(colors.get("text"), DEFAULT_TEXT),
    }


def task_color(done, text_hex):
    """Open tasks use the chosen text color. Executed tasks are dark grey."""
    return QColor(DONE_GREY if done else clean_hex(text_hex, DEFAULT_TEXT))


class GearButton(QPushButton):
    """A square button. The mark is a pixel gear, not a font glyph."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("task-gear")
        self.setFixedSize(18, 18)
        self.setCursor(Qt.PointingHandCursor)
        self.setToolTip("Task colors")
        self.setFlat(True)
        self._open = False
        self.setMouseTracking(True)

    def set_open(self, open_):
        self._open = bool(open_)
        self.update()

    def enterEvent(self, event):
        self.update()
        super().enterEvent(event)

    def leaveEvent(self, event):
        self.update()
        super().leaveEvent(event)

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, False)
        hot = self._open or self.underMouse() or self.isDown()
        ink = QColor("#ffffff" if hot else "#a0a0a0")
        edge = QColor("#ffffff" if hot else "#3a3a3a")
        painter.fillRect(self.rect(), QColor("#000000"))
        painter.setPen(edge)
        painter.drawRect(0, 0, self.width() - 1, self.height() - 1)
        scale = 2
        width = len(GEAR_BITS[0]) * scale
        height = len(GEAR_BITS) * scale
        left = (self.width() - width) // 2
        top = (self.height() - height) // 2
        for y, row in enumerate(GEAR_BITS):
            for x, bit in enumerate(row):
                if bit == "#":
                    painter.fillRect(left + x * scale, top + y * scale, scale, scale, ink)


class TaskDelegate(QStyledItemDelegate):
    def __init__(self, board):
        super().__init__(board)
        self.board = board

    def paint(self, painter, option, index):
        painter.save()
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, False)
        selected = bool(option.state & QStyle.StateFlag.State_Selected)
        painter.fillRect(option.rect, QColor("#111111" if selected else self.board.background))
        done = bool(index.data(TASK_DONE))
        color = task_color(done, self.board.text_color)
        font = QFont(option.font)
        font.setStrikeOut(done)
        painter.setFont(font)
        painter.setPen(color)
        text = index.data(Qt.DisplayRole) or ""
        metrics = QFontMetrics(font)
        rect = option.rect.adjusted(8, 0, -6, 0)
        elided = metrics.elidedText(str(text), Qt.ElideRight, max(0, rect.width()))
        painter.drawText(rect, Qt.AlignVCenter | Qt.AlignLeft, elided)
        if done and elided:
            width = metrics.horizontalAdvance(elided)
            line_y = rect.center().y()
            painter.drawLine(rect.left(), line_y, rect.left() + width, line_y)
        painter.restore()

    def setEditorData(self, editor, index):
        super().setEditorData(editor, index)
        if isinstance(editor, QLineEdit):
            editor.selectAll()

    def sizeHint(self, option, index):
        size = super().sizeHint(option, index)
        size.setHeight(22)
        return size


class _DragLabel(QLabel):
    """The title is the handle when the board is floating."""

    def __init__(self, board):
        super().__init__("TASKS", board)
        self.board = board
        self._drag = None
        self.setObjectName("task-title")

    def mousePressEvent(self, event):
        if self.board.is_floating() and event.button() == Qt.LeftButton:
            window = self.board.float_window()
            self._drag = event.globalPosition().toPoint() - window.frameGeometry().topLeft()
        else:
            self._drag = None
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event):
        if self._drag is not None and event.buttons() & Qt.LeftButton:
            self.board.float_window().move(event.globalPosition().toPoint() - self._drag)
        super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event):
        self._drag = None
        super().mouseReleaseEvent(event)


class TaskFloat(QWidget):
    """The same board, in a window that stays above the studio."""

    def __init__(self, board):
        super().__init__(
            None,
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.Tool,
        )
        self.board = board
        self.setWindowTitle("Tasks")
        self.setObjectName("task-float")
        self.resize(320, 420)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(1, 1, 1, 1)
        layout.setSpacing(0)
        self._edge = QColor("#2a2a2a")

    def paintEvent(self, _event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, False)
        painter.fillRect(self.rect(), QColor(self.board.background))
        painter.setPen(self._edge)
        painter.drawRect(0, 0, self.width() - 1, self.height() - 1)

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape and not _editing():
            self.board.dock()
            return
        super().keyPressEvent(event)


def _editing():
    from PySide6.QtWidgets import QApplication

    return isinstance(QApplication.focusWidget(), QLineEdit)


class TaskList(QListWidget):
    def __init__(self, board):
        super().__init__(board)
        self.board = board
        self.setObjectName("task-list")
        self.setFrameShape(QFrame.NoFrame)
        self.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self.setTextElideMode(Qt.ElideRight)
        self.setUniformItemSizes(True)
        self.viewport().setContextMenuPolicy(Qt.CustomContextMenu)
        self.viewport().customContextMenuRequested.connect(self._popup)

    def _popup(self, pos):
        item = self.itemAt(pos)
        self.board.menu_for(item).popup(self.viewport().mapToGlobal(pos))

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape and self.board.is_floating() and not _editing():
            self.board.dock()
            return
        super().keyPressEvent(event)


class _DockStrip(QWidget):
    """Sits in the objects pane while the board is above the studio."""

    def __init__(self, board):
        super().__init__()
        self.setObjectName("task-dock-strip")
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        note = QLabel("Tasks are above the studio.")
        note.setObjectName("title")
        layout.addWidget(note, 1)
        dock = QPushButton("Dock")
        dock.setToolTip("Put tasks back in the objects pane")
        dock.clicked.connect(board.dock)
        layout.addWidget(dock)


class TaskBoard(QWidget):
    """One drawing's tasks. place() docks it. Float lifts the same widget."""

    changed = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("task-board")
        self.setMinimumHeight(120)
        self.setMinimumWidth(0)
        self.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Expanding)
        self._tasks = []
        self._filling = False
        self.background = DEFAULT_BACKGROUND
        self.text_color = DEFAULT_TEXT
        self._home_layout = None
        self._float = None
        self._strip = None
        self._floating = False

        column = QVBoxLayout(self)
        column.setContentsMargins(0, 0, 0, 0)
        column.setSpacing(4)

        header = QWidget()
        header.setObjectName("task-header")
        row = QHBoxLayout(header)
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(4)
        self.title = _DragLabel(self)
        self.title.setMinimumWidth(0)
        row.addWidget(self.title, 1)
        self.gear = GearButton()
        self.gear.clicked.connect(self._toggle_props)
        row.addWidget(self.gear)
        self.float_button = QPushButton("float")
        self.float_button.setObjectName("task-float-button")
        self.float_button.setToolTip("Keep tasks above the studio")
        self.float_button.setFixedHeight(18)
        self.float_button.clicked.connect(self._toggle_float)
        row.addWidget(self.float_button)
        column.addWidget(header)

        self.props = QWidget()
        self.props.setObjectName("task-props")
        form = QVBoxLayout(self.props)
        form.setContentsMargins(0, 0, 0, 0)
        form.setSpacing(4)
        self.bg_edit = self._color_row(form, "BG", "background")
        self.text_edit = self._color_row(form, "TEXT", "text")
        self.props.hide()
        column.addWidget(self.props)

        self.empty = QLabel("Right-click to add a task.")
        self.empty.setStyleSheet("color: #3a3a3a; background: transparent;")
        column.addWidget(self.empty)

        self.list = TaskList(self)
        self.list.setItemDelegate(TaskDelegate(self))
        self.list.itemChanged.connect(self._renamed)
        column.addWidget(self.list, 1)
        self._apply_chrome()

    def place(self, layout):
        """Dock into this layout. Float returns here."""
        self._home_layout = layout
        layout.addWidget(self)

    def rows(self):
        return pack_tasks(self._tasks)

    def colors(self):
        return pack_colors({"background": self.background, "text": self.text_color})

    def set_state(self, rows, colors=None):
        """Replace the list. Does not record a change."""
        packed = pack_colors(colors)
        self.background = packed["background"]
        self.text_color = packed["text"]
        self._tasks = pack_tasks(rows)
        self._fill_fields()
        self._apply_chrome()
        self._refill()

    def is_floating(self):
        return bool(self._floating and self._float is not None and self._float.isVisible())

    def float_window(self):
        return self._float

    def menu_for(self, item):
        """Right-click. Delete and Execute stay off when the click missed a task."""
        menu = QMenu(self.list)
        menu.setStyleSheet(MENU_STYLE)
        new = menu.addAction("New task")
        new.triggered.connect(self.new_task)
        done = bool(item.data(TASK_DONE)) if item is not None else False
        execute = menu.addAction("Restore" if done else "Execute")
        execute.setEnabled(item is not None)
        execute.triggered.connect(lambda _checked=False, item=item: self.execute_item(item))
        delete = menu.addAction("Delete task")
        delete.setEnabled(item is not None)
        delete.triggered.connect(lambda _checked=False, item=item: self.delete_item(item))
        return menu

    def new_task(self):
        ident = self._fresh_id()
        self._tasks.append({"id": ident, "text": "task", "done": False})
        self._refill()
        self.changed.emit("New task.")
        item = self._item(ident)
        if item is not None:
            self.list.setCurrentItem(item)
            self.list.editItem(item)

    def execute_item(self, item):
        if item is None:
            return
        ident = item.data(TASK_ID)
        text = ""
        done = False
        for task in self._tasks:
            if task["id"] != ident:
                continue
            task["done"] = not task["done"]
            done = task["done"]
            text = task["text"]
            break
        else:
            return
        self._refill()
        self._reselect(ident)
        self.changed.emit(("Executed %s." if done else "Restored %s.") % text)

    def delete_item(self, item):
        if item is None:
            return
        ident = item.data(TASK_ID)
        text = ""
        kept = []
        found = False
        for task in self._tasks:
            if task["id"] == ident:
                text = task["text"]
                found = True
                continue
            kept.append(task)
        if not found:
            return
        self._tasks = kept
        self._refill()
        self.changed.emit("Deleted %s." % text)

    def dock(self):
        if self._home_layout is not None:
            self._home_layout.addWidget(self)
        self._floating = False
        self._hide_strip()
        if self._float is not None:
            self._float.hide()
        self._retitle_float()
        self.show()

    def lift(self):
        if self.is_floating():
            self._float.raise_()
            self._float.activateWindow()
            return
        host = self.window()
        if self._float is None:
            self._float = TaskFloat(self)
        self._float.layout().addWidget(self, 1)
        if host is not None and host is not self._float:
            frame = host.frameGeometry()
            if frame.width() > 2 and frame.height() > 2:
                x = frame.x() + max(0, frame.width() - self._float.width() - 24)
                y = frame.y() + 64
                self._float.move(x, y)
        self._floating = True
        self._float.show()
        self._float.raise_()
        self._show_strip()
        self._retitle_float()

    def shutdown(self):
        """Put the board back, then drop the floating window."""
        self.dock()
        if self._float is not None:
            self._float.hide()
            self._float.deleteLater()
            self._float = None

    def _toggle_float(self):
        if self.is_floating():
            self.dock()
        else:
            self.lift()

    def _toggle_props(self):
        self.props.setVisible(not self.props.isVisible())
        self.gear.set_open(self.props.isVisible())
        if self.props.isVisible():
            self.bg_edit.setFocus()
            self.bg_edit.selectAll()

    def _color_row(self, form, label, kind):
        row = QHBoxLayout()
        row.setContentsMargins(0, 0, 0, 0)
        row.setSpacing(6)
        name = QLabel(label)
        name.setFixedWidth(36)
        edit = QLineEdit()
        edit.setObjectName("task-" + kind)
        edit.setMaxLength(7)
        edit.textChanged.connect(lambda _text, kind=kind: self._color_edited(kind))
        edit.editingFinished.connect(lambda kind=kind: self._color_finished(kind))
        row.addWidget(name)
        row.addWidget(edit, 1)
        form.addLayout(row)
        return edit

    def _color_edited(self, kind):
        edit = self.bg_edit if kind == "background" else self.text_edit
        cleaned = clean_hex(edit.text(), None)
        if not cleaned:
            return
        current = self.background if kind == "background" else self.text_color
        if cleaned == current:
            return
        if kind == "background":
            self.background = cleaned
        else:
            self.text_color = cleaned
        self._apply_chrome()
        self.list.viewport().update()
        self.changed.emit("Task colors.")

    def _color_finished(self, kind):
        edit = self.bg_edit if kind == "background" else self.text_edit
        if clean_hex(edit.text(), None) is None:
            edit.setText(self.background if kind == "background" else self.text_color)

    def _fill_fields(self):
        for edit, value in ((self.bg_edit, self.background), (self.text_edit, self.text_color)):
            edit.blockSignals(True)
            edit.setText(value)
            edit.blockSignals(False)

    def _apply_chrome(self):
        bg = self.background
        ink = self.text_color
        self.setStyleSheet(
            "QWidget#task-board { background: %s; border-top: 1px solid #2a2a2a; }" % bg
        )
        self.title.setStyleSheet("color: %s; background: %s;" % (ink, bg))
        self.empty.setStyleSheet("color: #3a3a3a; background: %s;" % bg)
        self.list.setStyleSheet(
            "QListWidget { background: %s; border: none; outline: none; }"
            "QListWidget::item:selected { background: #111111; }" % bg
        )
        self.props.setStyleSheet(
            "QWidget#task-props { background: #000000; }"
            "QLabel { color: #3a3a3a; background: #000000; }"
        )
        if self._float is not None:
            self._float.update()

    def _retitle_float(self):
        if self.is_floating():
            self.float_button.setText("dock")
            self.float_button.setToolTip("Put tasks back in the objects pane")
        else:
            self.float_button.setText("float")
            self.float_button.setToolTip("Keep tasks above the studio")

    def _show_strip(self):
        if self._home_layout is None or self._strip is not None:
            return
        self._strip = _DockStrip(self)
        self._home_layout.addWidget(self._strip)

    def _hide_strip(self):
        if self._strip is None:
            return
        self._strip.hide()
        self._strip.setParent(None)
        self._strip.deleteLater()
        self._strip = None

    def _fresh_id(self):
        numbers = []
        for task in self._tasks:
            ident = task["id"]
            if ident.startswith("t") and ident[1:].isdigit():
                numbers.append(int(ident[1:]))
        return "t%d" % (max(numbers, default=0) + 1)

    def _item(self, ident):
        for index in range(self.list.count()):
            item = self.list.item(index)
            if item.data(TASK_ID) == ident:
                return item
        return None

    def _reselect(self, ident):
        item = self._item(ident)
        if item is not None:
            self.list.setCurrentItem(item)

    def _refill(self):
        current = self.list.currentItem()
        current_id = current.data(TASK_ID) if current is not None else ""
        self._filling = True
        self.list.blockSignals(True)
        self.list.clear()
        for task in self._tasks:
            item = QListWidgetItem(task["text"])
            item.setData(TASK_ID, task["id"])
            item.setData(TASK_DONE, bool(task["done"]))
            item.setFlags(
                Qt.ItemIsEnabled | Qt.ItemIsSelectable | Qt.ItemIsEditable
            )
            self.list.addItem(item)
        self.list.blockSignals(False)
        self._filling = False
        self.empty.setVisible(not self._tasks)
        self._reselect(current_id)
        self.list.viewport().update()

    def _renamed(self, item):
        if self._filling or item is None:
            return
        ident = item.data(TASK_ID)
        text = item.text().strip()
        if not text:
            self.delete_item(item)
            return
        for task in self._tasks:
            if task["id"] != ident:
                continue
            if task["text"] == text:
                return
            task["text"] = text
            self.list.viewport().update()
            self.changed.emit("Renamed %s." % text)
            return
