"""Tasks in the objects pane: white until executed, then grey, saved in the blueprint."""

import os
import shutil
import tempfile
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from PySide6.QtCore import Qt
from PySide6.QtGui import QColor
from PySide6.QtWidgets import QApplication, QVBoxLayout, QWidget

from vmi_studio.document import ArtFile, Ids, Node, Raster, refresh
from vmi_studio.tasks import (
    DONE_GREY,
    TaskBoard,
    clean_hex,
    pack_tasks,
    task_color,
)
from vmi_studio.vmib import load_blueprint, write_blueprint


def _app():
    return QApplication.instance() or QApplication([])


def _brightest(image):
    best = 0
    for y in range(image.height()):
        for x in range(image.width()):
            color = image.pixelColor(x, y)
            best = max(best, color.red(), color.green(), color.blue())
    return best


def _drawing(folder, name):
    path = os.path.abspath(os.path.join(folder, name))
    ids = Ids()
    layer = Node(ids.next(), "paint", "layer", raster=Raster(0, 0, 1, 1, bytes((255, 255, 255, 255))))
    return ArtFile(name, path, "clip", 1, 1, refresh([layer]), "")


class TaskTests(unittest.TestCase):
    def setUp(self):
        self.app = _app()

    def test_open_tasks_are_white_and_executed_tasks_are_struck_grey(self):
        board = TaskBoard()
        board.resize(220, 180)
        board.show()
        self.app.processEvents()
        board.new_task()
        item = board.list.item(0)
        board.list.closePersistentEditor(item)
        self.app.processEvents()
        self.assertEqual(board.rows(), [{"id": "t1", "text": "task", "done": False}])
        self.assertEqual(task_color(False, board.text_color), QColor("#FFFFFF"))
        self.assertGreater(_brightest(board.list.viewport().grab().toImage()), 200)

        board.execute_item(item)
        self.app.processEvents()
        self.assertTrue(board.rows()[0]["done"])
        self.assertEqual(task_color(True, board.text_color), QColor(DONE_GREY))
        done = _brightest(board.list.viewport().grab().toImage())
        self.assertGreater(done, 40)
        self.assertLess(done, 120)
        board.deleteLater()

    def test_empty_space_cannot_delete_and_colors_come_from_the_gear(self):
        board = TaskBoard()
        board.show()
        empty = board.menu_for(None)
        enabled = {action.text(): action.isEnabled() for action in empty.actions()}
        self.assertEqual(list(enabled), ["New task", "Execute", "Delete task"])
        self.assertTrue(enabled["New task"])
        self.assertFalse(enabled["Execute"])
        self.assertFalse(enabled["Delete task"])

        board.new_task()
        item = board.list.item(0)
        board.list.closePersistentEditor(item)
        on_task = {action.text(): action.isEnabled() for action in board.menu_for(item).actions()}
        self.assertTrue(on_task["Delete task"])
        self.assertTrue(on_task["Execute"])
        board.execute_item(item)
        restore = [action.text() for action in board.menu_for(board.list.item(0)).actions()]
        self.assertIn("Restore", restore)
        self.assertNotIn("Execute", restore)

        self.assertFalse(board.props.isVisible())
        board.gear.click()
        self.assertTrue(board.props.isVisible())
        board.text_edit.setText("#CCCCCC")
        board.bg_edit.setText("#112233")
        self.assertEqual(board.colors(), {"background": "#112233", "text": "#CCCCCC"})
        self.assertEqual(task_color(False, board.text_color).name(), "#cccccc")
        board.bg_edit.setText("nope")
        board.bg_edit.editingFinished.emit()
        self.assertEqual(board.background, "#112233")
        self.assertEqual(board.bg_edit.text(), "#112233")
        board.gear.click()
        self.assertFalse(board.props.isVisible())

        gear = board.gear.grab().toImage()
        colors = {
            gear.pixelColor(x, y).name()
            for y in range(gear.height())
            for x in range(gear.width())
        }
        self.assertGreater(len(colors), 1)
        board.deleteLater()

    def test_float_stays_above_and_dock_returns_it(self):
        host = QWidget()
        layout = QVBoxLayout(host)
        board = TaskBoard()
        board.place(layout)
        host.resize(480, 320)
        host.show()
        self.app.processEvents()
        board.lift()
        self.app.processEvents()
        self.assertTrue(board.is_floating())
        flags = board.float_window().windowFlags()
        self.assertTrue(flags & Qt.WindowType.WindowStaysOnTopHint)
        self.assertIs(board.parent(), board.float_window())
        self.assertTrue(host.findChild(QWidget, "task-dock-strip").isVisible())
        board.dock()
        self.app.processEvents()
        self.assertFalse(board.is_floating())
        self.assertIs(board.parent(), host)
        self.assertEqual(board.float_button.text(), "float")
        board.shutdown()
        host.close()

    def test_a_blueprint_keeps_the_tasks_and_the_colors(self):
        ids = Ids()
        layer = Node(ids.next(), "ink", "layer", raster=Raster(0, 0, 1, 1, bytes((0, 0, 0, 255))))
        art = ArtFile("door.clip", "/tmp/door.clip", "clip", 1, 1, refresh([layer]), "")
        folder = tempfile.mkdtemp(prefix="vmi-tasks-")
        path = os.path.join(folder, "door.vmib")
        blank = os.path.join(folder, "blank.vmib")
        try:
            write_blueprint(
                path,
                art,
                [],
                tasks=[
                    {"id": "t1", "text": "paint the door", "done": True},
                    {"id": "t2", "text": "   ", "done": False},
                ],
                task_colors={"background": "#101010", "text": "ccc"},
            )
            loaded = load_blueprint(path)
            write_blueprint(blank, art, [])
            empty = load_blueprint(blank)
        finally:
            shutil.rmtree(folder)
        self.assertEqual(
            loaded.project["tasks"],
            [{"id": "t1", "text": "paint the door", "done": True}],
        )
        self.assertEqual(loaded.project["task_colors"], {"background": "#101010", "text": "#CCCCCC"})
        self.assertEqual(empty.project["tasks"], [])
        self.assertEqual(empty.project["task_colors"]["text"], "#FFFFFF")
        self.assertIsNone(clean_hex("nope"))
        self.assertEqual(pack_tasks([{"id": "", "text": "x"}]), [])

    def test_each_drawing_keeps_its_tasks_and_the_session_brings_them_back(self):
        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        window_mod.QMessageBox.warning = lambda *_args, **_kwargs: None
        win = window_mod.MainWindow()
        folder = tempfile.mkdtemp(prefix="vmi-task-tabs-")
        try:
            door = _drawing(folder, "door.clip")
            wall = _drawing(folder, "wall.clip")
            win._stash(win._shown_index)
            index = win._begin_tab(door.path)
            win._loading_index = index
            win._loading = True
            win._opened(door)
            win.task_board.new_task()
            win.task_board.list.closePersistentEditor(win.task_board.list.item(0))
            win.task_board.execute_item(win.task_board.list.item(0))
            win.task_board.text_edit.setText("#CCCCCC")
            self.assertEqual(win.status.currentMessage(), "Task colors.")

            win._stash(win._shown_index)
            index = win._begin_tab(wall.path)
            win._loading_index = index
            win._loading = True
            win._opened(wall)
            self.assertEqual(win.task_board.rows(), [])

            win.tabs.setCurrentIndex(1)
            self.app.processEvents()
            self.assertEqual(win.task_board.rows(), [{"id": "t1", "text": "task", "done": True}])
            self.assertEqual(win.task_board.colors()["text"], "#CCCCCC")

            saved = {}

            def capture(data):
                saved.update(data)

            window_mod.save = capture
            win._save_session()
            self.assertEqual(saved["tasks"][0]["text"], "task")
            self.assertEqual(saved["task_colors"]["text"], "#CCCCCC")

            other = _drawing(folder, "other.clip")
            window_mod.load = lambda: {
                "path": other.path,
                "tasks": [{"id": "t9", "text": "paint the door", "done": False}],
                "task_colors": {"background": "#222222", "text": "#EEEEEE"},
            }
            win.show_file(other)
            self.assertEqual(win.task_board.rows()[0]["text"], "paint the door")
            self.assertEqual(win.task_board.colors()["background"], "#222222")
        finally:
            win.close()
            shutil.rmtree(folder, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
