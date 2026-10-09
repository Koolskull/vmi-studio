"""Font and theme, as two header dropdowns. A theme is only chosen."""

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import QComboBox, QHBoxLayout, QLabel, QWidget

from vmi_studio import fonts, theme


class SettingsBar(QWidget):
    """The header cluster. Settings, then the font menu, then the theme menu."""

    font_chosen = Signal(str)
    theme_chosen = Signal(str)

    def __init__(self):
        super().__init__()
        self.setObjectName("settingsBar")
        self._guard = False
        row = QHBoxLayout(self)
        row.setContentsMargins(8, 0, 8, 0)
        row.setSpacing(6)
        label = QLabel("Settings")
        label.setObjectName("settingsWord")
        row.addWidget(label)
        self.fonts = QComboBox()
        self.fonts.setObjectName("fontMenu")
        self.fonts.setFixedWidth(168)
        self.themes = QComboBox()
        self.themes.setObjectName("themeMenu")
        self.themes.setFixedWidth(120)
        for box in (self.fonts, self.themes):
            box.setFixedHeight(20)
            box.setMaxVisibleItems(16)
        self._fill_fonts()
        self._fill_themes()
        row.addWidget(self.fonts)
        row.addWidget(self.themes)
        self.fonts.currentIndexChanged.connect(self._font_current)
        self.themes.currentIndexChanged.connect(self._theme_current)

    def _fill_fonts(self):
        rows = list(fonts.catalog())
        order = {name: index for index, name in enumerate(fonts.CATEGORY_ORDER)}
        rows.sort(key=lambda face: (order.get(face.category, 99), face.name.lower()))
        previous = None
        for face in rows:
            if previous is not None and face.category != previous:
                self.fonts.insertSeparator(self.fonts.count())
            self.fonts.addItem(face.name, face.id)
            previous = face.category

    def _fill_themes(self):
        for item in theme.THEMES:
            self.themes.addItem(item.name, item.id)
            index = self.themes.count() - 1
            self.themes.setItemData(index, item.blurb, Qt.ToolTipRole)

    def _font_current(self, index):
        if self._guard or index < 0:
            return
        ident = self.fonts.itemData(index)
        if ident:
            self.font_chosen.emit(ident)

    def _theme_current(self, index):
        if self._guard or index < 0:
            return
        ident = self.themes.itemData(index)
        if ident:
            self.theme_chosen.emit(ident)

    def mark_font(self, ident):
        self._mark(self.fonts, ident)

    def mark_theme(self, ident):
        self._mark(self.themes, ident)

    def _mark(self, box, ident):
        self._guard = True
        try:
            for index in range(box.count()):
                if box.itemData(index) == ident:
                    box.setCurrentIndex(index)
                    return
        finally:
            self._guard = False
