"""The header chooses a font and one theme. A theme is not edited."""

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication, QLabel

from vmi_studio import theme


class SettingsTests(unittest.TestCase):
    def setUp(self):
        import vmi_studio.window as window_mod

        self.window_mod = window_mod
        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        self.app = QApplication.instance() or QApplication([])
        window_mod.apply_style(self.app, theme_id="studio")
        self.win = window_mod.MainWindow()

    def tearDown(self):
        self.win.close()
        self.window_mod.apply_style(self.app, theme_id="studio")
        self.assertEqual(theme.current().id, "studio")
        self.assertEqual(self.window_mod.INK, "#ffffff")

    def test_settings_chooses_a_font_and_a_theme(self):
        from vmi_studio import filmstrip

        tabs = self.win.tabs
        bar = self.win.settings_bar
        titles = [action.text() for action in self.win.menuBar().actions()]
        self.assertEqual(titles, ["File", "Edit", "View", "Layer"])
        self.assertNotIn("settings", [tabs.tabText(index) for index in range(tabs.count())])
        self.assertEqual(tabs.tabText(0), "browser")
        self.assertEqual(tabs.count(), 1)
        self.assertIs(self.win.menuBar().cornerWidget(Qt.TopRightCorner), bar)
        self.assertEqual(bar.findChild(QLabel, "settingsWord").text(), "Settings")

        names = [bar.themes.itemText(index) for index in range(bar.themes.count())]
        self.assertEqual(names, ["Studio", "Paper", "Charcoal", "Peach", "Mint", "Lilac"])
        for index, item in enumerate(theme.THEMES):
            self.assertEqual(bar.themes.itemData(index), item.id)
        font_ids = [bar.fonts.itemData(index) for index in range(bar.fonts.count())]
        self.assertIn("monospace", font_ids)

        self.win._font_id = ""
        mono = font_ids.index("monospace")
        if bar.fonts.currentIndex() == mono:
            bar.fonts.setCurrentIndex(-1)
        bar.fonts.setCurrentIndex(mono)
        self.app.processEvents()
        self.assertEqual(self.win._font_id, "monospace")
        self.assertEqual(self.win.status.currentMessage(), "UI font: Monospace")

        bar.themes.setCurrentIndex(1)
        self.app.processEvents()
        self.assertEqual(theme.current().id, "paper")
        self.assertEqual(theme.current().field, "#ffffff")
        self.assertEqual(self.window_mod.INK, "#000000")
        self.assertEqual(filmstrip.FIELD, "#ffffff")
        self.assertIn("#ffffff", self.app.styleSheet())
        self.assertEqual(self.win.browser_page.word.styleSheet().split(";")[0], "color: #000000")

        bar.themes.setCurrentIndex(2)
        self.app.processEvents()
        self.assertEqual(theme.current().id, "charcoal")
        self.assertIn("#43b9d8", self.app.styleSheet())
        self.assertIn("#43b9d8", self.win.tabs.styleSheet())

        bar.themes.setCurrentIndex(0)
        self.app.processEvents()
        self.assertEqual(theme.current().id, "studio")
        self.assertEqual(self.window_mod.INK, "#ffffff")
        self.assertEqual(filmstrip.GRID, "#040404")
        self.assertEqual(self.win.browser_page.word.styleSheet().split(";")[0], "color: #ffffff")
        self.assertIn("border-radius: 0", self.app.styleSheet())
