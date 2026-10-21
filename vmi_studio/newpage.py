"""A new picture. A preset fills the pixel size. The two fields are the custom size.

The working plate is 1920 by 1080. Larger presets are in the list. Each side
stops at 8192 pixels, which is the largest plate the composite will hold.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QHBoxLayout,
    QLabel,
    QListWidget,
    QListWidgetItem,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
)

from vmi_studio.document import ArtFile, Node, refresh
from vmi_studio import theme

# Name, width, height. The first row is the working plate.
PRESETS = (
    ("HD 1080", 1920, 1080),
    ("HD 720", 1280, 720),
    ("QHD", 2560, 1440),
    ("4K", 3840, 2160),
    ("Square", 1080, 1080),
    ("Portrait", 1080, 1920),
    ("Story", 720, 1280),
    ("Square 2K", 2048, 2048),
)

MIN_EDGE = 1
MAX_EDGE = 8192


def parse_plate(width, height):
    """Pixel size, or ValueError when a side is outside 1 to 8192."""
    try:
        wide = int(width)
        high = int(height)
    except (TypeError, ValueError) as exc:
        raise ValueError("Width and height are whole pixels.") from exc
    if wide < MIN_EDGE or high < MIN_EDGE or wide > MAX_EDGE or high > MAX_EDGE:
        raise ValueError("Each side is %d to %d pixels." % (MIN_EDGE, MAX_EDGE))
    return wide, high


def match_preset(width, height):
    """The preset name for this size, or an empty string when it is custom."""
    for name, wide, high in PRESETS:
        if wide == int(width) and high == int(height):
            return name
    return ""


def blank_picture(width, height, name=""):
    """An unsaved picture with one empty Ink layer. Save writes the blueprint."""
    wide, high = parse_plate(width, height)
    title = (name or "").strip() or "Untitled %d×%d" % (wide, high)
    layer = Node("n0", "Ink", "layer")
    return ArtFile(title, "", "vmib", wide, high, refresh([layer]), "")


class NewPicture(QDialog):
    """Preset list on the left. Width and height stay editable."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("New picture")
        self.setModal(True)
        self._guard = False
        root = QVBoxLayout(self)
        root.setContentsMargins(12, 12, 12, 12)
        root.setSpacing(8)
        note = QLabel("A preset fills the size. Type any width and height. Each side is 1 to 8192 pixels.")
        note.setWordWrap(True)
        root.addWidget(note)
        row = QHBoxLayout()
        row.setSpacing(12)
        self.presets = QListWidget()
        self.presets.setFixedWidth(220)
        for name, wide, high in PRESETS:
            item = QListWidgetItem("%s    %d×%d" % (name, wide, high))
            item.setData(Qt.UserRole, (wide, high))
            self.presets.addItem(item)
        self.presets.currentRowChanged.connect(self._preset)
        row.addWidget(self.presets)
        fields = QVBoxLayout()
        fields.setSpacing(8)
        fields.addWidget(QLabel("WIDTH"))
        self.width_box = self._spin()
        fields.addWidget(self.width_box)
        fields.addWidget(QLabel("HEIGHT"))
        self.height_box = self._spin()
        fields.addWidget(self.height_box)
        self.count_label = QLabel("")
        fields.addWidget(self.count_label)
        fields.addStretch(1)
        row.addLayout(fields, 1)
        root.addLayout(row)
        actions = QHBoxLayout()
        create = QPushButton("Create")
        create.setObjectName("export")
        create.clicked.connect(self.accept)
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        actions.addWidget(create)
        actions.addWidget(cancel)
        actions.addStretch(1)
        root.addLayout(actions)
        self.width_box.valueChanged.connect(self._typed)
        self.height_box.valueChanged.connect(self._typed)
        self.presets.setCurrentRow(0)
        self.setStyleSheet(_sheet())

    def _spin(self):
        box = QSpinBox()
        box.setRange(MIN_EDGE, MAX_EDGE)
        box.setFixedWidth(120)
        return box

    def plate(self):
        return parse_plate(self.width_box.value(), self.height_box.value())

    def _preset(self, row):
        if row < 0 or row >= len(PRESETS):
            return
        _name, wide, high = PRESETS[row]
        self._guard = True
        self.width_box.setValue(wide)
        self.height_box.setValue(high)
        self._guard = False
        self._sync_count()

    def _typed(self, _value):
        if self._guard:
            return
        wide, high = self.width_box.value(), self.height_box.value()
        matched = -1
        for index, (_name, preset_w, preset_h) in enumerate(PRESETS):
            if preset_w == wide and preset_h == high:
                matched = index
                break
        self._guard = True
        if matched < 0:
            self.presets.clearSelection()
            self.presets.setCurrentRow(-1)
        elif self.presets.currentRow() != matched:
            self.presets.setCurrentRow(matched)
        self._guard = False
        self._sync_count()

    def _sync_count(self):
        wide, high = self.width_box.value(), self.height_box.value()
        name = match_preset(wide, high)
        label = "%s pixels" % format(wide * high, ",")
        if name:
            label = "%s    %s" % (name, label)
        else:
            label = "Custom    %s" % label
        self.count_label.setText(label)


def _sheet():
    ui = theme.current()
    return theme.dialog_sheet(ui) + (
        "QListWidget { background: %s; color: %s; border: 1px solid %s; }"
        "QListWidget::item:selected { background: %s; color: %s; }"
        "QSpinBox { background: %s; color: %s; border: 1px solid %s; border-radius: 0; padding: 2px 4px; }"
        "QLabel { color: %s; }"
    ) % (
        ui.field, ui.quiet, ui.rule,
        ui.ink, ui.field,
        ui.field, ui.ink, ui.rule,
        ui.quiet,
    )
