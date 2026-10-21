"""OpenToonz cell timing on the exposure sheet, and the vertical grid."""

import os
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QPoint
from PySide6.QtGui import QColor, QImage, QPainter
from PySide6.QtWidgets import QApplication

from vmi_studio.document import ArtFile, Node, refresh
from vmi_studio.exposure import ExposureSheet, Onion, cel_mark, project_slots, sheet_from_slots
from vmi_studio.history import apply_snap, capture
from vmi_studio.xsheet import CELL_H, FRAME_W, HEAD, XSheet, run_command


def _ids(*numbers):
    return ["%03X" % number for number in numbers]


def _sheet(*numbers):
    slots = [("" if number is None else "%03X" % number) for number in numbers]
    pool = []
    for slot in slots:
        if slot and slot not in pool:
            pool.append(slot)
    sheet = sheet_from_slots(slots)
    sheet.pool = pool
    sheet._issued = len(pool)
    return sheet


class CellTimingTests(unittest.TestCase):
    def test_a_blank_is_not_a_hold_and_round_trips(self):
        sheet = _sheet(0, 0, None, 0)
        self.assertEqual(sheet.slots, ["000", "000", "", "000"])
        self.assertEqual([sheet.slot_state(index) for index in range(4)], ["", "hold", "", "reuse"])
        self.assertEqual(cel_mark(sheet, 2), "")
        self.assertEqual(sheet.onion(2), Onion("000", "000"))
        self.assertIsNone(sheet.onion(0).prev)
        with self.assertRaises(ValueError):
            sheet.boil_seed(2)
        with self.assertRaises(ValueError):
            sheet.hold(2)
        with self.assertRaises(ValueError):
            sheet.duplicate(2)
        sheet.reuse("000", 2)
        self.assertEqual(sheet.slots[2], "000")
        loaded = ExposureSheet.from_row(_sheet(1, None, 2).to_row())
        self.assertEqual(loaded.slots, ["001", "", "002"])
        self.assertEqual(loaded.pool, ["001", "002"])

    def test_reverse_swing_step_and_each_match_opentoonz(self):
        sheet = _sheet(1, 2, 3, 4)
        tail = "00A"
        sheet.slots.append(tail)
        sheet.reverse(0, 4)
        self.assertEqual(sheet.slots, ["004", "003", "002", "001", tail])
        sheet.slots = _ids(1, 2, 3, 4) + [tail]
        sheet.swing(0, 4)
        self.assertEqual(sheet.slots, ["001", "002", "003", "004", "003", "002", "001", tail])
        sheet.slots = ["001", "", "002", tail]
        sheet.step(0, 3, 2)
        self.assertEqual(sheet.slots, ["001", "001", "", "", "002", "002", tail])
        sheet.slots = _ids(1, 2, 3, 4, 5) + [tail]
        self.assertTrue(sheet.each(0, 5, 2))
        self.assertEqual(sheet.slots, ["001", "003", "005", tail])
        short = _sheet(1, 2)
        self.assertFalse(short.each(0, 2, 4))
        self.assertEqual(short.slots, ["001", "002"])

    def test_step_commands_and_reframe_keep_the_pool(self):
        sheet = _sheet(1, 1, 1, 2, None, 3, 3)
        pool = list(sheet.pool)
        sheet.reset_step(0, len(sheet))
        self.assertEqual(sheet.slots, ["001", "002", "", "003"])
        self.assertEqual(sheet.pool, pool)
        sheet = _sheet(1, 1, 2, None, 3)
        sheet.increase_step(0, len(sheet))
        self.assertEqual(sheet.slots, ["001", "001", "001", "002", "002", "", "003", "003"])
        sheet = _sheet(1, 1, 1, 2, None, 3, 3)
        sheet.decrease_step(0, len(sheet))
        self.assertEqual(sheet.slots, ["001", "001", "002", "", "003"])
        single = _sheet(4)
        single.decrease_step(0, 1)
        self.assertEqual(single.slots, ["004"])
        held = _sheet(1, 1, 1, 2)
        self.assertEqual(held.reframe(0, 2, 2), 2)
        self.assertEqual(held.slots, ["001", "001", "002"])
        spread = _sheet(1, 2, 3)
        spread.reframe(0, 3, 2)
        self.assertEqual(spread.slots, ["001", "001", "002", "002", "003", "003"])

    def test_autoexpose_fills_number_gaps_and_refuses_a_decrease(self):
        gap = _sheet(2, 5)
        self.assertTrue(gap.autoexpose(0, 2))
        self.assertEqual(gap.slots, ["002", "002", "002", "005"])
        ladder = _sheet(1, 3, 5, 7)
        self.assertTrue(ladder.autoexpose(0, 4))
        self.assertEqual(ladder.slots, ["001", "001", "003", "003", "005", "005", "007"])
        packed = _sheet(1, 2, 3)
        self.assertTrue(packed.autoexpose(0, 3))
        self.assertEqual(packed.slots, ["001", "002", "003"])
        fat = _sheet(1, 1, 1, 2)
        self.assertTrue(fat.autoexpose(0, 4))
        self.assertEqual(fat.slots, ["001", "002"])
        down = _sheet(5, 2, 9)
        self.assertFalse(down.autoexpose(0, 2))
        self.assertEqual(down.slots, ["005", "002", "009"])
        blank = _sheet(1, None, 3)
        self.assertTrue(blank.autoexpose(0, 3))
        self.assertEqual(blank.slots, ["001", "", "003"])
        outside = _sheet(1, 1, 5)
        self.assertTrue(outside.autoexpose(0, 2))
        self.assertEqual(outside.slots, ["001", "001", "001", "001", "005"])

    def test_repeat_insert_clear_and_cut_shift_the_tail(self):
        sheet = _sheet(1, 2, 3)
        sheet.repeat(0, 2, 1)
        self.assertEqual(sheet.slots, ["001", "002", "001", "002", "003"])
        self.assertFalse(sheet.repeat(0, 1, 0))
        sheet.insert_empty(1, 2)
        self.assertEqual(sheet.slots, ["001", "", "", "002", "001", "002", "003"])
        sheet.clear_cells(0, 1)
        self.assertEqual(sheet.slots[1:3], ["", ""])
        self.assertEqual(sheet.slots[0], "")
        sheet.remove_cells(1, 2)
        self.assertEqual(sheet.slots, ["", "002", "001", "002", "003"])
        order = ["00A", "001", "002", "003"]
        mixed = sheet_from_slots(order)
        mixed.random_cells(0, 4, 3)
        again = sheet_from_slots(order)
        again.random_cells(0, 4, 3)
        self.assertEqual(mixed.slots, again.slots)
        self.assertEqual(sorted(mixed.slots), sorted(order))
        self.assertEqual(mixed.pool, ["00A", "001", "002", "003"])

    def test_a_timeline_projects_holds_without_saving_a_sheet(self):
        first = Node("a", "A", "layer")
        second = Node("b", "B", "layer")
        slots = project_slots([(0, first), (3, second), (5, first)])
        self.assertEqual(slots, ["000", "000", "000", "001", "001", "000"])
        self.assertEqual(project_slots([(2, first)]), ["", "", "000"])
        self.assertEqual(project_slots([]), [])

    def test_undo_puts_the_sheet_back(self):
        folder = Node("f", "ink", "group", animation=True)
        folder.exposure = _sheet(1, 2)
        desk = ArtFile("shot.vmib", "", "vmib", 8, 8, refresh([folder]), "")

        class Window(object):
            art = desk
            objects = []
            playlist = ""

        window = Window()
        before = capture(window)
        folder.exposure.step(0, 2, 2)
        self.assertEqual(folder.exposure.slots, ["001", "001", "002", "002"])
        apply_snap(window, before)
        self.assertEqual(folder.exposure.slots, ["001", "002"])
        folder.exposure = None
        apply_snap(window, before)
        self.assertEqual(window.art.layers[0].exposure.slots, ["001", "002"])


class XSheetViewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.app = QApplication.instance() or QApplication([])

    def test_the_grid_numbers_frames_from_one_and_keeps_the_back_level_on_the_left(self):
        sheet = XSheet()
        sheet.set_columns([("back", ["000", "001"]), ("front", ["00A"])])
        sheet.set_playhead(1)
        self.assertEqual(sheet.frame_label(0), "1")
        self.assertEqual(sheet.frame_label(1), "2")
        self.assertEqual(sheet.cell_text(0, 0), "000")
        self.assertEqual(sheet.cell_text(1, 0), "00A")
        self.assertEqual(sheet.columns[0][0], "back")
        image = QImage(sheet.sizeHint(), QImage.Format_RGB32)
        image.fill(QColor("#123456"))
        painter = QPainter(image)
        sheet.render(painter, QPoint())
        painter.end()
        self.assertEqual(QColor(image.pixel(2, 2)).name(), "#000000")
        self.assertEqual(QColor(image.pixel(FRAME_W - 1, HEAD + 4)).name(), "#2a2a2a")
        self.assertGreater(image.height(), HEAD + CELL_H)
        self.assertTrue(run_command(sheet_from_slots(["001", "002"]), "step", 0, 2, 2))


class XSheetDeskTests(unittest.TestCase):
    def test_the_sheet_button_shows_the_grid_beside_the_filmstrip(self):
        from vmi_studio.window import MainWindow, apply_style
        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        app = QApplication.instance() or QApplication([])
        apply_style(app)
        win = MainWindow()
        back = Node("b", "back", "group", animation=True)
        back.exposure = _sheet(1, 3)
        front = Node("f", "front", "group", animation=True)
        front.exposure = _sheet(2)
        art = ArtFile("cut.clip", "", "clip", 8, 8, refresh([back, front]), "")
        win.show_file(art, restore=False)
        win._show_editor_page()
        win.resize(1400, 900)
        win.show()
        app.processEvents()
        self.assertFalse(win.xsheet_scroll.isVisible())
        self.assertTrue(win.film.isVisible())
        win.transport.sheet_button.click()
        app.processEvents()
        self.assertTrue(win.xsheet_scroll.isVisible())
        self.assertEqual([name for name, _slots in win.xsheet.columns], ["back", "front"])
        self.assertEqual(win.xsheet.cell_text(0, 0), "001")
        win._xsheet_command(0, 0, 2, "autoexpose", 0)
        self.assertEqual(back.exposure.slots, ["001", "001", "003"])
        win.history.undo(win)
        self.assertEqual(back.exposure.slots, ["001", "003"])
        win.close()
