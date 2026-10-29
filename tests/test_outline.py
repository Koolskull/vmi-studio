"""Clip Studio's border edge: width, on/off, and the pixels around the paint."""

import os
import struct
import tempfile
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtWidgets import QApplication

from vmi_studio.composite import composite_image, reset_plate_cache
from vmi_studio.document import ArtFile, Ids, Node, Raster, refresh
from vmi_studio.outline import (
    apply_clip_outlines,
    parse_effect_edge,
    render_outline,
)
from vmi_studio.vmib import load_blueprint, write_blueprint


def _dot(x, y, color=(255, 0, 0, 255)):
    return Raster(x, y, 1, 1, bytes(color))


def _edge_blob(on, px, rgb):
    name = "EffectEdge"
    body = len(name).to_bytes(4, "big") + name.encode("utf-16-be")
    body += (1 if on else 0).to_bytes(4, "big")
    body += struct.pack(">d", float(px))
    for channel in rgb:
        body += (int(channel) << 24).to_bytes(4, "big")
    return body


class OutlineTests(unittest.TestCase):
    def test_effect_edge_reads_the_width_and_the_color(self):
        edge = parse_effect_edge(_edge_blob(True, 4.0, (10, 20, 30)))
        self.assertEqual(edge["on"], True)
        self.assertEqual(edge["px"], 4.0)
        self.assertEqual(edge["rgb"], (10, 20, 30))
        off = parse_effect_edge(_edge_blob(False, 6.0, (0, 0, 0)))
        self.assertEqual(off["on"], False)
        self.assertEqual(off["px"], 6.0)
        self.assertIsNone(parse_effect_edge(b"not an effect"))

    def test_one_pixel_grows_a_round_outline(self):
        src = Raster(0, 0, 1, 1, bytes((255, 0, 0, 255)))
        made = render_outline(src, 1, (0, 0, 0))
        # Radius 1 is solid on the four neighbors and partial on the diagonals, so the crop is 3x3.
        self.assertEqual((made.x, made.y, made.w, made.h), (-1, -1, 3, 3))
        import numpy as np
        arr = np.frombuffer(made.rgba, dtype=np.uint8).reshape(made.h, made.w, 4)
        cx = -made.x
        cy = -made.y
        self.assertEqual(tuple(int(v) for v in arr[cy, cx]), (255, 0, 0, 255))
        self.assertEqual(tuple(int(v) for v in arr[cy - 1, cx]), (0, 0, 0, 255))
        self.assertEqual(tuple(int(v) for v in arr[cy + 1, cx]), (0, 0, 0, 255))
        diagonal = arr[cy - 1, cx - 1]
        self.assertEqual(int(diagonal[0]), 0)
        self.assertGreater(int(diagonal[3]), 0)
        self.assertLess(int(diagonal[3]), 255)
        # Two pixels away is outside the disc. A negative index would wrap onto the far edge.
        reset_plate_cache()
        layer = Node("n0", "ink", "layer", raster=Raster(5, 5, 1, 1, bytes((255, 0, 0, 255))))
        layer.outline = True
        layer.outline_px = 1
        layer.outline_rgb = (0, 0, 0)
        image = composite_image(16, 16, [layer])
        self.assertEqual(image.getpixel((5, 4)), (0, 0, 0, 255))
        self.assertEqual(image.getpixel((5, 3))[3], 0)

    def test_the_picture_draws_the_outline_and_can_turn_it_off(self):
        reset_plate_cache()
        layer = Node("n0", "ink", "layer", raster=_dot(10, 10))
        layer.outline = True
        layer.outline_px = 2
        layer.outline_rgb = (0, 0, 0)
        image = composite_image(30, 30, [layer])
        self.assertEqual(image.getpixel((10, 10)), (255, 0, 0, 255))
        self.assertEqual(image.getpixel((10, 8)), (0, 0, 0, 255))
        self.assertEqual(image.getpixel((10, 7))[3], 0)
        layer.outline = False
        if hasattr(layer, "_outline_cache"):
            del layer._outline_cache
        reset_plate_cache()
        plain = composite_image(30, 30, [layer])
        self.assertEqual(plain.getpixel((10, 10)), (255, 0, 0, 255))
        self.assertEqual(plain.getpixel((10, 8))[3], 0)

    def test_a_folder_border_does_not_spread_and_a_layer_keeps_its_own(self):
        reset_plate_cache()
        ink = Node("n1", "ink", "layer", raster=_dot(10, 10))
        folder = Node("n0", "group", "group", blend="normal")
        folder.children = [ink]
        folder.outline = True
        folder.outline_px = 2
        folder.outline_rgb = (0, 0, 255)
        image = composite_image(30, 30, refresh([folder]))
        self.assertEqual(image.getpixel((10, 10))[:3], (255, 0, 0))
        self.assertEqual(image.getpixel((10, 8))[3], 0)
        ink.outline = True
        ink.outline_px = 2
        ink.outline_rgb = (0, 0, 255)
        reset_plate_cache()
        owned = composite_image(30, 30, refresh([folder]))
        self.assertEqual(owned.getpixel((10, 10))[:3], (255, 0, 0))
        self.assertEqual(owned.getpixel((10, 8))[:3], (0, 0, 255))
        self.assertGreater(owned.getpixel((10, 8))[3], 200)

    def test_a_blueprint_keeps_the_outline(self):
        ids = Ids()
        layer = Node(ids.next(), "ink", "layer", raster=_dot(0, 0))
        layer.outline = True
        layer.outline_px = 5
        layer.outline_rgb = (1, 2, 3)
        art = ArtFile("door.clip", "/tmp/door.clip", "clip", 4, 4, refresh([layer]), "")
        folder = tempfile.mkdtemp(prefix="vmi-outline-")
        path = os.path.join(folder, "door.vmib")
        try:
            write_blueprint(path, art, [])
            loaded = load_blueprint(path)
        finally:
            import shutil
            shutil.rmtree(folder, ignore_errors=True)
        kept = loaded.layers[0]
        self.assertTrue(kept.outline)
        self.assertEqual(kept.outline_px, 5)
        self.assertEqual(kept.outline_rgb, (1, 2, 3))

    def test_the_control_sets_the_selected_layer(self):
        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        app = QApplication.instance() or QApplication([])
        window_mod.apply_style(app)
        win = window_mod.MainWindow()
        try:
            layer = Node("n0", "ink", "layer", raster=_dot(2, 2))
            art = ArtFile("ink.clip", "/tmp/ink.clip", "clip", 8, 8, refresh([layer]), "")
            win.show_file(art, restore=False)
            win._select_layer(layer.id)
            app.processEvents()
            self.assertFalse(win.outline_box.isChecked())
            win.outline_px.setValue(3)
            app.processEvents()
            self.assertEqual(layer.outline_px, 3)
            self.assertFalse(layer.outline)
            win.outline_box.setChecked(True)
            app.processEvents()
            self.assertTrue(layer.outline)
            image = composite_image(8, 8, win.art.layers)
            self.assertEqual(image.getpixel((2, 2))[:3], (255, 0, 0))
            self.assertGreater(image.getpixel((2, 0))[3], 200)
        finally:
            win.close()


class OutlineFileTests(unittest.TestCase):
    def test_a_clip_tree_pairs_an_outline_onto_the_same_layer(self):
        import sqlite3

        folder = tempfile.mkdtemp(prefix="vmi-edge-")
        db_path = os.path.join(folder, "layers.sqlite")
        clip_path = os.path.join(folder, "ink.clip")
        try:
            connection = sqlite3.connect(db_path)
            connection.execute("CREATE TABLE Canvas (CanvasRootFolder INTEGER)")
            connection.execute(
                "CREATE TABLE Layer (MainId INTEGER, LayerName TEXT, LayerFolder INTEGER, "
                "LayerFirstChildIndex INTEGER, LayerNextIndex INTEGER, LayerEffectInfo BLOB)"
            )
            connection.execute("INSERT INTO Canvas VALUES (1)")
            connection.execute(
                "INSERT INTO Layer VALUES (1, 'Root', 1, 2, 0, NULL)"
            )
            connection.execute(
                "INSERT INTO Layer VALUES (2, 'ink', 0, 0, 3, ?)",
                (_edge_blob(True, 4, (9, 8, 7)),),
            )
            connection.execute("INSERT INTO Layer VALUES (3, 'paper', 0, 0, 0, NULL)")
            connection.commit()
            connection.close()
            with open(db_path, "rb") as handle:
                blob = handle.read()
            header = b"CSFCHUNK" + b"\0" * 16
            chunk = b"CHNK" + b"SQLi" + b"\0" * 4 + len(blob).to_bytes(4, "big") + blob
            with open(clip_path, "wb") as handle:
                handle.write(header + chunk)
            ink = Node("n0", "ink", "layer", raster=_dot(0, 0))
            paper = Node("n1", "paper", "layer")
            art = ArtFile("ink.clip", clip_path, "clip", 4, 4, refresh([ink, paper]), "")
            self.assertEqual(apply_clip_outlines(art, clip_path), 1)
        finally:
            import shutil
            shutil.rmtree(folder, ignore_errors=True)
        self.assertTrue(ink.outline)
        self.assertEqual(ink.outline_px, 4)
        self.assertEqual(ink.outline_rgb, (9, 8, 7))
        self.assertFalse(paper.outline)
