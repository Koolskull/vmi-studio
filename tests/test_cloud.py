"""Cloud storage stays off the blueprint and off the public network."""

import json
import os
import shutil
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from vmi_studio.cloud import (
    BLOCK,
    CloudDialog,
    CloudError,
    empty,
    load,
    normalize,
    save,
    upload,
    usable,
)
from vmi_studio.document import ArtFile, Node
from vmi_studio.vmib import prepare_blueprint


SECRET = "secret-token-xyz"


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)
        self.server.bodies.append(body)
        self.server.paths.append(self.path)
        if self.server.mode == "redirect":
            self.send_response(302)
            self.send_header("Location", "http://127.0.0.1/stolen")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        payload = b'{"Hash":"QmOld"}\n{"Hash":"bafyreal"}\n'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _format, *_args):
        return


class FakeFtp:
    def __init__(self):
        self.commands = []
        self.payload = b""

    def cwd(self, directory):
        self.commands.append(("cwd", directory))

    def storbinary(self, command, handle, blocksize=8192):
        self.commands.append(("stor", command, blocksize))
        while True:
            block = handle.read(blocksize)
            if not block:
                break
            self.payload += block


class CloudTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.mkdtemp()
        self.store = os.path.join(self.folder, "cloud.json")
        self.previous = os.environ.get("VMI_CLOUD")
        os.environ["VMI_CLOUD"] = self.store

    def tearDown(self):
        if self.previous is None:
            os.environ.pop("VMI_CLOUD", None)
        else:
            os.environ["VMI_CLOUD"] = self.previous
        shutil.rmtree(self.folder)

    def _blueprint(self, name="room.vmib", payload=b"PK\x03\x04blueprint-bytes"):
        path = os.path.join(self.folder, name)
        with open(path, "wb") as handle:
            handle.write(payload)
        return path

    def test_a_missing_store_uploads_nothing_and_the_password_stays_out_of_the_zip(self):
        self.assertFalse(os.path.exists(self.store))
        self.assertFalse(usable(load()))
        path = self._blueprint()
        called = []

        def factory(_settings):
            called.append("called")
            return FakeFtp()

        settings = normalize({"enabled": False, "kind": "ftp", "ftp_host": "files.example"})
        with self.assertRaises(CloudError):
            upload(path, settings, ftp_factory=factory)
        self.assertEqual(called, [])
        self.assertTrue(os.path.isfile(path))
        art = ArtFile("a.clip", "/tmp/a.clip", "clip", 1, 1, [Node("n", "ink", "layer")], "")
        document, _blobs = prepare_blueprint(art, [])
        raw = json.dumps(document)
        self.assertNotIn(SECRET, raw)
        self.assertEqual(document["cloud"]["service"], "2kool.tv")
        self.assertEqual(document["cloud"]["upload"], "later")

    def test_the_store_is_private_and_a_bad_address_is_refused(self):
        from PySide6.QtWidgets import QApplication

        QApplication.instance() or QApplication([])
        dialog = CloudDialog(None, empty())
        dialog.enabled.setChecked(True)
        dialog.kind.setCurrentIndex(1)
        dialog.host.setText("files.example")
        dialog.password.setText(SECRET)
        dialog.directory.setText("blueprints")
        dialog.tls.setChecked(True)
        saved = dialog.commit()
        self.assertEqual(saved["ftp_password"], SECRET)
        self.assertEqual(os.stat(self.store).st_mode & 0o777, 0o600)
        self.assertEqual(load()["ftp_host"], "files.example")
        dialog.host.setText("files.example\r\nQUIT")
        self.assertIsNone(dialog.commit())
        self.assertEqual(load()["ftp_host"], "files.example")
        self.assertFalse(usable({"enabled": True, "kind": "ipfs", "ipfs_api": "http://user:pass@127.0.0.1:9"}))
        self.assertFalse(usable({"enabled": True, "kind": "ftp", "ftp_host": "files.example", "ftp_directory": "a/../../b"}))
        dialog.close()

    def test_ftp_streams_the_basename_and_hides_the_password(self):
        path = self._blueprint(payload=b"blueprint-body")
        fake = FakeFtp()
        settings = normalize({
            "enabled": True,
            "kind": "ftp",
            "ftp_host": "files.example",
            "ftp_directory": "blueprints",
            "ftp_password": SECRET,
            "ftp_tls": True,
        })

        def factory(got):
            self.assertTrue(got["ftp_tls"])
            self.assertEqual(got["ftp_password"], SECRET)
            return fake

        label = upload(path, settings, ftp_factory=factory)
        self.assertEqual(label, "files.example/blueprints/room.vmib")
        self.assertEqual(fake.commands[0], ("cwd", "blueprints"))
        self.assertEqual(fake.commands[1][0], "stor")
        self.assertEqual(fake.commands[1][1], "STOR room.vmib")
        self.assertEqual(fake.commands[1][2], BLOCK)
        self.assertEqual(fake.payload, b"blueprint-body")
        self.assertNotIn(SECRET, label)
        class Boom:
            def cwd(self, directory):
                return None

            def storbinary(self, command, handle, blocksize=8192):
                raise OSError("530 %s rejected" % SECRET)

        with self.assertRaises(CloudError) as caught:
            upload(path, settings, ftp_factory=lambda _settings: Boom())
        self.assertNotIn(SECRET, str(caught.exception))
        self.assertTrue(os.path.isfile(path))
        self.assertEqual(os.path.getsize(path), len(b"blueprint-body"))

        def refuse(_settings):
            raise AssertionError("connected")

        refused = dict(settings)
        refused["ftp_host"] = "files.example\n"
        with self.assertRaises(CloudError):
            upload(path, refused, ftp_factory=refuse)

    def test_ipfs_posts_the_file_once_and_does_not_follow_a_redirect(self):
        path = self._blueprint(payload=b"ipfs-blueprint-bytes")
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        server.bodies = []
        server.paths = []
        server.mode = "ok"
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        port = server.server_address[1]
        try:
            settings = normalize({
                "enabled": True,
                "kind": "ipfs",
                "ipfs_api": "http://127.0.0.1:%d" % port,
                "ftp_password": SECRET,
            })
            cid = upload(path, settings)
            self.assertEqual(cid, "bafyreal")
            self.assertEqual(len(server.paths), 1)
            self.assertIn("/api/v0/add", server.paths[0])
            self.assertIn("pin=true", server.paths[0])
            self.assertIn("cid-version=1", server.paths[0])
            self.assertIn(b"ipfs-blueprint-bytes", server.bodies[0])
            self.assertIn(b"room.vmib", server.bodies[0])
            self.assertNotIn(SECRET.encode("ascii"), server.bodies[0])
            server.mode = "redirect"
            before = len(server.paths)
            with self.assertRaises(CloudError) as caught:
                upload(path, settings)
            self.assertEqual(len(server.paths), before + 1)
            self.assertIn("refused", str(caught.exception))
            self.assertTrue(os.path.isfile(path))
        finally:
            server.shutdown()
            server.server_close()

    def test_the_menu_offers_cloud_storage_and_a_second_upload_waits(self):
        from PySide6.QtCore import QThread, Signal
        from PySide6.QtWidgets import QApplication

        import vmi_studio.cloud as cloudstore
        import vmi_studio.window as window_mod

        window_mod.save = lambda _data: None
        window_mod.load = lambda: None
        app = QApplication.instance() or QApplication([])
        win = window_mod.MainWindow()
        try:
            names = []
            for action in win.menuBar().actions():
                menu = action.menu()
                if menu is not None and action.text() == "File":
                    names = [item.text() for item in menu.actions() if item.text()]
            self.assertIn("Cloud storage", names)
            win.cloud_storage()
            app.processEvents()
            self.assertIsNotNone(win._cloud_dialog)
            self.assertTrue(win._cloud_dialog.isVisible())
            self.assertIn("not written into the blueprint", win._cloud_dialog.note.text())
            win._cloud_dialog.close()
            path = self._blueprint()
            win._offer_upload(path)
            self.assertNotIn("Uploading", win.status.currentMessage())
            save({
                "enabled": True,
                "kind": "ftp",
                "ftp_host": "files.example",
                "ftp_password": SECRET,
            })
            started = []

            class FakeThread(QThread):
                ok = Signal(str)
                bad = Signal(str)

                def __init__(self, file_path, settings):
                    super().__init__()
                    self.path = file_path
                    self.settings = settings
                    started.append(file_path)

                def start(self, priority=QThread.Priority.InheritPriority):
                    started.append("start")

            original = cloudstore.CloudThread
            cloudstore.CloudThread = FakeThread
            try:
                win._offer_upload(path)
                self.assertEqual(started, [path, "start"])
                self.assertEqual(win.status.currentMessage(), "Saved room.vmib. Uploading.")
                self.assertEqual(win._uploader.settings["ftp_password"], SECRET)
            finally:
                cloudstore.CloudThread = original
            gate = threading.Event()

            class Hold(QThread):
                def run(self):
                    gate.wait(timeout=5)

            hold = Hold()
            hold.start()
            self.assertTrue(hold.isRunning())
            try:
                win._uploader = hold
                win._offer_upload(path)
                self.assertIn("already running", win.status.currentMessage())
            finally:
                gate.set()
                hold.wait(1000)
            win._upload_name = "room.vmib"
            win._upload_ok("bafyreal")
            self.assertEqual(win.status.currentMessage(), "Saved room.vmib. Uploaded bafyreal.")
            win._upload_bad("the server did not accept the file")
            self.assertEqual(
                win.status.currentMessage(),
                "Saved room.vmib. Upload did not finish: the server did not accept the file.",
            )
            self.assertTrue(os.path.isfile(path))
        finally:
            win.close()
