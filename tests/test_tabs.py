"""Project tabs keep separate export folders. The load window dithers in three greys."""

import os
import tempfile
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QObject, QPoint, Qt, Signal
from PySide6.QtGui import QImage
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QLabel, QTabBar

from vmi_studio.desk import DeskObject
from vmi_studio.document import ArtFile, Ids, Node, Raster, refresh
from PIL import Image

from vmi_studio.loadgate import (
    DIM,
    DOT_MS,
    DOTS,
    GLYPHS,
    HEIGHT,
    QUIET_TEXT,
    SOFT_TEXT,
    WHITE_TEXT,
    WIDTH,
    AsciiBar,
    BAR_HASH,
    DitherView,
    TONE_HEX,
    _tone_rgb,
    liquid_bits,
    liquid_image,
    stage_fraction,
)
from vmi_studio.tabs import default_export_parent, export_root, same_export


def _cell_ink(image, index, cell):
    left = index * cell
    right = min(image.width(), left + cell)
    for y in range(image.height()):
        for x in range(max(0, left), right):
            color = image.pixelColor(x, y)
            if color.red() > 16 or color.green() > 16 or color.blue() > 16:
                return (color.red(), color.green(), color.blue())
    return None


def _mark_right(widget):
    image = widget.grab().toImage()
    right = 0
    for y in range(image.height()):
        for x in range(image.width()):
            color = image.pixelColor(x, y)
            if color.red() > 20 or color.green() > 20 or color.blue() > 20:
                right = max(right, x)
    return right


def _ink(widget):
    image = widget.grab().toImage()
    best = (0, 0, 0)
    for y in range(image.height()):
        for x in range(image.width()):
            color = image.pixelColor(x, y)
            rgb = (color.red(), color.green(), color.blue())
            if sum(rgb) > sum(best):
                best = rgb
    return best


def pixel():
    return Raster(0, 0, 1, 1, bytes((255, 255, 255, 255)))


def drawing(folder, name):
    path = os.path.abspath(os.path.join(folder, name))
    ids = Ids()
    layer = Node(ids.next(), "paint", "layer", raster=pixel())
    return ArtFile(name, path, "clip", 1, 1, refresh([layer]), "")


class IdleLoader(QObject):
    ok = Signal(object)
    bad = Signal(str)
    note = Signal(str)

    def __init__(self, path):
        super().__init__()
        self.path = path
        self.started = False

    def start(self):
        self.started = True

    def isRunning(self):
        return False


class PathTests(unittest.TestCase):
    def test_each_file_gets_its_own_export_root(self):
        door = default_export_parent("/work/city/door.clip")
        other = default_export_parent("/work/shop/door.clip")
        wall = default_export_parent("/work/city/wall.clip")
        self.assertNotEqual(door, other)
        self.assertNotEqual(door, wall)
        self.assertNotEqual(export_root(door, "door.clip"), export_root(other, "door.clip"))
        self.assertNotEqual(export_root(door, "door.clip"), export_root(wall, "wall.clip"))
        self.assertFalse(same_export("/exports", "door.clip", "/exports", "wall.clip"))
        self.assertTrue(same_export("/exports", "door.clip", "/exports", "door.psd"))
        self.assertTrue(same_export("/exports", "door.clip", "/exports/../exports", "door.clip"))
        self.assertFalse(same_export("", "door.clip", "/exports", "door.clip"))

    def test_liquid_frames_use_three_greys_and_move(self):
        from PySide6.QtGui import QColor

        first = liquid_bits(0)
        later = liquid_bits(5)
        self.assertEqual(len(first), HEIGHT)
        self.assertEqual(len(first[0]), WIDTH)
        flat = [level for row in first for level in row]
        self.assertEqual(set(flat), {0, 1, 2})
        self.assertNotEqual(first, later)
        image = liquid_image(0)
        self.assertEqual(image.format(), QImage.Format.Format_Indexed8)
        self.assertEqual(image.colorCount(), 3)
        self.assertEqual(
            [image.color(index) for index in range(3)],
            [QColor(hex_color).rgb() for hex_color in TONE_HEX],
        )
        self.assertEqual(TONE_HEX, ("#000000", "#444444", "#CCCCCC"))
        self.assertEqual(stage_fraction("Converting Clip Studio paint"), 0.36)
        self.assertEqual(stage_fraction("Reading 4 layers"), 0.7)
        self.assertEqual(stage_fraction("Painting the picture"), 0.84)
        self.assertLess(stage_fraction("Reading the blueprint"), 1)
        greys = {_tone_rgb(level) for level in (0, 1, 2)}
        self.assertEqual(greys, {(0, 0, 0), (0x44, 0x44, 0x44), (0xCC, 0xCC, 0xCC)})


class TabTests(unittest.TestCase):
    def setUp(self):
        import vmi_studio.window as window_mod

        self.window_mod = window_mod
        self._load_thread = window_mod.LoadThread
        self._warning = window_mod.QMessageBox.warning
        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        window_mod.QMessageBox.warning = lambda *_args, **_kwargs: None
        self.app = QApplication.instance() or QApplication([])
        window_mod.apply_style(self.app)
        self.win = window_mod.MainWindow()
        self.folder = tempfile.mkdtemp(prefix="vmi-tabs-")

    def tearDown(self):
        self.win.close()
        self.window_mod.LoadThread = self._load_thread
        self.window_mod.QMessageBox.warning = self._warning

    def adopt(self, art):
        self.win._stash(self.win._shown_index)
        index = self.win._begin_tab(art.path)
        self.win._loading_index = index
        self.win._loading = True
        self.win._opened(art)
        return index

    def test_tabs_keep_their_drawings_and_refuse_one_export_path(self):
        city = os.path.join(self.folder, "city")
        shop = os.path.join(self.folder, "shop")
        os.makedirs(city)
        os.makedirs(shop)
        door = drawing(city, "door.clip")
        other = drawing(shop, "door.clip")
        wall = drawing(city, "wall.clip")
        self.adopt(door)
        self.win.picked.add(door.layers[0].id)
        self.win.objects.append(DeskObject("o1", "door", [door.layers[0].id], explicit=True))
        kept = set(self.win.picked)
        self.adopt(wall)
        self.assertEqual(self.win.tabs.tabText(0), "browser")
        self.assertEqual(self.win.tabs.tabText(1), "door.clip")
        self.assertEqual(self.win.tabs.count(), 3)
        self.assertFalse(self.win._tab_bar.isHidden())
        self.assertEqual(self.win.art.file_name, "wall.clip")
        self.assertEqual(self.win._projects[0].picked, kept)
        self.assertEqual(self.win._projects[0].objects[0].name, "door")
        self.assertNotEqual(self.win._projects[0].export_dir, self.win._projects[1].export_dir)
        self.assertNotEqual(
            export_root(self.win._projects[0].export_dir, "door.clip"),
            export_root(self.win._projects[1].export_dir, "wall.clip"),
        )
        self.win._png_recent = [self.win._projects[1].export_dir]
        self.win.tabs.setCurrentIndex(1)
        self.app.processEvents()
        self.assertEqual(self.win.art.file_name, "door.clip")
        self.assertEqual(self.win.objects[0].name, "door")
        self.assertEqual(self.win._tab_export_dir(), self.win._projects[0].export_dir)
        self.assertNotEqual(self.win._tab_export_dir(), self.win._png_recent[0])
        dialog = self.win._png_dialog(
            "Export scene",
            self.win._tab_export_dir(),
            "This tab writes its own scene folder.",
        )
        self.assertEqual(dialog.path.text(), self.win._projects[0].export_dir)
        dialog.close()
        self.win.tabs.setCurrentIndex(2)
        self.app.processEvents()
        self.assertEqual(self.win.art.file_name, "wall.clip")
        self.assertEqual(self.win.objects, [])
        self.assertTrue(self.win._claim_export_dir(self.win._projects[0].export_dir))
        shared = os.path.join(self.folder, "shared")
        self.adopt(other)
        self.win._projects[0].export_dir = shared
        self.win.tabs.setCurrentIndex(3)
        self.app.processEvents()
        self.assertFalse(self.win._claim_export_dir(shared))
        self.assertIn("own path", self.win.status.currentMessage())
        self.assertIn(self.win._projects[0].path, self.win.status.currentMessage())

    def test_tabs_stay_short_and_the_close_mark_is_grey_until_hover(self):
        city = os.path.join(self.folder, "city")
        os.makedirs(city)
        door = drawing(city, "door.clip")
        wall = drawing(city, "wall.clip")
        self.adopt(door)
        self.adopt(wall)
        self.assertEqual(self.win.tabs.tabText(0), "browser")
        self.assertIsNone(self.win.tabs.tabButton(0, QTabBar.ButtonPosition.RightSide))
        self.assertLessEqual(self.win.tabs.tabRect(0).height(), 20)
        self.assertIn("border-radius: 0", self.win.tabs.styleSheet())
        self.assertIn("border-radius: 0", self.window_mod.STYLESHEET)
        button = self.win.tabs.tabButton(2, QTabBar.ButtonPosition.RightSide)
        self.assertIsInstance(button, self.window_mod.TabClose)
        self.assertEqual(button.width(), 18)
        self.assertEqual(button.height(), 12)
        self.assertEqual(self.win.tabs.elideMode(), Qt.TextElideMode.ElideRight)
        rect = self.win.tabs.tabRect(2)
        self.assertLessEqual(button.geometry().right(), rect.right())
        self.assertGreaterEqual(rect.right() - _mark_right(button) - button.x(), 5)
        quiet = _ink(button)
        self.assertEqual(quiet, (58, 58, 58))
        button._hover = True
        button.repaint()
        self.assertEqual(_ink(button), (255, 255, 255))
        self.win.tabs.setTabText(1, "a-very-long-project-file-name-that-cannot-fit.clip")
        self.app.processEvents()
        self.assertLessEqual(self.win.tabs.tabRect(1).width(), self.window_mod.ProjectTabs.MAX_WIDTH)
        button.click()
        self.app.processEvents()
        self.assertEqual(self.win.tabs.count(), 2)
        self.assertEqual(self.win.tabs.tabText(0), "browser")
        self.assertEqual(self.win.art.file_name, "door.clip")

    def test_the_load_window_stays_up_until_the_file_is_read(self):
        city = os.path.join(self.folder, "city")
        os.makedirs(city)
        door = drawing(city, "door.clip")
        self.adopt(door)
        self.win.picked.add(door.layers[0].id)
        self.window_mod.LoadThread = IdleLoader
        missing = os.path.join(city, "missing.clip")
        self.win.resize(1280, 800)
        self.win.show()
        self.app.processEvents()
        self.win.open_path(missing)
        self.app.processEvents()
        gate = self.win._gate
        self.assertIsNotNone(gate)
        self.assertTrue(gate.isVisible())
        self.assertTrue(self.win._loading)
        self.assertEqual(gate.windowModality(), Qt.WindowModality.ApplicationModal)
        self.assertTrue(gate.windowFlags() & Qt.WindowType.WindowStaysOnTopHint)
        self.assertEqual(gate.size().width(), 340)
        self.assertGreaterEqual(gate.size().height(), 280)
        labels = {label.text(): label for label in gate.findChildren(QLabel)}
        self.assertEqual(labels["LOADING"].styleSheet(), WHITE_TEXT)
        self.assertEqual(labels["missing.clip"].styleSheet(), WHITE_TEXT)
        note = "The studio cannot be used until this file is loaded."
        self.assertEqual(labels[note].styleSheet(), QUIET_TEXT)
        self.assertEqual(labels["thank you for your patience"].styleSheet(), SOFT_TEXT)
        self.assertEqual(_ink(labels["LOADING"]), (255, 255, 255))
        self.assertEqual(_ink(labels["missing.clip"]), (255, 255, 255))
        self.assertEqual(_ink(labels[note]), (58, 58, 58))
        self.assertEqual(_ink(labels["thank you for your patience"]), (160, 160, 160))
        texts = [label.text() for label in gate.findChildren(QLabel)]
        self.assertEqual(texts.count("LOADING"), 1)
        self.assertEqual(texts.count("missing.clip"), 1)
        name_at = labels["missing.clip"].mapTo(gate, QPoint(0, 0))
        load_at = labels["LOADING"].mapTo(gate, QPoint(0, 0))
        dots_at = gate.dots.mapTo(gate, QPoint(0, 0))
        view_at = gate.view.mapTo(gate, QPoint(0, 0))
        self.assertLess(name_at.x(), load_at.x())
        self.assertLessEqual(name_at.x() + labels["missing.clip"].width(), load_at.x())
        self.assertLess(abs(name_at.y() - load_at.y()), 6)
        self.assertGreaterEqual(dots_at.x(), load_at.x() + labels["LOADING"].width() - 1)
        self.assertLess(abs(dots_at.y() - load_at.y()), 6)
        self.assertLess(name_at.y(), view_at.y())
        self.assertLess(load_at.y(), view_at.y())
        self.assertEqual(gate.dots.styleSheet(), WHITE_TEXT)
        self.assertEqual(gate._dot_timer.interval(), DOT_MS)
        self.assertTrue(gate._dot_timer.isActive())
        self.assertIn(gate.dots.text(), DOTS)
        seen = gate.dots.text()
        gate._step_dots()
        self.assertEqual(gate.dots.text(), DOTS[(DOTS.index(seen) + 1) % len(DOTS)])
        long_name = "a-very-long-project-file-name-that-cannot-fit-in-the-header-at-all.clip"
        gate.set_name(long_name)
        self.app.processEvents()
        shown = gate.name.text()
        self.assertNotEqual(shown, long_name)
        self.assertLess(len(shown), len(long_name))
        self.assertTrue(shown.endswith("…") or shown.endswith("..."))
        self.assertLess(gate.name.mapTo(gate, QPoint(0, 0)).x() + gate.name.width(), load_at.x() + 1)
        gate.set_name("missing.clip")
        self.assertEqual(gate.name.text(), "missing.clip")
        for label in labels.values():
            self.assertGreaterEqual(label.geometry().top(), 0)
            self.assertLessEqual(label.geometry().bottom(), gate.height() - 1)
        shade = self.win._shade
        self.assertIsNotNone(shade)
        self.assertTrue(shade.isWindow())
        self.assertFalse(shade.isHidden())
        self.assertEqual(shade.geometry(), self.win.frameGeometry())
        veil = shade.grab().toImage().pixelColor(8, 8)
        self.assertEqual((veil.red(), veil.green(), veil.blue()), (0, 0, 0))
        self.assertEqual(veil.alpha(), DIM)
        self.assertTrue(self.win.isEnabled())
        self.assertTrue(gate.view._timer.isActive())
        before = liquid_bits(gate.view.tick)
        gate.view.step()
        self.assertNotEqual(before, liquid_bits(gate.view.tick))
        bar = gate.bar
        self.assertIsInstance(bar, AsciiBar)
        self.assertGreater(bar.geometry().top(), labels["thank you for your patience"].geometry().bottom())
        self.assertLessEqual(bar.geometry().bottom(), gate.height() - 1)
        self.assertTrue(bar._timer.isActive())
        bar.reset()
        seen = []
        for _index in range(len(GLYPHS)):
            seen.append(bar.glyph())
            bar.step()
        self.assertEqual(tuple(seen), GLYPHS)
        self.assertLess(bar.fraction, 1)
        crept = bar.fraction
        for _index in range(20):
            bar.step()
        self.assertGreater(bar.fraction, crept)
        self.assertLess(bar.fraction, 1)
        self.assertTrue(bar.render().startswith("["))
        self.assertTrue(bar.render().endswith("]"))
        self.assertIn(bar.glyph(), bar.render()[1:-1])
        gate.hear("Converting Clip Studio paint")
        self.assertGreaterEqual(bar.fraction, 0.36)
        self.assertLess(bar.fraction, 1)
        bar.tick = 0
        bar.repaint()
        text, head = bar.cells()
        self.assertEqual(text[head], "#")
        behind = [index for index, ch in enumerate(text) if ch == "#" and index != head]
        self.assertTrue(behind)
        image = bar.grab().toImage()
        self.assertEqual(_cell_ink(image, behind[0], bar._cell), (0xBB, 0xBB, 0xBB))
        self.assertEqual(_cell_ink(image, head, bar._cell), (255, 255, 255))
        self.assertEqual(BAR_HASH, "#BBBBBB")
        self.assertEqual(self.win.tabs.count(), 3)
        self.win._open_failed("missing.clip could not be read.")
        self.app.processEvents()
        self.assertFalse(gate.isVisible())
        self.assertFalse(gate.view._timer.isActive())
        self.assertFalse(gate._dot_timer.isActive())
        self.assertTrue(self.win._shade.isHidden())
        self.assertFalse(self.win._loading)
        self.assertEqual(self.win.tabs.count(), 2)
        self.assertEqual(self.win.tabs.tabText(0), "browser")
        self.assertEqual(self.win.art.file_name, "door.clip")
        self.assertEqual(self.win._shown_index, 0)
        self.assertIn(door.layers[0].id, self.win.picked)
        self.win.open_path(door.path)
        self.assertEqual(self.win.tabs.count(), 2)
        self.assertIn("already open", self.win.status.currentMessage())
        self.assertFalse(self.win._loading)

    def test_the_load_window_stays_until_the_picture_is_painted(self):
        city = os.path.join(self.folder, "city")
        os.makedirs(city)
        door = drawing(city, "door.clip")
        state = {"alive": False}
        self.win._picture_sync = lambda: False

        def alive():
            return state["alive"]

        def start():
            self.win._picture_pending = False
            state["alive"] = True

        self.win._job_alive = alive
        self.win._start_picture_job = start
        self.win._show_gate(door.file_name)
        self.win._loading = True
        self.win._loading_index = self.win._begin_tab(door.path)
        self.win._opened(door)
        self.assertTrue(self.win._gate.isVisible())
        self.assertLess(self.win._gate.bar.fraction, 1)
        self.assertTrue(self.win._hold_gate)
        self.assertGreater(self.win.model.rowCount(), 0)
        self.assertTrue(self.win.picture._image is None or self.win.picture._image.isNull())
        self.win._finish_picture(
            self.win._picture_generation,
            Image.new("RGBA", (1, 1), (255, 255, 255, 255)),
            "",
        )
        self.assertFalse(self.win._gate.isVisible())
        self.assertTrue(self.win._shade.isHidden())
        self.assertFalse(self.win._hold_gate)
        self.assertFalse(self.win._loading)
        self.assertEqual(self.win._gate.bar.fraction, 1)
        self.assertTrue(set(self.win._gate.bar.render()[1:-1]) <= {"#"})
        self.assertIsNotNone(self.win.picture._image)
        self.assertFalse(self.win.picture._image.isNull())

    def test_a_drawing_opens_full_size_and_a_tab_change_catches_up(self):
        city = os.path.join(self.folder, "city")
        os.makedirs(city)
        door = drawing(city, "door.clip")
        wall = drawing(city, "wall.clip")
        self.adopt(door)
        self.adopt(wall)
        self.assertFalse(self.win.picture.fit)
        self.assertFalse(self.win._projects[0].low_res)
        self.assertFalse(self.win._projects[1].low_res)
        self.win.picture.set_fit(True)
        self.assertTrue(self.win._projects[1].low_res)
        self.assertFalse(self.win._projects[0].low_res)
        full = self.win._gate
        if full is None:
            self.win._show_gate("wall.clip")
            full = self.win._gate
        else:
            self.win._show_gate("wall.clip")
        full_height = full.height()
        self.win._hide_gate()
        state = {"alive": False}
        self.win._picture_sync = lambda: False
        self.win._job_alive = lambda: state["alive"]

        def start():
            self.win._picture_pending = False
            state["alive"] = True

        self.win._start_picture_job = start
        self.win.tabs.setCurrentIndex(1)
        self.app.processEvents()
        gate = self.win._gate
        self.assertTrue(gate.isVisible())
        self.assertTrue(gate._catchup)
        self.assertFalse(gate._header.isVisible())
        self.assertFalse(gate._quiet.isVisible())
        self.assertFalse(gate._soft.isVisible())
        self.assertTrue(gate.view.isVisible())
        self.assertTrue(gate.bar.isVisible())
        self.assertFalse(gate._dot_timer.isActive())
        self.assertLess(gate.height(), full_height)
        self.assertIn("cannot be used", gate._quiet.text())
        self.assertEqual(gate._soft.text(), "thank you for your patience")
        self.assertTrue(self.win._shade.isVisible())
        self.assertFalse(self.win.picture.fit)
        self.assertEqual(self.win.art.file_name, "door.clip")
        state["alive"] = False
        self.win._finish_picture(
            self.win._picture_generation,
            Image.new("RGBA", (1, 1), (255, 255, 255, 255)),
            "",
        )
        self.assertFalse(gate.isVisible())
        self.assertTrue(self.win._shade.isHidden())
        self.assertFalse(self.win._loading)
        self.win._show_gate("door.clip")
        self.assertFalse(gate._catchup)
        self.assertTrue(gate._header.isVisible())
        self.assertTrue(gate._quiet.isVisible())
        self.assertTrue(gate._soft.isVisible())
        self.assertTrue(gate._dot_timer.isActive())
        self.win._hide_gate()
        self.win._loading = False
        self.win._picture_sync = lambda: True
        self.win.open_path(wall.path, low_res=True)
        self.assertTrue(self.win._projects[1].low_res)
        self.assertTrue(self.win.picture.fit)
        shop = drawing(city, "shop.clip")
        self.win._loading_index = self.win._begin_tab(shop.path, low_res=True)
        self.win._loading = True
        self.win._opened(shop)
        self.assertTrue(self.win.picture.fit)
        self.assertTrue(self.win._projects[-1].low_res)
        self.assertFalse(self.win._loading)

    def test_a_press_on_the_graphic_stays_on_three_greys(self):
        greys = {(0, 0, 0), (0x44, 0x44, 0x44), (0xCC, 0xCC, 0xCC)}
        view = DitherView()
        view.show()
        self.app.processEvents()
        QTest.mousePress(view, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, QPoint(60, 39))
        QTest.mouseMove(view, QPoint(96, 72))
        self.app.processEvents()
        self.assertEqual(view.image.format(), QImage.Format.Format_Indexed8)
        self.assertEqual(view.image.colorCount(), 3)
        for y in range(view.image.height()):
            for x in range(view.image.width()):
                color = view.image.pixelColor(x, y)
                self.assertIn((color.red(), color.green(), color.blue()), greys)
        view.step()
        self.assertEqual(view.image.format(), QImage.Format.Format_Indexed8)
        view.close()

    def test_file_open_selects_the_browser_tab_and_it_stays(self):
        city = os.path.join(self.folder, "city")
        os.makedirs(city)
        door = drawing(city, "door.clip")
        self.assertEqual(self.win.tabs.tabText(0), "browser")
        self.assertIsNone(self.win.tabs.tabButton(0, QTabBar.ButtonPosition.RightSide))
        self.adopt(door)
        self.assertEqual(self.win.tabs.count(), 2)
        self.assertEqual(self.win.tabs.currentIndex(), 1)
        real_open = self.window_mod.QFileDialog.getOpenFileName
        called = []

        def refuse(*_args, **_kwargs):
            called.append("dialog")
            return "", ""

        self.window_mod.QFileDialog.getOpenFileName = refuse
        try:
            self.win.choose_file()
            self.app.processEvents()
        finally:
            self.window_mod.QFileDialog.getOpenFileName = real_open
        self.assertEqual(called, [])
        self.assertEqual(self.win.tabs.currentIndex(), 0)
        self.assertIs(self.win._pages.currentWidget(), self.win.browser_page)
        button = self.win.tabs.tabButton(1, QTabBar.ButtonPosition.RightSide)
        button.click()
        self.app.processEvents()
        self.assertEqual(self.win.tabs.count(), 1)
        self.assertEqual(self.win.tabs.tabText(0), "browser")
        self.assertIsNone(self.win.art)
        self.assertFalse(self.win._tab_bar.isHidden())

    def test_insert_image_dims_the_studio_and_adds_a_layer(self):
        from PySide6.QtWidgets import QMenu

        city = os.path.join(self.folder, "city")
        os.makedirs(city)
        door = drawing(city, "door.clip")
        self.adopt(door)
        self.win.resize(900, 700)
        self.win.show()
        self.app.processEvents()
        png = os.path.join(city, "speck.png")
        Image.new("RGBA", (2, 3), (10, 20, 30, 255)).save(png)
        self.win.pick_image()
        self.app.processEvents()
        pick = self.win._image_pick
        self.assertIsNotNone(pick)
        self.assertTrue(pick.isVisible())
        self.assertEqual(pick.windowModality(), Qt.WindowModality.ApplicationModal)
        self.assertIn("*.png", list(pick.disk.nameFilters()))
        self.assertNotIn("*.clip", list(pick.disk.nameFilters()))
        shade = self.win._shade
        self.assertTrue(shade.isVisible())
        self.assertTrue(shade.isWindow())
        veil = shade.grab().toImage().pixelColor(8, 8)
        self.assertEqual((veil.red(), veil.green(), veil.blue()), (0, 0, 0))
        self.assertEqual(veil.alpha(), DIM)
        self.assertTrue(self.win.isEnabled())
        pick._cancel()
        self.app.processEvents()
        self.assertFalse(pick.isVisible())
        self.assertTrue(self.win._shade.isHidden())
        self.win.insert_image(png)
        names = [node.name for node in self.win.art.layers]
        self.assertIn("speck", names)
        speck = next(node for node in self.win.art.layers if node.name == "speck")
        self.assertEqual((speck.raster.w, speck.raster.h), (2, 3))
        self.assertEqual(len(speck.raster.rgba), 2 * 3 * 4)
        self.win.undo()
        self.assertNotIn("speck", [node.name for node in self.win.art.layers])
        menu = QMenu()
        self.win.tree._fill_menu(menu, self.win.art.layers[0])
        labels = [action.text() for action in menu.actions()]
        self.assertEqual(labels[0], "Rename")
        self.assertIn("Insert image", labels)
        menu.close()
