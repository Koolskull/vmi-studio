"""KOSV plate, effect stack, and blueprint round trip."""

import json
import os
import shutil
import tempfile
import unittest

from PIL import Image

from vmi_studio.document import ArtFile, Ids, Node, Raster, refresh
from vmi_studio.kosv import (
    LATER_PLATE,
    PLATE,
    PRODUCT,
    STACKS,
    apply_effects,
    clean_marks,
    effect_ids,
    make_effect,
    make_stack,
    plate_size,
)
from vmi_studio.vmib import load_blueprint, write_blueprint


def _solid(r, g, b, w=8, h=8):
    return Image.frombytes("RGBA", (w, h), bytes([r, g, b, 255]) * (w * h))


def _raster(r, g, b, w=4, h=4):
    return Raster(0, 0, w, h, bytes([r, g, b, 255]) * (w * h))


class CatalogTests(unittest.TestCase):
    def test_the_working_plate_is_1080p_and_4k_is_later(self):
        self.assertEqual(plate_size(), (1920, 1080))
        self.assertEqual(PLATE, (1920, 1080))
        self.assertEqual(plate_size(later=True), (3840, 2160))
        self.assertEqual(LATER_PLATE, (3840, 2160))

    def test_every_mandatory_effect_is_installed(self):
        self.assertEqual(effect_ids(), (
            "fisheye", "lens", "filmic-noise", "film-grain", "grain-plate",
            "texture-clip", "luma-transition", "jiggle", "vhs", "datamosh",
            "color-glow", "jpeg-glitch", "png-glitch",
        ))

    def test_factory_stacks_only_name_installed_effects(self):
        known = set(effect_ids())
        for name in ("lens", "stock", "tape", "melt", "break", "glow"):
            self.assertIn(name, STACKS)
            for effect in make_stack(name):
                self.assertIn(effect["id"], known)
                self.assertTrue(effect["enabled"])


class EffectTests(unittest.TestCase):
    def test_a_stack_is_deterministic_and_an_empty_stack_is_the_picture(self):
        image = _solid(20, 40, 200)
        same = apply_effects(image, [])
        self.assertEqual(same.tobytes(), image.tobytes())
        effect = make_effect("fisheye", {"amount": 0.6, "spread": 0.08, "seed": 3})
        first = apply_effects(image, [effect], frame=2)
        second = apply_effects(image, [effect], frame=2)
        self.assertEqual(first.tobytes(), second.tobytes())
        self.assertNotEqual(first.tobytes(), image.tobytes())

    def test_a_keyed_amount_holds_until_the_next_frame(self):
        image = _solid(255, 0, 0)
        image.paste(_solid(0, 0, 255, w=4, h=8), (4, 0))
        effect = make_effect("fisheye", {
            "spread": 0,
            "amount": {"keys": [{"frame": 0, "value": 0}, {"frame": 4, "value": 0.9}]},
        })
        early = apply_effects(image, [effect], frame=3)
        later = apply_effects(image, [effect], frame=4)
        self.assertNotEqual(early.tobytes(), later.tobytes())

    def test_a_black_and_white_matte_mixes_two_pictures(self):
        left = _solid(255, 0, 0)
        right = _solid(0, 0, 255)
        black = _solid(0, 0, 0)
        white = _solid(255, 255, 255)
        effect = make_effect("luma-transition", {"softness": 0.01})
        dark = apply_effects(left, [effect], maps={"other": right, "matte": black})
        bright = apply_effects(left, [effect], maps={"other": right, "matte": white})
        self.assertEqual(dark.getpixel((0, 0))[:3], (255, 0, 0))
        self.assertEqual(bright.getpixel((0, 0))[:3], (0, 0, 255))

    def test_a_p_frame_paints_on_the_i_it_names(self):
        red = _solid(200, 0, 0)
        blue = _solid(0, 0, 180)
        effect = make_effect("datamosh", {"gain": 1, "block": 4})
        clean = apply_effects(blue, [effect], maps={
            "frames": [red, blue],
            "marks": [
                {"type": "I"},
                {"type": "P", "origin": "a", "reference": "a", "gain": 1, "block": 4},
            ],
            "ids": ["a", "b"],
            "index": 1,
        })
        self.assertEqual(clean.getpixel((0, 0))[:3], (0, 0, 180))
        smeared = apply_effects(blue, [effect], maps={
            "frames": [red, blue],
            "marks": [
                {"type": "I"},
                {"type": "P", "origin": "a", "reference": "b", "gain": 1, "block": 4},
            ],
            "ids": ["a", "b"],
            "index": 1,
        })
        self.assertNotEqual(smeared.tobytes(), blue.tobytes())
        held = apply_effects(blue, [effect], maps={
            "frames": [red, blue],
            "marks": [
                {"type": "I"},
                {"type": "P", "origin": "a", "reference": "a", "gain": 0, "block": 4},
            ],
            "ids": ["a", "b"],
            "index": 1,
        })
        self.assertEqual(held.getpixel((0, 0))[:3], (200, 0, 0))

    def test_jpeg_and_png_glitches_follow_the_seed(self):
        image = _solid(10, 20, 30, w=16, h=16)
        jpeg = make_effect("jpeg-glitch", {"quality": 8, "block": 4, "order": 1, "seed": 9})
        png = make_effect("png-glitch", {"filter": 1, "rows": 4, "seed": 2})
        first = apply_effects(image, [jpeg, png], frame=1)
        second = apply_effects(image, [jpeg, png], frame=1)
        self.assertEqual(first.tobytes(), second.tobytes())
        self.assertNotEqual(first.tobytes(), image.tobytes())

    def test_the_composite_paints_an_effect_and_leaves_the_raster_alone(self):
        from vmi_studio.composite import composite_image

        raster = _raster(0, 180, 40)
        bare = Node("n0", "paint", "layer", raster=raster)
        plain = composite_image(4, 4, refresh([bare]))
        effected = Node("n1", "paint", "layer", raster=_raster(0, 180, 40))
        effected.effects = [make_effect("filmic-noise", {"amount": 1, "chroma": 1, "size": 1, "seed": 4})]
        painted = composite_image(4, 4, refresh([effected]))
        self.assertIsNotNone(plain)
        self.assertIsNotNone(painted)
        self.assertNotEqual(painted.tobytes(), plain.tobytes())
        self.assertEqual(effected.raster.rgba, raster.rgba)

    def test_a_cleaned_mark_does_not_freeze_the_effect_gain(self):
        omitted = clean_marks({"b": {"type": "P", "origin": "a", "reference": "a"}})
        self.assertEqual(omitted["b"]["type"], "P")
        self.assertEqual(omitted["b"]["origin"], "a")
        self.assertNotIn("gain", omitted["b"])
        self.assertNotIn("block", omitted["b"])
        kept = clean_marks({"b": {"type": "P", "origin": "a", "reference": "a", "gain": 1.5, "block": 16}})
        self.assertEqual(kept["b"]["gain"], 1.5)
        self.assertEqual(kept["b"]["block"], 16)
        red = _solid(200, 0, 0)
        blue = _solid(0, 0, 180)
        effect = make_effect("datamosh", {
            "block": 4,
            "gain": {"keys": [{"frame": 0, "value": 0}, {"frame": 4, "value": 1}]},
        })
        maps = {
            "frames": [red, blue],
            "marks": [{"type": "I"}, omitted["b"]],
            "ids": ["a", "b"],
            "index": 1,
        }
        held = apply_effects(blue, [effect], frame=0, maps=maps)
        clean = apply_effects(blue, [effect], frame=4, maps=maps)
        self.assertEqual(held.getpixel((0, 0))[:3], (200, 0, 0))
        self.assertEqual(clean.getpixel((0, 0))[:3], (0, 0, 180))

    def test_an_i_frame_keeps_the_effect_ahead_of_it(self):
        image = _solid(255, 0, 0)
        image.paste(_solid(0, 0, 255, w=4, h=8), (4, 0))
        other = _solid(0, 180, 0)
        fish = make_effect("fisheye", {"spread": 0, "amount": 0.9})
        plain = apply_effects(image, [fish])
        stacked = apply_effects(image, [fish, make_effect("datamosh", {"gain": 1, "block": 4})], maps={
            "frames": [image, other],
            "marks": [{"type": "I"}, {"type": "I"}],
            "ids": ["a", "b"],
            "index": 0,
        })
        self.assertEqual(stacked.tobytes(), plain.tobytes())

    def test_the_playhead_cel_is_the_datamosh_index(self):
        from vmi_studio.composite import composite_scene, reset_plate_cache

        red = Node("a", "a", "layer", raster=_raster(200, 0, 0))
        blue = Node("b", "b", "layer", raster=_raster(0, 0, 180))
        folder = Node("f", "shot", "group", animation=True)
        folder.children = [red, blue]
        folder.timeline = [red.id, blue.id]
        folder.frames = [0, 10]
        folder.show_frame = 10
        folder.effects = [make_effect("datamosh", {"gain": 0, "block": 4})]
        folder.cel_marks = {
            red.id: {"type": "I"},
            blue.id: {"type": "P", "origin": red.id, "reference": red.id},
        }
        reset_plate_cache()
        picture = composite_scene(4, 4, refresh([folder]), frame=0)
        self.assertEqual(picture.getpixel((0, 0))[:3], (200, 0, 0))
        self.assertEqual(blue.raster.rgba[:4], bytes([0, 0, 180, 255]))

    def test_a_folder_effect_paints_and_folder_cels_can_mosh(self):
        from vmi_studio.composite import composite_image, composite_scene, reset_plate_cache

        layer = Node("n", "paint", "layer", raster=_raster(0, 180, 40))
        group = Node("g", "folder", "group")
        group.children = [layer]
        group.effects = [make_effect("filmic-noise", {"amount": 1, "chroma": 1, "size": 1, "seed": 4})]
        bare_layer = Node("n2", "paint", "layer", raster=_raster(0, 180, 40))
        bare = Node("g2", "folder", "group")
        bare.children = [bare_layer]
        reset_plate_cache()
        plain = composite_image(4, 4, refresh([bare]))
        painted = composite_image(4, 4, refresh([group]))
        self.assertNotEqual(painted.tobytes(), plain.tobytes())
        self.assertEqual(layer.raster.rgba, bare_layer.raster.rgba)

        red_layer = Node("rl", "red", "layer", raster=_raster(200, 0, 0))
        blue_layer = Node("bl", "blue", "layer", raster=_raster(0, 0, 180))
        cel_a = Node("ca", "ca", "group")
        cel_a.children = [red_layer]
        cel_b = Node("cb", "cb", "group")
        cel_b.children = [blue_layer]
        shot = Node("shot", "shot", "group", animation=True)
        shot.children = [cel_a, cel_b]
        shot.timeline = [cel_a.id, cel_b.id]
        shot.frames = [0, 1]
        shot.show_frame = 1
        shot.effects = [make_effect("datamosh", {"gain": 0, "block": 4})]
        shot.cel_marks = {
            cel_a.id: {"type": "I"},
            cel_b.id: {"type": "P", "origin": cel_a.id, "reference": cel_a.id},
        }
        reset_plate_cache()
        melted = composite_scene(4, 4, refresh([shot]), frame=3)
        self.assertEqual(melted.getpixel((0, 0))[:3], (200, 0, 0))

    def test_a_keyed_child_effect_does_not_reuse_another_frames_plate(self):
        from vmi_studio.composite import composite_image, reset_plate_cache

        def picture(frame):
            image = _solid(255, 0, 0, w=8, h=8)
            image.paste(_solid(0, 0, 255, w=4, h=8), (4, 0))
            return image

        layer = Node("n", "paint", "layer", raster=Raster(0, 0, 8, 8, picture(0).tobytes()))
        layer.effects = [make_effect("fisheye", {
            "spread": 0,
            "amount": {"keys": [{"frame": 0, "value": 0}, {"frame": 4, "value": 0.9}]},
        })]
        group = Node("g", "folder", "group")
        group.children = [layer]
        reset_plate_cache()
        early = composite_image(8, 8, refresh([group]), frame=0)
        later = composite_image(8, 8, refresh([group]), frame=4)
        again = composite_image(8, 8, refresh([group]), frame=0)
        self.assertNotEqual(early.tobytes(), later.tobytes())
        self.assertEqual(again.tobytes(), early.tobytes())


class HistoryTests(unittest.TestCase):
    def test_undo_puts_the_effect_stack_back(self):
        from vmi_studio.history import apply_snap, capture

        layer = Node("n0", "paint", "layer", raster=_raster(1, 2, 3))
        art = ArtFile("plain.vmib", "", "vmib", 4, 4, refresh([layer]), "")

        class Desk:
            pass

        desk = Desk()
        desk.art = art
        desk.objects = []
        desk.playlist = ""
        before = capture(desk)
        layer.effects = make_stack("glow")
        layer.cel_marks = {"n0": {"type": "P", "reference": "n0", "origin": "", "gain": 1, "block": 8}}
        apply_snap(desk, before)
        self.assertEqual(layer.effects, [])
        self.assertEqual(layer.cel_marks, {})


class BlueprintTests(unittest.TestCase):
    def test_effects_marks_and_presets_round_trip_in_the_vmib(self):
        ids = Ids()
        first = Node(ids.next(), "hit", "layer", raster=_raster(255, 0, 0))
        second = Node(ids.next(), "smear", "layer", raster=_raster(0, 0, 255))
        folder = Node(ids.next(), "shot", "group", animation=True)
        folder.children = [first, second]
        folder.timeline = [first.id, second.id]
        folder.effects = [make_effect("vhs", {"tear": 0.2, "seed": 7}, preset="tape")]
        folder.cel_marks = {
            first.id: {"type": "I", "reference": "", "gain": 1, "block": 8},
            second.id: {"type": "P", "reference": first.id, "gain": 1.5, "block": 16},
        }
        art = ArtFile("shot.vmib", "", "vmib", 1920, 1080, refresh([folder]), "")
        art.kosv = {
            "presets": [{"name": "tape", "stack": make_stack("tape")}],
        }
        folder_path = tempfile.mkdtemp()
        path = os.path.join(folder_path, "shot.vmib")
        try:
            write_blueprint(path, art, [])
            loaded = load_blueprint(path)
        finally:
            shutil.rmtree(folder_path)
        self.assertEqual(loaded.kosv["product"], PRODUCT)
        self.assertEqual(loaded.kosv["plate"], [1920, 1080])
        self.assertEqual(loaded.kosv["presets"][0]["name"], "tape")
        self.assertEqual(loaded.kosv["presets"][0]["stack"][0]["id"], "vhs")
        shot = loaded.layers[0]
        self.assertEqual(shot.effects[0]["id"], "vhs")
        self.assertEqual(shot.effects[0]["preset"], "tape")
        self.assertEqual(shot.cel_marks[shot.children[1].id]["type"], "P")
        self.assertEqual(shot.cel_marks[shot.children[1].id]["reference"], shot.children[0].id)
        self.assertEqual(shot.cel_marks[shot.children[0].id]["type"], "I")

    def test_a_blueprint_without_kosv_still_opens(self):
        art = ArtFile("plain.clip", "/tmp/plain.clip", "clip", 4, 4, refresh([
            Node("n0", "ink", "layer", raster=_raster(1, 2, 3)),
        ]), "")
        folder_path = tempfile.mkdtemp()
        path = os.path.join(folder_path, "plain.vmib")
        try:
            write_blueprint(path, art, [])
            loaded = load_blueprint(path)
        finally:
            shutil.rmtree(folder_path)
        self.assertEqual(loaded.layers[0].effects, [])
        self.assertEqual(loaded.layers[0].cel_marks, {})
        self.assertEqual(loaded.kosv["presets"], [])
        self.assertEqual(loaded.kosv["product"], PRODUCT)


class ExportTests(unittest.TestCase):
    def test_scene_export_copies_presets_effects_and_marks(self):
        from vmi_studio.desk import DeskObject
        from vmi_studio.sceneio import write_scene

        ids = Ids()
        first = Node(ids.next(), "hit", "layer", raster=_raster(255, 0, 0))
        second = Node(ids.next(), "smear", "layer", raster=_raster(0, 0, 255))
        folder = Node(ids.next(), "shot", "group", animation=True)
        folder.children = [first, second]
        folder.effects = make_stack("glow")
        folder.cel_marks = {
            second.id: {
                "type": "P",
                "origin": first.id,
                "reference": first.id,
                "gain": 1.2,
                "block": 16,
            },
        }
        art = ArtFile("shot.vmib", "/tmp/shot.vmib", "vmib", 4, 4, refresh([folder]), "")
        art.kosv = {"presets": [{"name": "glow", "stack": make_stack("glow")}]}
        obj = DeskObject("o1", "shot", [first.id, second.id], origin="anim:shot")
        parent = tempfile.mkdtemp(prefix="vmi-kosv-export-")
        try:
            result = write_scene(parent, art, [obj])
            with open(os.path.join(result["root"], "scene.json"), encoding="utf-8") as handle:
                scene = json.load(handle)
            with open(os.path.join(result["root"], "objects", "shot", "object.json"), encoding="utf-8") as handle:
                saved = json.load(handle)
        finally:
            shutil.rmtree(parent, ignore_errors=True)
        self.assertEqual(scene["naming"], "SSSXXX")
        self.assertEqual(scene["kosv"]["product"], PRODUCT)
        self.assertEqual(scene["kosv"]["presets"], saved["presets"])
        self.assertEqual(saved["presets"][0]["name"], "glow")
        self.assertEqual(saved["effects"][0]["id"], folder.id)
        self.assertEqual(saved["effects"][0]["stack"][0]["id"], "color-glow")
        self.assertEqual(scene["objects"][0]["effects"], saved["effects"])
        mark = saved["celMarks"][folder.id][second.id]
        self.assertEqual(mark["type"], "P")
        self.assertEqual(mark["origin"], first.id)
        self.assertEqual(mark["reference"], first.id)
        self.assertEqual(scene["objects"][0]["celMarks"], saved["celMarks"])

    def test_a_picture_without_effects_omits_the_kosv_block(self):
        from vmi_studio.desk import DeskObject
        from vmi_studio.sceneio import write_scene

        ink = Node("n0", "ink", "layer", raster=_raster(1, 2, 3))
        art = ArtFile("plain.clip", "/tmp/plain.clip", "clip", 4, 4, refresh([ink]), "")
        parent = tempfile.mkdtemp(prefix="vmi-kosv-plain-")
        try:
            result = write_scene(parent, art, [DeskObject("o1", "ink", [ink.id])])
            with open(os.path.join(result["root"], "scene.json"), encoding="utf-8") as handle:
                scene = json.load(handle)
            with open(os.path.join(result["root"], "objects", "ink", "object.json"), encoding="utf-8") as handle:
                saved = json.load(handle)
        finally:
            shutil.rmtree(parent, ignore_errors=True)
        self.assertNotIn("kosv", scene)
        self.assertNotIn("presets", saved)
        self.assertNotIn("effects", saved)
        self.assertNotIn("celMarks", saved)


if __name__ == "__main__":
    unittest.main()
