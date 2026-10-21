"""Exposure sheet rows saved in a VMI blueprint."""

import json
import os
import shutil
import tempfile
import unittest
import zipfile

from vmi_studio.document import ArtFile, Ids, Node, refresh
from vmi_studio.exposure import ExposureSheet
from vmi_studio.vmib import (
    VERSION,
    load_blueprint,
    load_sheet,
    prepare_blueprint,
    write_blueprint,
    write_parts,
)


def _art(layers):
    return ArtFile("shot.vmib", "", "vmib", 32, 32, refresh(layers), "")


def _held_sheet():
    """Holds, a reused frame, and a pool id that no longer has a slot."""
    sheet = ExposureSheet(base_seed=11)
    first = sheet.append_blank()
    sheet.hold(count=2)
    sheet.append_blank()
    sheet.append_blank()
    sheet.reuse(first, 3)
    sheet.delete(1)
    sheet.active = 2
    return sheet


class ExposureSheetRowTests(unittest.TestCase):
    def test_a_new_node_has_no_exposure_sheet(self):
        self.assertIsNone(Node("n0", "ink", "layer").exposure)

    def test_to_row_returns_slots_pool_issued_base_seed_and_active(self):
        sheet = _held_sheet()
        row = sheet.to_row()
        self.assertEqual(set(row), {"slots", "pool", "issued", "base_seed", "active"})
        self.assertEqual(row["slots"], ["000", "000", "000", "002"])
        self.assertEqual(row["pool"], ["000", "001", "002"])
        self.assertEqual(row["issued"], 3)
        self.assertEqual(row["base_seed"], 11)
        self.assertEqual(row["active"], 2)
        row["slots"].append("FFF")
        row["pool"].append("FFF")
        self.assertEqual(sheet.slots, ["000", "000", "000", "002"])
        self.assertEqual(sheet.pool, ["000", "001", "002"])
        self.assertEqual(ExposureSheet.from_row(sheet.to_row()).to_row(), sheet.to_row())

    def test_from_row_rejects_bad_ids_slots_missing_from_the_pool_and_a_short_issued_count(self):
        good = ExposureSheet()
        good.append_blank()
        good.append_blank()
        row = good.to_row()

        bad_id = dict(row)
        bad_id["pool"] = ["000", "001", "GG0"]
        bad_id["issued"] = 3
        with self.assertRaises(ValueError):
            ExposureSheet.from_row(bad_id)
        short_token = dict(row)
        short_token["slots"] = ["12"]
        short_token["pool"] = ["12"]
        short_token["issued"] = 1
        with self.assertRaises(ValueError):
            ExposureSheet.from_row(short_token)
        not_a_list = dict(row)
        not_a_list["slots"] = "000"
        with self.assertRaises(ValueError):
            ExposureSheet.from_row(not_a_list)

        missing = dict(row)
        missing["slots"] = ["000", "003"]
        with self.assertRaises(ValueError):
            ExposureSheet.from_row(missing)

        short = dict(row)
        short["issued"] = len(row["pool"]) - 1
        with self.assertRaises(ValueError):
            ExposureSheet.from_row(short)

    def test_from_row_copies_ids_and_does_not_invent_them(self):
        gapped = {
            "slots": ["000", "002", "000"],
            "pool": ["000", "002"],
            "issued": 5,
            "base_seed": 4,
            "active": 1,
        }
        loaded = ExposureSheet.from_row(gapped)
        self.assertEqual(loaded.to_row(), gapped)
        self.assertNotIn("001", loaded.pool)
        self.assertEqual([loaded.boil_seed(index) for index in range(3)], [4, 6, 4])

        empty = {
            "slots": [],
            "pool": [],
            "issued": 0,
            "base_seed": 0,
            "active": 0,
        }
        blank = ExposureSheet.from_row(empty)
        self.assertEqual(blank.slots, [])
        self.assertEqual(blank.pool, [])
        self.assertEqual(blank.to_row(), empty)
        with self.assertRaises(ValueError):
            ExposureSheet.from_row({"issued": 0, "base_seed": 0, "active": 0})

    def test_load_sheet_returns_none_for_a_missing_or_empty_row(self):
        self.assertIsNone(load_sheet(None))
        self.assertIsNone(load_sheet({}))
        sheet = _held_sheet()
        loaded = load_sheet(sheet.to_row())
        self.assertEqual(loaded.to_row(), sheet.to_row())
        with self.assertRaises(ValueError):
            load_sheet({"note": "no sheet"})

    def test_blueprint_writes_only_an_exposure_sheet_row(self):
        self.assertEqual(VERSION, 1)
        ids = Ids()
        bare = Node(ids.next(), "paint", "layer")
        bare.exposure = {"slots": ["000"], "pool": ["000"], "issued": 1, "base_seed": 0, "active": 0}
        empty = Node(ids.next(), "open", "layer")
        empty.exposure = ExposureSheet(base_seed=2)
        folder = Node(ids.next(), "shot", "group")
        folder.children = [bare, empty]
        document, _blobs = prepare_blueprint(_art([folder]), [])
        self.assertEqual(document["version"], 1)
        saved_folder = document["layers"][0]
        self.assertNotIn("exposure", saved_folder)
        self.assertNotIn("exposure", saved_folder["children"][0])
        self.assertEqual(saved_folder["children"][1]["exposure"], empty.exposure.to_row())
        self.assertEqual(
            set(saved_folder["children"][1]["exposure"]),
            {"slots", "pool", "issued", "base_seed", "active"},
        )

    def test_blueprint_round_trip_keeps_the_sheet_and_its_boil_seed(self):
        ids = Ids()
        bare = Node(ids.next(), "paint", "layer")
        cel = Node(ids.next(), "cel", "layer")
        sheet = _held_sheet()
        cel.exposure = sheet
        folder = Node(ids.next(), "shot", "group", animation=True)
        folder.children = [bare, cel]
        art = _art([folder])
        folder_path = tempfile.mkdtemp(prefix="vmi-exposure-")
        path = os.path.join(folder_path, "shot.vmib")
        try:
            write_blueprint(path, art, [])
            with zipfile.ZipFile(path) as archive:
                saved = json.loads(archive.read("blueprint.json"))
            loaded = load_blueprint(path)
        finally:
            shutil.rmtree(folder_path)
        self.assertEqual(saved["version"], 1)
        saved_folder = saved["layers"][0]
        self.assertNotIn("exposure", saved_folder)
        self.assertNotIn("exposure", saved_folder["children"][0])
        self.assertEqual(saved_folder["children"][1]["exposure"], sheet.to_row())
        shot = loaded.layers[0]
        self.assertIsNone(shot.exposure)
        self.assertIsNone(shot.children[0].exposure)
        kept = shot.children[1].exposure
        self.assertIsInstance(kept, ExposureSheet)
        self.assertEqual(kept.to_row(), sheet.to_row())
        self.assertEqual(kept.slots, sheet.slots)
        self.assertEqual(kept.pool, sheet.pool)
        self.assertEqual(kept.to_row()["issued"], sheet.to_row()["issued"])
        self.assertEqual(kept.base_seed, sheet.base_seed)
        self.assertEqual(kept.active, sheet.active)
        seeds = [sheet.boil_seed(index) for index in range(len(sheet))]
        self.assertEqual([kept.boil_seed(index) for index in range(len(kept))], seeds)
        self.assertEqual(seeds, [11, 11, 11, 13])

    def test_an_old_blueprint_without_exposure_still_opens(self):
        document = {
            "format": "vmib",
            "version": 1,
            "name": "old",
            "width": 12,
            "height": 8,
            "layers": [
                {
                    "id": "g0",
                    "name": "folder",
                    "kind": "group",
                    "children": [{"id": "n0", "name": "ink", "kind": "layer"}],
                },
                {"id": "n1", "name": "paint", "kind": "layer", "exposure": {}},
                {"id": "n2", "name": "line", "kind": "layer", "exposure": None},
            ],
        }
        folder_path = tempfile.mkdtemp(prefix="vmi-exposure-old-")
        path = os.path.join(folder_path, "old.vmib")
        try:
            write_parts(path, document, [])
            loaded = load_blueprint(path)
        finally:
            shutil.rmtree(folder_path)
        self.assertEqual(loaded.width, 12)
        self.assertEqual(loaded.height, 8)
        self.assertIsNone(loaded.layers[0].exposure)
        self.assertIsNone(loaded.layers[0].children[0].exposure)
        self.assertIsNone(loaded.layers[1].exposure)
        self.assertIsNone(loaded.layers[2].exposure)
        self.assertEqual(VERSION, 1)


if __name__ == "__main__":
    unittest.main()
