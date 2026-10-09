"""The launcher lists work and does not read a drawing."""

import hashlib
import os
import tempfile
import time
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import QRect, QSize, Qt
from PySide6.QtGui import QColor, QIcon, QImage, QPainter
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication, QPushButton

from vmi_studio import catalog
from vmi_studio.launchui import DiskModel, ImagePick, Launcher, folder_png_path


class CatalogTests(unittest.TestCase):
    def test_lists_tags_finished_and_recent_round_trip(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "book.json")
        door = os.path.join(folder, "door.clip")
        wall = os.path.join(folder, "wall.kra")
        open(door, "wb").close()
        open(wall, "wb").close()
        book = catalog.empty()
        row, error = catalog.make_list(book, "Beetle radio", "project")
        self.assertEqual(error, "")
        task, terror = catalog.make_list(book, "Door cuts", "task", row["id"])
        self.assertEqual(terror, "")
        other, oerror = catalog.make_list(book, "Other show", "project")
        self.assertEqual(oerror, "")
        same, serror = catalog.make_list(book, "Door cuts", "task", other["id"])
        self.assertEqual(serror, "")
        self.assertNotEqual(same["id"], task["id"])
        again_task, task_dup = catalog.make_list(book, "door cuts", "task", row["id"])
        self.assertIsNone(again_task)
        self.assertIn("title", task_dup)
        again, duplicate = catalog.make_list(book, "beetle radio", "project")
        self.assertIsNone(again)
        self.assertIn("title", duplicate)
        added = catalog.add_paths(book, row["id"], [door, wall, door, os.path.join(folder, "notes.txt")])
        self.assertEqual(added["added"], [os.path.abspath(door), os.path.abspath(wall)])
        self.assertEqual(added["duplicate"], [os.path.abspath(door)])
        self.assertEqual(len(added["rejected"]), 1)
        self.assertTrue(catalog.set_fields(book, row["id"], door, label="cityscape", tag="ada", done=True))
        self.assertEqual(catalog.find_item(book, row["id"], door)["label"], "cityscape")
        self.assertEqual(catalog.find_item(book, row["id"], door)["tag"], "ada")
        self.assertTrue(catalog.done_for(book, door, catalog.find_item(book, row["id"], door)))
        catalog.touch_recent(book, wall, when="2026-10-08 09:00")
        catalog.touch_recent(book, door, when="2026-10-08 10:00")
        self.assertEqual(book["recent"][0]["path"], os.path.abspath(door))
        self.assertEqual(book["recent"][1]["path"], os.path.abspath(wall))
        for index in range(13):
            extra = os.path.join(folder, "f%d.psd" % index)
            open(extra, "wb").close()
            catalog.touch_recent(book, extra, when="2026-10-08 11:%02d" % index)
        self.assertEqual(len(book["recent"]), catalog.RECENT_LIMIT)
        self.assertEqual(book["recent"][0]["path"], os.path.abspath(os.path.join(folder, "f12.psd")))
        catalog.save(book, path)
        loaded = catalog.load(path)
        radio = catalog.find_list(loaded, row["id"])
        self.assertEqual(radio["title"], "Beetle radio")
        self.assertEqual(radio["style"], "project")
        self.assertEqual(radio["items"][0]["label"], "cityscape")
        self.assertEqual(radio["items"][0]["tag"], "ada")
        self.assertTrue(radio["items"][0]["done"])
        loaded_task = catalog.find_list(loaded, task["id"])
        self.assertEqual(loaded_task["style"], "task")
        self.assertEqual(loaded_task["project"], row["id"])
        self.assertEqual(radio["tasks"][0]["id"], task["id"])
        self.assertEqual(catalog.find_list(loaded, same["id"])["project"], other["id"])
        self.assertIn("converted", catalog.open_hint(door))
        self.assertIn("blueprint", catalog.open_hint(os.path.join(folder, "room.vmib")))
        self.assertTrue(catalog.same_drawing(door, door, loading=False))
        self.assertFalse(catalog.same_drawing(door, door, loading=True))
        self.assertFalse(catalog.same_drawing(door, wall))

    def test_an_empty_recent_list_can_remember_the_last_session(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "book.json")
        drawing = os.path.join(folder, "radio.clip")
        open(drawing, "wb").close()
        book = catalog.load(path, seed_path=drawing)
        self.assertEqual(book["recent"][0]["path"], os.path.abspath(drawing))
        self.assertEqual(book["recent"][0]["opened"], "")
        again = catalog.load(path, seed_path=os.path.join(folder, "other.kra"))
        self.assertEqual(len(again["recent"]), 1)
        self.assertEqual(again["recent"][0]["path"], os.path.abspath(drawing))

    def test_a_loose_task_list_joins_its_project(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "book.json")
        door = os.path.join(folder, "door.clip")
        open(door, "wb").close()
        raw = {
            "recent": [],
            "browse": "",
            "seq": 5,
            "lists": [
                {
                    "id": "l1",
                    "title": "Beetle Game Release",
                    "style": "project",
                    "items": [{
                        "path": door,
                        "label": "door.clip",
                        "tag": "koolskull",
                        "done": False,
                    }],
                },
                {"id": "l5", "title": "fart popp", "style": "task", "items": []},
            ],
        }
        with open(path, "w", encoding="utf-8") as handle:
            import json
            json.dump(raw, handle)
        book = catalog.load(path)
        project = catalog.find_list(book, "l1")
        task = catalog.find_list(book, "l5")
        self.assertEqual([item["style"] for item in book["lists"]], ["project"])
        self.assertEqual(task["project"], "l1")
        self.assertEqual(project["tasks"][0]["title"], "fart popp")
        self.assertFalse(catalog._has_loose_tasks(catalog.load(path) or book))
        stored = catalog.load(path)
        self.assertEqual(stored["lists"][0]["tasks"][0]["id"], "l5")


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.app = QApplication.instance() or QApplication([])
        self.folder = tempfile.mkdtemp()
        self.book_path = os.path.join(self.folder, "launcher.json")
        self.door = os.path.join(self.folder, "door.clip")
        self.notes = os.path.join(self.folder, "notes.txt")
        os.makedirs(os.path.join(self.folder, "extra"))
        open(self.door, "wb").close()
        open(os.path.join(self.folder, "extra", "wall.kra"), "wb").close()
        open(self.notes, "wb").close()

    def _launcher(self):
        win = Launcher(book_path=self.book_path)
        self.app.processEvents()
        return win

    def _names(self, win):
        root = win.browser.rootIndex()
        names = []
        for row in range(win.disk.rowCount(root)):
            names.append(win.disk.fileName(win.disk.index(row, 0, root)))
        return names

    def _wait_for(self, win, folder):
        seen = []
        win.disk.directoryLoaded.connect(seen.append)
        win.browse_at(folder, record=False)
        target = os.path.abspath(folder)
        for _index in range(50):
            self.app.processEvents()
            if any(os.path.abspath(path) == target for path in seen) and self._names(win):
                return
            time.sleep(0.02)
        self.app.processEvents()

    def test_new_project_sits_left_of_new_task_list(self):
        win = self._launcher()
        labels = [button.text() for button in win.findChildren(QPushButton)]
        self.assertNotIn("Browse", labels)
        self.assertLess(labels.index("New project"), labels.index("New task list"))

    def test_shift_left_and_right_move_between_the_panes(self):
        win = self._launcher()
        win.show()
        win.activateWindow()
        self.app.processEvents()
        win.projects.setFocus()
        self.app.processEvents()
        QTest.keyClick(win.projects, Qt.Key_Right, Qt.ShiftModifier)
        self.app.processEvents()
        self.assertTrue(win.file_table.hasFocus())
        self.assertEqual(win.status.currentMessage(), "Files")
        QTest.keyClick(win.file_table, Qt.Key_Right, Qt.ShiftModifier)
        self.app.processEvents()
        self.assertTrue(win.browser.hasFocus())
        self.assertEqual(win.status.currentMessage(), "Browser")
        QTest.keyClick(win.browser, Qt.Key_Left, Qt.ShiftModifier)
        self.app.processEvents()
        self.assertTrue(win.file_table.hasFocus())
        QTest.keyClick(win.file_table, Qt.Key_Left, Qt.ShiftModifier)
        self.app.processEvents()
        self.assertTrue(win.projects.hasFocus())
        QTest.keyClick(win.projects, Qt.Key_Left, Qt.ShiftModifier)
        self.app.processEvents()
        self.assertTrue(win.browser.hasFocus())
        win.hide()

    def test_open_emits_the_path_and_does_not_read_the_drawing(self):
        import vmi_studio.openers as openers

        called = []
        original = openers.open_drawing
        openers.open_drawing = lambda *args, **kwargs: called.append(args)
        try:
            win = self._launcher()
            opened = []
            win.open_requested.connect(opened.append)
            row = win.create_list("project", "Beetle radio")
            result = win.add_paths(row["id"], [self.door])
            self.assertEqual(len(result["added"]), 1)
            self.assertEqual(win.projects.count(), 1)
            self.assertIn("Beetle radio", win.projects.item(0).text())
            self.assertEqual(win.file_table.topLevelItemCount(), 1)
            item = win.file_table.topLevelItem(0)
            self.assertEqual(item.text(1), "door.clip")
            self.assertEqual(item.text(2), catalog.user_name())
            self.assertEqual(item.text(3), "working")
            win.file_table.setCurrentItem(None)
            win.file_table.setCurrentItem(item)
            self.app.processEvents()
            self.assertIn("converted", win.status.currentMessage())
            win.relabel_selected("cityscape")
            win.retag_selected("ada")
            win.mark_selected(True)
            item = win.file_table.topLevelItem(0)
            self.assertEqual(item.text(0), "cityscape")
            self.assertEqual(item.text(2), "ada")
            self.assertEqual(item.text(3), "finished")
            self.assertIn("1/1 finished", win.projects.item(0).text())
            win.open_selected()
            self.assertEqual(opened, [os.path.abspath(self.door)])
            self.assertEqual(called, [])
            win.note_opened(self.door)
            win.show_mode("recent")
            self.assertEqual(win.file_table.topLevelItem(0).text(1), "door.clip")
            self.assertEqual(win.file_table.topLevelItem(0).text(3), "finished")
            self.assertNotIn("last session", win.file_table.topLevelItem(0).text(4))
        finally:
            openers.open_drawing = original

    def test_the_browser_lists_drawings_and_adds_them_to_a_task(self):
        win = self._launcher()
        self._wait_for(win, self.folder)
        names = self._names(win)
        self.assertIn("door.clip", names)
        self.assertIn("extra", names)
        self.assertNotIn("notes.txt", names)
        project = win.create_list("project", "Beetle radio")
        task = win.create_list("task", "Door cuts")
        self.assertEqual(task["project"], project["id"])
        self.assertEqual(win.tasks_title.text(), "TASKS  Beetle radio")
        self.assertEqual(win.tasks.count(), 1)
        root = win.browser.rootIndex()
        for row in range(win.disk.rowCount(root)):
            index = win.disk.index(row, 0, root)
            if win.disk.fileName(index) == "door.clip":
                win.browser.setCurrentIndex(index)
                break
        result = win.add_browser_selection()
        self.assertEqual(result["added"], [os.path.abspath(self.door)])
        self.assertEqual(win.mode, "task")
        self.assertEqual(win.list_id, task["id"])
        self.assertEqual(win.file_table.topLevelItem(0).text(1), "door.clip")
        project_paths = [item["path"] for item in catalog.find_list(win.book, project["id"])["items"]]
        self.assertIn(os.path.abspath(self.door), project_paths)
        win.remove_selected()
        self.assertEqual(win.file_table.topLevelItemCount(), 0)
        self.assertTrue(os.path.isfile(self.door))
        other = win.create_list("project", "Other show")
        self.assertEqual(other["id"], win.project_id)
        self.assertEqual(win.tasks.count(), 0)
        self.assertEqual(win.tasks_title.text(), "TASKS  Other show")
        win.project_id = project["id"]
        win.mode = "task"
        win.list_id = task["id"]
        win.refresh()
        self.assertEqual(win.tasks.count(), 1)
        self.assertIn("Door cuts", win.tasks.item(0).text())
        win.project_id = other["id"]
        win.mode = "project"
        win.list_id = other["id"]
        win.note_opened(self.door)
        self.assertEqual(win.project_id, project["id"])
        self.assertEqual(win.tasks.count(), 1)

    def test_a_folder_uses_the_folder_picture_and_a_file_has_no_icon(self):
        with open(folder_png_path(), "rb") as handle:
            digest = hashlib.sha256(handle.read()).hexdigest()
        self.assertEqual(digest, "3c02bc97dee4d1ba9afffd11c529fe5df9eec16495551c065af81fcb489ca34e")
        source = QImage(folder_png_path()).convertToFormat(QImage.Format.Format_RGBA8888)
        expect = bytes(source.constBits())
        win = self._launcher()
        self._wait_for(win, self.folder)
        self._assert_folder_picture(win.disk, win.browser, "extra", "door.clip", expect)
        plate = os.path.join(self.folder, "plate.png")
        open(plate, "wb").close()
        pick = ImagePick()
        seen = []
        pick.disk.directoryLoaded.connect(seen.append)
        pick.browse_at(self.folder)
        target = os.path.abspath(self.folder)
        for _index in range(50):
            self.app.processEvents()
            if any(os.path.abspath(path) == target for path in seen) and pick.disk.rowCount(pick.view.rootIndex()):
                break
            time.sleep(0.02)
        self.app.processEvents()
        self.assertIsInstance(pick.disk, DiskModel)
        self._assert_folder_picture(pick.disk, pick.view, "extra", "plate.png", expect)
        icon = win.disk.data(self._row(win.disk, win.browser, "extra"), Qt.DecorationRole)
        sharp = icon.pixmap(QSize(16, 16), 2.0).toImage().convertToFormat(QImage.Format.Format_RGBA8888)
        self.assertEqual(sharp.width(), 32)
        self.assertEqual(sharp.devicePixelRatio(), 2.0)
        for y in range(16):
            for x in range(16):
                tone = source.pixelColor(x, y).rgba()
                self.assertEqual(sharp.pixelColor(x * 2, y * 2).rgba(), tone)
                self.assertEqual(sharp.pixelColor(x * 2 + 1, y * 2 + 1).rgba(), tone)
        painted = QImage(16, 16, QImage.Format.Format_RGBA8888)
        painted.fill(QColor(0, 0, 0, 0))
        painter = QPainter(painted)
        icon.paint(painter, QRect(0, 0, 16, 16))
        painter.end()
        painted = painted.convertToFormat(QImage.Format.Format_RGBA8888)
        self.assertEqual(bytes(painted.constBits()), expect)
        win.browser.resize(420, 240)
        win.browser.show()
        win.browser.scrollTo(self._row(win.disk, win.browser, "extra"))
        self.app.processEvents()
        grab = win.browser.viewport().grab().toImage()
        found = False
        for y in range(grab.height()):
            for x in range(min(grab.width(), 48)):
                tone = grab.pixelColor(x, y)
                if tone.red() == 203 and tone.green() == 203 and tone.blue() == 203:
                    found = True
                    break
            if found:
                break
        self.assertTrue(found)

    def _row(self, disk, view, name):
        root = view.rootIndex()
        for row in range(disk.rowCount(root)):
            index = disk.index(row, 0, root)
            if disk.fileName(index) == name:
                return index
        return None

    def _assert_folder_picture(self, disk, view, folder_name, file_name, expect):
        self.assertIsInstance(disk, DiskModel)
        self.assertEqual(view.iconSize(), QSize(16, 16))
        folder = self._row(disk, view, folder_name)
        drawn = self._row(disk, view, file_name)
        self.assertIsNotNone(folder)
        self.assertIsNotNone(drawn)
        icon = disk.data(folder, Qt.DecorationRole)
        self.assertIsInstance(icon, QIcon)
        self.assertFalse(icon.isNull())
        image = icon.pixmap(QSize(16, 16)).toImage().convertToFormat(QImage.Format.Format_RGBA8888)
        self.assertEqual(bytes(image.constBits()), expect)
        file_icon = disk.data(drawn, Qt.DecorationRole)
        self.assertIsInstance(file_icon, QIcon)
        self.assertTrue(file_icon.isNull())

    def test_a_missing_file_stays_listed_until_it_is_removed(self):
        win = self._launcher()
        row = win.create_list("project", "Radio")
        win.add_paths(row["id"], [self.door])
        os.remove(self.door)
        win.refresh()
        self.assertEqual(win.file_table.topLevelItem(0).text(3), "missing")
        opened = []
        win.open_requested.connect(opened.append)
        win.open_selected()
        self.assertEqual(opened, [])
        self.assertIn("missing", win.status.currentMessage())

    def test_a_drop_adds_the_original_drawing_and_recent_refuses_it(self):
        from PySide6.QtCore import QMimeData, QPointF, QUrl
        from PySide6.QtGui import QDropEvent

        win = self._launcher()
        self.assertTrue(win.file_table.acceptDrops())
        win.mode = "recent"
        win.list_id = ""
        refused = win.drop_files([self.door])
        self.assertEqual(refused["added"], [])
        self.assertIn("project", win.status.currentMessage())
        row = win.create_list("project", "Radio")
        win.mode = "project"
        win.list_id = row["id"]
        win.project_id = row["id"]
        mime = QMimeData()
        mime.setUrls([QUrl.fromLocalFile(self.door)])
        event = QDropEvent(
            QPointF(8, 8),
            Qt.DropAction.CopyAction,
            mime,
            Qt.MouseButton.LeftButton,
            Qt.KeyboardModifier.NoModifier,
        )
        win.file_table.dropEvent(event)
        stored = catalog.find_item(win.book, row["id"], self.door)
        self.assertEqual(stored["path"], os.path.abspath(self.door))
        self.assertTrue(os.path.isfile(self.door))
        notes = win.drop_files([self.notes])
        self.assertEqual(notes["added"], [])
        self.assertIn(os.path.abspath(self.notes), notes["rejected"])
        folder = win.drop_files([os.path.join(self.folder, "extra")])
        self.assertEqual(folder["added"], [os.path.abspath(os.path.join(self.folder, "extra", "wall.kra"))])

    def test_open_low_res_marks_the_request_and_emits_the_path(self):
        win = self._launcher()
        row = win.create_list("project", "Radio")
        win.add_paths(row["id"], [self.door])
        opened = []
        win.open_requested.connect(opened.append)
        win.open_path(self.door, low_res=True)
        self.assertEqual(opened, [os.path.abspath(self.door)])
        self.assertTrue(win._open_low_res)
        win.open_selected()
        self.assertFalse(win._open_low_res)
        self.assertEqual(opened[-1], os.path.abspath(self.door))
