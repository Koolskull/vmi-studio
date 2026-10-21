"""Exposure slots, the pool they point at, and the filmstrip mark."""

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from vmi_studio.exposure import FRAME_MAX, ExposureSheet, cel_mark


class ExposureTests(unittest.TestCase):
    def test_a_three_frame_hold_is_one_pool_frame(self):
        sheet = ExposureSheet()
        ident = sheet.append_blank()
        self.assertEqual(ident, "000")
        sheet.hold(count=2)
        self.assertEqual(sheet.slots, ["000", "000", "000"])
        self.assertEqual(sheet.pool, ["000"])
        self.assertEqual([sheet.slot_state(index) for index in range(3)], ["", "hold", "hold"])
        self.assertEqual(len({sheet.boil_seed(index) for index in range(3)}), 1)

    def test_duplicate_is_a_new_frame_and_reuse_is_not(self):
        sheet = ExposureSheet()
        first = sheet.append_blank()
        copied = sheet.duplicate(0)
        self.assertEqual(sheet.slots, [first, copied])
        self.assertEqual(copied, "001")
        self.assertEqual(sheet.pool, ["000", "001"])
        self.assertNotEqual(sheet.boil_seed(0), sheet.boil_seed(1))

        third = sheet.append_blank()
        self.assertEqual(third, "002")
        self.assertEqual(sheet.reuse("000", 2), "000")
        self.assertEqual(sheet.slots, ["000", "001", "000"])
        self.assertEqual(sheet.pool, ["000", "001", "002"])
        self.assertEqual(sheet.slot_state(2), "reuse")
        self.assertEqual(cel_mark(sheet, 2), "reuse")
        self.assertEqual(cel_mark(sheet, 0), "")

        sheet.reuse(first, 1)
        self.assertEqual(sheet.slots, ["000", "000", "000"])
        self.assertEqual(sheet.pool, ["000", "001", "002"])
        self.assertEqual(sheet.slot_state(1), "hold")
        self.assertEqual(len(sheet.pool), 3)

    def test_ids_stay_permanent_and_a_shared_frame_survives_delete(self):
        sheet = ExposureSheet()
        for _ in range(11):
            last = sheet.append_blank()
        self.assertEqual(last, "00A")
        sheet.reuse("00a", 0)
        self.assertEqual(sheet.slots[0], "00A")

        held = ExposureSheet()
        held.append_blank()
        held.hold(count=2)
        other = held.append_blank()
        self.assertEqual(held.delete(1), "000")
        self.assertEqual(held.slots, ["000", "000", other])
        self.assertIn("000", held.pool)

        removed = held.delete(2)
        self.assertEqual(removed, other)
        self.assertNotIn(other, held.slots)
        self.assertIn(other, held.pool)
        self.assertEqual(held.append_blank(), "002")
        self.assertIn(other, held.pool)
        brought = held.reuse(other, 0)
        self.assertEqual(brought, other)
        self.assertEqual(held.slots[0], other)

        only = ExposureSheet()
        only.append_blank()
        self.assertIsNone(only.delete(0))
        self.assertEqual(only.slots, ["000"])
        self.assertEqual(only.pool, ["000"])

        full = ExposureSheet()
        full._issued = FRAME_MAX
        self.assertEqual(full.append_blank(), "FFF")
        with self.assertRaises(ValueError):
            full.append_blank()
        with self.assertRaises(ValueError):
            held.reuse("12", 0)
        with self.assertRaises(ValueError):
            held.reuse("FFF", 0)

    def test_move_reorders_slots_and_not_the_pool(self):
        sheet = ExposureSheet()
        ids = [sheet.append_blank() for _ in range(3)]
        pool = list(sheet.pool)
        seed = sheet.boil_seed(0)
        self.assertEqual(sheet.move(0, 2), ids[0])
        self.assertEqual(sheet.slots, [ids[1], ids[2], ids[0]])
        self.assertEqual(sheet.pool, pool)
        self.assertEqual(sheet.boil_seed(2), seed)
        self.assertEqual(sheet.move(2, 0), ids[0])
        self.assertEqual(sheet.slots, ids)
        self.assertEqual(sheet.move(1, 1), ids[1])
        self.assertEqual(sheet.slots, ids)
        with self.assertRaises(IndexError):
            sheet.move(0, 3)

    def test_boil_seed_changes_only_when_the_pool_frame_changes(self):
        sheet = ExposureSheet(base_seed=4)
        sheet.append_blank()
        sheet.hold(count=2)
        sheet.append_blank()
        sheet.append_blank()
        sheet.reuse(sheet.slots[0], 4)
        self.assertEqual(sheet.slots, ["000", "000", "000", "001", "000"])
        seeds = [sheet.boil_seed(index) for index in range(len(sheet))]
        self.assertEqual(seeds, [4, 4, 4, 5, 4])
        for left in range(len(sheet)):
            for right in range(len(sheet)):
                if sheet.slots[left] == sheet.slots[right]:
                    self.assertEqual(seeds[left], seeds[right])
                else:
                    self.assertNotEqual(seeds[left], seeds[right])
        self.assertNotEqual(seeds, list(range(len(sheet))))

    def test_onion_is_the_previous_and_next_pool_ids(self):
        sheet = ExposureSheet()
        first = sheet.append_blank()
        sheet.hold(count=1)
        third = sheet.append_blank()
        self.assertEqual(sheet.slots, [first, first, third])
        edge = sheet.onion(0)
        self.assertIsNone(edge.prev)
        self.assertEqual(edge.next, first)
        middle = sheet.onion(1)
        self.assertEqual(middle.prev, first)
        self.assertEqual(middle.next, third)
        self.assertEqual(set(middle.__dict__), {"prev", "next"})
        self.assertIsInstance(middle.prev, str)
        sheet.active = 2
        self.assertEqual(sheet.onion(), sheet.onion(2))
        self.assertIsNone(sheet.onion(2).next)
        self.assertNotIn("pixels", middle.__dict__)

    def test_filmstrip_shows_hold_or_reuse_and_skips_a_missing_sheet(self):
        from PySide6.QtWidgets import QApplication

        from vmi_studio.filmstrip import Filmstrip

        app = QApplication.instance() or QApplication([])
        film = Filmstrip()
        self.assertIsNone(film.sheet)
        painted = []

        class Painter:
            def fillRect(self, *args):
                painted.append(args)

        film._show_exposure(Painter(), 0, 0, 30, 1)
        self.assertEqual(painted, [])
        self.assertEqual(cel_mark(None, 1), "")

        held = ExposureSheet()
        held.append_blank()
        held.hold(count=1)
        film.sheet = held
        film._show_exposure(Painter(), 0, 0, 30, 0)
        self.assertEqual(painted, [])
        film._show_exposure(Painter(), 0, 0, 30, 1)
        self.assertEqual(cel_mark(held, 1), "hold")
        self.assertEqual((painted[0][2], painted[0][3]), (4, 1))

        reused = ExposureSheet()
        reused.append_blank()
        reused.append_blank()
        reused.append_blank()
        reused.reuse(reused.slots[0], 2)
        painted.clear()
        film.sheet = reused
        film._show_exposure(Painter(), 0, 0, 30, 2)
        self.assertEqual(cel_mark(reused, 2), "reuse")
        self.assertEqual((painted[0][2], painted[0][3]), (2, 2))
        film.close()
        self.assertIsNotNone(app)

    def test_a_loaded_folder_sheet_marks_its_own_row(self):
        from PySide6.QtWidgets import QApplication

        from vmi_studio.document import Node
        from vmi_studio.filmstrip import Filmstrip

        app = QApplication.instance() or QApplication([])
        film = Filmstrip()
        folder = Node("f", "shot", "group", animation=True)
        sheet = ExposureSheet()
        sheet.append_blank()
        sheet.hold(count=1)
        folder.exposure = sheet
        other = Node("g", "other", "group", animation=True)
        self.assertIs(film.sheet_for(folder), sheet)
        self.assertIsNone(film.sheet_for(other))
        film.sheet = ExposureSheet()
        self.assertIs(film.sheet_for(folder), sheet)
        painted = []

        class Painter:
            def fillRect(self, *args):
                painted.append(args)

        film.sheet = None
        film._show_exposure(Painter(), 0, 0, 30, 1, folder)
        self.assertEqual(cel_mark(sheet, 1), "hold")
        self.assertEqual((painted[0][2], painted[0][3]), (4, 1))
        painted.clear()
        film._show_exposure(Painter(), 0, 0, 30, 1, other)
        self.assertEqual(painted, [])
        film.close()
        self.assertIsNotNone(app)
