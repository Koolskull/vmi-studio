"""UI faces from the bgcardbuilder catalog.

The kits in that repo are mostly WOFF. Qt loads TrueType, so a WOFF is
unpacked into the cache the first time it is used. Google faces in the same
catalog are downloaded the first time they are chosen. The default face is
the system monospace.
"""

import os
import re
import struct
import urllib.parse
import urllib.request
import zlib

from PySide6.QtGui import QFont, QFontDatabase, QFontInfo


DEFAULT_ID = "monospace"


def catalog_root():
    """Optional font catalog. Missing means the menu is system monospace only."""
    env = os.environ.get("BGCARDBUILDER")
    if env:
        return env
    home = os.path.join(os.path.expanduser("~"), "Documents", "work", "Github", "bgcardbuilder")
    if os.path.isdir(os.path.join(home, "src", "data")):
        return home
    return ""


ROOT = catalog_root()
CACHE = os.path.join(os.path.expanduser("~"), ".cache", "vmi-studio", "fonts")
CATEGORY_ORDER = ("monospace", "pixel", "sans-serif", "serif", "display")
CATEGORY_LABELS = {
    "monospace": "Monospace",
    "pixel": "Pixel",
    "sans-serif": "Sans",
    "serif": "Serif",
    "display": "Display",
}
_UA = "Mozilla/5.0 (Windows NT 6.1; WOW64; rv:40.0) Gecko/20100101 Firefox/40.0"
_ENTRY = re.compile(
    r"id:\s*'(?P<id>[^']+)'\s*,\s*"
    r"name:\s*'(?P<name>[^']*)'\s*,\s*"
    r"family:\s*'(?P<family>[^']*)'\s*,\s*"
    r"category:\s*'(?P<category>[^']+)'",
    re.S,
)
_FACE_BLOCK = re.compile(r"@font-face\s*\{([^}]+)\}", re.I)
_catalog = None
_loaded = {}
_installed = None
_system = None
_current = DEFAULT_ID


class Face(object):
    def __init__(self, ident, name, category, local=None, google=None):
        self.id = ident
        self.name = name
        self.category = category
        self.local = local
        self.google = google


def current_id():
    return _current


def catalog():
    global _catalog
    if _catalog is None:
        _catalog = _read_catalog()
    return list(_catalog)


def get(ident):
    for face in catalog():
        if face.id == ident:
            return face
    return None


def activate(ident):
    """Load ident and remember it. Raises when that face cannot be loaded."""
    global _current
    face = get(ident) or get(DEFAULT_ID)
    family = family_for(face, fetch=True)
    _current = face.id
    return face, family


def preview_family(ident):
    """Family for a menu row. Skips any face that still needs a download."""
    face = get(ident)
    if face is None:
        return None
    try:
        return family_for(face, fetch=False)
    except Exception:
        return None


def system_family():
    global _system
    if _system:
        return _system
    font = QFont()
    font.setStyleHint(QFont.StyleHint.Monospace)
    font.setFamily("monospace")
    _system = QFontInfo(font).family() or "Monospace"
    return _system


def family_for(face, fetch):
    if face.id in _loaded:
        return _loaded[face.id]
    if face.id == DEFAULT_ID:
        family = system_family()
        _loaded[face.id] = family
        return family
    if face.local:
        path = _local_ttf(face)
        return _remember(face, path)
    installed = _installed_family(face.google) if face.google else None
    if installed:
        _loaded[face.id] = installed
        return installed
    dest = os.path.join(CACHE, face.id + ".ttf")
    if os.path.isfile(dest) and os.path.getsize(dest) > 64:
        return _remember(face, dest)
    if not fetch or not face.google:
        raise RuntimeError("not loaded")
    _fetch_google(face, dest)
    return _remember(face, dest)


def _remember(face, path):
    ident = QFontDatabase.addApplicationFont(path)
    families = QFontDatabase.applicationFontFamilies(ident) if ident >= 0 else []
    if not families:
        if os.path.dirname(path) == CACHE:
            try:
                os.remove(path)
            except OSError:
                pass
        raise RuntimeError("Qt did not load %s" % face.name)
    _loaded[face.id] = families[0]
    return families[0]


def _installed_family(name):
    global _installed
    if not name:
        return None
    if _installed is None:
        _installed = {fam.lower(): fam for fam in QFontDatabase.families()}
    return _installed.get(name.lower())


def _local_ttf(face):
    src = face.local
    if src.lower().endswith((".ttf", ".otf")):
        return src
    dest = os.path.join(CACHE, face.id + ".ttf")
    if os.path.isfile(dest) and os.path.getmtime(dest) >= os.path.getmtime(src) and os.path.getsize(dest) > 64:
        return dest
    os.makedirs(CACHE, exist_ok=True)
    if src.lower().endswith(".woff"):
        with open(src, "rb") as handle:
            data = woff_to_sfnt(handle.read())
        with open(dest, "wb") as handle:
            handle.write(data)
        return dest
    if src.lower().endswith(".woff2"):
        _woff2_to_ttf(src, dest)
        return dest
    raise RuntimeError("unsupported font file")


def _fetch_google(face, dest):
    query = urllib.parse.quote_plus(face.google)
    css_url = "https://fonts.googleapis.com/css2?family=%s&display=swap" % query
    css = _read_url(css_url, 30)
    url = _pick_font_url(css)
    host = urllib.parse.urlparse(url).netloc
    if host != "fonts.gstatic.com":
        raise RuntimeError("unexpected font host")
    raw = _read_url(url, 60, binary=True)
    os.makedirs(CACHE, exist_ok=True)
    kind = raw[:4]
    if kind == b"wOF2":
        blob = dest + ".woff2"
        with open(blob, "wb") as handle:
            handle.write(raw)
        _woff2_to_ttf(blob, dest)
        return
    if kind == b"wOFF":
        with open(dest, "wb") as handle:
            handle.write(woff_to_sfnt(raw))
        return
    if kind not in (b"\x00\x01\x00\x00", b"OTTO"):
        raise RuntimeError("not a font")
    with open(dest, "wb") as handle:
        handle.write(raw)


def _read_url(url, timeout, binary=False):
    request = urllib.request.Request(url, headers={"User-Agent": _UA})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        data = response.read()
    return data if binary else data.decode("utf-8", "replace")


def _pick_font_url(css):
    fallback = None
    latin = None
    for block in _FACE_BLOCK.findall(css):
        match = re.search(r"url\(\s*([^)\s]+)\s*\)", block)
        if not match:
            continue
        url = match.group(1).strip("'\"")
        if "unicode-range" not in block:
            return url
        if fallback is None:
            fallback = url
        low = block.lower()
        if "/* latin */" in low or "u+0000-00ff" in low:
            latin = url
    if latin or fallback:
        return latin or fallback
    raise RuntimeError("no font file")


def _woff2_to_ttf(src, dest):
    from fontTools.ttLib import TTFont

    font = TTFont(src)
    part = dest + ".part"
    try:
        font.flavor = None
        font.save(part)
    finally:
        font.close()
    os.replace(part, dest)


def woff_to_sfnt(data):
    """Unpack a WOFF (not WOFF2) into an SFNT the font loader can open."""
    if len(data) < 44 or data[:4] != b"wOFF":
        raise ValueError("not a woff")
    flavor, _length, num_tables = struct.unpack(">IIH", data[4:14])
    entries = []
    offset = 44
    for _index in range(num_tables):
        tag, table_off, comp, orig, checksum = struct.unpack(">4sIIII", data[offset:offset + 20])
        offset += 20
        blob = data[table_off:table_off + comp]
        if comp != orig:
            blob = zlib.decompress(blob)
        if len(blob) != orig:
            raise ValueError("bad table %r" % tag)
        entries.append((tag, checksum, blob))
    entries.sort(key=lambda item: item[0])
    count = len(entries)
    search = 1
    selector = 0
    while search * 2 <= count:
        search *= 2
        selector += 1
    header = struct.pack(
        ">IHHHH",
        flavor,
        count,
        search * 16,
        selector,
        count * 16 - search * 16,
    )
    records = []
    body = []
    cursor = 12 + 16 * count
    for tag, checksum, blob in entries:
        records.append(struct.pack(">4sIII", tag, checksum, cursor, len(blob)))
        padded = blob + b"\0" * ((4 - (len(blob) % 4)) % 4)
        body.append(padded)
        cursor += len(padded)
    return header + b"".join(records) + b"".join(body)


def _css_name(family):
    head = family.split(",", 1)[0].strip()
    if len(head) >= 2 and head[0] == head[-1] and head[0] in "\"'":
        head = head[1:-1]
    return head


def _read_catalog():
    faces = [Face(DEFAULT_ID, "Monospace", "monospace")]
    css_path = os.path.join(ROOT, "src", "index.css")
    ts_path = os.path.join(ROOT, "src", "data", "fonts.ts")
    files = {}
    try:
        with open(css_path, encoding="utf-8") as handle:
            files = _font_files(handle.read())
    except OSError:
        files = {}
    try:
        with open(ts_path, encoding="utf-8") as handle:
            text = handle.read()
    except OSError:
        return faces
    seen = {DEFAULT_ID}
    for match in _ENTRY.finditer(text):
        ident = match.group("id")
        if ident in seen:
            continue
        seen.add(ident)
        name = _css_name(match.group("family"))
        local = files.get(name)
        faces.append(Face(
            ident,
            match.group("name"),
            match.group("category"),
            local=local,
            google=None if local else name,
        ))
    return faces


def _font_files(css):
    found = {}
    rank = {".ttf": 0, ".otf": 1, ".woff": 2, ".woff2": 3}
    for block in _FACE_BLOCK.findall(css):
        family = re.search(r"font-family:\s*([^;]+);", block)
        if not family:
            continue
        name = family.group(1).strip().strip("'\"")
        if name in found:
            continue
        best = None
        score = 99
        for url in re.findall(r"url\(\s*['\"]?([^)'\"]+)['\"]?\s*\)", block):
            path = url.split("?", 1)[0]
            ext = os.path.splitext(path)[1].lower()
            if ext not in rank or rank[ext] >= score:
                continue
            if path.startswith("/"):
                full = os.path.join(ROOT, "public", path.lstrip("/"))
            else:
                full = os.path.normpath(os.path.join(ROOT, path))
            if os.path.isfile(full):
                best = full
                score = rank[ext]
        if best:
            found[name] = best
    return found
