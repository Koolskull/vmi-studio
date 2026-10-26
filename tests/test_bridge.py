"""The Rust desk reads leaf rasters from this bridge."""

import json
import os
import tempfile
import unittest

from vmi_studio.bridge import dump_art
from vmi_studio.document import ArtFile, Node, Raster


class BridgeTests(unittest.TestCase):
    def test_a_leaf_raster_is_written_beside_its_stroke(self):
        raster = Raster(1, 0, 1, 1, bytes([9, 0, 0, 255]))
        layer = Node("n1", "Ink", "layer", raster=raster)
        layer.strokes = [{
            "color": "#FF3EB8",
            "size": 8,
            "softness": 0,
            "antialias": 0.5,
            "points": [[1, 0, 1], [2, 0, 0.5]],
            "grain": "hatch",
            "seed": 9,
        }]
        group = Node("n0", "Folder", "group")
        group.children.append(layer)
        art = ArtFile("leaf", "leaf.clip", "clip", 2, 1, [group])
        with tempfile.TemporaryDirectory() as root:
            manifest = os.path.join(root, "manifest.json")
            rasters = os.path.join(root, "rasters")
            dump_art(art, manifest, rasters)
            with open(manifest, encoding="utf-8") as handle:
                document = json.load(handle)
            leaf = document["layers"][0]["children"][0]
            self.assertEqual(leaf["raster"]["file"], "n1.rgba")
            self.assertEqual(leaf["strokes"][0]["seed"], 9)
            self.assertTrue(os.path.isabs(document["rasters_dir"]))
            with open(os.path.join(rasters, "n1.rgba"), "rb") as handle:
                blob = handle.read()
            self.assertEqual(blob, bytes([9, 0, 0, 255]))

    def test_a_missing_file_is_an_error_without_a_manifest(self):
        from vmi_studio.bridge import main

        with tempfile.TemporaryDirectory() as root:
            code = main([
                "open",
                os.path.join(root, "missing.clip"),
                "--manifest",
                os.path.join(root, "manifest.json"),
                "--rasters",
                os.path.join(root, "rasters"),
            ])
            self.assertEqual(code, 1)
            self.assertFalse(os.path.exists(os.path.join(root, "manifest.json")))


if __name__ == "__main__":
    unittest.main()
