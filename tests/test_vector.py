"""Vector previews. The raster importer is not involved."""

import os
import unittest

from vmi_studio.document import ArtFile, Node, Raster
from vmi_studio.vectorbake import apply_vector_previews, bake_preview, fringe_px, sidecar_path

_ARROW = os.path.join(
    os.path.expanduser("~"), "Documents", "work", "CSP", "BG", "arrow UP.clip",
)


def _stroke(curve, points, radius=6, anti_alias=0, hardness=1, opacity=1, color=(0, 0, 0), closed=False, composite=None):
    stroke = {
        "curve": curve,
        "closed": closed,
        "brush_radius": radius,
        "opacity": opacity,
        "color": list(color),
        "anti_alias": anti_alias,
        "hardness": hardness,
        "points": points,
    }
    if composite is not None:
        stroke["composite"] = composite
    return stroke


def _point(x, y, width=1, opacity=1, controls=None, scale=None):
    point = {
        "x": x,
        "y": y,
        "width_factor": width,
        "opacity_factor": opacity,
        "controls": controls or [],
    }
    if scale is not None:
        point["width_scale"] = scale
    return point


def _rgba(baked, x, y):
    origin_x, origin_y, w, h, rgba, _approx = baked
    local_x = x - origin_x
    local_y = y - origin_y
    if local_x < 0 or local_y < 0 or local_x >= w or local_y >= h:
        return (0, 0, 0, 0)
    index = (local_y * w + local_x) * 4
    return tuple(rgba[index:index + 4])


def _alpha(baked, x, y):
    return _rgba(baked, x, y)[3]


def _pixel(raster, x, y):
    local_x = x - raster.x
    local_y = y - raster.y
    index = (local_y * raster.w + local_x) * 4
    return raster.rgba[index:index + 4]


def _named(nodes, name):
    for node in nodes:
        if node.name == name and node.kind == "layer":
            return node
        found = _named(node.children, name)
        if found is not None:
            return found
    return None


def _partials(baked):
    _x, _y, _w, _h, rgba, _approx = baked
    return sum(1 for index in range(3, len(rgba), 4) if 0 < rgba[index] < 255)


class BakeTests(unittest.TestCase):
    def test_none_is_a_hard_round_stamp(self):
        baked = bake_preview(24, 16, [_stroke("straight", [_point(4, 8), _point(18, 8)], radius=6)])
        self.assertIsNotNone(baked)
        # Visible diameter is brush_radius * 2 * width_factor. The endpoint stamp
        # half-width is 6, so a pixel 6px out is covered and a pixel 7px out is not.
        self.assertEqual(_alpha(baked, 4, 8), 255)
        self.assertEqual(_alpha(baked, 4, 14), 255)
        self.assertEqual(_alpha(baked, 4, 2), 255)
        self.assertEqual(_alpha(baked, 4, 15), 0)
        self.assertEqual(_partials(baked), 0)
        self.assertFalse(baked[5])

    def test_taper_follows_the_width_factor(self):
        baked = bake_preview(
            28,
            20,
            [_stroke("straight", [_point(4, 10, width=1), _point(22, 10, width=0.05)], radius=8)],
        )
        # Thick end half-width is 8. An average of the two factors would miss y=17.
        self.assertGreater(_alpha(baked, 5, 17), 0)
        # Thin end half-width is 0.4. An average would still cover y=13.
        self.assertEqual(_alpha(baked, 21, 13), 0)

    def test_width_scale_thins_the_ends_of_one_stroke(self):
        baked = bake_preview(
            48,
            28,
            [_stroke("straight", [
                _point(6, 14, scale=0),
                _point(24, 14, scale=1),
                _point(42, 14, scale=0),
            ], radius=6)],
        )
        self.assertGreater(_alpha(baked, 24, 19), 0)
        self.assertEqual(_alpha(baked, 6, 16), 0)
        self.assertEqual(_alpha(baked, 42, 16), 0)

    def test_quadratic_and_cubic_follow_the_controls(self):
        quadratic = bake_preview(
            24,
            16,
            [_stroke("quadratic", [_point(2, 10, controls=[[10, 0]]), _point(18, 10)], radius=2)],
        )
        self.assertGreater(_alpha(quadratic, 10, 5), 0)
        self.assertEqual(_alpha(quadratic, 10, 10), 0)
        cubic = bake_preview(
            24,
            16,
            [_stroke(
                "cubic",
                [_point(2, 10, controls=[[6, 0], [14, 0]]), _point(18, 10)],
                radius=2,
            )],
        )
        self.assertGreater(_alpha(cubic, 10, 3), 0)
        self.assertEqual(_alpha(cubic, 10, 10), 0)

    def test_a_spline_is_drawn_and_marked(self):
        baked = bake_preview(
            24,
            16,
            [_stroke("spline", [_point(2, 8), _point(8, 8), _point(12, 3), _point(20, 8)], radius=4)],
        )
        self.assertIsNotNone(baked)
        self.assertTrue(baked[5])
        self.assertGreater(sum(baked[4][3::4]), 0)

    def test_unmapped_alias_is_soft_and_hardness_tightens_it(self):
        self.assertEqual(fringe_px(0, False), 0.0)
        self.assertEqual(fringe_px(2, False), 2.0)
        self.assertEqual(fringe_px(2, True), 0.0)
        soft = bake_preview(
            24,
            16,
            [_stroke("straight", [_point(4, 8), _point(18, 8)], radius=6, anti_alias=2, hardness=0)],
        )
        tight = bake_preview(
            24,
            16,
            [_stroke("straight", [_point(4, 8), _point(18, 8)], radius=6, anti_alias=2, hardness=1)],
        )
        mono = bake_preview(
            24,
            16,
            [_stroke("straight", [_point(4, 8), _point(18, 8)], radius=6, anti_alias=2)],
            monochrome=True,
        )
        self.assertGreater(_partials(soft), _partials(tight))
        self.assertGreater(_partials(soft), 0)
        self.assertEqual(_partials(mono), 0)
        self.assertEqual(_alpha(mono, 10, 8), 255)

    def test_composite_27_cuts_ink_and_white_paint_stays_white(self):
        ink = _stroke("straight", [_point(4, 12), _point(28, 12)], radius=6)
        eraser = _stroke(
            "straight",
            [_point(16, 2), _point(16, 22)],
            radius=6,
            color=(255, 255, 255),
            composite=27,
        )
        baked = bake_preview(32, 24, [ink, eraser])
        self.assertEqual(_rgba(baked, 8, 12), (0, 0, 0, 255))
        self.assertEqual(_alpha(baked, 16, 12), 0)
        _x, _y, _w, _h, rgba, _approx = baked
        for index in range(0, len(rgba), 4):
            if rgba[index + 3] > 0:
                self.assertEqual(rgba[index], 0)
                self.assertEqual(rgba[index + 1], 0)
                self.assertEqual(rgba[index + 2], 0)
        # An earlier eraser does not cut ink drawn after it.
        later = bake_preview(32, 24, [eraser, ink])
        self.assertEqual(_rgba(later, 16, 12), (0, 0, 0, 255))
        white = _stroke(
            "straight",
            [_point(4, 8), _point(18, 8)],
            radius=6,
            color=(255, 255, 255),
            composite=0,
        )
        painted = bake_preview(24, 16, [white])
        self.assertEqual(_rgba(painted, 10, 8), (255, 255, 255, 255))
        half = _stroke(
            "straight",
            [_point(10, 10)],
            radius=6,
            opacity=0.5,
            color=(255, 255, 255),
            composite=27,
        )
        dotted = bake_preview(24, 24, [_stroke("straight", [_point(10, 10)], radius=6), half])
        self.assertEqual(_rgba(dotted, 10, 10)[:3], (0, 0, 0))
        self.assertAlmostEqual(_rgba(dotted, 10, 10)[3], 128, delta=1)
        covered = _stroke(
            "straight",
            [_point(10, 10)],
            radius=8,
            color=(255, 255, 255),
            composite=27,
        )
        self.assertIsNone(bake_preview(24, 24, [_stroke("straight", [_point(10, 10)], radius=4), covered]))

    def test_a_soft_eraser_cuts_the_core_without_white(self):
        ink = _stroke("straight", [_point(4, 12), _point(28, 12)], radius=6, anti_alias=2, hardness=1)
        eraser = _stroke(
            "straight",
            [_point(16, 2), _point(16, 22)],
            radius=6,
            anti_alias=2,
            hardness=1,
            color=(255, 255, 255),
            composite=27,
        )
        baked = bake_preview(32, 24, [ink, eraser])
        self.assertEqual(_alpha(baked, 16, 12), 0)
        self.assertEqual(_rgba(baked, 8, 12)[:3], (0, 0, 0))
        self.assertGreater(_alpha(baked, 8, 12), 200)
        _x, _y, _w, _h, rgba, _approx = baked
        for index in range(0, len(rgba), 4):
            if rgba[index + 3] > 0:
                self.assertLess(rgba[index], 8)

    def test_a_rejected_body_draws_nothing(self):
        self.assertIsNone(bake_preview(16, 16, [{"error": "unsupported"}]))

    def test_blank_vector_layers_gain_a_preview_and_rasters_stay(self):
        kept = Raster(1, 2, 1, 1, bytes([9, 9, 9, 255]))
        raster_layer = Node("n0", "paint", "layer", raster=kept)
        blank = Node("n1", "ink", "layer")
        rejected = Node("n2", "nope", "layer")
        art = ArtFile("a.clip", "/tmp/a.clip", "clip", 24, 16, [raster_layer, blank, rejected])
        payload_layers = [
            {"id": 1, "name": "paint", "folder": False, "vector": None, "children": []},
            {
                "id": 2,
                "name": "ink",
                "folder": False,
                "monochrome": False,
                "vector": {"strokes": [_stroke("straight", [_point(4, 8), _point(18, 8)])]},
                "children": [],
            },
            {
                "id": 3,
                "name": "nope",
                "folder": False,
                "vector": {"error": "read_vector rejected", "strokes": []},
                "children": [],
            },
        ]
        from vmi_studio.vectorbake import _attach

        count = _attach(art.layers, payload_layers, art.width, art.height, [])
        self.assertEqual(count, 1)
        self.assertIs(raster_layer.raster, kept)
        self.assertEqual(kept.rgba, bytes([9, 9, 9, 255]))
        self.assertIsNotNone(blank.raster)
        self.assertGreater(blank.raster.w, 0)
        self.assertIsNone(rejected.raster)

    def test_duplicate_names_follow_tree_order(self):
        first = Node("n0", "Layer 1 Copy 2", "layer")
        second = Node("n1", "Layer 1 Copy 2", "layer")
        art = ArtFile("b.clip", "/tmp/b.clip", "clip", 24, 16, [first, second])
        red = _stroke("straight", [_point(6, 8), _point(8, 8)], radius=4, color=(255, 0, 0))
        blue = _stroke("straight", [_point(16, 8), _point(18, 8)], radius=4, color=(0, 0, 255))
        rows = [
            {"name": "Layer 1 Copy 2", "folder": False, "vector": {"strokes": [red]}, "children": []},
            {"name": "Layer 1 Copy 2", "folder": False, "vector": {"strokes": [blue]}, "children": []},
        ]
        from vmi_studio.vectorbake import _attach

        _attach(art.layers, rows, art.width, art.height, [])
        self.assertEqual(_pixel(first.raster, 7, 8)[0], 255)
        self.assertEqual(_pixel(second.raster, 17, 8)[2], 255)

    def test_arrow_up_paints_the_blank_vector_layer(self):
        if not os.path.isfile(_ARROW) or not sidecar_path():
            self.skipTest("arrow UP.clip or the vector sidecar is not on this machine")
        from vmi_studio.openers import open_drawing

        art = open_drawing(_ARROW)
        ink = _named(art.layers, "Layer 1")
        other = _named(art.layers, "Layer 1 Copy")
        self.assertIsNotNone(ink.raster)
        self.assertGreater(ink.raster.w, 0)
        self.assertFalse(any(0 < alpha < 255 for alpha in ink.raster.rgba[3::4]))
        self.assertIsNotNone(other.raster)
        self.assertIn("vector layer", art.note)

    def test_a_missing_sidecar_leaves_the_open_path_quiet(self):
        art = ArtFile("c.clip", "/tmp/missing.clip", "clip", 8, 8, [Node("n0", "ink", "layer")])
        art.note = "Clip Studio paint is loaded."
        count = apply_vector_previews(art, "/tmp/missing-vector.clip")
        self.assertEqual(count, 0)
        self.assertIsNone(art.layers[0].raster)
        self.assertIn("Clip Studio paint is loaded.", art.note)


if __name__ == "__main__":
    unittest.main()
