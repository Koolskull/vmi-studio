"""Pen, paint, eraser, and the vector spine.

The live stroke stays off the picture rebuild. These checks cover the pixels,
the spine saved in the blueprint, and one desk stroke with undo.
"""

import time
import unittest

import numpy as np

from vmi_studio.document import ArtFile, Node, Raster, refresh
from vmi_studio.paint import (
    INK_MODES,
    Brush,
    MaskCanvas,
    clean_strokes,
    dab,
    ink_spacing,
    paint_softness,
    raster_over,
    sample_stroke,
    stored_antialias,
    stroke_row,
)
from vmi_studio.vmib import _node_from, prepare_blueprint


def _opaque(size, color=(255, 255, 255, 255)):
    arr = np.zeros((size, size, 4), dtype=np.uint8)
    arr[:, :] = color
    return Raster(0, 0, size, size, np.ascontiguousarray(arr).tobytes())


def _alpha_at(raster, x, y):
    if raster is None:
        return 0
    local_x = int(x) - int(raster.x)
    local_y = int(y) - int(raster.y)
    if local_x < 0 or local_y < 0 or local_x >= raster.w or local_y >= raster.h:
        return 0
    return raster.rgba[(local_y * raster.w + local_x) * 4 + 3]


class InkMathTests(unittest.TestCase):
    def test_pen_paint_eraser_and_vector_are_on_the_bar(self):
        self.assertEqual(INK_MODES, ("pen", "paint", "eraser", "vector", "line"))
        self.assertGreater(paint_softness("paint", 0), 0.35)
        self.assertEqual(paint_softness("pen", 0), 0)

    def test_a_pen_line_is_solid_and_an_eraser_opens_a_hole(self):
        brush = Brush()
        brush.size = 8
        canvas = MaskCanvas(80, 40, None)
        points = [(4, 20, 1), (70, 20, 1)]
        samples = sample_stroke(points, lambda pressure: ink_spacing(brush.diameter_for(pressure), "pen"))
        for x, y, pressure in samples:
            canvas.stamp(x, y, brush, pressure, "#FFFFFF")
        self.assertGreater(len(samples), 8)
        row = int(round(20 - canvas.y))
        left = int(round(10 - canvas.x))
        right = int(round(64 - canvas.x))
        alpha = canvas.arr[row, left:right, 3]
        self.assertGreater(int(alpha.min()), 0)

        plate = _opaque(32)
        eraser = MaskCanvas(32, 32, plate)
        eraser.stamp(16, 16, brush, 1, "#000000", erase=True)
        punched = eraser.raster()
        self.assertEqual(_alpha_at(punched, 16, 16), 0)
        self.assertEqual(_alpha_at(punched, 0, 0), 255)

    def test_new_ink_covers_the_layer_without_erasing_the_rest(self):
        base = _opaque(8, (10, 20, 30, 255))
        dab_plate = np.zeros((8, 8, 4), dtype=np.uint8)
        self.assertTrue(dab(dab_plate, 2, 2, 1.5, 0, False, (255, 0, 0, 255), 1))
        extra = Raster(0, 0, 8, 8, np.ascontiguousarray(dab_plate).tobytes())
        merged = raster_over(base, extra)
        self.assertEqual(_alpha_at(merged, 6, 6), 255)
        self.assertEqual(merged.rgba[(2 * merged.w + 2) * 4], 255)
        self.assertIs(raster_over(base, None), base)

    def test_a_wide_brush_stays_cheap(self):
        brush = Brush()
        brush.size = 24
        canvas = MaskCanvas(1080, 1920, None)
        started = time.perf_counter()
        for index in range(200):
            canvas.stamp(40 + index * 4, 80, brush, 1, "#FFFFFF")
        elapsed = time.perf_counter() - started
        self.assertLess(elapsed, 0.2)
        self.assertGreater(int(canvas.arr[:, :, 3].max()), 0)

    def test_blueprint_keeps_the_spine_and_skips_an_empty_one(self):
        brush = Brush()
        ink = Node("a", "ink", "layer")
        ink.strokes = [stroke_row("#00FF00", [(1, 2, 1), (8, 2, 0.5)], brush, 0)]
        plain = Node("b", "plain", "layer")
        art = ArtFile("ink.vmib", "", "vmib", 32, 32, refresh([ink, plain]), "")
        document, _blobs = prepare_blueprint(art, [])
        saved = document["layers"][0]
        self.assertEqual(saved["strokes"][0]["color"], "#00FF00")
        self.assertEqual(len(saved["strokes"][0]["points"]), 2)
        self.assertNotIn("strokes", document["layers"][1])
        loaded = _node_from(saved, None, set())
        self.assertEqual(clean_strokes(loaded.strokes)[0]["points"][1][0], 8)
        self.assertEqual(_node_from(document["layers"][1], None, set()).strokes, [])

    def test_antialias_keeps_the_old_fringe_and_opens_the_wider_steps(self):
        hard = np.zeros((24, 24, 4), dtype=np.uint8)
        soft = np.zeros((24, 24, 4), dtype=np.uint8)
        half = np.zeros((24, 24, 4), dtype=np.uint8)
        one = np.zeros((24, 24, 4), dtype=np.uint8)
        wide = np.zeros((24, 24, 4), dtype=np.uint8)
        dab(hard, 12, 12, 4, 0, False, (255, 255, 255, 255), 1)
        dab(soft, 12, 12, 4, 0, True, (255, 255, 255, 255), 1)
        dab(half, 12, 12, 4, 0, 0.5, (255, 255, 255, 255), 1)
        dab(one, 12, 12, 4, 0, 1.0, (255, 255, 255, 255), 1)
        dab(wide, 12, 12, 4, 0, 2.0, (255, 255, 255, 255), 1)
        self.assertTrue(np.array_equal(soft, half))
        self.assertFalse(np.array_equal(hard, soft))
        self.assertFalse(np.array_equal(soft, one))
        self.assertGreater(int((wide[:, :, 3] > 0).sum()), int((soft[:, :, 3] > 0).sum()))
        self.assertTrue(np.all((hard[:, :, 3] == 0) | (hard[:, :, 3] == 255)))
        self.assertTrue(np.any((soft[:, :, 3] > 0) & (soft[:, :, 3] < 255)))
        brush = Brush()
        self.assertIs(stored_antialias(brush.antialias), True)
        brush.antialias = 2.0
        kept = clean_strokes([stroke_row("#FFFFFF", [(1, 1, 1)], brush, 0)])
        self.assertEqual(kept[0]["antialias"], 2.0)


class InkDeskTests(unittest.TestCase):
    def _desk(self, layers, width=64, height=64):
        from tests.test_studio import ToolWindowTests

        return ToolWindowTests()._open(layers, width, height)

    def test_pen_stroke_undoes_the_paint_then_the_layer(self):
        app, win = self._desk([])
        win.set_draw_mode("pen")
        self.assertEqual(win.picture.draw_mode, "pen")
        win._ink_press(8, 32, 1)
        win._ink_move(48, 32, 1)
        win._ink_release()
        app.processEvents()
        node = win.art.layers[-1]
        self.assertEqual(node.name, "Ink")
        self.assertGreater(_alpha_at(node.raster, 28, 32), 0)
        win.undo()
        self.assertIsNone(node.raster)
        win.undo()
        self.assertEqual(win.art.layers, [])
        win.close()

    def test_vector_spine_is_stored_and_drawn(self):
        app, win = self._desk([])
        win.set_draw_mode("vector")
        win._ink_press(6, 20, 1)
        win._ink_move(40, 24, 0.4)
        win._ink_move(58, 20, 1)
        win._ink_release()
        app.processEvents()
        node = win.art.layers[-1]
        self.assertEqual(len(node.strokes), 1)
        self.assertGreaterEqual(len(node.strokes[0]["points"]), 3)
        self.assertGreater(_alpha_at(node.raster, 30, 22), 0)
        win.undo()
        self.assertEqual(node.strokes, [])
        self.assertIsNone(node.raster)
        win.close()

    def test_eraser_punches_the_selected_layer_and_undo_puts_it_back(self):
        layer = Node("paint", "paint", "layer", raster=_opaque(32))
        app, win = self._desk(refresh([layer]), 32, 32)
        win.select_only(layer)
        win.draw_bar.brush.size = 8
        win.set_draw_mode("eraser")
        win._ink_press(16, 16, 1)
        win._ink_release()
        app.processEvents()
        self.assertEqual(win.art.layers, [layer])
        self.assertEqual(_alpha_at(layer.raster, 16, 16), 0)
        self.assertEqual(_alpha_at(layer.raster, 0, 0), 255)
        win.undo()
        self.assertEqual(_alpha_at(layer.raster, 16, 16), 255)
        win.close()

    def test_new_layer_sits_in_front_and_a_folder_keeps_it_inside(self):
        ink = Node("ink", "Ink", "layer")
        app, win = self._desk(refresh([ink]))
        win.select_only(ink)
        made = win.new_layer()
        self.assertEqual([node.name for node in win.art.layers], ["Ink", "Layer"])
        self.assertIs(win._current_node(), made)
        self.assertIsNone(made.raster)
        again = win.new_layer()
        self.assertEqual(again.name, "Layer 2")
        self.assertEqual([node.name for node in win.art.layers], ["Ink", "Layer", "Layer 2"])
        win.undo()
        self.assertEqual([node.name for node in win.art.layers], ["Ink", "Layer"])
        layer_labels = []
        for action in win.menuBar().actions():
            menu = action.menu()
            if menu is not None and action.text() == "Layer":
                layer_labels = [item.text() for item in menu.actions()]
        self.assertEqual(layer_labels[0], "New layer")
        self.assertIn("Line", layer_labels)
        from PySide6.QtWidgets import QMenu

        menu = QMenu()
        win.tree._fill_menu(menu, win.art.layers[0])
        labels = [action.text() for action in menu.actions()]
        self.assertEqual(labels[0], "Rename")
        self.assertIn("New layer", labels)
        menu.close()
        win.close()

        folder_ink = Node("ink", "Ink", "layer")
        folder = Node("g", "Folder", "group")
        folder.children = [folder_ink]
        app, win = self._desk(refresh([folder]))
        win.select_only(folder)
        win.new_layer()
        self.assertEqual([node.name for node in folder.children], ["Ink", "Layer"])
        win.close()

    def test_the_aa_button_cycles_off_half_one_and_two(self):
        app, win = self._desk([])
        bar = win.draw_bar
        self.assertEqual(bar.color(), "#FF3EB8")
        self.assertEqual(bar.aa_button.text(), "AA .5")
        self.assertEqual(bar.brush.antialias, 0.5)
        self.assertFalse(bar.aa_button.isCheckable())
        for label, value in (("AA 1", 1.0), ("AA 2", 2.0), ("AA off", 0.0), ("AA .5", 0.5)):
            bar.aa_button.click()
            app.processEvents()
            self.assertEqual(bar.aa_button.text(), label)
            self.assertEqual(bar.brush.antialias, value)
        win.close()

    def test_a_line_is_one_straight_stroke_and_stores_no_spine(self):
        app, win = self._desk([])
        win.set_draw_mode("line")
        win.draw_bar.brush.size = 4
        win._ink_press(4, 20, 1)
        win._ink_move(40, 28, 1)
        win._ink_move(58, 44, 1)
        self.assertEqual(len(win._spine), 2)
        self.assertEqual(win._spine[0][:2], (4.0, 20.0))
        self.assertEqual(win._spine[1][:2], (58.0, 44.0))
        win._ink_release()
        app.processEvents()
        node = win.art.layers[-1]
        self.assertEqual(node.strokes, [])
        self.assertGreater(_alpha_at(node.raster, 31, 32), 0)
        self.assertEqual(_alpha_at(node.raster, 31, 4), 0)
        win.undo()
        self.assertIsNone(node.raster)
        win.close()
