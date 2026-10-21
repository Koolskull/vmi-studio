"""A new picture. A preset fills the pixel size. The two fields are the custom size.

The working plate is 1920 by 1080. Larger presets are in the list. Each side
stops at 8192 pixels, which is the largest plate the composite will hold.

Every new picture has a Paper layer under an empty Ink layer. The 2kool grid
preset adds the cyan template from the Clip Studio file, on top of Ink.
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QButtonGroup,
    QDialog,
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
)

from vmi_studio.document import ArtFile, Node, Raster, refresh
from vmi_studio.koolgrid import PLATE, grid_raster
from vmi_studio.paint import hex_rgb, parse_hex
from vmi_studio import theme
from vmi_studio.toggles import watch

import numpy as np
from PIL import Image

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
    ("2kool grid", 3840, 3840),
)

GRID_PRESET = "2kool grid"
PAPER_WHITE = "#FFFFFF"
PAPER_GREY = "#808080"
PAPER_BLACK = "#000000"
PAPER_CHOICES = (
    ("White", PAPER_WHITE),
    ("Grey", PAPER_GREY),
    ("Black", PAPER_BLACK),
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


def _paper_raster(width, height, color):
    red, green, blue = hex_rgb(color or PAPER_WHITE)
    arr = np.empty((int(height), int(width), 4), dtype=np.uint8)
    arr[..., 0] = red
    arr[..., 1] = green
    arr[..., 2] = blue
    arr[..., 3] = 255
    return Raster(0, 0, int(width), int(height), np.ascontiguousarray(arr).tobytes())


def _texture_raster(width, height, path):
    try:
        image = Image.open(path).convert("RGBA")
    except (OSError, ValueError) as exc:
        raise ValueError("That texture could not be read.") from exc
    if image.size != (int(width), int(height)):
        image = image.resize((int(width), int(height)), Image.Resampling.LANCZOS)
    return Raster(0, 0, int(width), int(height), image.tobytes())


def blank_picture(width, height, name="", background=PAPER_WHITE, texture=None, grid=False):
    """An unsaved picture. Paper is the back. Ink is the empty draw layer.

    grid adds the 2kool template, and only on the 3840 plate that template
    was measured from. Save writes the blueprint.
    """
    wide, high = parse_plate(width, height)
    title = (name or "").strip() or "Untitled %d×%d" % (wide, high)
    color = parse_hex(background) or PAPER_WHITE
    if texture:
        paper_raster = _texture_raster(wide, high, texture)
    else:
        paper_raster = _paper_raster(wide, high, color)
    paper = Node("n0", "Paper", "layer", raster=paper_raster)
    paper.color = color
    ink = Node("n1", "Ink", "layer")
    layers = [paper, ink]
    if grid and wide == PLATE and high == PLATE:
        layers.append(Node("n2", "2Kool Grid", "layer", raster=grid_raster()))
    return ArtFile(title, "", "vmib", wide, high, refresh(layers), "")


class NewPicture(QDialog):
    """Preset list on the left. Width, height, and the paper color stay editable."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("New picture")
        self.setModal(True)
        self._guard = False
        self._texture = ""
        root = QVBoxLayout(self)
        root.setContentsMargins(12, 12, 12, 12)
        root.setSpacing(8)
        note = QLabel(
            "A preset fills the size. Type any width and height. "
            "Paper is the back layer. Draw on Ink. "
            "2kool grid is the 3840 template."
        )
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
        fields.addWidget(QLabel("PAPER"))
        paper_row = QHBoxLayout()
        paper_row.setSpacing(4)
        self._paper_group = QButtonGroup(self)
        self._paper_group.setExclusive(True)
        self.paper_buttons = {}
        for label, color in PAPER_CHOICES:
            button = QPushButton(label)
            button.setCheckable(True)
            button.setToolTip(color)
            self._paper_group.addButton(button)
            self.paper_buttons[color] = button
            paper_row.addWidget(button)
        self.paper_buttons[PAPER_WHITE].setChecked(True)
        for button in self.paper_buttons.values():
            watch(button)
        self._paper_group.buttonClicked.connect(self._paper_clicked)
        fields.addLayout(paper_row)
        self.paper_hex = QLineEdit(PAPER_WHITE)
        self.paper_hex.setFixedWidth(84)
        self.paper_hex.setToolTip("Paper color. White, grey, black, or any hex.")
        self.paper_hex.editingFinished.connect(self._paper_typed)
        fields.addWidget(self.paper_hex)
        texture_row = QHBoxLayout()
        texture_row.setSpacing(4)
        self.texture_button = QPushButton("Texture")
        self.texture_button.setToolTip("A picture fills Paper instead of the flat color.")
        self.texture_button.clicked.connect(self._pick_texture)
        self.texture_clear = QPushButton("Clear")
        self.texture_clear.clicked.connect(self._clear_texture)
        texture_row.addWidget(self.texture_button)
        texture_row.addWidget(self.texture_clear)
        texture_row.addStretch(1)
        fields.addLayout(texture_row)
        self.texture_label = QLabel("Flat color")
        self.texture_label.setWordWrap(True)
        fields.addWidget(self.texture_label)
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

    def background(self):
        """The paper hex. A texture, when set, fills Paper instead."""
        return parse_hex(self.paper_hex.text()) or PAPER_WHITE

    def texture_path(self):
        return self._texture or ""

    def wants_grid(self):
        """True only for the 3840 2kool template."""
        return match_preset(self.width_box.value(), self.height_box.value()) == GRID_PRESET

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

    def _paper_clicked(self, button):
        for color, item in self.paper_buttons.items():
            if item is button:
                self.paper_hex.setText(color)
                return

    def _paper_typed(self):
        parsed = parse_hex(self.paper_hex.text())
        if parsed is None:
            self.paper_hex.setText(self.background() if parse_hex(self.paper_hex.text()) else PAPER_WHITE)
            parsed = parse_hex(self.paper_hex.text()) or PAPER_WHITE
        self.paper_hex.setText(parsed)
        for color, button in self.paper_buttons.items():
            button.setChecked(color == parsed)

    def _pick_texture(self):
        path, _selected = QFileDialog.getOpenFileName(
            self,
            "Paper texture",
            "",
            "Pictures (*.png *.jpg *.jpeg *.tif *.tiff *.webp *.bmp)",
        )
        if not path:
            return
        self._texture = path
        self.texture_label.setText(path)

    def _clear_texture(self):
        self._texture = ""
        self.texture_label.setText("Flat color")

    def _sync_count(self):
        wide, high = self.width_box.value(), self.height_box.value()
        name = match_preset(wide, high)
        label = "%s pixels" % format(wide * high, ",")
        if name:
            label = "%s    %s" % (name, label)
        else:
            label = "Custom    %s" % label
        if name == GRID_PRESET:
            label = "%s    grid on top" % label
        self.count_label.setText(label)


def _sheet():
    ui = theme.current()
    return theme.dialog_sheet(ui) + (
        "QListWidget { background: %s; color: %s; border: 1px solid %s; }"
        "QListWidget::item:selected { background: %s; color: %s; }"
        "QSpinBox, QLineEdit { background: %s; color: %s; border: 1px solid %s; border-radius: 0; padding: 2px 4px; }"
        "QLabel { color: %s; }"
    ) % (
        ui.field, ui.quiet, ui.rule,
        ui.ink, ui.field,
        ui.field, ui.ink, ui.rule,
        ui.quiet,
    )
