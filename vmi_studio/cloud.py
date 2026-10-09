"""Optional upload of a blueprint that is already saved.

The zip on disk is the file. Settings live beside the session, mode 0600,
and can be pointed at another file with VMI_CLOUD. They are not written
into the blueprint. A missing or disabled store uploads nothing. A failed
upload leaves the zip where the save put it.
"""

import json
import os
from urllib.parse import urlparse

from PySide6.QtCore import QThread, Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
)


DEFAULT_IPFS = "http://127.0.0.1:5001"
BLOCK = 1024 * 1024


class CloudError(Exception):
    pass


def empty():
    return {
        "enabled": False,
        "kind": "ipfs",
        "ipfs_api": DEFAULT_IPFS,
        "ftp_host": "",
        "ftp_port": 21,
        "ftp_user": "",
        "ftp_password": "",
        "ftp_directory": "",
        "ftp_tls": True,
    }


def store_path():
    override = os.environ.get("VMI_CLOUD")
    if override:
        return override
    return os.path.join(os.path.expanduser("~"), ".cache", "vmi-studio", "cloud.json")


def normalize(raw):
    base = empty()
    if not isinstance(raw, dict):
        return base
    base["enabled"] = bool(raw.get("enabled"))
    kind = raw.get("kind")
    base["kind"] = kind if kind in ("ipfs", "ftp") else "ipfs"
    base["ipfs_api"] = str(raw.get("ipfs_api") or DEFAULT_IPFS).strip()
    base["ftp_host"] = str(raw.get("ftp_host") or "").strip()
    try:
        port = int(raw.get("ftp_port") or 21)
    except (TypeError, ValueError):
        port = 21
    base["ftp_port"] = port if 1 <= port <= 65535 else 21
    base["ftp_user"] = str(raw.get("ftp_user") or "")
    base["ftp_password"] = str(raw.get("ftp_password") or "")
    base["ftp_directory"] = str(raw.get("ftp_directory") or "").strip()
    base["ftp_tls"] = True if "ftp_tls" not in raw else bool(raw.get("ftp_tls"))
    return base


def load():
    try:
        with open(store_path(), encoding="utf-8") as handle:
            raw = json.load(handle)
    except (OSError, ValueError):
        return empty()
    return normalize(raw)


def save(data):
    path = store_path()
    parent = os.path.dirname(path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    blob = json.dumps(normalize(data), ensure_ascii=False, indent=2).encode("utf-8")
    temporary = path + ".tmp"
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        os.write(descriptor, blob)
    finally:
        os.close(descriptor)
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)
    os.chmod(path, 0o600)


def _broken(text):
    return any(ch in str(text or "") for ch in "\r\n\x00")


def _host_ok(host):
    text = str(host or "").strip()
    if not text or _broken(text):
        return False
    return not any(ch in text for ch in " \t/\\")


def _directory_ok(directory):
    text = str(directory or "")
    if not text:
        return True
    if _broken(text):
        return False
    parts = text.replace("\\", "/").split("/")
    return all(part != ".." for part in parts)


def _api_ok(url):
    parsed = urlparse(str(url or "").strip())
    if parsed.scheme not in ("http", "https"):
        return False
    if parsed.username or parsed.password:
        return False
    return bool(parsed.hostname)


def _basename_ok(name):
    if not name or name != os.path.basename(name):
        return False
    if any(ch in name for ch in '"\r\n\x00\\/'):
        return False
    return name.lower().endswith(".vmib")


def usable(data):
    data = normalize(data)
    if not data["enabled"]:
        return False
    if data["kind"] == "ipfs":
        return _api_ok(data["ipfs_api"])
    if data["kind"] == "ftp":
        return _host_ok(data["ftp_host"]) and _directory_ok(data["ftp_directory"])
    return False


def _scrub(exc, secret):
    text = "%s" % exc
    if secret:
        text = text.replace(secret, "")
    text = " ".join(text.split())
    if not text:
        text = "the server did not accept the file"
    return text[:180]


def _require_blueprint(path):
    if not path or not str(path).lower().endswith(".vmib") or not os.path.isfile(path):
        raise CloudError("Only a blueprint file is uploaded.")
    if not _basename_ok(os.path.basename(path)):
        raise CloudError("Only a blueprint file is uploaded.")


def _connect_ftp(settings):
    import ftplib

    host = settings["ftp_host"].strip()
    port = int(settings["ftp_port"])
    tls = bool(settings.get("ftp_tls", True))
    client = ftplib.FTP_TLS() if tls else ftplib.FTP()
    client.connect(host, port, timeout=120)
    try:
        client.login(settings.get("ftp_user") or "", settings.get("ftp_password") or "")
        if tls:
            client.prot_p()
        client.set_pasv(True)
    except Exception:
        try:
            client.close()
        except Exception:
            pass
        raise
    return client


def _ftp_upload(path, settings, ftp_factory=None):
    _require_blueprint(path)
    host = settings["ftp_host"].strip()
    directory = (settings.get("ftp_directory") or "").strip()
    if not _host_ok(host) or not _directory_ok(directory):
        raise CloudError("The FTP address was refused.")
    name = os.path.basename(path)
    own = ftp_factory is None
    client = _connect_ftp(settings) if own else ftp_factory(settings)
    try:
        if directory:
            client.cwd(directory)
        with open(path, "rb") as handle:
            client.storbinary("STOR " + name, handle, blocksize=BLOCK)
    finally:
        if own:
            try:
                client.quit()
            except Exception:
                pass
    folder = directory.strip("/")
    if folder:
        return "%s/%s/%s" % (host, folder, name)
    return "%s/%s" % (host, name)


def _cid_from(body):
    cid = ""
    for line in body.decode("utf-8", "replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(row, dict):
            found = row.get("Hash") or row.get("Cid") or ""
            if isinstance(found, str):
                cid = found.strip()
    if not cid or len(cid) > 128 or not cid.isalnum():
        return ""
    return cid


def _ipfs_upload(path, settings):
    import http.client

    _require_blueprint(path)
    api = (settings.get("ipfs_api") or DEFAULT_IPFS).strip()
    if not _api_ok(api):
        raise CloudError("The IPFS address was refused.")
    parsed = urlparse(api)
    name = os.path.basename(path)
    boundary = "vmi" + os.urandom(16).hex()
    prefix = (
        "--%s\r\n"
        "Content-Disposition: form-data; name=\"file\"; filename=\"%s\"\r\n"
        "Content-Type: application/octet-stream\r\n"
        "\r\n"
    ) % (boundary, name)
    suffix = "\r\n--%s--\r\n" % boundary
    prefix_b = prefix.encode("ascii")
    suffix_b = suffix.encode("ascii")
    size = os.path.getsize(path)
    length = len(prefix_b) + size + len(suffix_b)
    port = parsed.port or (443 if parsed.scheme == "https" else 80)
    if parsed.scheme == "https":
        conn = http.client.HTTPSConnection(parsed.hostname, port, timeout=120)
    else:
        conn = http.client.HTTPConnection(parsed.hostname, port, timeout=120)
    try:
        base = (parsed.path or "").rstrip("/")
        target = "%s/api/v0/add?pin=true&cid-version=1" % base
        conn.putrequest("POST", target)
        conn.putheader("Content-Type", "multipart/form-data; boundary=%s" % boundary)
        conn.putheader("Content-Length", str(length))
        conn.putheader("Connection", "close")
        conn.endheaders()
        conn.send(prefix_b)
        with open(path, "rb") as handle:
            while True:
                block = handle.read(BLOCK)
                if not block:
                    break
                conn.send(block)
        conn.send(suffix_b)
        response = conn.getresponse()
        if 300 <= response.status < 400:
            response.read()
            raise CloudError("The IPFS address was refused.")
        body = response.read()
        if response.status != 200:
            raise CloudError("IPFS did not accept the file.")
    finally:
        conn.close()
    cid = _cid_from(body)
    if not cid:
        raise CloudError("IPFS did not return an id.")
    return cid


def upload(path, settings, ftp_factory=None):
    """Send one saved .vmib. The password is never part of the error text."""
    settings = normalize(settings)
    secret = settings.get("ftp_password") or ""
    try:
        if not usable(settings):
            raise CloudError("Cloud storage is off.")
        if settings["kind"] == "ftp":
            return _ftp_upload(path, settings, ftp_factory)
        if settings["kind"] == "ipfs":
            return _ipfs_upload(path, settings)
        raise CloudError("Cloud storage is off.")
    except CloudError as exc:
        raise CloudError(_scrub(exc, secret)) from None
    except Exception as exc:
        raise CloudError(_scrub(exc, secret)) from None


class CloudThread(QThread):
    """Upload after the save thread has replaced the zip. One at a time."""

    ok = Signal(str)
    bad = Signal(str)

    def __init__(self, path, settings):
        super().__init__()
        self.path = path
        self.settings = dict(settings or {})

    def run(self):
        try:
            label = upload(self.path, self.settings)
        except CloudError as exc:
            self.bad.emit(str(exc))
            return
        self.ok.emit(label)


class CloudDialog(QDialog):
    """Where a saved blueprint may be copied. show() it. Tests call commit()."""

    def __init__(self, parent, settings):
        super().__init__(parent)
        self.setWindowTitle("Cloud storage")
        self.setMinimumWidth(460)
        data = normalize(settings)
        layout = QVBoxLayout(self)
        title = QLabel("CLOUD")
        title.setObjectName("title")
        layout.addWidget(title)
        self.enabled = QCheckBox("Upload a blueprint after it is saved")
        self.enabled.setChecked(bool(data["enabled"]))
        layout.addWidget(self.enabled)
        self.kind = QComboBox()
        self.kind.addItem("IPFS", "ipfs")
        self.kind.addItem("FTP", "ftp")
        self.kind.setCurrentIndex(0 if data["kind"] == "ipfs" else 1)
        layout.addWidget(self.kind)
        layout.addWidget(QLabel("IPFS API"))
        self.api = QLineEdit(data["ipfs_api"])
        self.api.setPlaceholderText(DEFAULT_IPFS)
        layout.addWidget(self.api)
        layout.addWidget(QLabel("FTP host"))
        self.host = QLineEdit(data["ftp_host"])
        layout.addWidget(self.host)
        port_row = QHBoxLayout()
        port_row.addWidget(QLabel("Port"))
        self.port = QSpinBox()
        self.port.setRange(1, 65535)
        self.port.setValue(int(data["ftp_port"]))
        port_row.addWidget(self.port)
        self.tls = QCheckBox("TLS")
        self.tls.setChecked(bool(data["ftp_tls"]))
        port_row.addWidget(self.tls)
        layout.addLayout(port_row)
        self.user = QLineEdit(data["ftp_user"])
        self.user.setPlaceholderText("User")
        layout.addWidget(self.user)
        self.password = QLineEdit(data["ftp_password"])
        self.password.setEchoMode(QLineEdit.EchoMode.Password)
        self.password.setPlaceholderText("Password")
        layout.addWidget(self.password)
        self.directory = QLineEdit(data["ftp_directory"])
        self.directory.setPlaceholderText("Folder on the server")
        layout.addWidget(self.directory)
        self.note = QLabel(
            "The password stays on this computer. It is not written into the blueprint."
        )
        self.note.setWordWrap(True)
        layout.addWidget(self.note)
        buttons = QHBoxLayout()
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        save_button = QPushButton("Save")
        save_button.setObjectName("export")
        save_button.clicked.connect(self._accept)
        buttons.addWidget(cancel)
        buttons.addWidget(save_button)
        layout.addLayout(buttons)

    def values(self):
        return normalize({
            "enabled": self.enabled.isChecked(),
            "kind": self.kind.currentData(),
            "ipfs_api": self.api.text(),
            "ftp_host": self.host.text(),
            "ftp_port": self.port.value(),
            "ftp_user": self.user.text(),
            "ftp_password": self.password.text(),
            "ftp_directory": self.directory.text(),
            "ftp_tls": self.tls.isChecked(),
        })

    def commit(self):
        data = self.values()
        if data["enabled"] and not usable(data):
            return None
        save(data)
        return data

    def _accept(self):
        saved = self.commit()
        if saved is None:
            self.note.setText("That address cannot be used. Nothing was saved.")
            return
        self.accept()
