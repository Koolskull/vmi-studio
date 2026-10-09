"""The first window. It lists work and browses drawings. It does not open one."""

import os
import sys

from PySide6.QtCore import QDir, QEvent, QSize, Qt, QTimer, Signal
from PySide6.QtGui import QAction, QIcon, QIconEngine, QImage, QKeySequence, QPainter, QPixmap
from PySide6.QtWidgets import (
    QAbstractItemView,
    QApplication,
    QDialog,
    QFileSystemModel,
    QGridLayout,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMenu,
    QPushButton,
    QSplitter,
    QTreeView,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from vmi_studio import catalog, theme

_FOLDER_ICON = None


def folder_png_path():
    """The shipped folder picture. Published source does not point at a home path."""
    if getattr(sys, "frozen", False):
        root = getattr(sys, "_MEIPASS", os.path.dirname(os.path.abspath(sys.executable)))
        return os.path.join(root, "vmi_studio", "icons", "folder.png")
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "icons", "folder.png")


class _FolderEngine(QIconEngine):
    """The folder picture, scaled only by nearest neighbor."""

    def __init__(self, image):
        super().__init__()
        self._image = image

    def clone(self):
        return _FolderEngine(self._image)

    def pixmap(self, size, mode, state):
        return self.scaledPixmap(size, mode, state, 1.0)

    def scaledPixmap(self, size, mode, state, scale):
        ratio = float(scale or 1)
        width = max(1, int(round(size.width() * ratio)))
        height = max(1, int(round(size.height() * ratio)))
        scaled = self._image.scaled(width, height, Qt.IgnoreAspectRatio, Qt.FastTransformation)
        pix = QPixmap.fromImage(scaled)
        pix.setDevicePixelRatio(ratio)
        return pix

    def paint(self, painter, rect, mode, state):
        device = painter.device()
        ratio = float(device.devicePixelRatioF() or 1) if device is not None else 1.0
        pix = self.scaledPixmap(rect.size(), mode, state, ratio)
        painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
        painter.drawPixmap(rect, pix)

    def availableSizes(self, mode, state):
        return [QSize(self._image.width(), self._image.height())]

    def isNull(self):
        return self._image.isNull()


def folder_icon():
    """One folder picture for every directory row."""
    global _FOLDER_ICON
    if _FOLDER_ICON is not None:
        return _FOLDER_ICON
    image = QImage(folder_png_path())
    _FOLDER_ICON = QIcon() if image.isNull() else QIcon(_FolderEngine(image))
    return _FOLDER_ICON


class DiskModel(QFileSystemModel):
    """Directory rows show the folder picture. File rows show no icon."""

    def data(self, index, role=Qt.DisplayRole):
        if role == Qt.DecorationRole and index.isValid() and index.column() == 0:
            path = self.filePath(index)
            # isDir is true before the row is fetched, and that paints a folder on a file.
            if path and os.path.isdir(path):
                return folder_icon()
            return QIcon()
        return super().data(index, role)


class FileList(QTreeWidget):
    """The middle list. A drawing dropped from another program joins it."""

    def __init__(self, host):
        super().__init__()
        self._host = host
        self.setAcceptDrops(True)
        self.setDragEnabled(False)
        self.setDragDropMode(QAbstractItemView.DragDropMode.DropOnly)
        self.setDefaultDropAction(Qt.DropAction.CopyAction)
        self.viewport().setAcceptDrops(True)

    def _local_paths(self, event):
        mime = event.mimeData() if event is not None else None
        if mime is None or not mime.hasUrls():
            return []
        paths = []
        for url in mime.urls():
            if url.isLocalFile():
                paths.append(url.toLocalFile())
        return paths

    def dragEnterEvent(self, event):
        if self._local_paths(event):
            event.acceptProposedAction()
            return
        event.ignore()

    def dragMoveEvent(self, event):
        if self._local_paths(event):
            event.acceptProposedAction()
            return
        event.ignore()

    def dropEvent(self, event):
        paths = self._local_paths(event)
        if not paths:
            event.ignore()
            return
        event.acceptProposedAction()
        self._host.drop_files(paths)


def _code_snapshot():
    here = os.path.dirname(os.path.abspath(__file__))
    found = []
    for name in os.listdir(here):
        if not name.endswith((".py", ".c")):
            continue
        path = os.path.join(here, name)
        try:
            found.append((name, os.path.getmtime(path), os.path.getsize(path)))
        except OSError:
            continue
    return tuple(sorted(found))


class _LineStatus:
    """showMessage / currentMessage for a page that is not its own window."""

    def __init__(self, label):
        self._label = label
        self._text = ""
        self._forward = None

    def showMessage(self, text):
        self._text = text or ""
        self._label.setText(self._text)
        forward = self._forward
        if forward is not None:
            forward(self._text)

    def currentMessage(self):
        return self._text


class LineDialog(QDialog):
    """One field. Empty text stays on the dialog."""

    def __init__(self, parent, title, prompt, text="", ok="Save"):
        super().__init__(parent)
        self.setWindowTitle(title)
        layout = QVBoxLayout(self)
        heading = QLabel(title.upper())
        heading.setObjectName("title")
        layout.addWidget(heading)
        layout.addWidget(QLabel(prompt))
        self.edit = QLineEdit()
        self.edit.setText(text or "")
        layout.addWidget(self.edit)
        row = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        confirm = QPushButton(ok)
        confirm.setObjectName("export")
        confirm.setDefault(True)
        confirm.clicked.connect(self._accept)
        row.addWidget(cancel)
        row.addWidget(confirm)
        layout.addLayout(row)
        self.edit.setFocus()
        if text:
            self.edit.selectAll()
        self.setMinimumWidth(420)

    def _accept(self):
        if not self.edit.text().strip():
            return
        self.accept()

    def value(self):
        return self.edit.text().strip()


class ConfirmDialog(QDialog):
    def __init__(self, parent, title, prompt, ok="Delete"):
        super().__init__(parent)
        self.setWindowTitle(title)
        layout = QVBoxLayout(self)
        heading = QLabel(title.upper())
        heading.setObjectName("title")
        layout.addWidget(heading)
        layout.addWidget(QLabel(prompt))
        row = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        confirm = QPushButton(ok)
        confirm.setObjectName("export")
        confirm.clicked.connect(self.accept)
        row.addWidget(cancel)
        row.addWidget(confirm)
        layout.addLayout(row)
        self.setMinimumWidth(420)


class Launcher(QWidget):
    """Projects, tasks, recent files, and a folder browser. Open is the load."""

    open_requested = Signal(str)
    leave_app = Signal()

    def __init__(self, book_path=None, seed_path="", dev=False):
        super().__init__()
        self.book_path = book_path or catalog.PATH
        self.book = catalog.load(self.book_path, seed_path=seed_path)
        self.mode = "recent" if self.book["recent"] else "project"
        self.list_id = ""
        self.project_id = ""
        self._guard = False
        self._trail = []
        self._pending_select = ""
        self._fitted = False
        self._did_select = False
        self._split_custom = False
        self._split_guard = False
        self._open_low_res = False
        self.dev = dev
        self._watch_base = None
        self._watch_pending = None
        self._timer = None
        self.setWindowTitle("VMI STUDIO")
        self.setObjectName("launcher")
        self.resize(1280, 800)
        self._build()
        self.refresh()
        self.browse_at(catalog.default_browse(self.book), record=False)
        app = QApplication.instance()
        if app is not None:
            app.installEventFilter(self)
        if dev:
            self._watch_base = _code_snapshot()
            timer = QTimer(self)
            timer.setInterval(400)
            timer.timeout.connect(self._watch)
            timer.start()
            self._timer = timer

    def stop_watch(self):
        if self._timer is not None:
            self._timer.stop()

    def _watch(self):
        now = _code_snapshot()
        if now == self._watch_base:
            self._watch_pending = None
            return
        if now != self._watch_pending:
            self._watch_pending = now
            return
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        os.chdir(root)
        current = os.environ.get("PYTHONPATH", "")
        parts = current.split(os.pathsep) if current else []
        if root not in parts:
            os.environ["PYTHONPATH"] = root if not current else root + os.pathsep + current
        os.execv(sys.executable, [sys.executable, "-m", "vmi_studio", "--dev"])

    def _build(self):
        outer = QVBoxLayout(self)
        outer.setContentsMargins(8, 8, 8, 8)
        outer.setSpacing(6)
        self.word = QLabel("VMI STUDIO")
        outer.addWidget(self.word)
        self.apply_theme()
        intro = QLabel(
            "Pick what to work on. The drawing stays closed until you open it. "
            "Task lists belong to the selected project. "
            "Shift+Left and Shift+Right move between the lists, the files, and the browser."
        )
        intro.setWordWrap(True)
        outer.addWidget(intro)
        modes = QHBoxLayout()
        modes.setSpacing(6)
        self.mode_buttons = {}
        for key, label in (
            ("project", "Projects"),
            ("task", "Tasks"),
            ("recent", "Recent"),
        ):
            button = QPushButton(label)
            button.clicked.connect(lambda _checked=False, name=key: self.show_mode(name))
            self.mode_buttons[key] = button
            modes.addWidget(button)
        modes.addStretch(1)
        leave = QPushButton("Exit")
        leave.clicked.connect(self.leave_app.emit)
        modes.addWidget(leave)
        outer.addLayout(modes)

        self.split = QSplitter(Qt.Horizontal)
        self.split.setHandleWidth(7)
        self.split.setChildrenCollapsible(False)
        self._panes = [self._lists_pane(), self._files_pane(), self._browser_pane()]
        for pane in self._panes:
            self.split.addWidget(pane)
        for index, factor in enumerate((2, 5, 4)):
            self.split.setStretchFactor(index, factor)
        self.split.splitterMoved.connect(self._split_dragged)
        outer.addWidget(self.split, 1)
        self._status_label = QLabel("")
        self.status = _LineStatus(self._status_label)
        outer.addWidget(self._status_label)
        self.status.showMessage("Select a drawing. Nothing loads until you open it.")

    def apply_theme(self):
        """The title follows the chosen theme. The rest of the page uses the app sheet."""
        word = getattr(self, "word", None)
        if word is None:
            return
        word.setStyleSheet(
            "color: %s; font-size: 14px; background: transparent;" % theme.current().ink
        )

    def _lists_pane(self):
        body = QWidget()
        layout = QVBoxLayout(body)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        self.recent_button = QPushButton("Recently used")
        self.recent_button.clicked.connect(lambda: self.show_mode("recent"))
        layout.addWidget(self.recent_button)
        projects = QLabel("PROJECTS")
        projects.setObjectName("title")
        layout.addWidget(projects)
        self.projects = QListWidget()
        self.projects.setObjectName("projects")
        self.projects.currentItemChanged.connect(lambda _cur, _prev: self._list_chosen("project"))
        layout.addWidget(self.projects, 1)
        self.tasks_title = QLabel("TASKS")
        self.tasks_title.setObjectName("title")
        layout.addWidget(self.tasks_title)
        self.tasks = QListWidget()
        self.tasks.setObjectName("tasks")
        self.tasks.currentItemChanged.connect(lambda _cur, _prev: self._list_chosen("task"))
        layout.addWidget(self.tasks, 1)
        row = QHBoxLayout()
        new_project = QPushButton("New project")
        new_project.clicked.connect(lambda: self.ask_new_list("project"))
        new_task = QPushButton("New task list")
        new_task.clicked.connect(lambda: self.ask_new_list("task"))
        row.addWidget(new_project)
        row.addWidget(new_task)
        layout.addLayout(row)
        row = QHBoxLayout()
        rename = QPushButton("Rename")
        rename.clicked.connect(self.ask_rename_list)
        delete = QPushButton("Delete list")
        delete.clicked.connect(self.ask_delete_list)
        row.addWidget(rename)
        row.addWidget(delete)
        layout.addLayout(row)
        return body

    def _files_pane(self):
        body = QWidget()
        layout = QVBoxLayout(body)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        self.files_title = QLabel("FILES")
        self.files_title.setObjectName("title")
        layout.addWidget(self.files_title)
        self.file_query = QLineEdit()
        self.file_query.setPlaceholderText("Filter this list")
        self.file_query.textChanged.connect(lambda _text: self._fill_files())
        layout.addWidget(self.file_query)
        self.file_table = FileList(self)
        self.file_table.setObjectName("files")
        self.file_table.setHeaderLabels(["Label", "File", "User", "State", "Opened", "Folder"])
        self.file_table.setRootIsDecorated(False)
        self.file_table.setAlternatingRowColors(False)
        self.file_table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.file_table.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.file_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        header = self.file_table.header()
        header.setStretchLastSection(True)
        header.setMinimumSectionSize(72)
        header.setSectionResizeMode(0, QHeaderView.Interactive)
        header.setSectionResizeMode(1, QHeaderView.Interactive)
        header.setSectionResizeMode(5, QHeaderView.Stretch)
        self.file_table.setColumnWidth(0, 220)
        self.file_table.setColumnWidth(1, 240)
        self.file_table.setColumnWidth(2, 110)
        self.file_table.setColumnWidth(3, 90)
        self.file_table.setColumnWidth(4, 130)
        self.file_table.itemSelectionChanged.connect(self._file_chosen)
        self.file_table.itemDoubleClicked.connect(lambda _item, _col: self.open_selected())
        self.file_table.setContextMenuPolicy(Qt.CustomContextMenu)
        self.file_table.customContextMenuRequested.connect(self._file_menu)
        for key in (Qt.Key_Return, Qt.Key_Enter):
            action = QAction(self.file_table)
            action.setShortcut(QKeySequence(key))
            action.setShortcutContext(Qt.WidgetShortcut)
            action.triggered.connect(self.open_selected)
            self.file_table.addAction(action)
        remove_key = QAction(self.file_table)
        remove_key.setShortcut(QKeySequence.Delete)
        remove_key.setShortcutContext(Qt.WidgetShortcut)
        remove_key.triggered.connect(self.remove_selected)
        self.file_table.addAction(remove_key)
        layout.addWidget(self.file_table, 1)
        row = QHBoxLayout()
        open_button = QPushButton("Open")
        open_button.setObjectName("export")
        open_button.clicked.connect(self.open_selected)
        finished = QPushButton("Finished")
        finished.clicked.connect(lambda: self.mark_selected(True))
        working = QPushButton("Working")
        working.clicked.connect(lambda: self.mark_selected(False))
        label = QPushButton("Label")
        label.clicked.connect(self.ask_label)
        tag = QPushButton("Tag")
        tag.clicked.connect(self.ask_tag)
        remove = QPushButton("Remove")
        remove.clicked.connect(self.remove_selected)
        for button in (open_button, finished, working, label, tag, remove):
            row.addWidget(button)
        layout.addLayout(row)
        return body

    def _browser_pane(self):
        body = QWidget()
        layout = QVBoxLayout(body)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        heading = QLabel("BROWSER")
        heading.setObjectName("title")
        layout.addWidget(heading)
        grid = QGridLayout()
        grid.setSpacing(6)
        self.place_buttons = {}
        for index, (label, path) in enumerate(catalog.places()):
            button = QPushButton(label)
            button.clicked.connect(lambda _checked=False, folder=path: self.browse_at(folder))
            self.place_buttons[label] = button
            grid.addWidget(button, index // 3, index % 3)
        layout.addLayout(grid)
        path_row = QHBoxLayout()
        back = QPushButton("Back")
        back.clicked.connect(self.browse_back)
        up = QPushButton("Up")
        up.clicked.connect(self.browse_up)
        self.path_edit = QLineEdit()
        self.path_edit.setPlaceholderText("Folder or drawing path")
        self.path_edit.returnPressed.connect(self._path_entered)
        path_row.addWidget(back)
        path_row.addWidget(up)
        path_row.addWidget(self.path_edit, 1)
        layout.addLayout(path_row)
        self.browser_query = QLineEdit()
        self.browser_query.setPlaceholderText("Filter this folder")
        self.browser_query.textChanged.connect(self._apply_browser_filter)
        layout.addWidget(self.browser_query)
        self.disk = DiskModel(self)
        self.disk.setFilter(QDir.AllDirs | QDir.Files | QDir.NoDotAndDotDot)
        self.disk.setNameFilterDisables(False)
        self.disk.setNameFilters(catalog.drawing_globs(""))
        self.disk.setResolveSymlinks(True)
        self.disk.directoryLoaded.connect(self._directory_loaded)
        self.browser = QTreeView()
        self.browser.setObjectName("browser")
        self.browser.setIconSize(QSize(16, 16))
        self.browser.setModel(self.disk)
        self.browser.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.browser.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.browser.setSortingEnabled(True)
        self.browser.sortByColumn(0, Qt.AscendingOrder)
        self.browser.setColumnHidden(2, True)
        self.browser.header().setStretchLastSection(False)
        self.browser.header().setSectionResizeMode(0, QHeaderView.Stretch)
        self.browser.doubleClicked.connect(self._browser_activated)
        self.browser.selectionModel().currentChanged.connect(self._browser_current)
        self.browser.setContextMenuPolicy(Qt.CustomContextMenu)
        self.browser.customContextMenuRequested.connect(self._browser_menu)
        layout.addWidget(self.browser, 1)
        row = QHBoxLayout()
        add = QPushButton("Add to list")
        add.clicked.connect(self.add_browser_selection)
        open_button = QPushButton("Open")
        open_button.clicked.connect(self.open_browser_selection)
        row.addWidget(add)
        row.addWidget(open_button)
        layout.addLayout(row)
        return body

    def eventFilter(self, watched, event):
        if (
            event.type() == QEvent.KeyPress
            and self.isVisible()
            and self._inside(watched)
            and self._pane_key(event)
        ):
            return True
        return super().eventFilter(watched, event)

    def _inside(self, watched):
        widget = watched if isinstance(watched, QWidget) else None
        while widget is not None:
            if widget is self:
                return True
            widget = widget.parentWidget()
        return False

    def _pane_key(self, event):
        if event.isAutoRepeat():
            return False
        mods = event.modifiers()
        if mods & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier):
            return False
        if not (mods & Qt.ShiftModifier):
            return False
        if event.key() == Qt.Key_Right:
            self._focus_pane(1)
            return True
        if event.key() == Qt.Key_Left:
            self._focus_pane(-1)
            return True
        return False

    def _pane_index(self, widget):
        while widget is not None and widget is not self:
            for index, pane in enumerate(self._panes):
                if widget is pane:
                    return index
            widget = widget.parentWidget()
        return None

    def _focus_pane(self, step):
        current = self._pane_index(QApplication.focusWidget())
        if current is None:
            nxt = 0 if step > 0 else len(self._panes) - 1
        else:
            nxt = (current + step) % len(self._panes)
        self._focus_pane_at(nxt)

    def _lists_focus(self):
        if self.mode == "task":
            return self.tasks
        if self.mode == "recent":
            return self.recent_button
        return self.projects

    def _focus_pane_at(self, index):
        if index == 0:
            widget = self._lists_focus()
        elif index == 1:
            widget = self.file_table
        else:
            widget = self.browser
        widget.setFocus()
        self.status.showMessage(("Lists", "Files", "Browser")[index])

    def show_mode(self, mode):
        if mode == "task":
            self._ensure_project()
            if not self.project_id:
                self.status.showMessage("Pick a project.")
                return
            self.mode = "task"
            if self.tasks.currentItem() is None and self.tasks.count():
                self.tasks.setCurrentRow(0)
            current = self.tasks.currentItem()
            self.list_id = current.data(Qt.UserRole) if current is not None else ""
            self._mark_modes()
            self._fill_files()
            self.tasks.setFocus()
            return
        if mode == "project":
            self._ensure_project()
            self.mode = "project"
            self.list_id = self.project_id
            self._fill_lists()
            self._fill_files()
            self.projects.setFocus()
            return
        self.mode = "recent"
        self.list_id = ""
        self._mark_modes()
        self._fill_files()
        self.file_table.setFocus()

    def _mark_modes(self):
        for key, button in self.mode_buttons.items():
            self._paint_current(button, key == self.mode)
        self._paint_current(self.recent_button, self.mode == "recent")
        for label, button in self.place_buttons.items():
            current = os.path.abspath(self.book.get("browse") or "")
            folder = dict(catalog.places()).get(label, "")
            self._paint_current(button, bool(folder) and os.path.abspath(folder) == current)

    def _paint_current(self, button, on):
        button.setProperty("current", "true" if on else "false")
        button.style().unpolish(button)
        button.style().polish(button)

    def _list_chosen(self, style):
        if self._guard:
            return
        if style == "project":
            current = self.projects.currentItem()
            if current is None:
                return
            self.project_id = current.data(Qt.UserRole) or ""
            self.mode = "project"
            self.list_id = self.project_id
            self._guard = True
            self.tasks.setCurrentItem(None)
            self._guard = False
            self._fill_lists()
            self._fill_files()
            return
        current = self.tasks.currentItem()
        if current is None:
            return
        self.mode = "task"
        self.list_id = current.data(Qt.UserRole) or ""
        project = catalog.parent_project(self.book, self.list_id)
        if project is not None:
            self.project_id = project["id"]
        self._mark_modes()
        self._fill_files()

    def refresh(self):
        self._fill_lists()
        self._fill_files()

    def _ensure_project(self):
        projects = catalog.lists_of(self.book, "project")
        ids = [item["id"] for item in projects]
        if self.project_id in ids:
            return
        self.project_id = ids[0] if ids else ""

    def _list_item(self, lst):
        done, total = catalog.progress(lst)
        text = lst["title"]
        if total:
            text = "%s    %d/%d finished" % (lst["title"], done, total)
        item = QListWidgetItem(text)
        item.setData(Qt.UserRole, lst["id"])
        return item

    def _fill_lists(self):
        self._ensure_project()
        self._guard = True
        self.projects.clear()
        self.tasks.clear()
        for lst in catalog.lists_of(self.book, "project"):
            item = self._list_item(lst)
            self.projects.addItem(item)
            if lst["id"] == self.project_id:
                self.projects.setCurrentItem(item)
        for lst in catalog.lists_of(self.book, "task", self.project_id):
            item = self._list_item(lst)
            self.tasks.addItem(item)
            if self.mode == "task" and lst["id"] == self.list_id:
                self.tasks.setCurrentItem(item)
        project = catalog.find_list(self.book, self.project_id)
        if project is None:
            self.tasks_title.setText("TASKS")
        else:
            self.tasks_title.setText("TASKS  %s" % project["title"])
        self._guard = False
        self._mark_modes()

    def _rows_for_mode(self):
        rows = []
        if self.mode == "recent":
            for item in self.book.get("recent") or []:
                rows.append(self._row(item["path"], None, item.get("opened") or "last session"))
            return rows
        lst = catalog.find_list(self.book, self.list_id)
        if lst is None:
            return rows
        for item in lst["items"]:
            opened = ""
            recent = catalog.recent_row(self.book, item["path"])
            if recent is not None:
                opened = recent.get("opened") or "last session"
            rows.append(self._row(item["path"], item, opened, self.list_id))
        return rows

    def _row(self, path, item, opened, list_id=""):
        label, tag = catalog.label_tag(self.book, path, item)
        done = catalog.done_for(self.book, path, item)
        exists = os.path.isfile(path)
        folder = os.path.dirname(path)
        short = os.path.basename(folder) or folder
        return {
            "path": path,
            "list_id": list_id,
            "label": label,
            "file": os.path.basename(path),
            "tag": tag,
            "state": catalog.state_text(done, exists),
            "opened": opened,
            "folder": short,
            "hay": " ".join((label, os.path.basename(path), tag, folder)),
        }

    def _fill_files(self):
        keep = ""
        current = self.file_table.currentItem()
        if current is not None:
            keep = current.data(0, Qt.UserRole) or ""
        self.file_table.clear()
        query = self.file_query.text().strip().lower()
        chosen = None
        for row in self._rows_for_mode():
            if query and query not in row["hay"].lower():
                continue
            item = QTreeWidgetItem([
                row["label"], row["file"], row["tag"], row["state"], row["opened"], row["folder"],
            ])
            item.setData(0, Qt.UserRole, row["path"])
            item.setData(0, Qt.UserRole + 1, row["list_id"])
            item.setToolTip(0, row["path"])
            item.setToolTip(5, row["path"])
            self.file_table.addTopLevelItem(item)
            if row["path"] == keep:
                chosen = item
        self.files_title.setText(self._files_heading())
        if chosen is not None:
            self.file_table.setCurrentItem(chosen)
        elif not self._did_select and self.mode == "recent" and self.file_table.topLevelItemCount():
            self._did_select = True
            self.file_table.setCurrentItem(self.file_table.topLevelItem(0))
        elif self.file_table.currentItem() is None:
            if self.file_table.topLevelItemCount() == 0:
                self.status.showMessage(self._empty_message())
            else:
                self.status.showMessage("Select a drawing. Nothing loads until you open it.")

    def _files_heading(self):
        if self.mode == "recent":
            return "RECENTLY USED"
        lst = catalog.find_list(self.book, self.list_id)
        if lst is None:
            return "FILES"
        done, total = catalog.progress(lst)
        kind = "PROJECT" if lst["style"] == "project" else "TASK"
        if total:
            return "%s  %s  %d/%d finished" % (kind, lst["title"], done, total)
        return "%s  %s" % (kind, lst["title"])

    def _empty_message(self):
        if self.mode == "recent":
            return "No drawing has been opened yet."
        if self.list_id:
            return "This list is empty. Add drawings from the browser."
        if self.mode == "task" and not self.project_id:
            return "Pick a project."
        if self.mode == "task":
            return "New task list, then add the drawings that job needs."
        return "New project, then add drawings from the browser."

    def _file_chosen(self):
        if self._guard:
            return
        entries = self._selected_entries()
        if len(entries) == 1:
            self.status.showMessage(catalog.selection_note(entries[0]["path"]))
        elif len(entries) > 1:
            self.status.showMessage("%d files selected. Open loads the current one." % len(entries))

    def _select_paths(self, paths):
        wanted = {os.path.abspath(path) for path in paths}
        first = None
        self.file_table.clearSelection()
        for index in range(self.file_table.topLevelItemCount()):
            item = self.file_table.topLevelItem(index)
            if (item.data(0, Qt.UserRole) or "") in wanted:
                item.setSelected(True)
                if first is None:
                    first = item
        if first is not None:
            self.file_table.setCurrentItem(first)
            self.file_table.scrollToItem(first)

    def _selected_entries(self):
        rows = []
        seen = set()
        items = self.file_table.selectedItems() or []
        if not items and self.file_table.currentItem() is not None:
            items = [self.file_table.currentItem()]
        for item in items:
            path = item.data(0, Qt.UserRole) or ""
            if not path or path in seen:
                continue
            seen.add(path)
            rows.append({"path": path, "list_id": item.data(0, Qt.UserRole + 1) or ""})
        return rows

    def _current_list_id(self):
        if self.mode == "project":
            item = self.projects.currentItem()
        elif self.mode == "task":
            item = self.tasks.currentItem()
        else:
            return self.list_id if self.mode != "recent" else ""
        if item is None:
            return ""
        return item.data(Qt.UserRole) or ""

    def note_opened(self, path):
        catalog.touch_recent(self.book, path)
        catalog.save(self.book, self.book_path)
        self._reveal_project(path)
        self.refresh()

    def _reveal_project(self, path):
        projects = catalog.projects_for(self.book, path)
        if not projects:
            return
        if any(item["id"] == self.project_id for item in projects):
            return
        self.project_id = projects[0]["id"]
        if self.mode == "recent":
            return
        task = catalog.find_list(self.book, self.list_id)
        if task is not None and task.get("project") == self.project_id:
            return
        self.mode = "project"
        self.list_id = self.project_id

    def open_selected(self, low_res=False):
        entries = self._selected_entries()
        if not entries:
            self.status.showMessage("Select a drawing.")
            return
        self.open_path(entries[0]["path"], low_res=low_res)

    def open_path(self, path, low_res=False):
        path = os.path.abspath(path) if path else ""
        if not path or not os.path.isfile(path):
            self.status.showMessage("That file is missing.")
            return
        if not catalog.is_drawing(path):
            self.status.showMessage("Open a .clip, .psd, .psb, .kra, .xcf, or .vmib file.")
            return
        self._open_low_res = bool(low_res)
        self.open_requested.emit(path)

    def _target_list_id(self):
        """The project or task list on screen. Recent has nowhere to drop a file."""
        if self.mode == "recent":
            return ""
        list_id = self._current_list_id()
        if self.mode in ("project", "task") and not list_id:
            list_id = self.list_id
        if not list_id or catalog.find_list(self.book, list_id) is None:
            return ""
        return list_id

    def drop_files(self, paths):
        """Add drawings by the path they already have. Nothing is copied."""
        list_id = self._target_list_id()
        if not list_id:
            self.status.showMessage("Pick a project or a task list, then drop the drawing there.")
            return {"added": [], "duplicate": [], "rejected": list(paths or [])}
        drawings = []
        for raw in paths or []:
            if not isinstance(raw, str) or not raw:
                continue
            path = os.path.abspath(raw)
            if os.path.isdir(path):
                try:
                    names = sorted(os.listdir(path))
                except OSError:
                    continue
                for name in names:
                    full = os.path.join(path, name)
                    if os.path.isfile(full):
                        drawings.append(full)
                continue
            drawings.append(path)
        return self.add_paths(list_id, drawings)

    def mark_selected(self, done):
        entries = self._selected_entries()
        if not entries:
            self.status.showMessage("Select a file first.")
            return
        for entry in entries:
            if entry["list_id"]:
                catalog.set_fields(self.book, entry["list_id"], entry["path"], done=done)
            else:
                catalog.set_done_for_path(self.book, entry["path"], done)
        self._store()
        self.refresh()
        word = "finished" if done else "working"
        self.status.showMessage("Marked %d %s." % (len(entries), word))

    def relabel_selected(self, label):
        entries = self._selected_entries()
        if len(entries) != 1:
            self.status.showMessage("Select one file to label.")
            return False
        entry = entries[0]
        list_id = entry["list_id"]
        if not list_id:
            hits = catalog.list_hits(self.book, entry["path"])
            if not hits:
                self.status.showMessage("Add this file to a project or a task to label it.")
                return False
            list_id = hits[0][0]["id"]
        if not catalog.set_fields(self.book, list_id, entry["path"], label=label):
            self.status.showMessage("Type a label.")
            return False
        self._store()
        self.refresh()
        self.status.showMessage("Labeled %s." % label)
        return True

    def retag_selected(self, tag):
        entries = self._selected_entries()
        if not entries:
            self.status.showMessage("Select a file first.")
            return False
        for entry in entries:
            list_id = entry["list_id"]
            if not list_id:
                hits = catalog.list_hits(self.book, entry["path"])
                if not hits:
                    continue
                for lst, _item in hits:
                    catalog.set_fields(self.book, lst["id"], entry["path"], tag=tag)
            else:
                catalog.set_fields(self.book, list_id, entry["path"], tag=tag)
        self._store()
        self.refresh()
        self.status.showMessage("Tagged %s." % (" ".join(str(tag or "").split()) or catalog.user_name()))
        return True

    def remove_selected(self):
        entries = self._selected_entries()
        if not entries:
            self.status.showMessage("Select a file first.")
            return
        removed = 0
        for entry in entries:
            if entry["list_id"]:
                if catalog.drop_item(self.book, entry["list_id"], entry["path"]):
                    removed += 1
                continue
            if self.mode == "recent":
                before = len(self.book.get("recent") or [])
                path = os.path.abspath(entry["path"])
                self.book["recent"] = [
                    item for item in self.book.get("recent") or [] if item["path"] != path
                ]
                if len(self.book["recent"]) != before:
                    removed += 1
        if not removed:
            self.status.showMessage("That row stays. Remove it from its project or task list.")
            return
        self._store()
        self.refresh()
        self.status.showMessage("Removed from the list. The drawing file is still there.")

    def create_list(self, style, title):
        project_id = self.project_id if style == "task" else ""
        if style == "task" and not project_id:
            self._ensure_project()
            project_id = self.project_id
        if style == "task" and not project_id:
            self.status.showMessage("Pick a project.")
            return None
        row, error = catalog.make_list(self.book, title, style, project_id)
        if error:
            self.status.showMessage(error)
            return None
        if style == "project":
            self.project_id = row["id"]
        else:
            self.project_id = row.get("project") or project_id
        self.mode = style
        self.list_id = row["id"]
        self._store()
        self.refresh()
        self.status.showMessage("Made %s." % row["title"])
        return row

    def rename_current_list(self, title):
        list_id = self._current_list_id() or self.list_id
        error = catalog.rename_list(self.book, list_id, title)
        if error:
            self.status.showMessage(error)
            return False
        self._store()
        self.refresh()
        return True

    def delete_list(self, list_id):
        lst = catalog.find_list(self.book, list_id)
        project = catalog.parent_project(self.book, list_id)
        if not catalog.drop_list(self.book, list_id):
            self.status.showMessage("That list is gone.")
            return False
        if lst is not None and lst.get("style") == "project" and self.project_id == list_id:
            self.project_id = ""
            if self.list_id == list_id:
                self.list_id = ""
                self.mode = "project"
        elif lst is not None and lst.get("style") == "task" and self.list_id == list_id and project is not None:
            self.mode = "project"
            self.project_id = project["id"]
            self.list_id = project["id"]
        self._store()
        self.refresh()
        self.status.showMessage("Deleted the list. The drawing files are still there.")
        return True

    def add_paths(self, list_id, paths, tag=None):
        result = catalog.add_paths(self.book, list_id, paths, tag=tag)
        if result["added"]:
            self._store()
            lst = catalog.find_list(self.book, list_id)
            if lst is not None and lst.get("style") == "task":
                self.project_id = lst.get("project") or self.project_id
                self.mode = "task"
            elif lst is not None:
                self.project_id = lst["id"]
                self.mode = lst.get("style") or "project"
            self.list_id = list_id
            self.refresh()
            self._select_paths(result["added"])
        if result["added"] and not result["duplicate"] and not result["rejected"]:
            self.status.showMessage("Added %d to the list, tagged %s." % (
                len(result["added"]),
                catalog.user_name() if tag is None else tag,
            ))
        elif result["added"]:
            self.status.showMessage("Added %d. %d already there. %d skipped." % (
                len(result["added"]), len(result["duplicate"]), len(result["rejected"]),
            ))
        elif result["duplicate"]:
            self.status.showMessage("Already in this list.")
        else:
            self.status.showMessage("Choose a drawing file. The browser lists .clip, .psd, .psb, .kra, .xcf, and .vmib.")
        return result

    def ask_new_list(self, style):
        if style == "task":
            self._ensure_project()
            if not self.project_id:
                self.status.showMessage("Pick a project.")
                return
            title, prompt = "New task list", "Title this task list."
        else:
            title, prompt = "New project", "Title this project list."
        dialog = LineDialog(self, title, prompt, ok="Create")
        if dialog.exec() != QDialog.Accepted:
            return
        self.create_list(style, dialog.value())

    def ask_rename_list(self):
        list_id = self._current_list_id() or (self.list_id if self.mode in ("project", "task") else "")
        lst = catalog.find_list(self.book, list_id)
        if lst is None:
            self.status.showMessage("Pick a project or a task list.")
            return
        dialog = LineDialog(self, "Rename", "Title this list.", lst["title"], ok="Rename")
        if dialog.exec() != QDialog.Accepted:
            return
        self.rename_current_list(dialog.value())

    def ask_delete_list(self):
        list_id = self._current_list_id() or (self.list_id if self.mode in ("project", "task") else "")
        lst = catalog.find_list(self.book, list_id)
        if lst is None:
            self.status.showMessage("Pick a project or a task list.")
            return
        dialog = ConfirmDialog(
            self,
            "Delete list",
            "Delete %s? The drawings stay on disk." % lst["title"],
        )
        if dialog.exec() != QDialog.Accepted:
            return
        self.delete_list(list_id)

    def ask_label(self):
        entries = self._selected_entries()
        if len(entries) != 1:
            self.status.showMessage("Select one file to label.")
            return
        label, _tag = catalog.label_tag(self.book, entries[0]["path"])
        dialog = LineDialog(self, "Label", "Label this file. The file name stays.", label, ok="Label")
        if dialog.exec() != QDialog.Accepted:
            return
        self.relabel_selected(dialog.value())

    def ask_tag(self):
        entries = self._selected_entries()
        if not entries:
            self.status.showMessage("Select a file first.")
            return
        _label, tag = catalog.label_tag(self.book, entries[0]["path"])
        dialog = LineDialog(
            self, "Tag", "Tag these files with a user.", tag or catalog.user_name(), ok="Tag",
        )
        if dialog.exec() != QDialog.Accepted:
            return
        self.retag_selected(dialog.value())

    def _file_menu(self, pos):
        item = self.file_table.itemAt(pos)
        if item is not None:
            self.file_table.setCurrentItem(item)
        menu = QMenu(self)
        menu.addAction("Open")
        menu.addAction("Open low res")
        menu.addAction("Finished")
        menu.addAction("Working")
        menu.addAction("Label")
        menu.addAction("Tag")
        menu.addAction("Remove")
        chosen = menu.exec(self.file_table.viewport().mapToGlobal(pos))
        if chosen is None:
            return
        text = chosen.text()
        if text == "Open":
            self.open_selected()
        elif text == "Open low res":
            self.open_selected(low_res=True)
        elif text == "Finished":
            self.mark_selected(True)
        elif text == "Working":
            self.mark_selected(False)
        elif text == "Label":
            self.ask_label()
        elif text == "Tag":
            self.ask_tag()
        elif text == "Remove":
            self.remove_selected()

    def browse_at(self, path, record=True):
        if not path:
            return
        path = os.path.abspath(path)
        if os.path.isfile(path):
            self._pending_select = path
            path = os.path.dirname(path)
        if not os.path.isdir(path):
            self.status.showMessage("That folder is not there.")
            return
        current = os.path.abspath(self.book.get("browse") or "") if self.book.get("browse") else ""
        if record and current and current != path:
            self._trail.append(current)
            self._trail = self._trail[-20:]
        self.book["browse"] = path
        catalog.save(self.book, self.book_path)
        index = self.disk.setRootPath(path)
        self.browser.setRootIndex(index)
        self.path_edit.setText(path)
        self._mark_modes()

    def browse_up(self):
        current = self.book.get("browse") or ""
        if not current:
            return
        parent = os.path.dirname(os.path.abspath(current))
        if parent == os.path.abspath(current):
            return
        self.browse_at(parent)

    def browse_back(self):
        if not self._trail:
            self.status.showMessage("No earlier folder.")
            return
        folder = self._trail.pop()
        self.browse_at(folder, record=False)

    def _path_entered(self):
        text = self.path_edit.text().strip()
        if not text:
            return
        if os.path.isfile(text):
            self.browse_at(text)
            return
        if os.path.isdir(text):
            self.browse_at(text)
            return
        self.status.showMessage("That path is not a folder or a drawing.")

    def _apply_browser_filter(self, text):
        self.disk.setNameFilters(catalog.drawing_globs(text))

    def _directory_loaded(self, path):
        pending = self._pending_select
        if not pending:
            return
        if os.path.abspath(os.path.dirname(pending)) != os.path.abspath(path):
            return
        self._select_browser_file(pending)
        self._pending_select = ""

    def _select_browser_file(self, path):
        index = self.disk.index(os.path.abspath(path))
        if not index.isValid():
            return
        self.browser.setCurrentIndex(index)
        self.browser.scrollTo(index)

    def _browser_activated(self, index):
        path = self.disk.filePath(index)
        if self.disk.isDir(index):
            self.browse_at(path)
            return
        self.open_path(path)

    def _browser_current(self, current, _previous):
        if not current.isValid():
            return
        path = self.disk.filePath(current)
        if catalog.is_drawing(path):
            self.status.showMessage(catalog.selection_note(path))

    def selected_browser_paths(self):
        paths = []
        seen = set()
        for index in self.browser.selectionModel().selectedRows(0):
            path = self.disk.filePath(index)
            found = []
            if self.disk.isDir(index):
                try:
                    names = sorted(os.listdir(path))
                except OSError:
                    names = []
                for name in names:
                    full = os.path.join(path, name)
                    if os.path.isfile(full) and catalog.is_drawing(full):
                        found.append(os.path.abspath(full))
            elif catalog.is_drawing(path) and os.path.isfile(path):
                found.append(os.path.abspath(path))
            for full in found:
                if full in seen:
                    continue
                seen.add(full)
                paths.append(full)
        return paths

    def add_browser_selection(self):
        list_id = self._current_list_id()
        if self.mode in ("project", "task") and not list_id:
            list_id = self.list_id
        if not list_id or catalog.find_list(self.book, list_id) is None:
            self.status.showMessage("Pick a project or a task list, then add.")
            return {"added": [], "duplicate": [], "rejected": []}
        return self.add_paths(list_id, self.selected_browser_paths())

    def open_browser_selection(self):
        paths = self.selected_browser_paths()
        if not paths:
            self.status.showMessage("Select a drawing in the browser.")
            return
        self.open_path(paths[0])

    def _browser_menu(self, pos):
        index = self.browser.indexAt(pos)
        if index.isValid():
            self.browser.setCurrentIndex(index)
        menu = QMenu(self)
        menu.addAction("Open")
        menu.addAction("Open low res")
        menu.addAction("Add to list")
        chosen = menu.exec(self.browser.viewport().mapToGlobal(pos))
        if chosen is None:
            return
        if chosen.text() == "Open":
            self.open_browser_selection()
        elif chosen.text() == "Open low res":
            paths = self.selected_browser_paths()
            if not paths:
                self.status.showMessage("Select a drawing in the browser.")
                return
            self.open_path(paths[0], low_res=True)
        elif chosen.text() == "Add to list":
            self.add_browser_selection()

    def _store(self):
        catalog.save(self.book, self.book_path)

    def _split_dragged(self, _pos, _index):
        if self._split_guard:
            return
        self._split_custom = True

    def _clear_split_guard(self):
        self._split_guard = False

    def _apply_split(self):
        if self._split_custom:
            return
        width = self.split.width()
        if width < 480:
            return
        inner = width - 14
        self._split_guard = True
        self.split.setSizes([int(inner * 0.20), int(inner * 0.44), int(inner * 0.36)])
        QTimer.singleShot(0, self._clear_split_guard)

    def _fit_wide(self):
        if self.window() is not self or os.environ.get("QT_QPA_PLATFORM") == "offscreen":
            self._apply_split()
            return
        screen = QApplication.primaryScreen()
        if screen is not None:
            ratio = screen.devicePixelRatio() or 1
            wide = int(screen.geometry().width() * ratio) >= 3200
            if wide:
                if self.screen() is not screen:
                    self.setScreen(screen)
                if not self.isMaximized():
                    self.showMaximized()
        self._apply_split()

    def showEvent(self, event):
        super().showEvent(event)
        self.refresh()
        if self._fitted:
            return
        self._fitted = True
        self._fit_wide()
        QTimer.singleShot(400, self._fit_wide)
        QTimer.singleShot(1200, self._fit_wide)

    def closeEvent(self, event):
        event.accept()
        if self.parent() is None:
            self.leave_app.emit()


class ImagePick(QDialog):
    """In-app browser for a new layer. Same field as the project browser."""

    chosen = Signal(str)
    dismissed = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Insert image")
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.Tool
        )
        self.setWindowModality(Qt.WindowModality.ApplicationModal)
        self.setStyleSheet(theme.dialog_sheet())
        self._folder = ""
        self.resize(760, 520)
        outer = QVBoxLayout(self)
        outer.setContentsMargins(12, 12, 12, 12)
        outer.setSpacing(6)
        heading = QLabel("INSERT")
        heading.setObjectName("title")
        outer.addWidget(heading)
        note = QLabel("Pick a picture. It becomes a layer in front of the current row.")
        note.setWordWrap(True)
        outer.addWidget(note)
        grid = QGridLayout()
        grid.setSpacing(6)
        self.place_buttons = {}
        for index, (label, path) in enumerate(catalog.places()):
            button = QPushButton(label)
            button.clicked.connect(lambda _checked=False, folder=path: self.browse_at(folder))
            self.place_buttons[label] = button
            grid.addWidget(button, index // 3, index % 3)
        outer.addLayout(grid)
        path_row = QHBoxLayout()
        up = QPushButton("Up")
        up.clicked.connect(self.browse_up)
        self.path_edit = QLineEdit()
        self.path_edit.setPlaceholderText("Folder or image path")
        self.path_edit.returnPressed.connect(self._path_entered)
        path_row.addWidget(up)
        path_row.addWidget(self.path_edit, 1)
        outer.addLayout(path_row)
        self.query = QLineEdit()
        self.query.setPlaceholderText("Filter this folder")
        self.query.textChanged.connect(self._apply_filter)
        outer.addWidget(self.query)
        self.disk = DiskModel(self)
        self.disk.setFilter(QDir.AllDirs | QDir.Files | QDir.NoDotAndDotDot)
        self.disk.setNameFilterDisables(False)
        self.disk.setNameFilters(catalog.image_globs(""))
        self.disk.setResolveSymlinks(True)
        self.view = QTreeView()
        self.view.setObjectName("browser")
        self.view.setIconSize(QSize(16, 16))
        self.view.setModel(self.disk)
        self.view.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.view.setSortingEnabled(True)
        self.view.sortByColumn(0, Qt.AscendingOrder)
        self.view.setColumnHidden(2, True)
        self.view.header().setStretchLastSection(False)
        self.view.header().setSectionResizeMode(0, QHeaderView.Stretch)
        self.view.doubleClicked.connect(self._activate)
        outer.addWidget(self.view, 1)
        row = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self._cancel)
        open_button = QPushButton("Open")
        open_button.setObjectName("export")
        open_button.clicked.connect(self._open_current)
        row.addWidget(cancel)
        row.addWidget(open_button)
        outer.addLayout(row)
        self._status = QLabel("Select a picture.")
        outer.addWidget(self._status)

    def browse_at(self, path):
        if not path:
            return
        path = os.path.abspath(path)
        if os.path.isfile(path):
            folder = os.path.dirname(path)
        else:
            folder = path
        if not os.path.isdir(folder):
            self._status.setText("That folder is not there.")
            return
        self._folder = folder
        self.path_edit.setText(folder)
        self.view.setRootIndex(self.disk.setRootPath(folder))
        for label, button in self.place_buttons.items():
            place = dict(catalog.places()).get(label, "")
            on = bool(place) and os.path.abspath(place) == folder
            button.setProperty("current", "true" if on else "false")
            button.style().unpolish(button)
            button.style().polish(button)

    def browse_up(self):
        if not self._folder:
            return
        parent = os.path.dirname(self._folder)
        if parent == self._folder:
            return
        self.browse_at(parent)

    def _path_entered(self):
        text = self.path_edit.text().strip()
        if not text:
            return
        if os.path.isfile(text) and catalog.is_image(text):
            self._choose(text)
            return
        self.browse_at(text)

    def _apply_filter(self, text):
        self.disk.setNameFilters(catalog.image_globs(text))

    def _activate(self, index):
        if not index.isValid():
            return
        path = self.disk.filePath(index)
        if self.disk.isDir(index):
            self.browse_at(path)
            return
        if catalog.is_image(path):
            self._choose(path)
            return
        self._status.setText("Choose a picture.")

    def _open_current(self):
        self._activate(self.view.currentIndex())

    def _choose(self, path):
        path = os.path.abspath(path)
        self.chosen.emit(path)
        self.hide()

    def _cancel(self):
        self.hide()
        self.dismissed.emit()

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self._cancel()
            return
        super().keyPressEvent(event)

    def showEvent(self, event):
        super().showEvent(event)
        screen = self.screen() or QApplication.primaryScreen()
        if screen is None:
            return
        geo = screen.availableGeometry()
        frame = self.frameGeometry()
        frame.moveCenter(geo.center())
        self.move(frame.topLeft())
