"""A new picture starts from a preset or from typed pixel sizes."""

import os
import unittest

from vmi_studio.koolgrid import pixel
from vmi_studio.newpage import (
    MAX_EDGE,
    PRESETS,
    blank_picture,
    match_preset,
    parse_plate,
)


class PlateTests(unittest.TestCase):
    def test_presets_start_at_the_working_plate(self):
        self.assertEqual(PRESETS[0], ("HD 1080", 1920, 1080))
        self.assertEqual(parse_plate(1920, 1080), (1920, 1080))
        self.assertEqual(parse_plate("640", "360"), (640, 360))
        self.assertEqual(match_preset(1080, 1920), "Portrait")
        self.assertEqual(match_preset(3840, 3840), "2kool grid")
        self.assertEqual(match_preset(640, 360), "")
        self.assertEqual(PRESETS[-1], ("2kool grid", 3840, 3840))
        for value in (0, -1, MAX_EDGE + 1):
            with self.assertRaises(ValueError):
                parse_plate(value, 100)

    def test_a_blank_picture_is_unsaved_and_has_one_ink_layer(self):
        art = blank_picture(1280, 720)
        self.assertEqual(art.path, "")
        self.assertEqual(art.kind, "vmib")
        self.assertEqual((art.width, art.height), (1280, 720))
        self.assertEqual(art.file_name, "Untitled 1280×720")
        self.assertEqual([node.name for node in art.layers], ["Paper", "Ink"])
        self.assertEqual(art.layers[0].raster.rgba[:4], bytes([255, 255, 255, 255]))
        self.assertIsNone(art.layers[1].raster)
        grey = blank_picture(8, 8, background="#808080")
        self.assertEqual(grey.layers[0].raster.rgba[:4], bytes([128, 128, 128, 255]))
        self.assertEqual(pixel(1920, 1920), (44, 176, 255, 255))
        self.assertEqual(pixel(200, 1407)[:3], (44, 176, 255))
        self.assertEqual(pixel(200, 1407)[3], 158)
        self.assertEqual(pixel(10, 10), (0, 0, 0, 0))
        self.assertEqual(pixel(1505, 125)[3], 158)


class NewPictureDialogTests(unittest.TestCase):
    def _dialog(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtWidgets import QApplication

        from vmi_studio.newpage import NewPicture

        QApplication.instance() or QApplication([])
        return NewPicture()

    def test_a_preset_fills_the_fields_and_typing_makes_a_custom_size(self):
        dialog = self._dialog()
        self.assertEqual(dialog.plate(), (1920, 1080))
        self.assertEqual(dialog.background(), "#FFFFFF")
        self.assertFalse(dialog.wants_grid())
        self.assertIn("HD 1080", dialog.count_label.text())
        dialog.paper_buttons["#808080"].click()
        self.assertEqual(dialog.background(), "#808080")
        dialog.presets.setCurrentRow(len(PRESETS) - 1)
        self.assertEqual(dialog.plate(), (3840, 3840))
        self.assertTrue(dialog.wants_grid())
        dialog.presets.setCurrentRow(0)
        dialog.presets.setCurrentRow(5)
        self.assertEqual(dialog.plate(), (1080, 1920))
        dialog.width_box.setValue(640)
        dialog.height_box.setValue(360)
        self.assertEqual(dialog.plate(), (640, 360))
        self.assertEqual(dialog.presets.currentRow(), -1)
        self.assertIn("Custom", dialog.count_label.text())
        dialog.width_box.setValue(1920)
        dialog.height_box.setValue(1080)
        self.assertEqual(dialog.presets.currentRow(), 0)
        dialog.close()


class NewPictureDeskTests(unittest.TestCase):
    def test_new_picture_opens_a_fitted_plate_ready_to_draw(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from tests.test_studio import ToolWindowTests

        app, win = ToolWindowTests()._open([])
        self.assertEqual(win.browser_page.new_picture_button.text(), "New picture")
        win.open_blank(320, 180)
        app.processEvents()
        self.assertEqual((win.art.width, win.art.height), (320, 180))
        self.assertEqual(win.art.path, "")
        self.assertEqual([node.name for node in win.art.layers], ["Paper", "Ink"])
        self.assertEqual(win.tree.currentIndex().isValid() and win.model.node_of(win.tree.currentIndex()).name, "Ink")
        self.assertTrue(win.draw_bar.brush.use_size_pressure)
        self.assertTrue(win.draw_bar.brush.use_opacity_pressure)
        self.assertTrue(win.draw_bar.size_pressure.isChecked())
        self.assertTrue(win.draw_bar.opacity_pressure.isChecked())
        stage = win.picture.parentWidget().layout()
        self.assertIs(stage.itemAt(0).widget(), win.draw_bar.tool_column)
        self.assertLess(stage.indexOf(win.picture), stage.indexOf(win.effect_bar))
        self.assertTrue(win.picture.fit)
        self.assertFalse(win.picture._image.isNull())
        self.assertIn("320×180", win.caption.text())
        win.set_draw_mode("pen")
        win._ink_press(20, 40, 1)
        win._ink_move(80, 40, 1)
        win._ink_release()
        app.processEvents()
        win._ink_press(20, 90, 1)
        win._ink_move(80, 90, 1)
        win._ink_release()
        app.processEvents()
        self.assertEqual([node.name for node in win.art.layers], ["Paper", "Ink"])
        self.assertIsNotNone(win.art.layers[1].raster)
        win.undo()
        win.undo()
        self.assertEqual([node.name for node in win.art.layers], ["Paper", "Ink"])
        self.assertIsNone(win.art.layers[1].raster)
        win.open_blank(320, 180)
        app.processEvents()
        self.assertEqual(win.art.file_name, "Untitled 320×180 2")
        win.close()

    def test_new_picture_ink_is_white_and_the_view_center_is_the_plate_center(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from tests.test_studio import ToolWindowTests

        app, win = ToolWindowTests()._open([])
        self.assertEqual(win.draw_bar.color(), "#FF3EB8")
        win.open_blank(320, 180)
        app.processEvents()
        self.assertEqual(win.draw_bar.color(), "#FFFFFF")
        win.draw_bar.black_swatch.click()
        self.assertEqual(win.draw_bar.color(), "#000000")
        win.draw_bar.white_swatch.click()
        self.assertEqual(win.draw_bar.color(), "#FFFFFF")
        picture = win.picture
        self.assertGreater(picture.width(), 40)
        self.assertGreater(picture.height(), 40)
        scale = picture._shown_scale()
        self.assertGreater(abs(scale - 1.0), 0.15)
        cx, cy = picture.document_point((picture.width() / 2.0, picture.height() / 2.0))
        self.assertAlmostEqual(cx, 160, delta=1.0)
        self.assertAlmostEqual(cy, 90, delta=1.0)
        ox, oy = picture.document_point((picture.width() / 2.0 + 12.0, picture.height() / 2.0))
        self.assertAlmostEqual(ox - cx, 12.0 / scale, delta=0.25)
        self.assertAlmostEqual(oy, cy, delta=0.25)
        win.close()

    def test_shortcuts_duplicate_flatten_and_a_new_frame(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtCore import Qt
        from PySide6.QtTest import QTest

        from tests.test_studio import ToolWindowTests
        from vmi_studio.document import ArtFile, Node, refresh

        app, win = ToolWindowTests()._open([])
        win.open_blank(32, 32)
        app.processEvents()
        tips = {ident: button.toolTip() for ident, button in win.draw_bar.buttons.items()}
        self.assertEqual(tips["pen"], "Pen    P")
        self.assertEqual(tips["paint"], "Paint    B")
        self.assertEqual(tips["eraser"], "Eraser    E")
        self.assertEqual(tips["vector"], "Vector    V")
        self.assertEqual(tips["line"], "Line    L")
        self.assertEqual(tips["warp-target"], "Create warp target    T")
        self.assertEqual(tips["warp-mask"], "Draw warp mask    M")
        self.assertEqual(win.draw_bar.fore_swatch.toolTip(), "Ink    X")
        self.assertEqual(win.draw_bar.clear_button.toolTip(), "Clear    C")
        self.assertEqual(win.draw_bar.size_box.toolTip(), "Size    [ ]")
        QTest.keyClick(win, Qt.Key_P)
        app.processEvents()
        self.assertEqual(win.picture.draw_mode, "pen")
        QTest.keyClick(win, Qt.Key_B)
        app.processEvents()
        self.assertEqual(win.picture.draw_mode, "paint")
        win.draw_bar.hex_edit.setFocus()
        app.processEvents()
        QTest.keyClick(win.draw_bar.hex_edit, Qt.Key_E)
        app.processEvents()
        self.assertEqual(win.picture.draw_mode, "paint")
        win.draw_bar.hex_edit.clearFocus()
        before = win.draw_bar.brush.size
        QTest.keyClick(win, Qt.Key_BracketRight)
        app.processEvents()
        self.assertEqual(win.draw_bar.brush.size, before + 1)
        QTest.keyClick(win, Qt.Key_X)
        app.processEvents()
        self.assertEqual(win.draw_bar.color(), "#000000")
        QTest.keyClick(win, Qt.Key_X)
        app.processEvents()
        self.assertEqual(win.draw_bar.color(), "#FFFFFF")
        ink = win.art.layers[1]
        win.duplicate_layers()
        app.processEvents()
        self.assertEqual([node.name for node in win.art.layers], ["Paper", "Ink", "Ink 2"])
        folder = Node("anim", "Shot", "group", animation=True)
        cel = Node("cel", "1", "layer")
        folder.children = [cel]
        folder.timeline = [cel.id]
        folder.frames = [0]
        art = ArtFile("shot.vmib", "", "vmib", 16, 16, refresh([folder]), "")
        win.show_file(art, restore=False)
        app.processEvents()
        win.select_only(cel)
        win.film.set_playhead(0)
        QTest.keyClick(win, Qt.Key_F, Qt.ShiftModifier)
        app.processEvents()
        self.assertEqual(len(folder.children), 2)
        self.assertEqual(folder.frames.count(0), 1)
        self.assertIn(1, folder.frames)
        self.assertEqual(folder.timeline[folder.frames.index(0)], cel.id)
        win.close()

    def test_shift_f_opens_an_animation_and_onion_skin(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtCore import Qt
        from PySide6.QtTest import QTest

        from tests.test_studio import ToolWindowTests
        from vmi_studio.composite import composite_scene
        from vmi_studio.document import Raster, cel_at

        app, win = ToolWindowTests()._open([])
        win.open_blank(32, 32)
        app.processEvents()
        self.assertFalse(win._onion)
        self.assertIn("background: #000000", win.transport.onion_button.styleSheet())
        self.assertIn("background: #ffffff", win.draw_bar.size_pressure.styleSheet())
        win.draw_bar.size_pressure.setChecked(False)
        app.processEvents()
        self.assertIn("background: #000000", win.draw_bar.size_pressure.styleSheet())
        self.assertTrue(win.draw_bar.opacity_pressure.isChecked())
        ink = next(node for node in win.art.layers if node.name == "Ink")
        rgba = bytearray(32 * 32 * 4)
        rgba[0:4] = bytes((255, 0, 0, 255))
        ink.raster = Raster(0, 0, 32, 32, bytes(rgba))
        QTest.keyClick(win, Qt.Key_F, Qt.ShiftModifier)
        app.processEvents()
        self.assertEqual(win.art.layers[0].name, "Paper")
        folder = next(node for node in win.art.layers if getattr(node, "animation", False))
        self.assertEqual(folder.name, "Animation")
        self.assertEqual(cel_at(folder, 0).name, "Ink")
        self.assertNotEqual(cel_at(folder, 1).name, "Ink")
        self.assertEqual(win.film.playhead, 1)
        self.assertTrue(win._onion)
        self.assertIn("background: #ffffff", win.transport.onion_button.styleSheet())
        self.assertEqual(tuple(cel_at(folder, 0).raster.rgba[0:4]), (255, 0, 0, 255))
        plain = composite_scene(32, 32, win.art.layers, None)
        self.assertEqual(plain.getpixel((0, 0)), (255, 255, 255, 255))
        skinned = win._flat.getpixel((0, 0))
        self.assertGreater(skinned[2], skinned[0])
        self.assertGreater(skinned[0], skinned[1])
        self.assertEqual(win._flat.getpixel((8, 8)), (255, 255, 255, 255))
        win.transport.onion_button.click()
        app.processEvents()
        self.assertFalse(win._onion)
        self.assertEqual(win._flat.getpixel((0, 0)), (255, 255, 255, 255))
        win.close()
