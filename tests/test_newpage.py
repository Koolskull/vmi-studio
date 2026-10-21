"""A new picture starts from a preset or from typed pixel sizes."""

import os
import unittest

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
        self.assertEqual(match_preset(640, 360), "")
        for value in (0, -1, MAX_EDGE + 1):
            with self.assertRaises(ValueError):
                parse_plate(value, 100)

    def test_a_blank_picture_is_unsaved_and_has_one_ink_layer(self):
        art = blank_picture(1280, 720)
        self.assertEqual(art.path, "")
        self.assertEqual(art.kind, "vmib")
        self.assertEqual((art.width, art.height), (1280, 720))
        self.assertEqual(art.file_name, "Untitled 1280×720")
        self.assertEqual([node.name for node in art.layers], ["Ink"])
        self.assertIsNone(art.layers[0].raster)


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
        self.assertIn("HD 1080", dialog.count_label.text())
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
        self.assertEqual(win.art.layers[0].name, "Ink")
        self.assertTrue(win.picture.fit)
        self.assertFalse(win.picture._image.isNull())
        self.assertIn("320×180", win.caption.text())
        win.set_draw_mode("pen")
        win._ink_press(20, 40, 1)
        win._ink_move(80, 40, 1)
        win._ink_release()
        app.processEvents()
        self.assertEqual(len(win.art.layers), 1)
        self.assertIsNotNone(win.art.layers[0].raster)
        win.undo()
        self.assertIsNone(win.art.layers[0].raster)
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
