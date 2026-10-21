"""Onion skin tints the picture. It does not recolor the stored cels."""

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtWidgets import QApplication

from vmi_studio.composite import composite_scene
from vmi_studio.document import Node, Raster, refresh
from vmi_studio.onion import apply_onion, neighbors, next_frame_slot
from vmi_studio.toggles import toggle_icon


def _raster(width, height, color, x=None, y=None):
    rgba = bytearray(width * height * 4)
    if x is None:
        for index in range(0, len(rgba), 4):
            rgba[index:index + 4] = bytes(color)
    else:
        index = (int(y) * width + int(x)) * 4
        rgba[index:index + 4] = bytes(color)
    return Raster(0, 0, width, height, bytes(rgba))


def _icon_alpha(icon):
    image = icon.pixmap(16, 16).toImage()
    px = max(1, int(round(image.width() * 3 / 16)))
    py = max(1, int(round(image.height() * 3 / 16)))
    return image.pixelColor(px, py).alpha()


class OnionTests(unittest.TestCase):
    def test_a_new_frame_keeps_the_current_drawing_in_the_past(self):
        target, frames = next_frame_slot([0], 0)
        self.assertEqual(target, 1)
        self.assertEqual(frames, [0])
        target, frames = next_frame_slot([0, 1], 0)
        self.assertEqual(target, 1)
        self.assertEqual(frames, [0, 2])
        target, frames = next_frame_slot([0, 8], 0)
        self.assertEqual((target, frames), (1, [0, 8]))
        target, frames = next_frame_slot([0], 3)
        self.assertEqual((target, frames), (3, [0]))

    def test_a_hold_is_the_current_cel(self):
        first = Node("a", "1", "layer")
        later = Node("b", "2", "layer")
        folder = Node("folder", "Shot", "group", animation=True)
        folder.children = [first, later]
        folder.timeline = [first.id, later.id]
        folder.frames = [0, 4]
        past, future, current = neighbors(folder, 2)
        self.assertEqual(current.id, first.id)
        self.assertEqual(past, [])
        self.assertEqual([cel.id for cel in future], [later.id])

    def test_past_is_purple_and_future_is_green(self):
        paper = Node("paper", "Paper", "layer", raster=_raster(8, 8, (255, 255, 255, 255)))
        past = Node("past", "1", "layer", raster=_raster(8, 8, (255, 0, 0, 255), 1, 1))
        current = Node("now", "2", "layer", raster=_raster(8, 8, (255, 0, 0, 255), 3, 3))
        future = Node("next", "3", "layer", raster=_raster(8, 8, (0, 0, 255, 255), 5, 5))
        folder = Node("anim", "Animation", "group", animation=True)
        folder.children = [past, current, future]
        folder.timeline = [past.id, current.id, future.id]
        folder.frames = [0, 1, 2]
        folder.show_frame = 1
        layers = refresh([paper, folder])
        scene = composite_scene(8, 8, layers, current.id)
        self.assertEqual(scene.getpixel((1, 1)), (255, 255, 255, 255))
        self.assertEqual(scene.getpixel((3, 3)), (255, 0, 0, 255))
        skinned = apply_onion(scene, 8, 8, layers, 1, None)
        self.assertEqual(scene.getpixel((1, 1)), (255, 255, 255, 255))
        self.assertIsNot(skinned, scene)
        purple = skinned.getpixel((1, 1))
        green = skinned.getpixel((5, 5))
        self.assertGreater(purple[2], purple[0])
        self.assertGreater(purple[0], purple[1])
        self.assertGreater(green[1], green[0])
        self.assertGreater(green[1], green[2])
        self.assertEqual(skinned.getpixel((3, 3)), (255, 0, 0, 255))
        self.assertEqual(skinned.getpixel((0, 0)), (255, 255, 255, 255))
        self.assertEqual(tuple(past.raster.rgba[36:40]), (255, 0, 0, 255))

    def test_toggle_icons_differ_when_on_and_off(self):
        QApplication.instance() or QApplication([])
        self.assertGreater(_icon_alpha(toggle_icon(True, "#ffffff")), 200)
        self.assertLess(_icon_alpha(toggle_icon(False, "#ffffff")), 20)
