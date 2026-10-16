"""Fetch an iCloud Drive file onto this Mac before the studio reads it."""

import os

# UF_DATALESS. The reported size is the cloud size. The bytes are not here yet.
_DATALESS = 0x40000000
_CONTAINER = os.path.join("Library", "Mobile Documents", "com~apple~CloudDocs")


def drive_root(home=None):
    """The iCloud Drive folder, or an empty string when this Mac has none."""
    home = home or os.path.expanduser("~")
    path = os.path.join(home, _CONTAINER)
    if os.path.isdir(path):
        return path
    return ""


def in_drive(path, home=None):
    """True when path is iCloud Drive or a file inside it."""
    root = drive_root(home)
    if not root or not path:
        return False
    try:
        path = os.path.abspath(path)
        root = os.path.abspath(root)
    except (OSError, ValueError):
        return False
    if path == root:
        return True
    try:
        return os.path.commonpath([path, root]) == root
    except ValueError:
        return False


def warm(path):
    """List a folder so the UI thread can open it without waiting on iCloud.

    The first listing is what asks macOS for permission. Call this off the UI
    thread. A folder outside iCloud is listed the same way.
    """
    path = os.path.abspath(path) if path else ""
    name = os.path.basename(path) or path or "that folder"
    if not path:
        raise OSError("That folder is not there.")
    try:
        os.listdir(path)
    except OSError as exc:
        raise OSError("iCloud did not open %s. %s" % (name, exc)) from exc
    return path


def is_evicted(path):
    """True when iCloud still holds the bytes and this Mac does not."""
    if not path:
        return False
    try:
        flags = os.lstat(path).st_flags
    except (OSError, AttributeError):
        return False
    return bool(flags & _DATALESS)


def ensure_local(path):
    """Download path when it is only in iCloud. A local file is left alone.

    Opening the file asks the system to fetch it. The call returns when the
    bytes are here. A file outside iCloud is unchanged.
    """
    path = os.path.abspath(path) if path else ""
    if not path or not is_evicted(path):
        return path
    name = os.path.basename(path)
    try:
        with open(path, "rb") as handle:
            handle.read(1)
    except OSError as exc:
        raise OSError("iCloud did not download %s. %s" % (name, exc)) from exc
    if is_evicted(path):
        raise OSError("iCloud did not finish downloading %s." % name)
    return path
