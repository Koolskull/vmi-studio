"""Fixed color themes. A theme is chosen. Its colors are not edited."""


class Theme(object):
    def __init__(self, ident, name, blurb, field, ink, quiet, rule, row, gutter, grid, frame_line, soft, title, number):
        self.id = ident
        self.name = name
        self.blurb = blurb
        self.field = field
        self.ink = ink
        self.quiet = quiet
        self.rule = rule
        self.row = row
        self.gutter = gutter
        self.grid = grid
        self.frame_line = frame_line
        self.soft = soft
        self.title = title
        self.number = number


def _invert(value):
    text = value.lstrip("#")
    red = 255 - int(text[0:2], 16)
    green = 255 - int(text[2:4], 16)
    blue = 255 - int(text[4:6], 16)
    return "#%02x%02x%02x" % (red, green, blue)


def _flipped(source, ident, name, blurb):
    return Theme(
        ident, name, blurb,
        _invert(source.field), _invert(source.ink), _invert(source.quiet),
        _invert(source.rule), _invert(source.row), _invert(source.gutter),
        _invert(source.grid), _invert(source.frame_line), _invert(source.soft),
        _invert(source.title), _invert(source.number),
    )


STUDIO = Theme(
    "studio", "Studio", "Black field. White on the current row.",
    "#000000", "#ffffff", "#3a3a3a", "#2a2a2a", "#111111", "#141414",
    "#040404", "#333333", "#a0a0a0", "#2a2a2a", "#a0a0a0",
)
PAPER = _flipped(STUDIO, "paper", "Paper", "Studio, inverted, for a light field.")
# Blue frame from Monokai Charcoal high contrast. The charcoal stays black.
CHARCOAL = Theme(
    "charcoal", "Charcoal", "High contrast charcoal with a blue frame.",
    "#000000", "#ffffff", "#43b9d8", "#43b9d8", "#163844", "#0c2430",
    "#071820", "#1d4e62", "#7ec8e0", "#43b9d8", "#7ec8e0",
)
PEACH = Theme(
    "peach", "Peach", "Warm light pastel.",
    "#fff1e8", "#7a4a3a", "#c4a090", "#e7cfc2", "#ffe4d6", "#f3ddd0",
    "#f8e6da", "#edd5c6", "#a88474", "#d7b5a4", "#a88474",
)
MINT = Theme(
    "mint", "Mint", "Cool light pastel.",
    "#eef8f2", "#3d6b58", "#9bb8aa", "#cfe3d8", "#dff3e8", "#d5ebe0",
    "#e4f3eb", "#c5ddd0", "#6f9a86", "#b7d4c6", "#6f9a86",
)
LILAC = Theme(
    "lilac", "Lilac", "Soft light pastel.",
    "#f6f0fa", "#6a5080", "#b4a3c4", "#e2d4ee", "#efe4f7", "#e6d8f0",
    "#f0e6f6", "#ddd0ea", "#9178a8", "#cbb8dc", "#9178a8",
)

THEMES = (STUDIO, PAPER, CHARCOAL, PEACH, MINT, LILAC)
DEFAULT_ID = STUDIO.id
_BY_ID = {item.id: item for item in THEMES}
_current = STUDIO


def current():
    return _current


def get(ident):
    return _BY_ID.get(ident)


def choose(ident):
    """Remember one theme. An unknown id stays on Studio."""
    global _current
    _current = get(ident) or STUDIO
    return _current


def _sheet(text, ui, family):
    safe = (family or "monospace").replace("\\", "").replace('"', "")
    text = text.replace("@family", safe)
    for name in (
        "frame_line", "field", "quiet", "rule", "gutter", "grid",
        "title", "soft", "number", "ink", "row",
    ):
        text = text.replace("@" + name, getattr(ui, name))
    return text


_APP = """
QWidget { background: @field; color: @quiet; font-size: 12px; }
* { border-radius: 0; }
QMenuBar, QTreeView, QTreeWidget, QListWidget, QPlainTextEdit, QPushButton, QLineEdit, QSpinBox, QComboBox, QScrollArea, QLabel, QStatusBar, QDialog, QMessageBox, QHeaderView::section { font-family: "@family"; }
QLabel#title { color: @title; }
QMenuBar { background: @field; color: @quiet; border-bottom: 1px solid @rule; }
QMenuBar::item:selected { background: @row; color: @ink; }
QMenu { background: @field; color: @quiet; border: 1px solid @rule; border-radius: 0; }
QMenu::item:selected { background: @row; color: @ink; }
QTreeView, QTreeWidget, QListWidget, QPlainTextEdit { background: @field; color: @quiet; border: none; outline: none; }
QTreeView::item:selected, QTreeWidget::item:selected, QListWidget::item:selected { background: @row; color: @ink; }
QHeaderView::section { background: @field; color: @title; border: none; border-bottom: 1px solid @rule; padding: 2px 4px; }
QSplitter::handle { background: @gutter; }
QSplitter::handle:hover { background: @quiet; }
QSplitter::handle:pressed { background: @ink; }
QSplitter#pictureSplit::handle,
QSplitter#pictureSplit::handle:hover,
QSplitter#pictureSplit::handle:pressed { background: @field; border: none; }
QPushButton { background: @field; color: @quiet; border: 1px solid @rule; padding: 4px 8px; border-radius: 0; }
QPushButton:hover { color: @ink; border-color: @ink; }
QPushButton:pressed, QPushButton[current="true"] { color: @ink; border-color: @ink; }
QPushButton#export { background: @ink; color: @field; border: 1px solid @ink; }
QPushButton#export:pressed { background: @ink; color: @field; }
QPushButton#scenePreview { color: @ink; border: 1px solid @ink; padding: 0 6px; }
QLineEdit, QSpinBox { background: @field; color: @ink; border: 1px solid @rule; border-radius: 0; padding: 2px 4px; selection-background-color: @ink; selection-color: @field; }
QComboBox { background: @field; color: @quiet; border: 1px solid @rule; border-radius: 0; padding: 2px 6px; }
QComboBox:focus, QComboBox:on { color: @ink; border-color: @ink; }
QComboBox::drop-down { border: none; width: 16px; }
QComboBox QAbstractItemView { background: @field; color: @quiet; border: 1px solid @rule; selection-background-color: @row; selection-color: @ink; outline: 0; }
QScrollArea { background: @field; border: none; }
QTreeWidget::item { padding: 0px; border: none; }
QStatusBar { background: @field; color: @quiet; border-top: 1px solid @rule; }
QScrollBar:vertical { background: @field; width: 10px; margin: 0; }
QScrollBar::handle:vertical { background: @rule; min-height: 24px; border-radius: 0; }
QScrollBar::handle:vertical:hover { background: @quiet; }
QScrollBar:horizontal { background: @field; height: 10px; margin: 0; }
QScrollBar::handle:horizontal { background: @rule; min-width: 24px; border-radius: 0; }
QScrollBar::handle:horizontal:hover { background: @quiet; }
QScrollBar::add-line, QScrollBar::sub-line { height: 0; width: 0; }
QScrollBar::add-page, QScrollBar::sub-page { background: @field; }
QCheckBox::indicator, QRadioButton::indicator {
    border-radius: 0; width: 12px; height: 12px;
    border: 1px solid @quiet; background: @field;
}
QCheckBox::indicator:checked, QRadioButton::indicator:checked {
    background: @ink; border-color: @ink;
}
QComboBox::drop-down, QSpinBox::up-button, QSpinBox::down-button,
QToolButton, QProgressBar, QProgressBar::chunk,
QSlider::groove, QSlider::handle { border-radius: 0; }
QMessageBox { background: @field; color: @quiet; }
"""


def stylesheet(ui, family):
    return _sheet(_APP, ui, family)


def tab_sheet(ui=None):
    ui = ui or current()
    return _sheet(
        "QTabBar { background: @field; border: none; }"
        "QTabBar::tab { background: @field; color: @quiet; border: 1px solid @rule;"
        " border-radius: 0; padding: 0 0 0 8px; margin: 0 4px 0 0; }"
        "QTabBar::tab:selected { color: @ink; border-color: @ink; }",
        ui, "",
    )


def transport_sheet(ui=None):
    ui = ui or current()
    return _sheet(
        "QPushButton { background: @field; color: @quiet; border: 1px solid @rule;"
        " border-radius: 0; padding: 0 8px; }"
        "QPushButton:hover { color: @ink; border-color: @ink; }"
        'QPushButton[on="true"] { color: @ink; border-color: @ink; }',
        ui, "",
    )


def zoom_sheet(ui=None):
    ui = ui or current()
    return _sheet(
        "QPushButton { padding: 0; border: 1px solid @rule; border-radius: 0;"
        " background: @field; color: @quiet; }"
        "QPushButton:hover { border-color: @ink; color: @ink; }",
        ui, "",
    )


def cut_sheet(ui=None):
    ui = ui or current()
    return _sheet(
        "QPushButton { background: @field; color: @quiet; border: 1px solid @rule;"
        " border-radius: 0; padding: 4px 8px; }"
        "QPushButton:hover { color: @ink; border-color: @ink; }"
        "QPushButton#cutOn { background: @ink; color: @field; border-color: @ink; }",
        ui, "",
    )


def lock_sheet(ui=None):
    ui = ui or current()
    return _sheet("QPushButton { padding: 0; border: none; background: @field; }", ui, "")


def dialog_sheet(ui=None):
    ui = ui or current()
    return _sheet(
        "QDialog { background: @field; color: @quiet; border: 1px solid @rule; border-radius: 0; }"
        "QLabel { background: @field; border: none; }"
        "QTreeView { background: @field; color: @quiet; border: none; }"
        "QLineEdit { background: @field; color: @ink; border: 1px solid @rule; border-radius: 0; }"
        "QPushButton { background: @field; color: @quiet; border: 1px solid @rule; border-radius: 0; padding: 4px 8px; }"
        "QPushButton:hover { color: @ink; border-color: @ink; }"
        "QPushButton#export { background: @ink; color: @field; border: 1px solid @ink; }",
        ui, "",
    )


def bind_modules(ui=None):
    """Point the painted chrome at this theme. Call after both modules have loaded."""
    ui = ui or current()
    import vmi_studio.filmstrip as filmstrip
    import vmi_studio.window as window

    window.INK = ui.ink
    window.QUIET = ui.quiet
    window.RULE = ui.rule
    window.FIELD = ui.field
    window.ROW = ui.row
    filmstrip.INK = ui.ink
    filmstrip.QUIET = ui.quiet
    filmstrip.RULE = ui.rule
    filmstrip.FIELD = ui.field
    filmstrip.GRID = ui.grid
    filmstrip.FRAME_LINE = ui.frame_line
    filmstrip.NUMBER = ui.number
    filmstrip.TimelineGutter.REST = ui.gutter
    filmstrip.TimelineGutter.HOVER = ui.quiet
    filmstrip.TimelineGutter.HELD = ui.ink
