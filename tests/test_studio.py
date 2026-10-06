"""Headless checks for the layer stack, the picture, and the scene folder."""

import json
import os
import struct
import tempfile
import unittest

from PIL import Image

from vmi_studio.composite import (
    blend_summary, composite_image, composite_scene, ink_count, pick_layer,
    plate_stats, reset_plate_cache, resolve_mode, visible_paint,
)
from vmi_studio.navigate import clamp_zoom, format_zoom, gesture, view_to_image, wheel_factor
from vmi_studio.desk import (
    DeskObject, apply_frames, arrange_objects, frames_of, merge_animations, paint_order,
    plan_scene, read_names, reset_object_ids, slots_from_subfolders, slots_of,
    static_asset_folder,
)
from vmi_studio.warp import WARP_PINK, mark_warps, object_role, warp_layer_kind, warp_system_folder
from vmi_studio.document import (
    ArtFile, Ids, Node, Raster, arrange, can_arrange, move_cel, move_into, new_folder,
    capture_flags, paint_layers, panel_order, preview_paint, refresh, rename_node,
    set_animation, set_mute, set_solo, set_visible, timeline_cels, walk,
)
from vmi_studio.cut import cut_folder, erase_folder, parent_of
from vmi_studio.naming import parse_sf, sanitize, slot_key
from vmi_studio.sceneio import write_scene
from vmi_studio import kra
from vmi_studio.xcf import parse

_BG = os.path.join(os.path.expanduser("~"), "Documents", "work", "CSP", "BG")
BUTTONS = os.environ.get("VMI_STUDIO_BUTTONS", os.path.join(_BG, "buttons.clip"))
GAMER = os.environ.get("VMI_STUDIO_GAMER", os.path.join(_BG, "gamer.kra"))


def pixel(r, g, b, a=255):
    return Raster(0, 0, 1, 1, bytes((r, g, b, a)))


class TreeTests(unittest.TestCase):
    def test_names_and_positions(self):
        self.assertEqual(parse_sf("door#hover@f2"), (1, 2))
        self.assertEqual(slot_key(1, 2), "001002")
        self.assertEqual(sanitize("door#hover"), "door")

    def test_hide_group_leaves_the_read_empty(self):
        reset_object_ids()
        ids = Ids()
        group = Node(ids.next(), "Exterior", "group")
        group.children = [Node(ids.next(), "door", "layer"), Node(ids.next(), "sky", "layer")]
        art = ArtFile("room.clip", "", "clip", 8, 8, refresh([group]), "")
        hidden = set_visible(art.layers, art.layers[0].id, False)
        self.assertEqual(hidden[0].omit, "hidden")
        self.assertEqual([node.omit for node in hidden[0].children], ["hidden folder", "hidden folder"])
        art.layers = hidden
        self.assertEqual(read_names(art), [])
        art.layers = set_visible(hidden, hidden[0].id, True)
        self.assertEqual(sorted(obj.name for obj in read_names(art)), ["door", "sky"])

    def test_rename_brings_a_reference_back(self):
        ids = Ids()
        art = ArtFile("room.clip", "", "clip", 8, 8, refresh([Node(ids.next(), "REF_grid", "layer")]), "")
        self.assertEqual(art.layers[0].omit, "reference name")
        nxt = rename_node(art.layers, art.layers[0].id, "grid")
        self.assertEqual(nxt[0].name, "grid")
        self.assertEqual(nxt[0].omit, "")

    def test_shift_range_is_the_rows_between(self):
        from vmi_studio.window import range_ids

        rows = ["door#hover", "door", "sky"]
        self.assertEqual(range_ids(rows, "door#hover", "sky"), rows)
        self.assertEqual(range_ids(rows, "sky", "door"), ["door", "sky"])
        self.assertEqual(range_ids(rows, None, "door"), ["door"])
        self.assertEqual(range_ids(rows, "missing", "sky"), ["sky"])

    def test_name_rules_stagger_with_the_name(self):
        from vmi_studio.window import name_rule

        sky = name_rule(40, 24, 260, 21, 500)
        door = name_rule(72, 32, 260, 43, 500)
        self.assertEqual(sky, (72, 21, 500, 21))
        self.assertEqual(door, (112, 43, 500, 43))
        self.assertLess(sky[0], door[0])
        self.assertIsNone(name_rule(40, 400, 260, 21, 80))

    def test_new_folder_wraps_the_front_layers(self):
        ids = Ids()
        layers = refresh([
            Node(ids.next(), "sky", "layer"),
            Node(ids.next(), "door", "layer"),
            Node(ids.next(), "door#hover", "layer"),
        ])
        _sky, door, hover = layers
        nxt, focus = new_folder(layers, hover.id, [door.id, hover.id])
        self.assertEqual([node.name for node in nxt], ["sky", "Folder"])
        self.assertEqual([node.name for node in nxt[1].children], ["door", "door#hover"])
        self.assertEqual(focus, nxt[1].id)

    def test_move_into_refuses_a_folder_inside_its_child(self):
        ids = Ids()
        exterior = Node(ids.next(), "Exterior", "group")
        door = Node(ids.next(), "door", "layer")
        exterior.children = [door]
        sky = Node(ids.next(), "sky", "layer")
        layers = refresh([exterior, sky])
        moved, _focus = move_into(layers, [sky.id], exterior.id)
        self.assertEqual([node.name for node in moved], ["Exterior"])
        self.assertEqual([node.name for node in moved[0].children], ["door", "sky"])
        self.assertIsNone(move_into(layers, [exterior.id], door.id))

    def test_arrange_puts_the_back_layer_in_front(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        layers = refresh([sky, door])
        self.assertFalse(can_arrange(layers, [sky.id], sky.id, "before"))
        moved = arrange(layers, [sky.id], door.id, "before")
        self.assertIsNotNone(moved)
        nxt, _focus = moved
        self.assertEqual([node.name for node in panel_order(nxt)], ["sky", "door"])
        image = composite_image(1, 1, visible_paint(nxt))
        self.assertEqual(image.getpixel((0, 0))[:3], (255, 0, 0))

    def test_arrange_keeps_a_run_together_and_refuses_a_folder_inside_itself(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer")
        door = Node(ids.next(), "door", "layer")
        hover = Node(ids.next(), "door#hover", "layer")
        layers = refresh([sky, door, hover])
        nxt, _focus = arrange(layers, [hover.id, door.id], sky.id, "after")
        self.assertEqual([node.name for node in panel_order(nxt)], ["sky", "door#hover", "door"])
        exterior = Node(ids.next(), "Exterior", "group")
        inside = Node(ids.next(), "door", "layer")
        exterior.children = [inside]
        other = Node(ids.next(), "sky", "layer")
        tree = refresh([exterior, other])
        self.assertIsNone(arrange(tree, [exterior.id], inside.id, "into"))
        self.assertIsNone(arrange(tree, [exterior.id], inside.id, "before"))
        self.assertEqual([node.name for node in tree[0].children], ["door"])
        nested, _focus = arrange(tree, [other.id], exterior.id, "into")
        self.assertEqual([node.name for node in nested], ["Exterior"])
        self.assertEqual([node.name for node in nested[0].children], ["door", "sky"])

    def test_mute_and_solo_choose_what_is_in_the_picture(self):
        ids = Ids()
        exterior = Node(ids.next(), "Exterior", "group")
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        exterior.children = [door]
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        layers = refresh([exterior, sky])
        set_mute(layers, exterior.id, True)
        self.assertTrue(door.visible)
        self.assertEqual([node.name for node in visible_paint(layers)], ["sky"])
        set_mute(layers, exterior.id, False)
        set_solo(layers, door.id, True)
        self.assertEqual([node.name for node in preview_paint(layers)], ["door"])
        set_visible(layers, door.id, False)
        self.assertEqual([node.name for node in visible_paint(layers)], [])
        set_visible(layers, door.id, True)
        set_solo(layers, door.id, False)
        self.assertEqual([node.name for node in visible_paint(layers)], ["door", "sky"])

    def test_hide_removes_paint_from_the_picture(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        layers = refresh([sky, door])
        hidden = set_visible(layers, door.id, False)
        image = composite_image(1, 1, visible_paint(hidden))
        self.assertEqual([node.name for node in visible_paint(hidden)], ["sky"])
        self.assertEqual(image.getpixel((0, 0)), (255, 0, 0, 255))

        group = Node(ids.next(), "Exterior", "group")
        group.children = [Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))]
        sky2 = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        folder = refresh([group, sky2])
        folder = set_visible(folder, group.id, False)
        self.assertEqual([node.name for node in visible_paint(folder)], ["sky"])


class ExportTests(unittest.TestCase):
    def test_one_object_two_positions(self):
        reset_object_ids()
        ids = Ids()
        door = Node(ids.next(), "door", "layer", raster=pixel(255, 0, 0))
        hover = Node(ids.next(), "door#hover", "layer", raster=pixel(0, 255, 0))
        art = ArtFile("room.clip", "/tmp/room.clip", "clip", 4, 4, refresh([door, hover]), "")
        objects = read_names(art)
        self.assertEqual([obj.name for obj in objects], ["door"])
        self.assertEqual([slot["key"] for slot in slots_of(art, objects[0])], ["000000", "001000"])
        parent = tempfile.mkdtemp(prefix="vmi-studio-")
        try:
            result = write_scene(parent, art, objects)
            self.assertTrue(os.path.isfile(os.path.join(result["root"], "HOW.txt")))
            self.assertTrue(os.path.isfile(os.path.join(result["root"], "scene.json")))
            self.assertTrue(os.path.isfile(os.path.join(result["root"], "layers.tsx")))
            folder = os.path.join(result["root"], "objects", "door")
            self.assertTrue(os.path.isfile(os.path.join(folder, "000000.PNG")))
            self.assertTrue(os.path.isfile(os.path.join(folder, "001000.PNG")))
            self.assertTrue(os.path.isfile(os.path.join(folder, "object.json")))
            rest = Image.open(os.path.join(folder, "000000.PNG"))
            self.assertEqual(rest.size, (4, 4))
            self.assertEqual(rest.getpixel((0, 0)), (255, 0, 0, 255))
        finally:
            import shutil
            shutil.rmtree(parent, ignore_errors=True)


class XcfTests(unittest.TestCase):
    def test_uncompressed_tile(self):
        art = parse(_xcf_red(), "dot.xcf")
        self.assertEqual(art.width, 2)
        self.assertEqual(art.layers[0].name, "red")
        self.assertEqual(art.layers[0].raster.rgba[:4], b"\xff\x00\x00\xff")
        image = composite_image(art.width, art.height, visible_paint(art.layers))
        self.assertGreater(ink_count(image), 0)

    def test_version_11_pointer_width(self):
        art = parse(_xcf_red(version=11), "dot.xcf")
        self.assertEqual(art.layers[0].name, "red")
        self.assertEqual(art.layers[0].raster.rgba[0], 255)


class ClipTests(unittest.TestCase):
    def test_buttons_clip_has_layer_paint(self):
        if not os.path.isfile(BUTTONS):
            self.skipTest("buttons.clip is not on this machine")
        from vmi_studio.openers import open_drawing

        art = open_drawing(BUTTONS)
        names = [node.name for node in walk(art.layers)]
        self.assertIn("UI Icon", names, names)
        image = composite_image(art.width, art.height, visible_paint(art.layers))
        self.assertIsNotNone(image)
        self.assertGreater(ink_count(image), 0)
        before = image.tobytes()
        changed = False
        for layer in paint_layers(art.layers):
            if layer.raster is None:
                continue
            layer.visible = False
            refresh(art.layers)
            nxt = composite_image(art.width, art.height, visible_paint(art.layers))
            layer.visible = True
            refresh(art.layers)
            after = nxt.tobytes() if nxt is not None else b""
            if after != before:
                changed = True
                break
        self.assertTrue(changed, "hiding a layer did not change the picture")


class KraTests(unittest.TestCase):
    def test_cards_has_clear_and_ink(self):
        if not os.path.isfile(GAMER):
            self.skipTest("gamer.kra is not on this machine")
        art = kra.load(GAMER, only="Cards")
        group = next(node for node in art.layers if node.name == "Group 2")
        self.assertEqual(group.kind, "group")
        cards = next(node for node in group.children if node.name == "Cards")
        self.assertIsNotNone(cards.raster)
        image = Image.frombytes("RGBA", (cards.raster.w, cards.raster.h), cards.raster.rgba)
        hist = image.getchannel("A").histogram()
        self.assertGreater(hist[0], 0)
        self.assertGreater(sum(hist[1:]), 0)

    def test_empty_tiles_use_the_default_pixel(self):
        if not os.path.isfile(GAMER):
            self.skipTest("gamer.kra is not on this machine")
        art = kra.load(GAMER, only="Background")
        background = next(node for node in walk(art.layers) if node.name == "Background")
        self.assertIsNotNone(background.raster)
        self.assertEqual(background.raster.w, art.width)
        self.assertEqual(background.raster.rgba[:4], b"\xff\xff\xff\xff")


def solid(r, g, b, x, y, w=2, h=2):
    return Raster(x, y, w, h, bytes((r, g, b, 255)) * (w * h))


class AnimationTests(unittest.TestCase):
    def _art(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(0, 0, 80))
        ink = Node(ids.next(), "ink", "layer", raster=pixel(255, 0, 0))
        tone = Node(ids.next(), "tone", "layer", raster=solid(0, 255, 0, 4, 5))
        shade = Node(ids.next(), "shade", "group")
        shade.children = [tone]
        line = Node(ids.next(), "line", "group")
        line.children = [ink, shade]
        fill = Node(ids.next(), "fill", "layer", raster=solid(0, 0, 255, 4, 5))
        color = Node(ids.next(), "color", "group")
        color.children = [fill]
        beetle = Node(ids.next(), "beetle", "group")
        beetle.children = [line, color]
        layers = refresh([sky, beetle])
        return ArtFile("beetle.clip", "", "clip", 30, 16, layers, ""), beetle

    def test_a_frame_holds_its_subfolders_and_the_timeline_can_move(self):
        reset_object_ids()
        art, beetle = self._art()
        set_animation(art.layers, beetle.id, True)
        self.assertEqual([cel.name for cel in timeline_cels(beetle)], ["color", "line"])
        stored = [cel.name for cel in beetle.children]
        color = next(cel for cel in beetle.children if cel.name == "color")
        self.assertTrue(move_cel(beetle, color.id, 1))
        self.assertEqual([cel.name for cel in beetle.children], stored)
        self.assertEqual([cel.name for cel in timeline_cels(beetle)], ["line", "color"])
        objects = read_names(art)
        sprite = next(obj for obj in objects if obj.name == "beetle")
        self.assertEqual([obj.name for obj in objects], ["sky", "beetle"])
        slots = slots_of(art, sprite)
        self.assertEqual([slot["key"] for slot in slots], ["000000", "000001"])
        self.assertEqual([layer.name for layer in slots[0]["layers"]], ["ink", "tone"])
        self.assertEqual([layer.name for layer in slots[1]["layers"]], ["fill"])
        parent = tempfile.mkdtemp(prefix="vmi-studio-ani-")
        try:
            result = write_scene(parent, art, [sprite])
            folder = os.path.join(result["root"], "objects", "beetle")
            frame = Image.open(os.path.join(folder, "000001.PNG"))
            self.assertEqual(frame.size, (30, 16))
            self.assertEqual(frame.getpixel((4, 5)), (0, 0, 255, 255))
            self.assertEqual(frame.getpixel((0, 0)), (0, 0, 0, 0))
            other = Image.open(os.path.join(folder, "000000.PNG"))
            self.assertEqual(other.getpixel((0, 0)), (255, 0, 0, 255))
            self.assertEqual(other.getpixel((4, 5)), (0, 255, 0, 255))
        finally:
            import shutil
            shutil.rmtree(parent, ignore_errors=True)

    def test_the_picture_shows_one_frame_and_a_nested_folder_is_not_its_own_animation(self):
        art, beetle = self._art()
        line = next(node for node in beetle.children if node.name == "line")
        set_animation(art.layers, beetle.id, True)
        self.assertIs(set_animation(art.layers, line.id, True), beetle)
        self.assertFalse(line.animation)
        color = next(node for node in beetle.children if node.name == "color")
        shown = [node.name for node in preview_paint(art.layers, color.id)]
        self.assertEqual(shown, ["sky", "fill"])
        image = composite_image(art.width, art.height, preview_paint(art.layers, color.id))
        self.assertEqual(image.getpixel((0, 0)), (0, 0, 80, 255))
        self.assertEqual(image.getpixel((4, 5)), (0, 0, 255, 255))

    def test_a_hidden_frame_drops_out_of_the_sequence(self):
        reset_object_ids()
        art, beetle = self._art()
        set_animation(art.layers, beetle.id, True)
        color = next(node for node in beetle.children if node.name == "color")
        set_visible(art.layers, color.id, False)
        _ids, assign = frames_of(beetle)
        self.assertEqual(set(assign.values()), {(0, 0)})
        objects = merge_animations(art, [])
        self.assertEqual([slot["key"] for slot in slots_of(art, objects[0])], ["000000"])


class ObjectTests(unittest.TestCase):
    def test_button_positions_keep_a_name_the_user_typed(self):
        reset_object_ids()
        ids = Ids()
        door = Node(ids.next(), "Layer 1", "layer", raster=pixel(255, 0, 0))
        art = ArtFile("room.clip", "", "clip", 4, 4, refresh([door]), "")
        obj = DeskObject("o1", "door", [door.id], explicit=True)
        obj.labels["001000"] = "Over"
        obj.labels["000001"] = "blink"
        labels = plan_scene(art, [obj])["objects"][0]["labels"]
        self.assertEqual(labels["000000"], "rest")
        self.assertEqual(labels["001000"], "Over")
        self.assertEqual(labels["002000"], "pressed")
        self.assertEqual(labels["003000"], "after")
        self.assertEqual(labels["000001"], "blink")
        parent = tempfile.mkdtemp(prefix="vmi-studio-name-")
        try:
            result = write_scene(parent, art, [obj])
            with open(os.path.join(result["root"], "objects", "door", "object.json"), encoding="utf-8") as handle:
                saved = json.load(handle)
            self.assertEqual(saved["labels"]["001000"], "Over")
            self.assertEqual(saved["labels"]["000001"], "blink")
            self.assertEqual(saved["name"], "door")
        finally:
            import shutil
            shutil.rmtree(parent, ignore_errors=True)


class FolderMenuTests(unittest.TestCase):
    def test_a_plain_folder_is_one_static_picture(self):
        ids = Ids()
        ink = Node(ids.next(), "ink", "layer")
        folder = Node(ids.next(), "machine", "group")
        folder.children = [ink]
        refresh([folder])
        self.assertTrue(static_asset_folder(folder))
        folder.children.append(Node(ids.next(), "door#hover", "layer"))
        refresh([folder])
        self.assertFalse(static_asset_folder(folder))
        folder.animation = True
        folder.children = [ink]
        refresh([folder])
        self.assertFalse(static_asset_folder(folder))

    def test_subfolders_take_the_next_positions_and_a_named_one_keeps_its_word(self):
        ids = Ids()
        base = Node(ids.next(), "base", "layer")
        body_ink = Node(ids.next(), "paint", "layer")
        body = Node(ids.next(), "body", "group")
        body.children = [body_ink]
        glass_ink = Node(ids.next(), "line", "layer")
        glass = Node(ids.next(), "glass", "group")
        glass.children = [glass_ink]
        hover_ink = Node(ids.next(), "glow", "layer")
        hover = Node(ids.next(), "hover", "group")
        hover.children = [hover_ink]
        folder = Node(ids.next(), "machine", "group")
        # Stored back to front, so the panel top is base, then body, glass, and hover.
        folder.children = [hover, glass, body, base]
        refresh([folder])
        _layers, assign = slots_from_subfolders(folder)
        self.assertEqual(assign[base.id], (0, 0))
        self.assertEqual(assign[hover_ink.id], (1, 0))
        self.assertEqual(assign[body_ink.id], (2, 0))
        self.assertEqual(assign[glass_ink.id], (3, 0))

    def test_a_static_export_is_not_marked_as_an_animation(self):
        ids = Ids()
        ink = Node(ids.next(), "ink", "layer", raster=pixel(0, 0, 255))
        folder = Node(ids.next(), "machine", "group")
        folder.children = [ink]
        art = ArtFile("room.clip", "", "clip", 1, 1, refresh([folder]), "")
        static = DeskObject(
            "o1", "arcade", [ink.id], {ink.id: (0, 0)},
            origin="static:%s" % folder.id, explicit=True,
        )
        anim = DeskObject("o2", "wave", [ink.id], {ink.id: (0, 0)}, origin="anim:wave")
        planned = {item["name"]: item for item in plan_scene(art, [static, anim])["objects"]}
        self.assertFalse(planned["arcade"]["animation"])
        self.assertEqual(planned["arcade"]["type"], "static")
        self.assertEqual(planned["arcade"]["warpLayer"], "")
        self.assertEqual(planned["arcade"]["sound"], "")
        self.assertEqual(plan_scene(art, [static])["playlist"], "")
        self.assertTrue(planned["wave"]["animation"])

    def test_assigned_fields_override_the_derived_type(self):
        ids = Ids()
        ink = Node(ids.next(), "ink", "layer", raster=pixel(0, 0, 255))
        art = ArtFile("room.clip", "", "clip", 1, 1, refresh([ink]), "")
        obj = DeskObject(
            "o1", "arcade", [ink.id], {ink.id: (0, 0)},
            kind="warp target", warp_layer="warp map", sound="sfx:tik",
        )
        plan = plan_scene(art, [obj], playlist="battle")
        row = plan["objects"][0]
        self.assertEqual(plan["playlist"], "battle")
        self.assertEqual(row["type"], "warp target")
        self.assertEqual(row["warpLayer"], "warp map")
        self.assertEqual(row["sound"], "sfx:tik")
        obj.sound = "laser"
        obj.warp_layer = "displacement"
        junk = plan_scene(art, [obj], playlist="lofi")
        self.assertEqual(junk["playlist"], "")
        self.assertEqual(junk["objects"][0]["sound"], "")
        self.assertEqual(junk["objects"][0]["warpLayer"], "")
        kept = apply_frames(obj, [ink.id], 0, 0, "stack", [ink])
        self.assertEqual(kept.kind, "warp target")
        self.assertEqual(kept.role, obj.role)


class WarpTests(unittest.TestCase):
    def test_names_pink_polygons_and_monitor_vectors_are_warp_targets(self):
        ids = Ids()
        quad = Node(ids.next(), "glass", "layer", raster=pixel(255, 31, 217))
        sky = Node(ids.next(), "sky", "layer", raster=pixel(40, 80, 200))
        wt = Node(ids.next(), "WT-1", "layer")
        mask = Node(ids.next(), "Sceen Mask", "layer", raster=pixel(0, 0, 0))
        booth = Node(ids.next(), "DJ Booth Warp Target", "layer")
        screen = Node(ids.next(), "screenmask", "layer", raster=pixel(1, 1, 1))
        vector = Node(ids.next(), "Layer 17", "layer")
        photo = Node(ids.next(), "Photo 8", "layer")
        monitor = Node(ids.next(), "Monitor", "group")
        monitor.children = [vector, photo]
        targets = Node(ids.next(), "TV Warp Targets", "group")
        targets.children = [wt]
        teevees = Node(ids.next(), "teevees", "group")
        teevees.children = [screen, targets]
        root = refresh([quad, sky, mask, booth, monitor, teevees])
        mark_warps(root)
        by_name = {node.name: node for node in walk(root)}
        self.assertEqual(by_name["glass"].warp, "target")
        self.assertEqual(by_name["sky"].warp, "")
        self.assertEqual(warp_layer_kind([by_name["WT-1"], by_name["screenmask"]]), "warp map")
        self.assertEqual(warp_layer_kind([by_name["screenmask"]]), "mask")
        self.assertEqual(warp_layer_kind([by_name["sky"]]), "")
        self.assertEqual(by_name["WT-1"].warp, "target")
        self.assertEqual(by_name["TV Warp Targets"].warp, "target")
        self.assertEqual(by_name["DJ Booth Warp Target"].warp, "target")
        self.assertEqual(by_name["Sceen Mask"].warp, "window")
        self.assertEqual(by_name["screenmask"].warp, "window")
        self.assertEqual(by_name["Layer 17"].warp, "target")
        self.assertEqual(by_name["Photo 8"].warp, "")
        self.assertEqual(by_name["Monitor"].warp, "window")
        self.assertEqual(by_name["teevees"].warp, "")
        art = ArtFile("room.clip", "", "clip", 1, 1, root, "")
        self.assertEqual(object_role(art, DeskObject("m", "monitor", [vector.id])), "window")
        self.assertEqual(object_role(art, DeskObject("w", "wt", [wt.id])), "target")
        self.assertTrue(warp_system_folder(by_name["TV Warp Targets"]))
        self.assertTrue(warp_system_folder(by_name["Monitor"]))
        self.assertTrue(warp_system_folder(by_name["teevees"]))
        self.assertFalse(warp_system_folder(by_name["sky"]))
        red, green, blue = (int(WARP_PINK[i:i + 2], 16) for i in (1, 3, 5))
        self.assertGreater(red, green)
        self.assertGreater(blue, green)
        self.assertLess(max(red, green, blue), 120)

    def test_object_stack_is_the_z_order_and_a_child_is_inside_its_parent(self):
        ids = Ids()
        back_layer = Node(ids.next(), "wall", "layer", raster=pixel(1, 0, 0))
        front_layer = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 1))
        art = ArtFile("room.clip", "", "clip", 1, 1, refresh([back_layer, front_layer]), "")
        back = DeskObject("o1", "wall", [back_layer.id], stack=0)
        front = DeskObject("o2", "door", [front_layer.id], stack=4)
        # The layer index would put wall behind door. The outliner can reverse that.
        arrange_objects([back, front], [
            {"id": "o1", "children": [{"id": "o2", "children": []}]},
        ])
        self.assertEqual(front.parent, "o1")
        self.assertEqual(back.parent, "")
        ordered = paint_order([back, front])
        self.assertEqual([obj.name for obj in ordered], ["wall", "door"])
        planned = plan_scene(art, [back, front])["objects"]
        by_name = {item["name"]: item for item in planned}
        self.assertEqual(by_name["wall"]["zIndex"], 0)
        self.assertEqual(by_name["door"]["zIndex"], 1)
        self.assertEqual(by_name["door"]["parent"], "wall")
        self.assertEqual(by_name["wall"]["parent"], "")
        self.assertEqual(object_role(art, back), "")


class WindowTests(unittest.TestCase):
    def _desk(self, layers):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtWidgets import QApplication

        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        app = QApplication.instance() or QApplication([])
        window_mod.apply_style(app)
        win = window_mod.MainWindow()
        art = ArtFile("room.clip", "", "clip", 1, 1, layers, "")
        win.show_file(art, restore=False)
        app.processEvents()
        return app, win

    def test_the_object_outliner_puts_the_front_on_top_and_nests(self):
        ids = Ids()
        wall = Node(ids.next(), "wall", "layer", raster=pixel(1, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 1))
        _app, win = self._desk(refresh([wall, door]))
        back = DeskObject("o1", "wall", [wall.id], stack=0)
        front = DeskObject("o2", "door", [door.id], stack=3)
        win.objects = [back, front]
        win._fill_objects()
        tree = win.objects_list
        self.assertEqual(tree.topLevelItem(0).text(0), "door")
        self.assertEqual(tree.topLevelItem(1).text(0), "wall")
        item = tree.takeTopLevelItem(0)
        tree.topLevelItem(0).addChild(item)
        win._apply_object_tree()
        self.assertEqual(front.parent, "o1")
        self.assertEqual(tree.topLevelItem(0).text(0), "wall")
        self.assertEqual(tree.topLevelItem(0).child(0).text(0), "door")
        planned = plan_scene(win.art, win.objects)
        by_name = {row["name"]: row for row in planned["objects"]}
        self.assertEqual(by_name["wall"]["zIndex"], 0)
        self.assertEqual(by_name["door"]["zIndex"], 1)
        self.assertEqual(by_name["door"]["parent"], "wall")
        win.close()

    def test_a_warp_system_folder_offers_prepare_first_and_makes_an_object(self):
        from PySide6.QtGui import QColor
        from PySide6.QtWidgets import QDialog, QMenu

        import vmi_studio.window as window_mod

        ids = Ids()
        quad = Node(ids.next(), "WT-1", "layer")
        mask = Node(ids.next(), "screenmask", "layer", raster=pixel(20, 20, 20))
        folder = Node(ids.next(), "teevees", "group")
        folder.children = [quad, mask]
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        app, win = self._desk(refresh([folder, sky]))
        menu = QMenu()
        win.tree._fill_menu(menu, folder)
        labels = []
        for action in menu.actions():
            if action.isSeparator():
                continue
            widget = action.defaultWidget() if hasattr(action, "defaultWidget") else None
            labels.append(widget.text() if widget is not None else action.text())
        self.assertEqual(labels[0], window_mod.WARP_PREPARE)
        self.assertLess(labels.index(window_mod.WARP_PREPARE), labels.index("Rename"))
        button = menu.actions()[0].defaultWidget()
        button.resize(320, 32)
        button.show()
        app.processEvents()
        color = button.grab().toImage().pixelColor(6, 6)
        self.assertGreater(color.red(), color.green())
        self.assertGreater(color.red(), color.blue())
        plain = QMenu()
        win.tree._fill_menu(plain, sky)
        plain_labels = [action.text() for action in plain.actions()]
        self.assertNotIn(window_mod.WARP_PREPARE, plain_labels)

        class FakeName:
            def __init__(self, _parent, _layers, name=""):
                pass

            def exec(self):
                return QDialog.DialogCode.Accepted

            def name(self):
                return "teevee"

        real_name = window_mod.NameDialog
        window_mod.NameDialog = FakeName
        try:
            win.prepare_warp_object(folder)
        finally:
            window_mod.NameDialog = real_name
        self.assertEqual([obj.name for obj in win.objects], ["teevee"])
        made = win.objects[0]
        self.assertEqual(made.role, "target")
        self.assertEqual(made.kind, "warp target")
        self.assertEqual(made.warp_layer, "warp map")
        self.assertTrue(made.origin.startswith("warp:"))
        self.assertTrue(made.explicit)
        self.assertEqual(set(made.layer_ids), {quad.id, mask.id})
        self.assertEqual(set(made.assign.values()), {(0, 0)})
        self.assertIn("warp target", win.status.currentMessage())
        pink = QColor(window_mod.WARP_PINK)
        self.assertLess(pink.value(), 120)
        win.close()

    def test_a_static_folder_offers_green_export_actions_first(self):
        from PySide6.QtWidgets import QMenu

        import vmi_studio.window as window_mod

        ids = Ids()
        ink = Node(ids.next(), "ink", "layer", raster=pixel(0, 0, 255))
        folder = Node(ids.next(), "machine", "group")
        folder.children = [ink]
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        app, win = self._desk(refresh([folder, sky]))
        menu = QMenu()
        win.tree._fill_menu(menu, folder)
        labels = []
        for action in menu.actions():
            if action.isSeparator():
                continue
            widget = action.defaultWidget() if hasattr(action, "defaultWidget") else None
            labels.append(widget.text() if widget is not None else action.text())
        self.assertEqual(window_mod.STATIC_EXPORT, "create new object")
        self.assertEqual(labels[0], window_mod.STATIC_EXPORT)
        self.assertEqual(labels[1], window_mod.SUBFOLDER_OBJECT)
        self.assertLess(labels.index(window_mod.STATIC_EXPORT), labels.index("Rename"))
        self.assertIn("Animation", labels)

        def sample(widget):
            widget.resize(320, 32)
            widget.show()
            app.processEvents()
            color = widget.grab().toImage().pixelColor(6, 6)
            return color.red(), color.green(), color.blue()

        bright = sample(menu.actions()[0].defaultWidget())
        dark = sample(menu.actions()[1].defaultWidget())
        self.assertGreater(bright[1], bright[0])
        self.assertGreater(bright[1], bright[2])
        self.assertGreater(dark[1], dark[0])
        self.assertGreater(bright[1], dark[1])
        plain = QMenu()
        win.tree._fill_menu(plain, sky)
        plain_labels = [action.text() for action in plain.actions()]
        self.assertEqual(plain_labels[0], "Rename")
        self.assertNotIn(window_mod.STATIC_EXPORT, plain_labels)
        win.close()

    def test_export_static_writes_only_that_folder_and_subfolders_make_an_object(self):
        from PySide6.QtWidgets import QDialog
        import vmi_studio.window as window_mod

        ids = Ids()
        blue = Node(ids.next(), "ink", "layer", raster=pixel(0, 0, 255))
        body_ink = Node(ids.next(), "paint", "layer", raster=pixel(0, 255, 0))
        body = Node(ids.next(), "body", "group")
        body.children = [body_ink]
        folder = Node(ids.next(), "machine", "group")
        folder.children = [body, blue]
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        app, win = self._desk(refresh([folder, sky]))
        parent = tempfile.mkdtemp(prefix="vmi-static-")
        names = iter(["arcade", "cabinet"])

        class FakeName:
            def __init__(self, _parent, _layers, name=""):
                self._name = next(names)

            def exec(self):
                return QDialog.DialogCode.Accepted

            def name(self):
                return self._name

        real_name = window_mod.NameDialog
        real_dir = window_mod.QFileDialog.getExistingDirectory
        window_mod.NameDialog = FakeName
        window_mod.QFileDialog.getExistingDirectory = lambda *_args, **_kwargs: parent
        try:
            win.export_static_folder(folder)
            png = os.path.join(parent, "room", "objects", "arcade", "000000.PNG")
            self.assertTrue(os.path.isfile(png))
            with Image.open(png) as image:
                # Blue sits in front of the green body. The red sky is outside the folder.
                self.assertEqual(image.getpixel((0, 0))[:3], (0, 0, 255))
            with open(os.path.join(parent, "room", "objects", "arcade", "object.json"), encoding="utf-8") as handle:
                saved = json.load(handle)
            self.assertEqual(saved["type"], "static")
            self.assertFalse(saved["animation"])
            self.assertEqual([obj.name for obj in win.objects], ["arcade"])
            self.assertEqual(set(win.objects[0].assign.values()), {(0, 0)})
            win.object_from_subfolders(folder)
            made = win.objects[1]
            self.assertEqual(made.name, "cabinet")
            self.assertEqual(made.assign[blue.id], (0, 0))
            self.assertEqual(made.assign[body_ink.id], (1, 0))
            self.assertTrue(made.explicit)
            self.assertTrue(made.origin.startswith("slots:"))
            self.assertIn("001000", win.status.currentMessage())
            self.assertEqual(made.kind, "")
        finally:
            window_mod.NameDialog = real_name
            window_mod.QFileDialog.getExistingDirectory = real_dir
            import shutil
            shutil.rmtree(parent, ignore_errors=True)
        win.close()

    def test_create_new_object_lands_in_the_list_and_does_not_write(self):
        from PySide6.QtWidgets import QDialog
        import vmi_studio.window as window_mod

        ids = Ids()
        ink = Node(ids.next(), "ink", "layer", raster=pixel(0, 0, 255))
        folder = Node(ids.next(), "machine", "group")
        folder.children = [ink]
        _app, win = self._desk(refresh([folder]))
        offered = window_mod.NameDialog(win, ["ink"], "luvseat")
        self.assertEqual(offered.edit.text(), "luvseat")
        offered.close()
        pane = win.split.widget(2)
        visible = [button.text() for button in pane.findChildren(window_mod.QPushButton) if not button.isHidden()]
        self.assertNotIn("New object", visible)
        self.assertNotIn("Delete", visible)
        self.assertNotIn("Find frames", visible)
        self.assertNotIn("Assign frame", visible)
        self.assertNotIn("Earlier", visible)
        self.assertFalse(win.timeline.isVisible())
        body = win.objects_list.parentWidget()
        list_at = body.layout().indexOf(win.objects_list)
        detail_at = max(
            body.layout().indexOf(body.layout().itemAt(index).widget())
            for index in range(body.layout().count())
            if body.layout().itemAt(index).widget() is not None
            and body.layout().itemAt(index).widget() is not win.objects_list
            and body.layout().itemAt(index).widget() is not win.object_note
        )
        self.assertGreater(list_at, body.layout().indexOf(win.object_note))
        self.assertLess(list_at, detail_at)
        seen = []

        class FakeName:
            def __init__(self, _parent, _layers, name=""):
                seen.append(name)

            def exec(self):
                return QDialog.DialogCode.Accepted

            def name(self):
                return "arcade"

        def boom(*_args, **_kwargs):
            raise AssertionError("create new object must not ask for a folder")

        real_name = window_mod.NameDialog
        real_dir = window_mod.QFileDialog.getExistingDirectory
        window_mod.NameDialog = FakeName
        window_mod.QFileDialog.getExistingDirectory = boom
        try:
            win.create_folder_object(folder)
        finally:
            window_mod.NameDialog = real_name
            window_mod.QFileDialog.getExistingDirectory = real_dir
        self.assertEqual(seen, ["machine"])
        self.assertEqual([obj.name for obj in win.objects], ["arcade"])
        made = win.objects[0]
        self.assertTrue(made.explicit)
        self.assertTrue(made.origin.startswith("static:"))
        self.assertEqual(made.kind, "")
        self.assertEqual(set(made.assign.values()), {(0, 0)})
        self.assertIn("object list", win.status.currentMessage())
        self.assertEqual(win.object_name.text(), "arcade")
        self.assertEqual(win.kind_note.text(), "Exports as static.")
        win.object_kind.setCurrentIndex(win.object_kind.findData("button"))
        self.assertEqual(made.kind, "button")
        self.assertEqual(win.kind_note.text(), "Exports as button.")
        win.object_warp.setCurrentIndex(win.object_warp.findData("mask"))
        win.object_sound.setCurrentIndex(win.object_sound.findData("sfx:hit1"))
        win.playlist_box.setCurrentIndex(win.playlist_box.findData("menu"))
        self.assertEqual(made.warp_layer, "mask")
        self.assertEqual(made.sound, "sfx:hit1")
        self.assertEqual(win.playlist, "menu")
        plan = plan_scene(win.art, win.objects, win.playlist)
        self.assertEqual(plan["playlist"], "menu")
        self.assertEqual(plan["objects"][0]["type"], "button")
        self.assertEqual(plan["objects"][0]["warpLayer"], "mask")
        self.assertEqual(plan["objects"][0]["sound"], "sfx:hit1")
        win.object_kind.setCurrentIndex(win.object_kind.findData("warp target"))
        self.assertEqual(made.kind, "warp target")
        self.assertEqual(made.role, "target")
        win.object_kind.setCurrentIndex(win.object_kind.findData("static"))
        self.assertEqual(made.role, "")
        made.role = "window"
        win.object_kind.setCurrentIndex(win.object_kind.findData("warp target"))
        win.object_kind.setCurrentIndex(win.object_kind.findData("button"))
        self.assertEqual(made.role, "window")
        win.object_name.setText("Arcade")
        win._name_edited()
        self.assertEqual(made.name, "Arcade")
        other = DeskObject("o9", "door", [ink.id], explicit=True)
        win.objects.append(other)
        win.object_name.setText("door")
        win._name_edited()
        self.assertEqual(made.name, "Arcade")
        self.assertIn("already", win.status.currentMessage())
        win.close()

    def test_the_object_outliner_uses_the_same_rules_as_the_layers(self):
        ids = Ids()
        wall = Node(ids.next(), "wall", "layer", raster=pixel(1, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 1))
        app, win = self._desk(refresh([wall, door]))
        back = DeskObject("o1", "wall", [wall.id], stack=0)
        front = DeskObject("o2", "door", [door.id], stack=1, parent="o1")
        win.objects = [back, front]
        win.resize(1200, 900)
        win.show()
        app.processEvents()
        win._fill_objects()
        app.processEvents()
        tree = win.objects_list
        parent = tree.topLevelItem(0)
        child = parent.child(0)
        self.assertEqual(parent.text(0), "wall")
        self.assertEqual(child.text(0), "door")
        image = tree.viewport().grab().toImage()

        def rule_start(item):
            rect = tree.visualRect(tree.indexFromItem(item))
            y = rect.bottom()
            for x in range(rect.left(), rect.right()):
                color = image.pixelColor(x, y)
                if (color.red(), color.green(), color.blue()) == (42, 42, 42):
                    return x
            return None

        parent_start = rule_start(parent)
        child_start = rule_start(child)
        self.assertIsNotNone(parent_start)
        self.assertGreater(child_start, parent_start)
        child_rect = tree.visualRect(tree.indexFromItem(child))
        branch = child_rect.x() + 4 + 16 + 8
        guide = image.pixelColor(branch, child_rect.top() + 2)
        self.assertEqual((guide.red(), guide.green(), guide.blue()), (42, 42, 42))
        win.close()

    def test_level_fields_round_trip_through_the_session_and_the_folder(self):
        import vmi_studio.window as window_mod
        signature = window_mod.signature

        ids = Ids()
        ink = Node(ids.next(), "ink", "layer", raster=pixel(255, 0, 0))
        app, win = self._desk(refresh([ink]))
        held = {}
        window_mod.save = lambda data: held.update(data)
        session = {
            "path": win.art.path,
            "signature": signature(win.art.layers),
            "objects": [{
                "id": "o9",
                "name": "arcade",
                "layer_ids": [ink.id],
                "explicit": True,
                "kind": "button",
                "warp_layer": "mask",
                "sound": "sfx:hit2",
                "origin": "static:x",
                "role": "window",
            }],
            "playlist": "battle",
        }
        window_mod.load = lambda: session
        win.show_file(win.art, restore=True)
        app.processEvents()
        self.assertEqual(win.playlist, "battle")
        self.assertEqual(win.playlist_box.currentData(), "battle")
        made = win.objects[0]
        self.assertEqual(made.kind, "button")
        self.assertEqual(made.warp_layer, "mask")
        self.assertEqual(made.sound, "sfx:hit2")
        self.assertEqual(made.role, "window")
        self.assertEqual(win.object_kind.currentData(), "button")
        bad = dict(session)
        bad["objects"] = [dict(session["objects"][0], sound="boom", warp_layer="nope", kind="laser")]
        bad["playlist"] = "jazz"
        window_mod.load = lambda: bad
        win.show_file(win.art, restore=True)
        self.assertEqual(win.playlist, "")
        self.assertEqual(win.objects[0].sound, "")
        self.assertEqual(win.objects[0].warp_layer, "")
        self.assertEqual(win.objects[0].kind, "")
        win.objects[0].kind = "button"
        win.objects[0].sound = "sfx:ring1"
        win.objects[0].warp_layer = "mask"
        win.playlist = "menu"
        win._save_session()
        self.assertEqual(held["playlist"], "menu")
        self.assertEqual(held["objects"][0]["sound"], "sfx:ring1")
        self.assertEqual(held["objects"][0]["warp_layer"], "mask")
        self.assertEqual(held["objects"][0]["kind"], "button")
        parent = tempfile.mkdtemp(prefix="vmi-level-")
        try:
            result = write_scene(parent, win.art, win.objects, win.playlist)
            with open(os.path.join(result["root"], "scene.json"), encoding="utf-8") as handle:
                scene = json.load(handle)
            self.assertEqual(scene["playlist"], "menu")
            self.assertEqual(scene["objects"][0]["type"], "button")
            self.assertEqual(scene["objects"][0]["warpLayer"], "mask")
            self.assertEqual(scene["objects"][0]["sound"], "sfx:ring1")
            with open(os.path.join(result["root"], "objects", "arcade", "object.json"), encoding="utf-8") as handle:
                saved = json.load(handle)
            self.assertEqual(saved["warpLayer"], "mask")
            self.assertEqual(saved["sound"], "sfx:ring1")
            with open(os.path.join(result["root"], "layers.tsx"), encoding="utf-8") as handle:
                tsx = handle.read()
            self.assertIn('export const PLAYLIST = "menu";', tsx)
            self.assertIn('"sfx:ring1"', tsx)
            with open(os.path.join(result["root"], "HOW.txt"), encoding="utf-8") as handle:
                how = handle.read()
            self.assertIn("Webamp playlist menu.", how)
            self.assertIn("mask", how)
        finally:
            import shutil
            shutil.rmtree(parent, ignore_errors=True)
        win.close()

    def test_shift_then_ctrl_g_inserts_the_selection(self):
        """Same job as Clip Studio: Shift selects the span, Ctrl+G folders it once."""
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        hover = Node(ids.next(), "door#hover", "layer", raster=pixel(0, 255, 0))
        app, win = self._desk(refresh([sky, door, hover]))
        rows = win.tree.visible_nodes()
        self.assertEqual([node.name for node in rows], ["door#hover", "door", "sky"])

        from PySide6.QtCore import QPoint, Qt
        from PySide6.QtTest import QTest

        win.resize(900, 700)
        win.show()
        app.processEvents()
        hover_index = win.model.index_for(rows[0])
        door_index = win.model.index_for(rows[1])
        hover_rect = win.tree.visualRect(hover_index)
        door_rect = win.tree.visualRect(door_index)
        self.assertGreater(hover_rect.height(), 0)
        self.assertGreater(door_rect.height(), 0)

        def click(index, modifiers):
            rect = win.tree.visualRect(index)
            _split, gutter, _track, _mute, _solo, _clip, _opacity, _blend = win.tree.track_marks(index)
            pos = QPoint(gutter.left() - 20, rect.center().y())
            QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, modifiers, pos)
            app.processEvents()

        click(hover_index, Qt.NoModifier)
        self.assertEqual(win.picked, {rows[0].id})
        click(door_index, Qt.ShiftModifier)
        self.assertEqual(win.picked, {rows[0].id, rows[1].id})

        win.tree.setFocus()
        QTest.keyClick(win.tree, Qt.Key_G, Qt.ControlModifier)
        app.processEvents()
        self.assertEqual([node.name for node in win.art.layers], ["sky", "Folder"])
        self.assertEqual([node.name for node in win.art.layers[1].children], ["door", "door#hover"])
        self.assertEqual(win.picked, {win.art.layers[1].id})
        win.close()

    def test_ctrl_click_skips_the_layer_between(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        hover = Node(ids.next(), "door#hover", "layer", raster=pixel(0, 255, 0))
        _app, win = self._desk(refresh([sky, door, hover]))
        rows = win.tree.visible_nodes()
        win.select_only(rows[0])
        win.toggle_pick(rows[2])
        self.assertEqual(win.picked, {rows[0].id, rows[2].id})
        win.group_selection()
        self.assertEqual([node.name for node in win.art.layers], ["door", "Folder"])
        self.assertEqual([node.name for node in win.art.layers[1].children], ["sky", "door#hover"])
        win.close()

    def test_eye_only_hides_and_the_plus_only_opens(self):
        from PySide6.QtCore import QPoint, Qt
        from PySide6.QtTest import QTest

        ids = Ids()
        group = Node(ids.next(), "beetle", "group")
        group.children = [
            Node(ids.next(), "line", "layer", raster=pixel(255, 0, 0)),
            Node(ids.next(), "color", "layer", raster=pixel(0, 0, 255)),
        ]
        app, win = self._desk(refresh([group]))
        win.resize(900, 700)
        win.show()
        app.processEvents()
        folder = win.art.layers[0]
        index = win.model.index_for(folder)
        self.assertTrue(win.tree.isExpanded(index))
        disc, eye, _name = win.tree.row_marks(index)
        self.assertGreater(eye.left(), disc.right() + 2)
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, eye.center())
        app.processEvents()
        self.assertFalse(folder.visible)
        self.assertTrue(win.tree.isExpanded(index))
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, eye.center())
        app.processEvents()
        self.assertTrue(folder.visible)
        self.assertTrue(win.tree.isExpanded(index))
        gap = QPoint(disc.right() + 2, disc.center().y())
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, gap)
        app.processEvents()
        self.assertTrue(folder.visible)
        self.assertTrue(win.tree.isExpanded(index))
        self.assertEqual(win.picked, {folder.id})
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, disc.center())
        app.processEvents()
        self.assertFalse(win.tree.isExpanded(index))
        self.assertTrue(folder.visible)
        win.close()

    def test_animation_timeline_does_not_reorder_the_stack(self):
        ids = Ids()
        line = Node(ids.next(), "line", "group")
        line.children = [Node(ids.next(), "ink", "layer", raster=pixel(255, 0, 0))]
        color = Node(ids.next(), "color", "group")
        color.children = [Node(ids.next(), "fill", "layer", raster=pixel(0, 0, 255))]
        beetle = Node(ids.next(), "beetle", "group")
        beetle.children = [line, color]
        app, win = self._desk(refresh([beetle]))
        win.resize(900, 700)
        win.show()
        app.processEvents()
        stored = [node.name for node in win.art.layers[0].children]
        win.mark_animation(win.art.layers[0])
        app.processEvents()
        self.assertTrue(win.art.layers[0].animation)
        self.assertEqual([win.timeline.item(row).text() for row in range(win.timeline.count())], ["000000  color", "000001  line"])
        win._move_frame(1)
        app.processEvents()
        self.assertEqual([node.name for node in win.art.layers[0].children], stored)
        self.assertEqual(win.timeline.item(0).text(), "000000  line")
        win.close()

    def test_objects_start_empty_until_a_name_is_typed(self):
        ids = Ids()
        door = Node(ids.next(), "Layer 1", "layer", raster=pixel(255, 0, 0))
        _app, win = self._desk(refresh([door]))
        self.assertEqual(win.objects, [])
        self.assertEqual(win.slots.topLevelItemCount(), 4)
        self.assertEqual(win.slots.topLevelItem(0).text(1), "rest")
        self.assertEqual(win.slots.topLevelItem(1).text(1), "hover")
        self.assertEqual(win.slots.topLevelItem(2).text(1), "pressed")
        made = win._create_named_object("door", [door])
        self.assertEqual(made.name, "door")
        self.assertIsNone(win._create_named_object("Door", [door]))
        win.delete_objects()
        self.assertEqual(win.objects, [])
        win.close()

    def test_window_shows_the_layer_stack(self):
        if not os.path.isfile(BUTTONS):
            self.skipTest("buttons.clip is not on this machine")
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtWidgets import QApplication

        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        from vmi_studio.openers import open_drawing

        app = QApplication.instance() or QApplication([])
        window_mod.apply_style(app)
        win = window_mod.MainWindow()
        art = open_drawing(BUTTONS)
        win.show_file(art, restore=False)
        app.processEvents()
        self.assertGreater(win.ink, 0)
        self.assertEqual(win.caption.text(), "Visible stack")
        self.assertGreater(win.model.rowCount(), 0)
        before = win.ink
        for layer in paint_layers(art.layers):
            if layer.raster is None:
                continue
            win.toggle_visible(layer)
            if win.ink != before:
                win.toggle_visible(layer)
                return
            win.toggle_visible(layer)
        self.fail("the picture ignored the layer eye")

    def test_drag_rearranges_only_when_the_lock_is_open(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        hover = Node(ids.next(), "door#hover", "layer", raster=pixel(0, 255, 0))
        app, win = self._desk(refresh([sky, door, hover]))
        self.assertTrue(win.arrange_locked)
        self.assertFalse(win.arrange_lock.isChecked())
        win.resize(900, 700)
        win.show()
        app.processEvents()

        def names():
            return [node.name for node in win.tree.visible_nodes()]

        def drag_sky_to_the_front():
            from PySide6.QtCore import QEvent, QPoint, QPointF
            from PySide6.QtGui import QMouseEvent
            from PySide6.QtTest import QTest

            rows = win.tree.visible_nodes()
            self.assertEqual([node.name for node in rows], ["door#hover", "door", "sky"])
            hover_rect = win.tree.visualRect(win.model.index_for(rows[0]))
            sky_rect = win.tree.visualRect(win.model.index_for(rows[2]))
            self.assertGreater(hover_rect.height(), 0)
            _split, gutter, _track, _mute, _solo, _clip, _opacity, _lane = win.tree.track_marks(win.model.index_for(rows[2]))
            name_x = gutter.left() - 16
            start = QPoint(name_x, sky_rect.center().y())
            end = QPoint(name_x, hover_rect.top() + 1)
            viewport = win.tree.viewport()
            QTest.mousePress(viewport, Qt.LeftButton, Qt.NoModifier, start)
            for point in (start, end):
                event = QMouseEvent(
                    QEvent.Type.MouseMove,
                    QPointF(point),
                    QPointF(viewport.mapToGlobal(point)),
                    Qt.LeftButton,
                    Qt.LeftButton,
                    Qt.NoModifier,
                )
                app.sendEvent(viewport, event)
            QTest.mouseRelease(viewport, Qt.LeftButton, Qt.NoModifier, end)
            app.processEvents()

        from PySide6.QtCore import Qt

        drag_sky_to_the_front()
        self.assertEqual(names(), ["door#hover", "door", "sky"])
        self.assertIn("Unlock", win.status.currentMessage())
        win.arrange_lock.setChecked(True)
        app.processEvents()
        self.assertFalse(win.arrange_locked)
        drag_sky_to_the_front()
        self.assertEqual(names(), ["sky", "door#hover", "door"])
        image = composite_image(1, 1, visible_paint(win.art.layers))
        self.assertEqual(image.getpixel((0, 0))[:3], (255, 0, 0))
        win.close()

    def test_mute_and_solo_change_the_picture_from_the_track(self):
        from PySide6.QtCore import QPoint, Qt
        from PySide6.QtTest import QTest

        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        app, win = self._desk(refresh([sky, door]))
        win.resize(900, 700)
        win.show()
        app.processEvents()
        self.assertEqual([node.name for node in visible_paint(win.art.layers)], ["sky", "door"])
        rows = win.tree.visible_nodes()
        door_row = next(node for node in rows if node.name == "door")
        sky_row = next(node for node in rows if node.name == "sky")
        _split, _gutter, _track, mute, solo, _clip, _opacity, lane = win.tree.track_marks(win.model.index_for(door_row))
        self.assertGreater(lane.width(), 8)
        self.assertGreater(mute.left(), _gutter.right())
        picked = set(win.picked)
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, mute.center())
        app.processEvents()
        self.assertTrue(door_row.mute)
        self.assertTrue(door_row.visible)
        self.assertEqual([node.name for node in preview_paint(win.art.layers)], ["sky"])
        self.assertEqual(win.picked, picked)
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, mute.center())
        app.processEvents()
        self.assertFalse(door_row.mute)
        self.assertEqual([node.name for node in preview_paint(win.art.layers)], ["sky", "door"])
        _split, _gutter, _track, _mute, sky_solo, _clip, _opacity, _lane = win.tree.track_marks(win.model.index_for(sky_row))
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, sky_solo.center())
        app.processEvents()
        self.assertTrue(sky_row.solo)
        self.assertEqual([node.name for node in preview_paint(win.art.layers)], ["sky"])
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, sky_solo.center())
        app.processEvents()
        self.assertEqual([node.name for node in preview_paint(win.art.layers)], ["sky", "door"])
        _split, _gutter, _track, _mute, solo, clip, opacity, blend = win.tree.track_marks(win.model.index_for(door_row))
        self.assertGreater(clip.left(), solo.right())
        self.assertGreater(opacity.left(), clip.right())
        self.assertGreater(blend.left(), opacity.right())
        self.assertGreater(blend.width(), 40)
        self.assertFalse(door_row.clip)
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, clip.center())
        app.processEvents()
        self.assertTrue(door_row.clip)
        self.assertEqual(win.picked, picked)
        self.assertIn("clips to the layer below", win.status.currentMessage())
        calls = []
        win.choose_blend = lambda node, pos: calls.append(node.id)
        picked = set(win.picked)
        QTest.mouseClick(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, blend.center())
        app.processEvents()
        self.assertEqual(calls, [door_row.id])
        self.assertEqual(win.picked, picked)
        self.assertFalse(door_row.mute)
        full = door_row.opacity
        spot = opacity.center()
        QTest.mousePress(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, spot)
        QTest.mouseMove(win.tree.viewport(), spot + QPoint(0, 40))
        QTest.mouseRelease(win.tree.viewport(), Qt.LeftButton, Qt.NoModifier, spot + QPoint(0, 40))
        app.processEvents()
        self.assertLess(door_row.opacity, full)
        self.assertEqual(win.picked, picked)
        self.assertIn("opacity is", win.status.currentMessage())
        win.undo()
        app.processEvents()
        self.assertEqual(door_row.opacity, full)
        win.close()

    def test_mute_paints_the_row_before_the_picture_and_defers_the_save(self):
        import vmi_studio.window as window_mod

        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        app, win = self._desk(refresh([sky, door]))
        self.assertEqual(win._flat.getpixel((0, 0))[:3], (0, 0, 255))
        held = {}
        window_mod.save = lambda data: held.update(data)
        order = []
        real = win.recomposite

        def wrapped():
            order.append((door.mute, win.status.currentMessage()))
            real()

        win.recomposite = wrapped
        win.toggle_mute(door)
        app.processEvents()
        self.assertEqual(order, [(True, "Muted door.")])
        self.assertEqual(held, {})
        self.assertTrue(win._save_timer.isActive())
        self.assertEqual(win._flat.getpixel((0, 0))[:3], (255, 0, 0))
        win.toggle_mute(door)
        self.assertEqual(win._flat.getpixel((0, 0))[:3], (0, 0, 255))
        self.assertEqual(order[-1], (False, "Unmuted door."))
        self.assertFalse(win.picture.is_busy())
        win.close()

    def test_unsolo_survives_a_deleted_picture_job_and_shows_a_loading_bar(self):
        from PySide6.QtCore import QCoreApplication, QEvent, QThread

        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        app, win = self._desk(refresh([sky, door]))
        set_solo(win.art.layers, sky.id, True)
        win.recomposite()
        self.assertEqual(win._flat.getpixel((0, 0))[:3], (255, 0, 0))
        dead = QThread(win)
        dead.deleteLater()
        QCoreApplication.sendPostedEvents(dead, QEvent.Type.DeferredDelete)
        win._picture_job = dead
        win._picture_sync = lambda: False
        started = []

        def start():
            started.append("go")
            win._picture_pending = False

        win._start_picture_job = start
        win.toggle_solo(sky)
        self.assertFalse(sky.solo)
        self.assertEqual(started, ["go"])
        self.assertIsNone(win._picture_job)
        self.assertTrue(win.picture.is_busy())
        self.assertEqual(win.status.currentMessage(), "Loading the rest of the picture.")
        image = win.picture.grab().toImage()
        white = 0
        for y in range(image.height() - 32, image.height() - 8):
            for x in range(8, image.width() - 8):
                color = image.pixelColor(x, y)
                if (color.red(), color.green(), color.blue()) == (255, 255, 255):
                    white += 1
        self.assertGreater(white, 0)
        win._finish_picture(win._picture_generation, win._build_picture(), "")
        self.assertFalse(win.picture.is_busy())
        self.assertEqual(win.status.currentMessage(), "Solo off. Every track is back in the picture.")
        self.assertEqual(win._flat.getpixel((0, 0))[:3], (0, 0, 255))
        win.picture.set_busy(False)
        win.close()

    def test_a_blend_choice_is_kept_for_the_same_file(self):
        from vmi_studio.composite import assign_blend
        from vmi_studio.window import SPLIT_VERSION, signature

        import vmi_studio.window as window_mod

        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(255, 255, 255))
        app, win = self._desk(refresh([sky, door]))
        held = {}
        window_mod.save = lambda data: held.update(data)
        assign_blend(door, "multiply")
        win.recomposite()
        app.processEvents()
        self.assertEqual(composite_image(1, 1, win.art.layers).getpixel((0, 0))[:3], (255, 0, 0))
        win._save_session()
        self.assertEqual(held["split_version"], SPLIT_VERSION)
        saved = next(row for row in held["blends"] if row["id"] == door.id)
        self.assertEqual(saved["blend"], "multiply")
        self.assertTrue(saved["shapes"])
        door.blend = "normal"
        door.shapes = True
        session = {
            "path": win.art.path,
            "signature": signature(win.art.layers),
            "blends": list(held["blends"]),
            "split_version": 1,
            "splitter": [562, 704, 640],
        }
        window_mod.load = lambda: session
        win._split_custom = False
        win._split_saved = [1, 2, 3]
        win.show_file(win.art, restore=True)
        app.processEvents()
        self.assertEqual(door.blend, "multiply")
        self.assertIsNone(win._split_saved)
        win.close()

    def test_track_rules_meet_the_names_without_a_vertical_bar(self):
        ids = Ids()
        group = Node(ids.next(), "Exterior", "group")
        group.children = [Node(ids.next(), "door", "layer")]
        sky = Node(ids.next(), "sky", "layer")
        app, win = self._desk(refresh([group, sky]))
        win.resize(900, 700)
        win.show()
        app.processEvents()
        tree = win.tree
        image = tree.viewport().grab().toImage()

        def rgb(x, y):
            color = image.pixelColor(x, y)
            return (color.red(), color.green(), color.blue())

        def rule_start(index):
            rect = tree.visualRect(index)
            _split, _gutter, _track, mute, _solo, _clip, _opacity, _lane = tree.track_marks(index)
            y = rect.bottom()
            for x in range(0, mute.left()):
                if rgb(x, y) == (42, 42, 42):
                    return x
            return None

        sky_index = tree.model().index_for(next(node for node in tree.visible_nodes() if node.name == "sky"))
        door_index = tree.model().index_for(next(node for node in tree.visible_nodes() if node.name == "door"))
        sky_rect = tree.visualRect(sky_index)
        _split, gutter, _track, _mute, _solo, _clip, _opacity, _lane = tree.track_marks(sky_index)
        self.assertEqual(rgb(gutter.center().x(), sky_rect.center().y()), (0, 0, 0))
        sky_start = rule_start(sky_index)
        door_start = rule_start(door_index)
        self.assertIsNotNone(sky_start)
        self.assertGreater(door_start, sky_start)
        win.close()

    def test_a_special_blend_stays_grey_until_that_row_is_chosen(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", blend="multiply", raster=pixel(0, 0, 255))
        app, win = self._desk(refresh([sky, door]))
        win.resize(1100, 700)
        win.show()
        app.processEvents()
        tree = win.tree
        sky_index = tree.model().index_for(sky)
        door_index = tree.model().index_for(door)

        def peak(index):
            _split, _gutter, _track, _mute, _solo, _clip, _opacity, box = tree.track_marks(index)
            image = tree.viewport().grab().toImage()
            best = 0
            for y in range(box.top() + 2, box.bottom() - 1):
                for x in range(box.left() + 4, box.right() - 4):
                    color = image.pixelColor(x, y)
                    best = max(best, color.red(), color.green(), color.blue())
            return best

        tree.setCurrentIndex(sky_index)
        app.processEvents()
        self.assertLess(peak(door_index), 90)
        tree.setCurrentIndex(door_index)
        app.processEvents()
        self.assertGreater(peak(door_index), 200)
        self.assertLess(peak(sky_index), 90)
        win.close()

    def test_an_unfocused_clip_stays_grey_until_that_row_is_highlighted(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", clip=True, raster=pixel(0, 0, 255))
        door.mute = True
        app, win = self._desk(refresh([sky, door]))
        win.resize(1100, 700)
        win.show()
        app.processEvents()
        tree = win.tree
        tree.setFocus()
        sky_index = tree.model().index_for(sky)
        door_index = tree.model().index_for(door)
        tree.setCurrentIndex(sky_index)
        app.processEvents()

        def corner(box):
            image = tree.viewport().grab().toImage()
            color = image.pixelColor(box.left() + 1, box.top() + 1)
            return (color.red(), color.green(), color.blue())

        _split, _gutter, _track, mute, _solo, clip, _opacity, _blend = tree.track_marks(door_index)
        self.assertEqual(corner(clip), (58, 58, 58))
        self.assertEqual(corner(mute), (255, 255, 255))
        tree.setCurrentIndex(door_index)
        app.processEvents()
        _split, _gutter, _track, _mute, _solo, clip, _opacity, _blend = tree.track_marks(door_index)
        self.assertEqual(corner(clip), (255, 255, 255))
        door.clip = False
        tree.setCurrentIndex(sky_index)
        tree.viewport().update()
        app.processEvents()
        _split, _gutter, _track, mute, _solo, clip, _opacity, _blend = tree.track_marks(door_index)
        self.assertEqual(corner(clip), (0, 0, 0))
        self.assertEqual(corner(mute), (255, 255, 255))
        win.close()

    def test_a_picture_click_highlights_the_layer_and_opens_its_folder(self):
        from PySide6.QtCore import QPoint, Qt
        from PySide6.QtGui import QColor, QPixmap
        from PySide6.QtTest import QTest

        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=fill(0, 255, 0, w=8, h=8))
        folder = Node(ids.next(), "booth", "group")
        sign = Node(ids.next(), "sign", "layer", raster=fill(0, 0, 255, w=2, h=2, x=1, y=1))
        folder.children = [sign]
        app, win = self._desk(refresh([sky, folder]))
        win.art.width = 8
        win.art.height = 8
        win.picture.set_document(8, 8)
        win.resize(900, 700)
        win.show()
        app.processEvents()
        win.tree.collapseAll()
        app.processEvents()
        self.assertFalse(win.tree.isExpanded(win.model.index_for(folder)))
        chosen = win.choose_picture(1, 1)
        self.assertIs(chosen, sign)
        self.assertEqual(win.picked, {sign.id})
        self.assertTrue(win.tree.isExpanded(win.model.index_for(folder)))
        self.assertEqual(win.model.node_of(win.tree.currentIndex()).id, sign.id)
        self.assertEqual(win.status.currentMessage(), "sign")
        missed = win.choose_picture(0, 0)
        self.assertIs(missed, sky)

        view = win.picture
        self.assertGreater(view.width(), 8)
        pix = QPixmap(10, 10)
        pix.fill(QColor("#ffffff"))
        view.set_image(pix)
        view.fit = True
        view.zoom = 1.0
        view.pan_x = 0.0
        view.pan_y = 0.0
        view.rotation = 0.0
        app.processEvents()
        center = view.image_at(QPoint(view.width() // 2, view.height() // 2))
        self.assertIsNotNone(center)
        picked = []
        view.on_pick = lambda x, y: picked.append((x, y))
        QTest.mouseClick(view, Qt.LeftButton, Qt.NoModifier, QPoint(view.width() // 2, view.height() // 2))
        app.processEvents()
        self.assertEqual(len(picked), 1)
        QTest.mousePress(view, Qt.LeftButton, Qt.ShiftModifier, QPoint(30, 40))
        QTest.mouseMove(view, QPoint(50, 40))
        QTest.mouseRelease(view, Qt.LeftButton, Qt.ShiftModifier, QPoint(50, 40))
        app.processEvents()
        self.assertAlmostEqual(view.rotation, 8.0, places=3)
        self.assertEqual(len(picked), 1)
        QTest.mousePress(view, Qt.MiddleButton, Qt.NoModifier, QPoint(40, 40))
        QTest.mouseMove(view, QPoint(55, 48))
        QTest.mouseRelease(view, Qt.MiddleButton, Qt.NoModifier, QPoint(55, 48))
        app.processEvents()
        self.assertAlmostEqual(view.pan_x, 15, places=3)
        self.assertAlmostEqual(view.pan_y, 8, places=3)
        win.close()

    def test_a_wide_screen_keeps_the_4k_gutters_and_drops_a_narrow_save(self):
        from vmi_studio.window import choose_split, saved_split_fits, split_sizes

        stale = [562, 704, 640]
        wide = [344, 3138, 344]
        # The 4K panel is 1920 logical pixels at scale 2. Same ratio, half the counts.
        self.assertEqual(split_sizes(1920), [172, 1562, 172])
        self.assertEqual(split_sizes(3840), wide)
        self.assertEqual(choose_split(1920, None, 3840), [172, 1562, 172])
        self.assertEqual(sum(split_sizes(3840)), 3840 - 14)
        self.assertIsNone(split_sizes(400))
        self.assertFalse(saved_split_fits(stale, 3840))
        self.assertTrue(saved_split_fits(wide, 3840))
        self.assertEqual(choose_split(3840, stale, 3840), wide)
        self.assertIsNone(choose_split(3840, stale, 2560))
        self.assertIsNone(choose_split(2000, None, 2560))
        self.assertIsNone(choose_split(3840, None, 3840, custom=True))

        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(0, 0, 255))
        app, win = self._desk(refresh([sky]))
        win.resize(3840, 1000)
        win.show()
        app.processEvents()
        win._split_custom = False
        win._screen_width = lambda: 3840
        win._apply_split(stale)
        self.assertEqual(win.split.width(), 3840)
        self.assertEqual(win.split.sizes(), [344, 3138, 344])
        kept = [800, 2226, 800]
        win._split_custom = False
        win._apply_split(kept)
        self.assertEqual(win.split.sizes(), kept)
        self.assertTrue(win._split_custom)
        win._apply_split([100, 100, 100])
        self.assertEqual(win.split.sizes(), kept)
        win.close()


class FontTests(unittest.TestCase):
    def test_the_card_builder_catalog_is_the_menu(self):
        from vmi_studio import fonts

        rows = fonts.catalog()
        if len(rows) < 100:
            self.skipTest("the font catalog is not on this machine")
        by_id = {face.id: face for face in rows}
        self.assertEqual(fonts.DEFAULT_ID, "monospace")
        self.assertGreater(len(rows), 100)
        self.assertEqual(by_id["atwriter"].name, "Another Typewriter")
        self.assertTrue(by_id["atwriter"].local.endswith("atwriter-webfont.woff"))
        self.assertEqual(by_id["fira-code"].google, "Fira Code")
        self.assertIsNone(by_id["fira-code"].local)
        self.assertEqual(by_id["remilia-mincho"].category, "serif")
        self.assertTrue(by_id["hina-mincho"].local.endswith(".ttf"))

    def test_the_default_face_is_monospace_and_a_kit_face_can_replace_it(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtGui import QFontInfo
        from PySide6.QtWidgets import QApplication

        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        app = QApplication.instance() or QApplication([])
        self.assertEqual(window_mod.apply_style(app), "monospace")
        self.assertTrue(QFontInfo(app.font()).fixedPitch())
        win = window_mod.MainWindow()
        titles = [action.text() for action in win.menuBar().actions()]
        self.assertEqual(titles, ["File", "Edit", "View", "Font", "Layer"])
        checked = [action.text() for action in win._font_actions.values() if action.isChecked()]
        self.assertEqual(checked, ["Monospace"])
        if "atwriter" not in win._font_actions:
            win.close()
            self.skipTest("the font catalog is not on this machine")
        win._preview_font_menu()
        self.assertEqual(win._font_actions["atwriter"].font().family(), "Another Typewriter")
        win.set_ui_font("atwriter")
        self.assertIn("Typewriter", QFontInfo(app.font()).family())
        self.assertIn("Typewriter", app.styleSheet())
        self.assertTrue(win._font_actions["atwriter"].isChecked())
        self.assertFalse(win._font_actions["monospace"].isChecked())
        win.show()
        app.processEvents()
        self.assertIn("Typewriter", win.caption.fontInfo().family())
        win.set_ui_font("monospace")
        self.assertTrue(QFontInfo(app.font()).fixedPitch())
        win.close()


def fill(r, g, b, a=255, w=4, h=4, x=0, y=0):
    return Raster(x, y, w, h, bytes((r, g, b, a)) * (w * h))


class BlendTests(unittest.TestCase):
    def test_modes_and_the_glow_flag(self):
        self.assertEqual(resolve_mode("lddg", True)[0], "linear_dodge")
        self.assertEqual(resolve_mode("lddg", False)[:2], ("add_glow", True))
        self.assertEqual(resolve_mode("div ", False)[0], "glow_dodge")
        self.assertEqual(resolve_mode("mul ")[0], "multiply")
        self.assertTrue(resolve_mode("nope")[2])

    def test_a_clip_stays_inside_its_base_and_not_the_canvas(self):
        ids = Ids()
        green = Node(ids.next(), "ground", "layer", raster=fill(0, 255, 0))
        red = Node(ids.next(), "base", "layer", raster=fill(255, 0, 0, w=1, h=1, x=1, y=1))
        blue = Node(ids.next(), "mask", "layer", clip=True, raster=fill(0, 0, 255))
        image = composite_image(4, 4, refresh([green, red, blue]))
        self.assertEqual(image.getpixel((0, 0)), (0, 255, 0, 255))
        self.assertEqual(image.getpixel((1, 1)), (0, 0, 255, 255))
        self.assertEqual(image.getpixel((3, 3)), (0, 255, 0, 255))

    def test_a_second_preview_does_not_resize_the_layer_again(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=fill(0, 255, 0, w=40, h=20))
        calls = []
        real = Image.Image.resize

        def spy(image, size, resample=None, *args, **kwargs):
            calls.append(tuple(size))
            return real(image, size, resample, *args, **kwargs)

        Image.Image.resize = spy
        try:
            layers = refresh([sky])
            first = composite_scene(40, 20, layers, max_edge=10)
            resized = len(calls)
            self.assertGreater(resized, 0)
            second = composite_scene(40, 20, layers, max_edge=10)
            self.assertEqual(len(calls), resized)
            self.assertEqual(list(first.getdata()), list(second.getdata()))
            set_visible(layers, sky.id, False)
            composite_scene(40, 20, layers, max_edge=10)
            self.assertEqual(len(calls), resized)
        finally:
            Image.Image.resize = real

    def test_multiply_does_not_darken_the_paint_outside_the_base(self):
        ids = Ids()
        ground = Node(ids.next(), "ground", "layer", raster=fill(255, 0, 0))
        base = Node(ids.next(), "base", "layer", raster=fill(255, 255, 255, w=2, h=2, x=1, y=1))
        shade = Node(ids.next(), "shade", "layer", blend="mul ", clip=True, raster=fill(0, 0, 0))
        image = composite_image(4, 4, refresh([ground, base, shade]))
        self.assertEqual(image.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(image.getpixel((1, 1)), (0, 0, 0, 255))
        self.assertEqual(image.getpixel((2, 2)), (0, 0, 0, 255))
        self.assertEqual(image.getpixel((3, 3)), (255, 0, 0, 255))

    def test_a_second_clip_sees_the_first_clip(self):
        ids = Ids()
        base = Node(ids.next(), "base", "layer", raster=fill(255, 0, 0, w=1, h=1))
        white = Node(ids.next(), "white", "layer", clip=True, raster=fill(255, 255, 255, w=1, h=1))
        blue = Node(ids.next(), "blue", "layer", blend="multiply", clip=True, raster=fill(0, 0, 255, w=1, h=1))
        image = composite_image(1, 1, refresh([base, white, blue]))
        self.assertEqual(image.getpixel((0, 0)), (0, 0, 255, 255))

    def test_a_clear_clip_leaves_the_base_and_a_soft_base_keeps_its_coverage(self):
        ids = Ids()
        base = Node(ids.next(), "base", "layer", raster=fill(255, 0, 0, w=1, h=1))
        clear = Node(ids.next(), "clear", "layer", clip=True, raster=fill(0, 0, 255, a=0, w=1, h=1))
        image = composite_image(1, 1, refresh([base, clear]))
        self.assertEqual(image.getpixel((0, 0)), (255, 0, 0, 255))
        soft = Node(ids.next(), "soft", "layer", raster=fill(255, 0, 0, a=128, w=1, h=1))
        ink = Node(ids.next(), "ink", "layer", clip=True, raster=fill(0, 0, 255, w=1, h=1))
        covered = composite_image(1, 1, refresh([soft, ink]))
        self.assertEqual(covered.getpixel((0, 0)), (0, 0, 255, 128))

    def test_pass_through_reaches_the_paint_below_and_normal_does_not(self):
        ids = Ids()
        red = Node(ids.next(), "red", "layer", raster=fill(255, 0, 0, w=1, h=1))
        grey = Node(ids.next(), "grey", "layer", blend="multiply", raster=fill(128, 128, 128, w=1, h=1))
        opened = Node(ids.next(), "opened", "group", blend="pass through")
        opened.children = [grey]
        through = composite_image(1, 1, refresh([red, opened]))
        self.assertEqual(through.getpixel((0, 0)), (128, 0, 0, 255))
        grey2 = Node(ids.next(), "grey", "layer", blend="multiply", raster=fill(128, 128, 128, w=1, h=1))
        closed = Node(ids.next(), "closed", "group", blend="normal")
        closed.children = [grey2]
        red2 = Node(ids.next(), "red", "layer", raster=fill(255, 0, 0, w=1, h=1))
        isolated = composite_image(1, 1, refresh([red2, closed]))
        self.assertEqual(isolated.getpixel((0, 0)), (128, 128, 128, 255))

    def test_add_glow_is_stronger_than_add_until_full_opacity(self):
        ids = Ids()

        def stack(blend, opacity, shapes=True):
            black = Node(ids.next(), "black", "layer", raster=fill(0, 0, 0, w=1, h=1))
            light = Node(ids.next(), "light", "layer", blend=blend, opacity=opacity, raster=fill(255, 255, 255, w=1, h=1))
            light.shapes = shapes
            return composite_image(1, 1, refresh([black, light])).getpixel((0, 0))

        self.assertEqual(stack("lddg", 128, True), (128, 128, 128, 255))
        self.assertEqual(stack("lddg", 128, False), (192, 192, 192, 255))
        self.assertEqual(stack("add glow", 255), (255, 255, 255, 255))
        self.assertEqual(stack("lddg", 255, False), (255, 255, 255, 255))

    def test_a_clipped_folder_stays_inside_the_base_below_it(self):
        ids = Ids()
        ground = Node(ids.next(), "ground", "layer", raster=fill(0, 255, 0))
        base = Node(ids.next(), "base", "layer", raster=fill(255, 0, 0, w=1, h=1, x=1, y=1))
        blue = Node(ids.next(), "ink", "layer", raster=fill(0, 0, 255))
        folder = Node(ids.next(), "parts", "group", clip=True)
        folder.children = [blue]
        image = composite_scene(4, 4, refresh([ground, base, folder]))
        self.assertEqual(image.getpixel((0, 0)), (0, 255, 0, 255))
        self.assertEqual(image.getpixel((1, 1)), (0, 0, 255, 255))
        self.assertEqual(image.getpixel((2, 2)), (0, 255, 0, 255))

    def test_color_dodge_brightens_only_the_base(self):
        ids = Ids()
        ground = Node(ids.next(), "ground", "layer", raster=fill(40, 40, 40))
        base = Node(ids.next(), "base", "layer", raster=fill(128, 128, 128, w=1, h=1, x=1, y=1))
        dodge = Node(ids.next(), "dodge", "layer", blend="div ", clip=True, raster=fill(255, 255, 255))
        image = composite_image(4, 4, refresh([ground, base, dodge]))
        self.assertEqual(image.getpixel((1, 1)), (255, 255, 255, 255))
        self.assertEqual(image.getpixel((0, 0)), (40, 40, 40, 255))

    def test_the_picture_hides_a_layer_and_one_animation_cel_keeps_its_clip(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=fill(255, 0, 0, w=1, h=1))
        door = Node(ids.next(), "door", "layer", raster=fill(0, 0, 255, w=1, h=1))
        layers = refresh([sky, door])
        hidden = set_visible(layers, door.id, False)
        image = composite_scene(1, 1, hidden)
        self.assertEqual(image.getpixel((0, 0)), (255, 0, 0, 255))

        sky2 = Node(ids.next(), "sky", "layer", raster=fill(0, 180, 0))
        base = Node(ids.next(), "base", "layer", raster=fill(255, 255, 255, w=1, h=1, x=1, y=1))
        shade = Node(ids.next(), "shade", "layer", blend="multiply", clip=True, raster=fill(128, 128, 128))
        cel = Node(ids.next(), "cel", "group")
        cel.children = [base, shade]
        other = Node(ids.next(), "other", "layer", raster=fill(255, 0, 0))
        folder = Node(ids.next(), "anim", "group")
        folder.children = [cel, other]
        tree = refresh([sky2, folder])
        set_animation(tree, folder.id, True)
        front = composite_scene(4, 4, tree)
        self.assertEqual(front.getpixel((0, 0)), (255, 0, 0, 255))
        held = composite_scene(4, 4, tree, cel.id)
        self.assertEqual(held.getpixel((0, 0)), (0, 180, 0, 255))
        self.assertEqual(held.getpixel((1, 1)), (128, 128, 128, 255))
        self.assertEqual(held.getpixel((3, 3)), (0, 180, 0, 255))


def _half_canvas():
    """4x4 RGBA. The left half is red, the right half is blue."""
    raw = bytearray(4 * 4 * 4)
    for y in range(4):
        for x in range(4):
            index = (y * 4 + x) * 4
            raw[index:index + 4] = b"\xff\x00\x00\xff" if x < 2 else b"\x00\x00\xff\xff"
    return bytes(raw)


class CutTests(unittest.TestCase):
    def test_a_rect_cut_moves_every_layer_in_the_folder_and_leaves_the_rest(self):
        ids = Ids()
        door = Node(ids.next(), "door", "layer", raster=Raster(0, 0, 4, 4, _half_canvas()))
        cushion = Node(ids.next(), "cushion", "layer", raster=Raster(0, 0, 4, 4, _half_canvas()))
        nested = Node(ids.next(), "seat", "group")
        nested.children = [cushion]
        couch = Node(ids.next(), "couch", "group")
        couch.children = [nested, door]
        sky = Node(ids.next(), "sky", "layer", raster=Raster(0, 0, 4, 4, bytes([0, 255, 0, 255]) * 16))
        layers = refresh([couch, sky])
        points = [(0, 0), (2, 0), (2, 4), (0, 4)]
        made, count = cut_folder(layers, couch.id, points, 4, 4)
        self.assertEqual(count, 2)
        self.assertEqual(made.name, "Cut")
        self.assertEqual([node.name for node in layers], ["couch", "Cut", "sky"])
        self.assertEqual([node.name for node in made.children], ["seat", "door"])
        self.assertEqual([node.name for node in made.children[0].children], ["cushion"])
        self.assertEqual(parent_of(layers, door.id).name, "couch")
        self.assertEqual(sky.raster.rgba[3], 255)
        self.assertEqual(sky.raster.rgba[0], 0)
        # Left pixels left the couch. The far right stayed. The edge column belongs to the mask.
        self.assertEqual(door.raster.rgba[3], 0)
        self.assertEqual(door.raster.rgba[(3 * 4) + 3], 255)
        cut_door = made.children[1]
        self.assertEqual(cut_door.raster.rgba[3], 255)
        self.assertEqual(cut_door.raster.x, 0)
        again, _count = cut_folder(layers, couch.id, [(2, 0), (4, 0), (4, 4), (2, 4)], 4, 4)
        self.assertEqual(again.name, "Cut 2")

    def test_a_cut_keeps_clipping_masks_on_their_base(self):
        ids = Ids()
        raw = bytearray(4 * 4 * 4)
        for y in range(4):
            for x in range(2):
                raw[(y * 4 + x) * 4:(y * 4 + x) * 4 + 4] = b"\xff\x00\x00\xff"
        base = Node(ids.next(), "base", "layer", raster=Raster(0, 0, 4, 4, bytes(raw)))
        shade = Node(ids.next(), "shade", "layer", clip=True, raster=fill(0, 0, 255))
        block = Node(ids.next(), "block", "group")
        block.children = [base, shade]
        glow = Node(ids.next(), "glow", "layer", clip=True, raster=fill(0, 255, 0))
        city = Node(ids.next(), "city", "group")
        city.children = [block, glow]
        layers = refresh([city])
        before = composite_scene(4, 4, layers)
        made, count = cut_folder(layers, city.id, [(0, 0), (4, 0), (4, 4), (0, 4)], 4, 4)
        self.assertEqual(count, 3)
        self.assertEqual([node.name for node in made.children], ["block", "glow"])
        self.assertFalse(made.children[0].clip)
        self.assertEqual([node.name for node in made.children[0].children], ["base", "shade"])
        self.assertFalse(made.children[0].children[0].clip)
        self.assertTrue(made.children[0].children[1].clip)
        self.assertTrue(made.children[1].clip)
        self.assertTrue(shade.clip)
        after = composite_scene(4, 4, [made])
        self.assertEqual(after.getpixel((0, 0)), before.getpixel((0, 0)))
        self.assertEqual(after.getpixel((3, 0)), before.getpixel((3, 0)))
        self.assertEqual(after.getpixel((3, 0))[3], 0)

    def test_a_clip_without_its_base_stays_on_the_original(self):
        ids = Ids()
        raw = bytearray(4 * 4 * 4)
        for y in range(4):
            raw[(y * 4) * 4:(y * 4) * 4 + 4] = b"\xff\x00\x00\xff"
        base = Node(ids.next(), "base", "layer", raster=Raster(0, 0, 4, 4, bytes(raw)))
        shade = Node(ids.next(), "shade", "layer", clip=True, raster=fill(0, 0, 255))
        city = Node(ids.next(), "city", "group")
        city.children = [base, shade]
        layers = refresh([city])
        made, count = cut_folder(layers, city.id, [(2, 0), (4, 0), (4, 4), (2, 4)], 4, 4)
        self.assertIsNone(made)
        self.assertEqual(count, 0)
        # (2, 0) is inside the selection. It stays, because the base was not cut.
        self.assertEqual(shade.raster.rgba[(2 * 4) + 3], 255)
        self.assertEqual(base.raster.rgba[3], 255)
        self.assertEqual([node.name for node in layers], ["city"])

    def test_krita_inherit_alpha_is_the_clipping_mask(self):
        import xml.etree.ElementTree as ET
        from vmi_studio.kra import _read_box

        box = ET.fromstring(
            "<layers>"
            "<layer name=\"shade\" nodetype=\"paintlayer\" inheritalpha=\"1\" opacity=\"255\"/>"
            "<layer name=\"base\" nodetype=\"paintlayer\" inheritalpha=\"0\" opacity=\"255\"/>"
            "</layers>"
        )
        nodes = _read_box(box, Ids())
        self.assertEqual([node.name for node in nodes], ["base", "shade"])
        self.assertFalse(nodes[0].clip)
        self.assertTrue(nodes[1].clip)

    def test_delete_clears_the_selection_on_the_folder_only(self):
        ids = Ids()
        door = Node(ids.next(), "door", "layer", raster=Raster(0, 0, 4, 4, _half_canvas()))
        couch = Node(ids.next(), "couch", "group")
        couch.children = [door]
        sky = Node(ids.next(), "sky", "layer", raster=Raster(0, 0, 4, 4, bytes([0, 255, 0, 255]) * 16))
        layers = refresh([couch, sky])
        count = erase_folder(layers, couch.id, [(0, 0), (2, 0), (2, 4), (0, 4)], 4, 4)
        self.assertEqual(count, 1)
        self.assertEqual([node.name for node in layers], ["couch", "sky"])
        self.assertEqual(door.raster.rgba[3], 0)
        self.assertEqual(door.raster.rgba[(3 * 4) + 3], 255)
        self.assertEqual(sky.raster.rgba[0:4], b"\x00\xff\x00\xff")
        self.assertEqual(erase_folder(layers, couch.id, [(0, 0), (1, 0), (1, 1), (0, 1)], 4, 4), 0)

    def test_shift_x_cuts_the_folder_holding_the_selected_layer(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtWidgets import QApplication
        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        app = QApplication.instance() or QApplication([])
        window_mod.apply_style(app)
        win = window_mod.MainWindow()
        ids = Ids()
        door = Node(ids.next(), "door", "layer", raster=Raster(0, 0, 4, 4, _half_canvas()))
        couch = Node(ids.next(), "couch", "group")
        couch.children = [door]
        art = ArtFile("room.clip", "", "clip", 4, 4, refresh([couch]), "")
        win.show_file(art, restore=False)
        win.show()
        app.processEvents()
        win.select_only(door)
        win.toggle_cut()
        self.assertTrue(win.picture.cut_active)
        self.assertEqual(win._cut_target, couch.id)
        self.assertTrue(win.cut_bar.isVisible())
        labels = [button.text() for button in win.cut_bar.findChildren(window_mod.QPushButton)]
        self.assertEqual(labels, ["Rect", "Lasso", "Polygon", "Cut to folder", "Delete", "Done"])
        win._set_cut_tool("lasso")
        self.assertEqual(win.picture.cut_tool, "lasso")
        win._set_cut_tool("poly")
        self.assertEqual(win.picture.cut_tool, "poly")
        win.picture.selection = [(0, 0), (2, 0), (2, 4), (0, 4)]

        class TakeCut:
            def __init__(self, _parent, name):
                self._name = name

            def exec(self):
                return window_mod.QDialog.DialogCode.Accepted

            def name(self):
                return self._name

            def wants_object(self):
                return False

        real_cut = window_mod.CutFolderDialog
        window_mod.CutFolderDialog = TakeCut
        try:
            win.cut_selection()
        finally:
            window_mod.CutFolderDialog = real_cut
        self.assertEqual([node.name for node in win.art.layers], ["couch", "Cut"])
        self.assertIn("unchanged", win.status.currentMessage())
        win.picture.selection = [(2, 0), (4, 0), (4, 4), (2, 4)]
        win.delete_selection()
        self.assertIn("Cleared", win.status.currentMessage())
        self.assertEqual([node.name for node in win.art.layers], ["couch", "Cut"])
        win.toggle_cut()
        self.assertFalse(win.picture.cut_active)
        self.assertFalse(win.cut_bar.isVisible())
        from PySide6.QtCore import QEvent, Qt
        from PySide6.QtGui import QKeyEvent

        win.select_only(win.art.layers[0])
        shift_x = QKeyEvent(QEvent.Type.KeyPress, Qt.Key_X, Qt.ShiftModifier)
        self.assertTrue(win.eventFilter(win.picture, shift_x))
        self.assertTrue(win.picture.cut_active)
        self.assertEqual(win._cut_target, win.art.layers[0].id)
        win._set_cut_tool("poly")
        win.picture._draft = [(0, 0), (3, 0), (3, 3)]
        enter = QKeyEvent(QEvent.Type.KeyPress, Qt.Key_Return, Qt.NoModifier)
        self.assertTrue(win.eventFilter(win.picture, enter))
        self.assertEqual(win.picture.selection, [(0, 0), (3, 0), (3, 3)])
        win.close()

    def test_cut_to_folder_asks_for_a_name_and_can_make_an_object(self):
        os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        from PySide6.QtWidgets import QApplication, QDialog
        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        app = QApplication.instance() or QApplication([])
        window_mod.apply_style(app)
        win = window_mod.MainWindow()
        ids = Ids()
        door = Node(ids.next(), "panel", "layer", raster=Raster(0, 0, 4, 4, _half_canvas()))
        couch = Node(ids.next(), "couch", "group")
        couch.children = [door]
        art = ArtFile("room.clip", "", "clip", 4, 4, refresh([couch]), "")
        win.show_file(art, restore=False)
        win.show()
        app.processEvents()
        win.select_only(couch)
        win.toggle_cut()
        win.picture.selection = [(0, 0), (2, 0), (2, 4), (0, 4)]
        asked = window_mod.CutFolderDialog(win, "Cut")
        self.assertEqual(asked.windowTitle(), "Cut to folder")
        self.assertEqual(asked.edit.text(), "Cut")
        self.assertEqual(asked.as_object.text(), "Make this an object")
        self.assertFalse(asked.wants_object())
        asked.edit.setText("   ")
        asked._accept()
        self.assertNotEqual(asked.result(), QDialog.DialogCode.Accepted)
        asked.close()

        class Answer:
            def __init__(self, _parent, _name):
                pass

            def exec(self):
                return QDialog.DialogCode.Accepted if self.ok else QDialog.DialogCode.Rejected

            ok = False
            label = "door"
            make = False

            def name(self):
                return self.label

            def wants_object(self):
                return self.make

        real_cut = window_mod.CutFolderDialog
        window_mod.CutFolderDialog = Answer
        try:
            win.cut_selection()
            self.assertEqual([node.name for node in win.art.layers], ["couch"])
            Answer.ok = True
            Answer.label = "seat"
            Answer.make = True
            win.objects.append(DeskObject("o1", "seat", [], explicit=True))
            win.cut_selection()
            self.assertEqual([node.name for node in win.art.layers], ["couch"])
            self.assertIn("already an object", win.status.currentMessage())
            win.objects.clear()
            Answer.label = "door"
            win.cut_selection()
        finally:
            window_mod.CutFolderDialog = real_cut
        made = next(node for node in win.art.layers if node.kind == "group" and node.name == "door")
        self.assertEqual([node.name for node in win.art.layers], ["couch", "door"])
        self.assertEqual(len(win.objects), 1)
        obj = win.objects[0]
        self.assertEqual(obj.name, "door")
        self.assertTrue(obj.explicit)
        self.assertEqual(obj.kind, "")
        self.assertEqual(obj.origin, "static:%s" % made.id)
        self.assertEqual(obj.layer_ids, [child.id for child in made.children])
        self.assertNotIn(door.id, obj.layer_ids)
        self.assertEqual(obj.assign[made.children[0].id], (0, 0))
        self.assertIn("object list", win.status.currentMessage())
        self.assertIn("unchanged", win.status.currentMessage())
        win.close()

    def test_the_note_names_clips_and_blend_modes(self):
        ids = Ids()
        base = Node(ids.next(), "base", "layer", raster=fill(255, 255, 255, w=1, h=1))
        shade = Node(ids.next(), "shade", "layer", blend="mul ", clip=True, raster=fill(0, 0, 0, w=1, h=1))
        light = Node(ids.next(), "light", "layer", blend="lddg", clip=True, raster=fill(255, 255, 255, w=1, h=1))
        light.shapes = False
        weird = Node(ids.next(), "weird", "layer", blend="nope", raster=fill(0, 0, 255, w=1, h=1))
        layers = refresh([base, shade, light, weird])
        note = blend_summary(layers)
        self.assertIn("2 clipping masks.", note)
        self.assertIn("multiply", note)
        self.assertIn("add (glow)", note)
        self.assertIn("Drawn as normal: nope.", note)
        set_visible(layers, shade.id, False)
        set_visible(layers, light.id, False)
        self.assertNotIn("clipping", blend_summary(layers))

    def test_a_tall_multiply_stays_even_across_blend_bands(self):
        ids = Ids()
        white = Node(ids.next(), "white", "layer", raster=Raster(0, 0, 1, 200, bytes((255, 255, 255, 255)) * 200))
        grey = Node(ids.next(), "grey", "layer", blend="multiply", raster=Raster(0, 0, 1, 200, bytes((128, 128, 128, 255)) * 200))
        image = composite_image(1, 200, refresh([white, grey]))
        for y in (0, 127, 128, 199):
            self.assertEqual(image.getpixel((0, y)), (128, 128, 128, 255))

    def test_a_folder_plate_is_the_paint_and_a_solo_skips_the_rest(self):
        import vmi_studio.composite as composite_mod

        ids = Ids()
        bg = Node(ids.next(), "bg", "layer", raster=Raster(0, 0, 1, 1, bytes((255, 0, 0, 255))))
        folder = Node(ids.next(), "room", "group")
        children = []
        for i in range(30):
            group = Node(ids.next(), "part%d" % i, "group")
            group.children = [Node(ids.next(), "px%d" % i, "layer", raster=Raster(10 + i, 20, 1, 1, bytes((0, 0, 255, 255))))]
            children.append(group)
        folder.children = children
        layers = refresh([bg, folder])
        set_solo(layers, folder.id, True)
        sizes = []
        real = Image.new

        def spy(mode, size, color=0):
            sizes.append((size[0], size[1]))
            return real(mode, size, color)

        composite_mod.Image.new = spy
        try:
            image = composite_scene(5000, 2500, layers)
        finally:
            composite_mod.Image.new = real
        self.assertEqual(image.getpixel((0, 0))[3], 0)
        self.assertEqual(image.getpixel((10, 20)), (0, 0, 255, 255))
        self.assertEqual(image.getpixel((39, 20)), (0, 0, 255, 255))
        big = [size for size in sizes if size[0] * size[1] > 10000]
        self.assertEqual(big, [(5000, 2500)])

    def test_the_fit_view_keeps_a_clip_inside_its_base(self):
        ids = Ids()
        ground = Node(ids.next(), "ground", "layer", raster=fill(0, 255, 0, w=200, h=100))
        base = Node(ids.next(), "base", "layer", raster=fill(255, 0, 0, w=40, h=40, x=40, y=40))
        blue = Node(ids.next(), "mask", "layer", clip=True, raster=fill(0, 0, 255, w=200, h=100))
        image = composite_scene(200, 100, refresh([ground, base, blue]), max_edge=100)
        self.assertEqual(image.size, (100, 50))
        self.assertEqual(image.getpixel((0, 0)), (0, 255, 0, 255))
        self.assertEqual(image.getpixel((25, 25)), (0, 0, 255, 255))
        self.assertEqual(image.getpixel((45, 25)), (0, 255, 0, 255))

    def test_the_menu_names_the_mode_and_a_choice_changes_the_mix(self):
        from vmi_studio.composite import assign_blend, blend_label
        from vmi_studio.document import apply_blends, blend_rows

        self.assertEqual(blend_label("mul ", True), "Multiply")
        self.assertEqual(blend_label("add (glow)", True), "Add (glow)")
        self.assertEqual(blend_label("lddg", False), "Add (glow)")
        self.assertEqual(blend_label("div ", False), "Glow dodge")
        self.assertEqual(blend_label("norm", True), "Normal")
        self.assertEqual(blend_label("pass through", True), "Pass through")
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=pixel(255, 0, 0))
        door = Node(ids.next(), "door", "layer", raster=pixel(255, 255, 255))
        layers = refresh([sky, door])
        self.assertEqual(composite_image(1, 1, layers).getpixel((0, 0))[:3], (255, 255, 255))
        assign_blend(door, "multiply")
        self.assertTrue(door.shapes)
        self.assertEqual(composite_image(1, 1, layers).getpixel((0, 0))[:3], (255, 0, 0))
        assign_blend(door, "add (glow)")
        self.assertFalse(door.shapes)
        rows = blend_rows(layers)
        door.blend = "normal"
        door.shapes = True
        apply_blends(layers, rows)
        self.assertEqual(door.blend, "add (glow)")
        self.assertFalse(door.shapes)


class TrackSpeedTests(unittest.TestCase):
    """Mute and solo reuse folder plates. The picture stays the same composite."""

    def _room(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=fill(255, 0, 0, w=2, h=1))
        door_px = Node(ids.next(), "door", "layer", raster=fill(0, 0, 255, w=1, h=1, x=0, y=0))
        sign_px = Node(ids.next(), "sign", "layer", raster=fill(0, 255, 0, w=1, h=1, x=1, y=0))
        door = Node(ids.next(), "Door", "group")
        door.children = [door_px]
        sign = Node(ids.next(), "Sign", "group")
        sign.children = [sign_px]
        room = Node(ids.next(), "room", "group")
        room.children = [door, sign]
        layers = refresh([sky, room])
        return layers, sky, door_px, sign_px, door, sign, room

    def test_a_repeat_reuses_folders_and_unmute_comes_back(self):
        reset_plate_cache()
        layers, _sky, door_px, _sign_px, _door, sign, _room = self._room()
        first = composite_scene(2, 1, layers)
        self.assertEqual(first.getpixel((0, 0)), (0, 0, 255, 255))
        self.assertEqual(first.getpixel((1, 0)), (0, 255, 0, 255))
        self.assertGreater(plate_stats()["misses"], 0)
        second = composite_scene(2, 1, layers)
        self.assertEqual(list(first.getdata()), list(second.getdata()))
        self.assertGreater(plate_stats()["hits"], 0)
        self.assertEqual(plate_stats()["misses"], 0)
        set_mute(layers, door_px.id, True)
        muted = composite_scene(2, 1, layers)
        self.assertEqual(muted.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(muted.getpixel((1, 0)), (0, 255, 0, 255))
        self.assertGreater(plate_stats()["hits"], 0)
        set_mute(layers, door_px.id, False)
        restored = composite_scene(2, 1, layers)
        self.assertEqual(list(restored.getdata()), list(first.getdata()))
        self.assertGreater(plate_stats()["hits"], 0)
        self.assertFalse(sign.mute)

    def test_a_flag_snapshot_mutes_without_touching_the_layer(self):
        reset_plate_cache()
        layers, _sky, door_px, _sign_px, _door, _sign, _room = self._room()
        first = composite_scene(2, 1, layers)
        flags = capture_flags(layers)
        visible, _mute, solo = flags[door_px.id]
        flags[door_px.id] = (visible, True, solo)
        muted = composite_scene(2, 1, layers, flags=flags)
        self.assertFalse(door_px.mute)
        self.assertEqual(muted.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(muted.getpixel((1, 0)), (0, 255, 0, 255))
        again = composite_scene(2, 1, layers)
        self.assertEqual(list(again.getdata()), list(first.getdata()))
        self.assertGreater(plate_stats()["hits"], 0)

    def test_a_clip_is_not_stamped_into_the_cached_folder(self):
        reset_plate_cache()
        ids = Ids()
        ground = Node(ids.next(), "ground", "layer", raster=fill(0, 255, 0, w=2, h=2))
        ink = Node(ids.next(), "ink", "layer", raster=fill(255, 0, 0, w=2, h=2))
        base = Node(ids.next(), "base", "group")
        base.children = [ink]
        mask = Node(ids.next(), "mask", "layer", clip=True, raster=fill(255, 255, 255, a=128, w=2, h=2))
        layers = refresh([ground, base, mask])
        first = composite_scene(2, 2, layers)
        second = composite_scene(2, 2, layers)
        self.assertEqual(list(first.getdata()), list(second.getdata()))
        self.assertGreater(plate_stats()["hits"], 0)
        pixel = first.getpixel((0, 0))
        self.assertEqual(pixel[0], 255)
        self.assertGreater(pixel[1], 0)
        self.assertLess(pixel[1], 255)
        self.assertEqual(pixel[3], 255)
        # The folder plate is the red ink. Export still paints that red, then the clip.
        exported = composite_image(2, 2, [ink])
        self.assertEqual(exported.getpixel((0, 0)), (255, 0, 0, 255))

    def test_pass_through_stays_exact_while_a_sibling_folder_is_kept(self):
        reset_plate_cache()
        ids = Ids()
        red = Node(ids.next(), "red", "layer", raster=fill(255, 0, 0, w=1, h=1, x=0, y=0))
        grey = Node(ids.next(), "grey", "layer", blend="multiply", raster=fill(128, 128, 128, w=1, h=1, x=0, y=0))
        opened = Node(ids.next(), "opened", "group", blend="pass through")
        opened.children = [grey]
        blue = Node(ids.next(), "blue", "layer", raster=fill(0, 0, 255, w=1, h=1, x=1, y=0))
        kept = Node(ids.next(), "kept", "group")
        kept.children = [blue]
        layers = refresh([red, opened, kept])
        first = composite_scene(2, 1, layers)
        self.assertEqual(first.getpixel((0, 0)), (128, 0, 0, 255))
        self.assertEqual(first.getpixel((1, 0)), (0, 0, 255, 255))
        second = composite_scene(2, 1, layers)
        self.assertEqual(list(first.getdata()), list(second.getdata()))
        self.assertGreater(plate_stats()["hits"], 0)
        set_mute(layers, grey.id, True)
        muted = composite_scene(2, 1, layers)
        self.assertEqual(muted.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(muted.getpixel((1, 0)), (0, 0, 255, 255))
        self.assertGreater(plate_stats()["hits"], 0)
        isolated = Node(ids.next(), "closed", "group")
        isolated.children = [Node(ids.next(), "grey", "layer", blend="multiply", raster=fill(128, 128, 128, w=1, h=1))]
        blocked = composite_scene(1, 1, refresh([
            Node(ids.next(), "red", "layer", raster=fill(255, 0, 0, w=1, h=1)),
            isolated,
        ]))
        self.assertEqual(blocked.getpixel((0, 0)), (128, 128, 128, 255))

    def test_solo_of_a_folder_and_solo_of_a_leaf_are_different(self):
        reset_plate_cache()
        layers, _sky, door_px, sign_px, door, _sign, room = self._room()
        set_solo(layers, room.id, True)
        folder = composite_scene(2, 1, layers)
        self.assertEqual(folder.getpixel((0, 0)), (0, 0, 255, 255))
        self.assertEqual(folder.getpixel((1, 0)), (0, 255, 0, 255))
        set_solo(layers, room.id, False)
        set_solo(layers, door_px.id, True)
        leaf = composite_scene(2, 1, layers)
        self.assertEqual(leaf.getpixel((0, 0)), (0, 0, 255, 255))
        self.assertEqual(leaf.getpixel((1, 0))[3], 0)
        set_mute(layers, room.id, True)
        covered = composite_scene(2, 1, layers)
        # The leaf is still soloed, and the muted folder drops it. Sky stays out.
        self.assertEqual(covered.getpixel((0, 0))[3], 0)
        self.assertEqual(covered.getpixel((1, 0))[3], 0)
        self.assertTrue(door_px.solo)
        set_mute(layers, room.id, False)
        back = composite_scene(2, 1, layers)
        self.assertEqual(list(back.getdata()), list(leaf.getdata()))
        set_solo(layers, door.id, True)
        nested = composite_scene(2, 1, layers)
        self.assertEqual(nested.getpixel((0, 0)), (0, 0, 255, 255))
        self.assertEqual(nested.getpixel((1, 0))[3], 0)
        self.assertTrue(sign_px.visible)

    def test_clip_kra_and_xcf_mute_round_trips(self):
        """The same mute contract on a Clip file, a Krita file, and an XCF."""
        reset_plate_cache()
        if os.path.isfile(BUTTONS):
            from vmi_studio.openers import open_drawing

            art = open_drawing(BUTTONS)
            self._mute_round_trip(art.layers, art.width, art.height, "clip")
        if os.path.isfile(GAMER):
            art = kra.load(GAMER)
            self._mute_round_trip(art.layers, art.width, art.height, "kra")
        xcf = parse(_xcf_red(), "dot.xcf")
        layer = xcf.layers[0]
        before = composite_scene(xcf.width, xcf.height, xcf.layers)
        self.assertEqual(before.getpixel((0, 0))[:3], (255, 0, 0))
        set_mute(xcf.layers, layer.id, True)
        muted = composite_scene(xcf.width, xcf.height, xcf.layers)
        self.assertEqual(muted.getpixel((0, 0))[3], 0)
        set_mute(xcf.layers, layer.id, False)
        restored = composite_scene(xcf.width, xcf.height, xcf.layers)
        self.assertEqual(list(restored.getdata()), list(before.getdata()))

    def _mute_round_trip(self, layers, width, height, kind):
        reset_plate_cache()
        edge = 160
        first = composite_scene(width, height, layers, max_edge=edge)
        self.assertIsNotNone(first, kind)
        again = composite_scene(width, height, layers, max_edge=edge)
        self.assertEqual(first.tobytes(), again.tobytes(), kind)
        changed = False
        tries = 0
        for node in reversed(preview_paint(layers)):
            if getattr(node, "raster", None) is None:
                continue
            set_mute(layers, node.id, True)
            muted = composite_scene(width, height, layers, max_edge=edge)
            set_mute(layers, node.id, False)
            restored = composite_scene(width, height, layers, max_edge=edge)
            self.assertEqual(first.tobytes(), restored.tobytes(), kind)
            tries += 1
            if muted is not None and muted.tobytes() != first.tobytes():
                changed = True
                break
            if tries >= 4:
                break
        self.assertTrue(changed, kind)


class PickTests(unittest.TestCase):
    def test_the_front_layer_wins_and_a_hole_falls_through(self):
        ids = Ids()
        back = Node(ids.next(), "back", "layer", raster=fill(255, 0, 0, w=4, h=4))
        front = Node(ids.next(), "front", "layer", raster=Raster(1, 1, 1, 1, bytes((0, 0, 255, 255))))
        layers = refresh([back, front])
        self.assertEqual(pick_layer(4, 4, layers, 1, 1), front.id)
        self.assertEqual(pick_layer(4, 4, layers, 0, 0), back.id)
        self.assertIsNone(pick_layer(4, 4, layers, 9, 0))

    def test_a_clip_stays_inside_its_base(self):
        ids = Ids()
        ground = Node(ids.next(), "ground", "layer", raster=fill(0, 255, 0, w=8, h=8))
        base = Node(ids.next(), "base", "layer", raster=fill(255, 0, 0, w=2, h=2, x=3, y=3))
        mask = Node(ids.next(), "mask", "layer", clip=True, raster=fill(0, 0, 255, w=8, h=8))
        layers = refresh([ground, base, mask])
        self.assertEqual(pick_layer(8, 8, layers, 0, 0), ground.id)
        self.assertEqual(pick_layer(8, 8, layers, 3, 3), mask.id)
        self.assertEqual(pick_layer(8, 8, layers, 4, 4), mask.id)

    def test_hidden_muted_and_unsoloed_layers_are_not_there(self):
        ids = Ids()
        back = Node(ids.next(), "back", "layer", raster=fill(255, 0, 0, w=2, h=2))
        front = Node(ids.next(), "front", "layer", raster=fill(0, 0, 255, w=2, h=2))
        hidden = refresh([back, front])
        set_visible(hidden, front.id, False)
        self.assertEqual(pick_layer(2, 2, hidden, 0, 0), back.id)
        muted = refresh([
            Node(ids.next(), "back", "layer", raster=fill(255, 0, 0, w=2, h=2)),
            Node(ids.next(), "front", "layer", raster=fill(0, 0, 255, w=2, h=2)),
        ])
        set_mute(muted, muted[1].id, True)
        self.assertEqual(pick_layer(2, 2, muted, 0, 0), muted[0].id)
        soloed = refresh([
            Node(ids.next(), "back", "layer", raster=fill(255, 0, 0, w=2, h=2)),
            Node(ids.next(), "front", "layer", raster=fill(0, 0, 255, w=2, h=2)),
        ])
        set_solo(soloed, soloed[0].id, True)
        self.assertEqual(pick_layer(2, 2, soloed, 0, 0), soloed[0].id)

    def test_a_click_inside_a_folder_is_the_layer(self):
        ids = Ids()
        sky = Node(ids.next(), "sky", "layer", raster=fill(0, 255, 0, w=4, h=4))
        folder = Node(ids.next(), "booth", "group")
        sign = Node(ids.next(), "sign", "layer", raster=fill(0, 0, 255, w=2, h=2, x=1, y=1))
        folder.children = [sign]
        layers = refresh([sky, folder])
        self.assertEqual(pick_layer(4, 4, layers, 1, 1), sign.id)
        self.assertEqual(pick_layer(4, 4, layers, 0, 0), sky.id)

    def test_an_animation_click_uses_the_cel_on_screen(self):
        ids = Ids()
        folder = Node(ids.next(), "walk", "group")
        red = Node(ids.next(), "red", "layer", raster=fill(255, 0, 0, w=2, h=2))
        blue = Node(ids.next(), "blue", "layer", raster=fill(0, 0, 255, w=2, h=2))
        folder.children = [red, blue]
        layers = refresh([folder])
        set_animation(layers, folder.id, True)
        self.assertEqual(pick_layer(2, 2, layers, 0, 0), blue.id)
        self.assertEqual(pick_layer(2, 2, layers, 0, 0, red.id), red.id)


class NavigateTests(unittest.TestCase):
    def test_the_wheel_matches_kooldraw(self):
        import math

        self.assertAlmostEqual(wheel_factor(100, 0), math.exp(-0.18), places=5)
        self.assertGreater(wheel_factor(-100, 0), 1)
        self.assertAlmostEqual(wheel_factor(10000, 0), 1 / 1.35, places=5)
        self.assertAlmostEqual(wheel_factor(-10000, 0), 1.35, places=5)
        self.assertEqual(wheel_factor(0, 0), 1)
        self.assertEqual(clamp_zoom(0), 1)
        self.assertEqual(clamp_zoom(1000), 64)
        self.assertEqual(format_zoom(1), "1")
        self.assertEqual(format_zoom(1.25), "1.3")

    def test_pan_rotate_and_pick_follow_the_mouse_buttons(self):
        self.assertEqual(gesture(0, space=True), "pan")
        self.assertEqual(gesture(1), "pan")
        self.assertEqual(gesture(0, shift=True), "rotate")
        self.assertEqual(gesture(0, shift=True, alt=True), "pick")
        self.assertEqual(gesture(0, shift=True, pointer="pen"), "pick")
        self.assertEqual(gesture(0, shift=True, ctrl=True), "none")
        self.assertEqual(gesture(2), "none")
        self.assertEqual(gesture(0), "pick")

    def test_a_turned_view_maps_back_to_the_pixel(self):
        # 90° clockwise puts image pixel (9, 5) on screen (50, 54).
        hit = view_to_image(50, 54, 100, 100, 10, 10, 1, 0, 0, 90)
        self.assertIsNotNone(hit)
        self.assertAlmostEqual(hit[0], 9, places=5)
        self.assertAlmostEqual(hit[1], 5, places=5)
        panned = view_to_image(60, 50, 100, 100, 10, 10, 1, 10, 0, 0)
        self.assertAlmostEqual(panned[0], 5, places=5)
        self.assertAlmostEqual(panned[1], 5, places=5)
        self.assertIsNone(view_to_image(0, 0, 100, 100, 10, 10, 1, 0, 0, 0))


class HistoryTests(unittest.TestCase):
    def _doc(self):
        ids = Ids()
        base = Node(ids.next(), "base", "layer", raster=Raster(0, 0, 4, 4, _half_canvas()))
        door = Node(ids.next(), "door", "layer", clip=True, raster=Raster(0, 0, 4, 4, _half_canvas()))
        couch = Node(ids.next(), "couch", "group")
        couch.children = [base, door]
        art = ArtFile("room.clip", "", "clip", 4, 4, refresh([couch]), "")
        doc = type("Doc", (), {})()
        doc.art = art
        doc.objects = []
        doc.playlist = ""
        return doc, base, door

    def test_undo_puts_a_cut_and_its_clip_back(self):
        from vmi_studio.history import History

        doc, base, door = self._doc()
        before = door.raster.rgba
        history = History()
        with history.editing(doc, "the cut"):
            made, count = cut_folder(doc.art.layers, doc.art.layers[0].id, [(0, 0), (2, 0), (2, 4), (0, 4)], 4, 4)
        self.assertEqual(count, 2)
        self.assertEqual([node.name for node in made.children], ["base", "door"])
        self.assertTrue(made.children[1].clip)
        self.assertEqual(door.raster.rgba[3], 0)
        self.assertEqual(history.undo(doc), "the cut")
        self.assertEqual([node.name for node in doc.art.layers], ["couch"])
        self.assertEqual(base.raster.rgba, before)
        self.assertEqual(door.raster.rgba, before)
        self.assertTrue(door.clip)
        self.assertEqual(history.redo(doc), "the cut")
        self.assertEqual([node.name for node in doc.art.layers], ["couch", "Cut"])
        self.assertTrue(doc.art.layers[1].children[1].clip)
        with history.editing(doc, "mute"):
            doc.art.layers[0].mute = True
        self.assertEqual(history.redo(doc), None)
        self.assertEqual(history.undo(doc), "mute")
        self.assertFalse(doc.art.layers[0].mute)

    def test_a_two_finger_tap_undoes_and_a_drag_does_not(self):
        from vmi_studio.history import FingerChords

        taps = FingerChords()
        self.assertIsNone(taps.update([(1, 0, 0, True, True), (2, 8, 0, True, True)], 0))
        self.assertEqual(taps.update([(1, 0, 0, False, True), (2, 8, 0, False, True)], 120), "undo")
        three = FingerChords()
        three.update([(1, 0, 0, True, True), (2, 4, 0, True, True), (3, 8, 0, True, True)], 0)
        self.assertEqual(
            three.update([(1, 0, 0, False, True), (2, 4, 0, False, True), (3, 8, 0, False, True)], 80),
            "redo",
        )
        drag = FingerChords()
        drag.update([(1, 0, 0, True, True), (2, 4, 0, True, True)], 0)
        drag.update([(1, 80, 0, True, True), (2, 84, 0, True, True)], 40)
        self.assertIsNone(drag.update([(1, 80, 0, False, True), (2, 84, 0, False, True)], 90))
        pen = FingerChords()
        pen.update([(1, 0, 0, True, False), (2, 4, 0, True, False)], 0)
        self.assertIsNone(pen.update([(1, 0, 0, False, False), (2, 4, 0, False, False)], 50))
        one = FingerChords()
        one.update([(1, 0, 0, True, True)], 0)
        self.assertIsNone(one.update([(1, 1, 0, False, True)], 30))


class BlueprintTests(unittest.TestCase):
    def test_a_blueprint_keeps_the_arrangement_the_clip_and_the_objects(self):
        import os
        import tempfile
        import zipfile

        from vmi_studio.vmib import MIME, load_blueprint, write_blueprint

        ids = Ids()
        base = Node(ids.next(), "base", "layer", raster=fill(255, 0, 0))
        shade = Node(ids.next(), "shade", "layer", clip=True, blend="multiply", raster=fill(0, 0, 255, 128))
        block = Node(ids.next(), "block", "group", blend="pass through")
        block.children = [base, shade]
        glow = Node(ids.next(), "glow", "layer", clip=True, raster=fill(0, 255, 0))
        glow.shapes = False
        glow.mute = True
        city = Node(ids.next(), "city", "group")
        city.children = [block, glow]
        sky = Node(ids.next(), "sky", "layer", raster=fill(0, 0, 0, 0))
        layers = refresh([city, sky])
        made, _count = cut_folder(layers, city.id, [(0, 0), (4, 0), (4, 4), (0, 4)], 4, 4, "cityscape")
        art = ArtFile("BEETL RADIO STATION.clip", "/tmp/radio.clip", "clip", 4, 4, layers, "")
        obj = DeskObject(
            "o3", "cityscape", [child.id for child in walk([made]) if child.kind == "layer"],
            origin="static:%s" % made.id, explicit=True, parent="", stack=4,
            role="target", kind="warp target", warp_layer="warp map", sound="sfx:tik",
        )
        obj.labels = {"000000": "rest"}
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "radio.vmib")
        try:
            write_blueprint(path, art, [obj], playlist="battle", arrange_locked=False)
            with zipfile.ZipFile(path) as archive:
                first = archive.infolist()[0]
                self.assertEqual(first.filename, "mimetype")
                self.assertEqual(first.compress_type, zipfile.ZIP_STORED)
                self.assertEqual(archive.read("mimetype").decode("ascii"), MIME)
            loaded = load_blueprint(path)
        finally:
            import shutil
            shutil.rmtree(folder)
        self.assertEqual(loaded.kind, "vmib")
        self.assertEqual(loaded.project["source_file"], "BEETL RADIO STATION.clip")
        self.assertEqual(loaded.project["source_kind"], "clip")
        self.assertEqual(loaded.project["playlist"], "battle")
        self.assertFalse(loaded.project["arrange_locked"])
        self.assertEqual([node.name for node in loaded.layers], ["city", "cityscape", "sky"])
        cut = loaded.layers[1]
        self.assertEqual([node.name for node in cut.children], ["block", "glow"])
        self.assertEqual([node.name for node in cut.children[0].children], ["base", "shade"])
        shade_back = cut.children[0].children[1]
        glow_back = cut.children[1]
        self.assertTrue(shade_back.clip)
        self.assertEqual(shade_back.blend, "multiply")
        self.assertTrue(glow_back.clip)
        self.assertFalse(glow_back.shapes)
        self.assertTrue(glow_back.mute)
        self.assertEqual(shade_back.raster.rgba, fill(0, 0, 255, 128).rgba)
        saved = loaded.project["objects"]
        self.assertEqual(saved[0].name, "cityscape")
        self.assertEqual(saved[0].sound, "sfx:tik")
        self.assertEqual(saved[0].warp_layer, "warp map")
        self.assertEqual(saved[0].kind, "warp target")
        self.assertEqual(saved[0].stack, 4)
        self.assertEqual(saved[0].labels["000000"], "rest")
        self.assertIn(shade_back.id, saved[0].layer_ids)

    def test_ctrl_z_undoes_the_clip_button(self):
        from PySide6.QtCore import Qt
        from PySide6.QtTest import QTest

        ids = Ids()
        door = Node(ids.next(), "door", "layer", raster=pixel(0, 0, 255))
        app, win = WindowTests._desk(self, refresh([door]))
        win.show()
        app.processEvents()
        win.toggle_clip(door)
        self.assertTrue(door.clip)
        QTest.keyClick(win, Qt.Key_Z, Qt.ControlModifier)
        app.processEvents()
        self.assertFalse(door.clip)
        self.assertIn("Undid the clip", win.status.currentMessage())
        QTest.keyClick(win, Qt.Key_Z, Qt.ControlModifier | Qt.ShiftModifier)
        app.processEvents()
        self.assertTrue(door.clip)
        self.assertIn("Redid the clip", win.status.currentMessage())
        win.close()


class Buf:
    def __init__(self):
        self.data = bytearray()

    def add(self, blob):
        self.data += blob

    def u32(self, number):
        at = len(self.data)
        self.data += struct.pack(">I", number & 0xFFFFFFFF)
        return at

    def u64(self, number):
        at = len(self.data)
        self.data += struct.pack(">Q", number & 0xFFFFFFFFFFFFFFFF)
        return at

    def patch(self, at, number, wide):
        if wide:
            self.data[at:at + 8] = struct.pack(">Q", number)
        else:
            self.data[at:at + 4] = struct.pack(">I", number & 0xFFFFFFFF)

    def pointer(self, wide):
        return self.u64(0) if wide else self.u32(0)


def _xcf_red(version=0):
    wide = version >= 11
    buf = Buf()
    if version == 0:
        buf.add(b"gimp xcf file\0")
    else:
        buf.add(("gimp xcf v%03d\0" % version).encode("ascii"))
    buf.u32(2)
    buf.u32(2)
    buf.u32(0)
    if version >= 4:
        buf.u32(150)
    buf.u32(17)
    buf.u32(4)
    buf.u32(0)
    buf.u32(0)
    buf.u32(0)
    layer_ptr = buf.pointer(wide)
    buf.pointer(wide)
    layer_at = len(buf.data)
    buf.patch(layer_ptr, layer_at, wide)
    buf.u32(2)
    buf.u32(2)
    buf.u32(0)
    name = b"red\0"
    buf.u32(len(name))
    buf.add(name)
    buf.u32(8)
    buf.u32(4)
    buf.u32(1)
    buf.u32(6)
    buf.u32(4)
    buf.u32(255)
    buf.u32(15)
    buf.u32(8)
    buf.u32(0)
    buf.u32(0)
    buf.u32(0)
    buf.u32(0)
    hier_ptr = buf.pointer(wide)
    buf.pointer(wide)
    hier_at = len(buf.data)
    buf.patch(hier_ptr, hier_at, wide)
    buf.u32(2)
    buf.u32(2)
    buf.u32(4)
    level_ptr = buf.pointer(wide)
    buf.pointer(wide)
    level_at = len(buf.data)
    buf.patch(level_ptr, level_at, wide)
    buf.u32(2)
    buf.u32(2)
    tile_ptr = buf.pointer(wide)
    buf.pointer(wide)
    tile_at = len(buf.data)
    buf.patch(tile_ptr, tile_at, wide)
    buf.add(bytes([255, 0, 0, 255]) * 4)
    return bytes(buf.data)


if __name__ == "__main__":
    unittest.main()
