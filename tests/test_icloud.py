"""iCloud Drive is a place in the browser, and an evicted file is fetched on open."""

import os
import tempfile
import unittest
from unittest.mock import patch

from vmi_studio import catalog, icloud


class ICloudTests(unittest.TestCase):
    def test_icloud_is_a_place_when_the_drive_is_there(self):
        home = tempfile.mkdtemp()
        os.makedirs(os.path.join(home, "Documents"))
        drive = os.path.join(home, "Library", "Mobile Documents", "com~apple~CloudDocs")
        os.makedirs(drive)
        labels = [label for label, _path in catalog.places(home)]
        self.assertIn("iCloud", labels)
        self.assertLess(labels.index("iCloud"), labels.index("Computer"))
        self.assertEqual(dict(catalog.places(home))["iCloud"], drive)

    def test_a_home_without_icloud_has_no_icloud_place(self):
        home = tempfile.mkdtemp()
        labels = [label for label, _path in catalog.places(home)]
        self.assertNotIn("iCloud", labels)
        self.assertEqual(icloud.drive_root(home), "")
        self.assertFalse(icloud.in_drive(home, home))

    def test_in_drive_is_the_folder_and_its_children(self):
        home = tempfile.mkdtemp()
        drive = os.path.join(home, "Library", "Mobile Documents", "com~apple~CloudDocs")
        child = os.path.join(drive, "Drawings")
        os.makedirs(child)
        self.assertTrue(icloud.in_drive(drive, home))
        self.assertTrue(icloud.in_drive(os.path.join(child, "door.clip"), home))
        self.assertFalse(icloud.in_drive(home, home))
        self.assertFalse(icloud.in_drive("", home))

    def test_warm_lists_a_folder(self):
        folder = tempfile.mkdtemp()
        open(os.path.join(folder, "a.clip"), "wb").close()
        self.assertEqual(icloud.warm(folder), os.path.abspath(folder))

    def test_warm_names_a_folder_that_will_not_open(self):
        with patch("vmi_studio.icloud.os.listdir", side_effect=OSError("Operation not permitted")):
            with self.assertRaises(OSError) as caught:
                icloud.warm(os.path.join(tempfile.gettempdir(), "Drawings"))
        self.assertIn("Drawings", str(caught.exception))
        self.assertIn("not permitted", str(caught.exception))

    def test_a_local_file_is_not_opened_again(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "door.clip")
        with open(path, "wb") as handle:
            handle.write(b"CLIP")
        with patch("vmi_studio.icloud.is_evicted", return_value=False):
            with patch("builtins.open") as opened:
                self.assertEqual(icloud.ensure_local(path), os.path.abspath(path))
        opened.assert_not_called()
        self.assertFalse(icloud.is_evicted(path))

    def test_an_evicted_file_is_read_until_the_bytes_are_here(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "door.clip")
        with open(path, "wb") as handle:
            handle.write(b"CLIP")
        with patch("vmi_studio.icloud.is_evicted", side_effect=[True, False]):
            self.assertEqual(icloud.ensure_local(path), os.path.abspath(path))

    def test_a_failed_download_names_the_file(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "door.clip")
        with open(path, "wb") as handle:
            handle.write(b"CLIP")
        with patch("vmi_studio.icloud.is_evicted", return_value=True):
            with patch("builtins.open", side_effect=OSError("offline")):
                with self.assertRaises(OSError) as caught:
                    icloud.ensure_local(path)
        self.assertIn("door.clip", str(caught.exception))
        self.assertIn("offline", str(caught.exception))

    def test_the_note_says_an_evicted_drawing_will_download(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "door.clip")
        with open(path, "wb") as handle:
            handle.write(b"CLIP")
        with patch("vmi_studio.catalog.is_evicted", return_value=True):
            note = catalog.selection_note(path)
        self.assertIn("iCloud", note)
        self.assertIn("downloads", note)
