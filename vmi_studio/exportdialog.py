"""Percent, resample, and a folder, asked before a picture is written."""

import os

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)


def favorite_places(drawing_path=""):
    """Home, Desktop, Documents, and the folder that holds the drawing."""
    home = os.path.expanduser("~")
    rows = [
        ("Home", home),
        ("Desktop", os.path.join(home, "Desktop")),
        ("Documents", os.path.join(home, "Documents")),
    ]
    folder = os.path.dirname(drawing_path) if drawing_path else ""
    if folder:
        rows.append(("Drawing", folder))
    return rows


class PngDialog(QDialog):
    """One settings panel. Scene export and Shift+E both use it.

    Tests call show and close. They do not call exec.
    """

    def __init__(
        self,
        parent=None,
        title="Export PNG",
        directory="",
        percent=100,
        resample="nearest",
        recents=None,
        favorites=None,
        places=None,
        note="",
    ):
        super().__init__(parent)
        self.setModal(False)
        self.setWindowTitle(title)
        self._favorites = [path for path in (favorites or []) if path]
        self._resample = "bicubic" if str(resample).lower() == "bicubic" else "nearest"
        root = QVBoxLayout(self)
        root.setContentsMargins(12, 12, 12, 12)
        root.setSpacing(8)
        if note:
            label = QLabel(note)
            label.setWordWrap(True)
            root.addWidget(label)
        root.addWidget(QLabel("PERCENT"))
        percent_row = QHBoxLayout()
        self.percent = QSpinBox()
        self.percent.setRange(1, 800)
        self.percent.setValue(max(1, min(800, int(percent or 100))))
        self.percent.setSuffix("%")
        percent_row.addWidget(self.percent)
        for amount in (25, 50, 100, 200):
            button = QPushButton("%d%%" % amount)
            button.clicked.connect(lambda _checked=False, value=amount: self.percent.setValue(value))
            percent_row.addWidget(button)
        percent_row.addStretch(1)
        root.addLayout(percent_row)
        root.addWidget(QLabel("RESAMPLE"))
        sample_row = QHBoxLayout()
        self.nearest = QPushButton("Nearest")
        self.bicubic = QPushButton("Bicubic")
        self.nearest.setCheckable(True)
        self.bicubic.setCheckable(True)
        self.nearest.clicked.connect(lambda: self._set_resample("nearest"))
        self.bicubic.clicked.connect(lambda: self._set_resample("bicubic"))
        sample_row.addWidget(self.nearest)
        sample_row.addWidget(self.bicubic)
        sample_row.addStretch(1)
        root.addLayout(sample_row)
        self._set_resample(self._resample)
        root.addWidget(QLabel("FOLDER"))
        path_row = QHBoxLayout()
        self.path = QLineEdit(directory or "")
        browse = QPushButton("Browse")
        browse.clicked.connect(self._browse)
        path_row.addWidget(self.path, 1)
        path_row.addWidget(browse)
        root.addLayout(path_row)
        root.addWidget(QLabel("PLACES"))
        self._places = QWidget()
        self._places_row = QHBoxLayout(self._places)
        self._places_row.setContentsMargins(0, 0, 0, 0)
        root.addWidget(self._places)
        self._fill_places(places or [])
        root.addWidget(QLabel("RECENT"))
        self._recents = QWidget()
        self._recent_row = QHBoxLayout(self._recents)
        self._recent_row.setContentsMargins(0, 0, 0, 0)
        root.addWidget(self._recents)
        self._fill_paths(self._recent_row, recents or [])
        root.addWidget(QLabel("FAVORITES"))
        fav_head = QHBoxLayout()
        star = QPushButton("Star this folder")
        star.clicked.connect(self._star)
        fav_head.addWidget(star)
        fav_head.addStretch(1)
        root.addLayout(fav_head)
        self._favs = QWidget()
        self._fav_row = QHBoxLayout(self._favs)
        self._fav_row.setContentsMargins(0, 0, 0, 0)
        root.addWidget(self._favs)
        self._fill_paths(self._fav_row, self._favorites)
        buttons = QHBoxLayout()
        buttons.addStretch(1)
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        export = QPushButton("Export")
        export.setObjectName("export")
        export.clicked.connect(self._confirm)
        buttons.addWidget(cancel)
        buttons.addWidget(export)
        root.addLayout(buttons)
        self.resize(520, 360)

    def _set_resample(self, name):
        self._resample = "bicubic" if name == "bicubic" else "nearest"
        self.nearest.setChecked(self._resample == "nearest")
        self.bicubic.setChecked(self._resample == "bicubic")

    def _fill_places(self, places):
        for label, path in places:
            if not path:
                continue
            button = QPushButton(label)
            button.setToolTip(path)
            button.clicked.connect(lambda _checked=False, folder=path: self.path.setText(folder))
            self._places_row.addWidget(button)
        self._places_row.addStretch(1)

    def _fill_paths(self, layout, paths):
        while layout.count():
            item = layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()
        shown = []
        for path in paths:
            if not path or path in shown:
                continue
            shown.append(path)
            button = QPushButton(os.path.basename(path) or path)
            button.setToolTip(path)
            button.clicked.connect(lambda _checked=False, folder=path: self.path.setText(folder))
            layout.addWidget(button)
        layout.addStretch(1)

    def _star(self):
        path = self.path.text().strip()
        if not path or path in self._favorites:
            return
        self._favorites.append(path)
        self._fill_paths(self._fav_row, self._favorites)

    def _browse(self):
        start = self.path.text().strip() or os.path.expanduser("~")
        chosen = QFileDialog.getExistingDirectory(self, "Export folder", start)
        if chosen:
            self.path.setText(chosen)

    def _confirm(self):
        if not self.path.text().strip():
            return
        self.accept()

    def values(self):
        return {
            "percent": int(self.percent.value()),
            "resample": self._resample,
            "path": self.path.text().strip(),
            "favorites": list(self._favorites),
        }
