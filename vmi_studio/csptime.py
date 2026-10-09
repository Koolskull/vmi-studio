"""Clip Studio animation, read from the file's own timeline.

An animation folder is not just a stack of layers. Clip Studio stores a
cut: a frame range, a rate, and a specification that puts a cel on a frame.
The cel shows from that frame until the next specified cel. A child that
is never specified stays in the folder and never reaches the picture.
"""

import os
import sqlite3
import struct
import tempfile
import zlib


# Bytes in one array element, after the string table. Unknown types are skipped
# as a failed curve so a later ImageCelName can still be read.
_ARRAY_BYTES = {
    "Byte[]": 1,
    "Single[]": 4,
    "String[]": 4,
    "Int32[]": 4,
    "UInt32[]": 4,
    "Float2[]": 8,
    "Double[]": 8,
    "Float3[]": 12,
    "Quat[]": 16,
    "Double2[]": 16,
    "Double3[]": 24,
    "Matrix44[]": 64,
}


def read_clip_time(path):
    """The timeline in a .clip, or None when the file has none."""
    blob = _sqlite_bytes(path)
    if not blob:
        return None
    handle = tempfile.NamedTemporaryFile(prefix="vmi-clip-", suffix=".sqlite", delete=False)
    try:
        handle.write(blob)
        handle.close()
        return _from_sqlite(handle.name, path)
    except (OSError, sqlite3.Error, ValueError, zlib.error):
        return None
    finally:
        try:
            os.remove(handle.name)
        except OSError:
            pass


def apply_clip_time(art, info):
    """Mark the animation folders and put each specified cel on its frame."""
    if art is None or not info:
        return art
    art.clip_time = {
        "name": info.get("name") or "Timeline",
        "fps": int(info.get("fps") or 24),
        "start": int(info.get("start") or 0),
        "end": int(info.get("end") or 0),
        "current": int(info.get("current") or 0),
    }
    groups = [node for node in _walk(art.layers) if node.kind == "group"]
    used = set()
    for spec in info.get("folders") or []:
        node = _match_folder(groups, used, spec)
        if node is None:
            continue
        used.add(id(node))
        _place(node, spec)
        if node.frames:
            node.show_frame = art.clip_time["current"]
    return art


def _walk(nodes):
    for node in nodes:
        yield node
        yield from _walk(node.children)


def _match_folder(groups, used, spec):
    wanted = list(spec.get("cels") or [])
    for node in groups:
        if id(node) in used or node.name != spec.get("name"):
            continue
        names = [child.name for child in node.children]
        if sorted(names) != sorted(wanted):
            continue
        return node
    return None


def _place(node, spec):
    """Specified cels land on their frames. The rest stay off the timeline."""
    pools = {}
    for child in node.children:
        pools.setdefault(child.name, []).append(child)
    # Clip Studio's link order, so two cels with one name stay in file order.
    ordered = []
    for name in spec.get("cels") or []:
        pool = pools.get(name) or []
        if pool:
            ordered.append(pool.pop(0))
    by_name = {}
    for child in ordered:
        by_name.setdefault(child.name, []).append(child)
    seen = {}
    keys = []
    for frame, name in sorted((spec.get("keys") or {}).items(), key=lambda item: int(item[0])):
        pool = by_name.get(name) or []
        if pool:
            cel = pool.pop(0)
            seen[name] = cel
        else:
            # The same cel can be specified again later in the cut.
            cel = seen.get(name)
            if cel is None:
                continue
        keys.append((int(frame), cel))
    node.animation = True
    node.timeline = [cel.id for _frame, cel in keys]
    node.frames = [frame for frame, _cel in keys]
    for child in node.children:
        child.animation = False
        child.timeline = []
        child.frames = []


def _sqlite_bytes(path):
    try:
        handle = open(path, "rb")
    except OSError:
        return None
    with handle:
        if handle.read(8) != b"CSFCHUNK":
            return None
        handle.seek(24)
        while True:
            header = handle.read(16)
            if len(header) < 16 or header[:4] != b"CHNK":
                return None
            size = int.from_bytes(header[12:16], "big")
            if size < 0:
                return None
            if header[4:8] == b"SQLi":
                blob = handle.read(size)
                return blob if len(blob) == size else None
            handle.seek(size, os.SEEK_CUR)


def _from_sqlite(db_path, clip_path):
    connection = sqlite3.connect(db_path)
    try:
        root = connection.execute("SELECT CanvasRootFolder FROM Canvas").fetchone()
        if not root:
            return None
        layers = {}
        for row in connection.execute(
            "SELECT MainId, LayerName, LayerUuid, AnimationFolder, "
            "LayerFirstChildIndex, LayerNextIndex FROM Layer"
        ):
            layers[row[0]] = {
                "id": row[0],
                "name": row[1] or "",
                "uuid": _uuid_bytes(row[2]),
                "animation": bool(row[3]),
                "first": row[4] or 0,
                "next": row[5] or 0,
            }
        time_row = connection.execute(
            "SELECT TimeLineName, FrameRate, StartFrame, EndFrame, CurrentFrame "
            "FROM TimeLine ORDER BY MainId LIMIT 1"
        ).fetchone()
        if time_row is None:
            return None
        mixers = {}
        for uuid, blob in connection.execute(
            "SELECT LayerUuidWithTrack, TrackActionMixer FROM Track"
        ):
            if uuid and blob:
                mixers[bytes(uuid)] = bytes(blob)
        chunks = _external_chunks(clip_path)
    finally:
        connection.close()

    def kids(ident):
        found = []
        child = layers.get(ident, {}).get("first") or 0
        seen = set()
        while child and child in layers and child not in seen:
            seen.add(child)
            found.append(layers[child])
            child = layers[child]["next"]
        return found

    def walk(ident):
        node = layers.get(ident)
        if node is None:
            return
        if node["animation"]:
            yield node
        for child in kids(ident):
            yield from walk(child["id"])

    name, fps_raw, start, end, current = time_row
    fps = int(round(float(fps_raw or 24)))
    folders = []
    for node in walk(root[0]):
        cels = [child["name"] for child in kids(node["id"])]
        raw = mixers.get(node["uuid"] or b"")
        keys = _keys_from_mixer(chunks.get(raw), fps) if raw else {}
        folders.append({"name": node["name"], "cels": cels, "keys": keys})
    return {
        "name": name or "Timeline",
        "fps": fps,
        "start": int(round(float(start or 0))),
        "end": int(round(float(end or 0))),
        "current": int(round(float(current or 0))),
        "folders": folders,
    }


def _uuid_bytes(value):
    if isinstance(value, (bytes, bytearray, memoryview)):
        raw = bytes(value)
        return raw if len(raw) == 16 else None
    text = (value or "").replace("-", "").strip()
    if len(text) != 32:
        return None
    try:
        return bytes.fromhex(text)
    except ValueError:
        return None


def _external_chunks(path):
    found = {}
    try:
        handle = open(path, "rb")
    except OSError:
        return found
    with handle:
        if handle.read(8) != b"CSFCHUNK":
            return found
        handle.seek(24)
        while True:
            header = handle.read(16)
            if len(header) < 16 or header[:4] != b"CHNK":
                break
            size = int.from_bytes(header[12:16], "big")
            if header[4:8] != b"Exta":
                handle.seek(size, os.SEEK_CUR)
                continue
            blob = handle.read(size)
            if len(blob) < 16:
                continue
            length = int.from_bytes(blob[:8], "big")
            if length < 8 or 8 + length > len(blob):
                continue
            ident = bytes(blob[8:8 + length])
            found[ident] = blob[length + 16:]
    return found


def _keys_from_mixer(blob, fps):
    """Cel name per display frame, from the track's ImageCelName curve.

    The mixer is a ``cmt 0100binc`` document. Key times are on a 60 Hz clock.
    A display frame is that time times the timeline's frame rate, divided by
    60. The cel name is the key's tag. A folder with no such curve has no
    specified cels.
    """
    raw = _inflate(blob) if blob else None
    if not raw:
        return {}
    strings, start = _string_table(raw)
    if not strings or "FCurve" not in strings or start is None:
        return {}
    rate = float(fps or 24)
    if rate <= 0:
        rate = 24.0
    fcurve = strings.index("FCurve")
    chosen = None
    pos = start
    size = len(raw)
    while pos + 12 <= size:
        kind, keys, cursor = _curve_at(raw, pos, strings, fcurve, rate)
        if kind is None or cursor <= pos:
            pos += 1
            continue
        if kind == "ImageCelName" and chosen is None:
            chosen = keys
        pos = cursor
    return chosen or {}


def _string_table(raw):
    if len(raw) < 20 or raw[:12] not in (b"cmt 0100binc", b"cmt 0110binc"):
        return None, None
    count = struct.unpack_from("<I", raw, 16)[0]
    if count > 4096:
        return None, None
    cursor = 20
    strings = []
    for _index in range(count):
        if cursor >= len(raw):
            return None, None
        length = raw[cursor]
        cursor += 1
        end = cursor + length
        if end > len(raw):
            return None, None
        try:
            strings.append(raw[cursor:end].decode("utf-8"))
        except UnicodeDecodeError:
            return None, None
        cursor = end
    return strings, cursor


def _curve_at(raw, pos, strings, fcurve, rate):
    """One FCurve at this byte, or (None, None, pos) when the header is not one."""
    if struct.unpack_from("<I", raw, pos)[0] != fcurve:
        return None, None, pos
    if struct.unpack_from("<I", raw, pos + 4)[0] != 0:
        return None, None, pos
    props = struct.unpack_from("<I", raw, pos + 8)[0]
    if not 1 <= props <= 8:
        return None, None, pos
    cursor = pos + 12
    kind = None
    for _index in range(props):
        if cursor + 8 > len(raw):
            return None, None, pos
        name_i, value_i = struct.unpack_from("<II", raw, cursor)
        cursor += 8
        if name_i >= len(strings) or value_i >= len(strings):
            return None, None, pos
        name, value = strings[name_i], strings[value_i]
        if name == "Type" and kind is None:
            kind = value
        elif name == "Type":
            return None, None, pos
    if not kind or cursor + 4 > len(raw):
        return None, None, pos
    field_count = struct.unpack_from("<I", raw, cursor)[0]
    cursor += 4
    if field_count > 64:
        return None, None, pos
    frames = None
    tags = None
    for _index in range(field_count):
        if cursor + 12 > len(raw):
            return None, None, pos
        field_i, type_i, count = struct.unpack_from("<III", raw, cursor)
        cursor += 12
        if field_i >= len(strings) or type_i >= len(strings) or count > 100000:
            return None, None, pos
        field, type_name = strings[field_i], strings[type_i]
        width = _ARRAY_BYTES.get(type_name)
        if width is None:
            return None, None, pos
        nbytes = count * width
        if cursor + nbytes > len(raw):
            return None, None, pos
        if field == "Frame" and type_name == "Single[]":
            frames = struct.unpack_from("<%df" % count, raw, cursor) if count else ()
        elif field == "Tag" and type_name == "String[]":
            indexes = struct.unpack_from("<%dI" % count, raw, cursor) if count else ()
            if any(index >= len(strings) for index in indexes):
                return None, None, pos
            tags = [strings[index] for index in indexes]
        cursor += nbytes
        if cursor + 8 > len(raw):
            return None, None, pos
        end_a, end_b = struct.unpack_from("<II", raw, cursor)
        cursor += 8
        if end_a != 0 or end_b != 0:
            return None, None, pos
    if frames is None or tags is None or len(frames) != len(tags):
        return kind, {}, cursor
    keys = {}
    for time_60, tag in zip(frames, tags):
        if not tag:
            continue
        frame = int(round(float(time_60) * rate / 60.0))
        if frame < 0:
            continue
        keys[frame] = tag
    return kind, keys, cursor


def _inflate(blob):
    for start in (4, 0):
        try:
            return zlib.decompress(blob[start:])
        except zlib.error:
            continue
    return None


