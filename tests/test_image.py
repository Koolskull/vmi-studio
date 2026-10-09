"""Image-material previews. The raster importer is not involved."""

import unittest

from vmi_studio.document import ArtFile, Node, Raster
from vmi_studio.imagebake import place_image, source_point


def _pixel(raster, x, y):
    local_x = x - raster.x
    local_y = y - raster.y
    if raster is None or local_x < 0 or local_y < 0 or local_x >= raster.w or local_y >= raster.h:
        return (0, 0, 0, 0)
    index = (local_y * raster.w + local_x) * 4
    return tuple(raster.rgba[index:index + 4])


def _image(sw, sh, rgba, corners, mask=None):
    record = {"width": sw, "height": sh, "rgba": rgba, "corners": corners}
    if mask is not None:
        record["mask"] = mask
    return record


class WarpTests(unittest.TestCase):
    def test_a_placed_rectangle_keeps_each_pixel_and_the_outside_is_clear(self):
        src = bytes((
            10, 20, 30, 255,
            40, 50, 60, 255,
            70, 80, 90, 255,
            1, 2, 3, 255,
        ))
        placed = place_image(20, 20, _image(2, 2, src, [(5, 7), (7, 7), (5, 9), (7, 9)]))
        self.assertEqual(_pixel(placed, 5, 7), (10, 20, 30, 255))
        self.assertEqual(_pixel(placed, 6, 7), (40, 50, 60, 255))
        self.assertEqual(_pixel(placed, 5, 8), (70, 80, 90, 255))
        self.assertEqual(_pixel(placed, 6, 8), (1, 2, 3, 255))
        self.assertEqual(_pixel(placed, 4, 7), (0, 0, 0, 0))
        self.assertEqual(_pixel(placed, 7, 7), (0, 0, 0, 0))
        self.assertEqual(_pixel(placed, 5, 9), (0, 0, 0, 0))
        self.assertEqual(placed.x, 5)
        self.assertEqual(placed.y, 7)
        self.assertEqual(placed.w, 2)
        self.assertEqual(placed.h, 2)

    def test_a_rectangle_that_starts_off_the_canvas_keeps_the_visible_column(self):
        src = bytes((
            10, 0, 0, 255,
            20, 0, 0, 255,
            30, 0, 0, 255,
            40, 0, 0, 255,
        ))
        placed = place_image(4, 4, _image(2, 2, src, [(-1, 0), (1, 0), (-1, 2), (1, 2)]))
        self.assertEqual((placed.x, placed.y, placed.w, placed.h), (0, 0, 1, 2))
        self.assertEqual(_pixel(placed, 0, 0), (20, 0, 0, 255))
        self.assertEqual(_pixel(placed, 0, 1), (40, 0, 0, 255))
        self.assertEqual(_pixel(placed, 1, 0), (0, 0, 0, 0))

    def test_a_mirrored_rectangle_swaps_the_source_corners(self):
        src = bytes((
            255, 0, 0, 255,
            0, 0, 255, 255,
            0, 255, 0, 255,
            255, 255, 255, 255,
        ))
        placed = place_image(12, 12, _image(2, 2, src, [(7, 5), (5, 5), (7, 7), (5, 7)]))
        self.assertEqual(_pixel(placed, 6, 5), (255, 0, 0, 255))
        self.assertEqual(_pixel(placed, 5, 5), (0, 0, 255, 255))
        self.assertEqual(_pixel(placed, 6, 6), (0, 255, 0, 255))
        self.assertEqual(_pixel(placed, 5, 6), (255, 255, 255, 255))
        self.assertEqual(_pixel(placed, 4, 5), (0, 0, 0, 0))
        self.assertEqual(_pixel(placed, 7, 5), (0, 0, 0, 0))

    def test_a_mask_cuts_the_image_before_the_warp(self):
        src = bytes((255, 255, 255, 255)) * 4
        mask = {"width": 2, "height": 2, "gray": bytes((255, 0, 128, 0))}
        placed = place_image(8, 8, _image(2, 2, src, [(1, 1), (3, 1), (1, 3), (3, 3)], mask))
        self.assertEqual(_pixel(placed, 1, 1)[3], 255)
        self.assertEqual(_pixel(placed, 2, 1)[3], 0)
        self.assertEqual(_pixel(placed, 1, 2)[3], 128)
        self.assertEqual(_pixel(placed, 2, 2)[3], 0)

    def test_the_four_corners_map_back_to_the_source_rectangle(self):
        corners = [(0, 0), (10, 0), (0, 10), (8, 12)]
        self.assertEqual(source_point(10, 10, corners, 0, 0), (0.0, 0.0))
        top = source_point(10, 10, corners, 10, 0)
        left = source_point(10, 10, corners, 0, 10)
        far = source_point(10, 10, corners, 8, 12)
        self.assertAlmostEqual(top[0], 10.0, places=6)
        self.assertAlmostEqual(top[1], 0.0, places=6)
        self.assertAlmostEqual(left[0], 0.0, places=6)
        self.assertAlmostEqual(left[1], 10.0, places=6)
        self.assertAlmostEqual(far[0], 10.0, places=6)
        self.assertAlmostEqual(far[1], 10.0, places=6)

    def test_a_skewed_quad_shows_the_image_and_leaves_the_outside_clear(self):
        corners = [(10.0, 10.0), (40.0, 14.0), (6.0, 50.0), (46.0, 42.0)]
        src = bytes((255, 0, 0, 255)) * (8 * 8)
        placed = place_image(64, 64, _image(8, 8, src, corners))
        self.assertEqual(_pixel(placed, 0, 0), (0, 0, 0, 0))
        self.assertEqual(_pixel(placed, 63, 63), (0, 0, 0, 0))
        center = _pixel(placed, 25, 29)
        self.assertEqual(center[0], 255)
        self.assertGreater(center[3], 200)
        outside = source_point(8, 8, corners, 10.5, 9.0)
        self.assertTrue(outside[1] < 0 or outside[0] < 0 or outside[0] >= 8 or outside[1] >= 8)
        self.assertEqual(_pixel(placed, 10, 9), (0, 0, 0, 0))

    def test_a_half_pixel_mirror_blends_the_two_source_pixels(self):
        src = bytes((255, 0, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255, 255, 255, 0, 255))
        corners = [(7.5, 5.0), (5.5, 5.0), (7.5, 7.0), (5.5, 7.0)]
        placed = place_image(12, 12, _image(2, 2, src, corners))
        mixed = _pixel(placed, 6, 5)
        self.assertGreater(mixed[0], 120)
        self.assertLess(mixed[0], 136)
        self.assertEqual(mixed[1], 0)
        self.assertGreater(mixed[2], 120)
        self.assertLess(mixed[2], 136)
        self.assertEqual(mixed[3], 255)
        self.assertEqual(_pixel(placed, 8, 5), (0, 0, 0, 0))

    def test_an_existing_raster_is_left_alone(self):
        from vmi_studio.imagebake import _attach

        kept = Raster(0, 0, 1, 1, bytes((1, 2, 3, 4)))
        painted = Node("n0", "Photo", "layer", raster=kept)
        empty = Node("n1", "Grid", "layer")
        art = ArtFile("shop.clip", "/tmp/shop.clip", "clip", 8, 8, [painted, empty])
        src = bytes((9, 9, 9, 255))
        rows = [
            {"name": "Photo", "folder": False, "image": _image(1, 1, src, [(0, 0), (1, 0), (0, 1), (1, 1)])},
            {"name": "Grid", "folder": False, "image": _image(1, 1, src, [(2, 2), (3, 2), (2, 3), (3, 3)])},
        ]
        count = _attach(art.layers, rows, 8, 8)
        self.assertEqual(count, 1)
        self.assertIs(painted.raster, kept)
        self.assertEqual(_pixel(empty.raster, 2, 2), (9, 9, 9, 255))
        self.assertEqual(_pixel(empty.raster, 1, 2), (0, 0, 0, 0))


if __name__ == "__main__":
    unittest.main()
