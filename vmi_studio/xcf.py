"""Read a GIMP XCF. 8-bit RLE and uncompressed tiles become pictures. Zlib stays names."""

import os
import struct
import zlib

from vmi_studio.document import ArtFile, Ids, Node, Raster, refresh


PROP_END = 0
PROP_OPACITY = 6
PROP_VISIBLE = 8
PROP_OFFSETS = 15
PROP_COMPRESSION = 17
PROP_GROUP_ITEM = 29
PROP_ITEM_PATH = 30

COMPRESS_NONE = 0
COMPRESS_RLE = 1


class XcfError(Exception):
    pass


def load(path):
    path = os.path.abspath(path)
    with open(path, "rb") as handle:
        data = handle.read()
    art = parse(data, os.path.basename(path))
    art.path = path
    return art


def parse(data, file_name):
    if len(data) < 26 or not data.startswith(b"gimp xcf "):
        raise XcfError("This is not a GIMP file.")
    version = _version(data[:14])
    if version < 0:
        raise XcfError("This GIMP file uses a version this reader does not know.")
    offset = {"v": 14}
    width = _u32(data, offset)
    height = _u32(data, offset)
    base_type = _u32(data, offset)
    if version >= 4:
        _u32(data, offset)
    props = _properties(data, offset)
    compression = _prop_u32(props.get(PROP_COMPRESSION), COMPRESS_RLE)
    wide = version >= 11
    layer_offsets = []
    pointer = _pointer(data, offset, wide)
    while pointer:
        layer_offsets.append(pointer)
        pointer = _pointer(data, offset, wide)
    indexed = base_type == 2
    drafts = []
    skipped = False
    for index, layer_offset in enumerate(layer_offsets):
        layer = _read_layer(data, layer_offset, wide)
        path = _prop_path(layer["props"].get(PROP_ITEM_PATH), index)
        group = PROP_GROUP_ITEM in layer["props"]
        visible = _prop_u32(layer["props"].get(PROP_VISIBLE), 1) != 0
        opacity = _prop_u32(layer["props"].get(PROP_OPACITY), 255)
        origin_x, origin_y = _prop_offsets(layer["props"].get(PROP_OFFSETS))
        raster = None
        if not group and not indexed and layer["hierarchy"] and compression in (COMPRESS_NONE, COMPRESS_RLE):
            try:
                raster = _hierarchy(
                    data, layer["hierarchy"], wide, compression,
                    origin_x, origin_y, layer["width"], layer["height"],
                )
                if raster is None:
                    skipped = True
            except (XcfError, zlib.error):
                skipped = True
        elif not group and not indexed and compression not in (COMPRESS_NONE, COMPRESS_RLE):
            skipped = True
        drafts.append({
            "path": path,
            "node": Node(
                "",
                layer["name"] or ("group" if group else "layer"),
                "group" if group else "layer",
                visible=visible,
                opacity=max(0, min(255, opacity)),
                raster=raster,
            ),
        })
    if indexed:
        note = "The stack is loaded. Indexed GIMP color is left as names."
    elif skipped:
        note = "The stack is loaded. Some layers did not include a picture this reader knows."
    elif any(draft["node"].raster is not None for draft in drafts):
        note = "Every layer's paint is loaded."
    else:
        note = "The stack is loaded."
    layers = refresh(_assemble(drafts))
    return ArtFile(file_name, "", "xcf", width, height, layers, note)


def _version(magic):
    tag = magic[9:13]
    if tag == b"file":
        return 0
    if len(tag) == 4 and tag[:1] == b"v" and tag[1:].isdigit():
        return int(tag[1:])
    return -1


def _assemble(drafts):
    built = []
    for draft in drafts:
        path = draft["path"]
        built.append({
            "path": path,
            "index": path[-1] if path else 0,
            "node": draft["node"],
            "kids": [],
        })
    by_path = {".".join(str(part) for part in item["path"]): item for item in built}
    roots = []
    for item in built:
        if len(item["path"]) <= 1:
            roots.append(item)
            continue
        parent_key = ".".join(str(part) for part in item["path"][:-1])
        parent = by_path.get(parent_key)
        if parent:
            parent["kids"].append(item)
        else:
            roots.append(item)
    ids = Ids()

    def freeze(lst):
        ordered = sorted(lst, key=lambda item: item["index"], reverse=True)
        out = []
        for item in ordered:
            node = item["node"]
            children = freeze(item["kids"])
            if children:
                node.kind = "group"
            node.children = children
            node.id = ids.next()
            out.append(node)
        return out

    return freeze(roots)


def _read_layer(data, at, wide):
    offset = {"v": at}
    width = _u32(data, offset)
    height = _u32(data, offset)
    _u32(data, offset)
    name = _string(data, offset)
    props = _properties(data, offset)
    hierarchy = _pointer(data, offset, wide)
    _pointer(data, offset, wide)
    return {"width": width, "height": height, "name": name, "props": props, "hierarchy": hierarchy}


def _hierarchy(data, at, wide, compression, origin_x, origin_y, layer_w, layer_h):
    if not layer_w or not layer_h:
        return None
    offset = {"v": at}
    _u32(data, offset)
    _u32(data, offset)
    bpp = _u32(data, offset)
    if bpp < 1 or bpp > 4:
        return None
    levels = []
    level = _pointer(data, offset, wide)
    while level:
        levels.append(level)
        level = _pointer(data, offset, wide)
    if not levels:
        return None
    level_offset = {"v": levels[0]}
    _u32(data, level_offset)
    _u32(data, level_offset)
    tiles = []
    tile = _pointer(data, level_offset, wide)
    while tile:
        tiles.append(tile)
        tile = _pointer(data, level_offset, wide)
    rgba = bytearray(layer_w * layer_h * 4)
    tiles_x = max(1, (layer_w + 63) // 64)
    for index, tile_at in enumerate(tiles):
        tx = index % tiles_x
        ty = index // tiles_x
        tw = min(64, layer_w - tx * 64)
        th = min(64, layer_h - ty * 64)
        if tw <= 0 or th <= 0:
            continue
        channels = _tile_channels(data, tile_at, compression, tw, th, bpp)
        if channels is None:
            continue
        for y in range(th):
            for x in range(tw):
                sample = y * tw + x
                di = ((ty * 64 + y) * layer_w + (tx * 64 + x)) * 4
                if bpp == 1:
                    rgba[di] = rgba[di + 1] = rgba[di + 2] = channels[0][sample]
                    rgba[di + 3] = 255
                elif bpp == 2:
                    rgba[di] = rgba[di + 1] = rgba[di + 2] = channels[0][sample]
                    rgba[di + 3] = channels[1][sample]
                else:
                    rgba[di] = channels[0][sample]
                    rgba[di + 1] = channels[1][sample]
                    rgba[di + 2] = channels[2][sample]
                    rgba[di + 3] = channels[3][sample] if bpp > 3 else 255
    return Raster(origin_x, origin_y, layer_w, layer_h, bytes(rgba))


def _tile_channels(data, at, compression, tw, th, bpp):
    count = tw * th
    if compression == COMPRESS_NONE:
        if at + count * bpp > len(data):
            return None
        channels = []
        for channel in range(bpp):
            plane = bytearray(count)
            for i in range(count):
                plane[i] = data[at + i * bpp + channel]
            channels.append(plane)
        return channels
    cursor = {"v": at}
    return [_rle(data, cursor, count) for _channel in range(bpp)]


def _rle(data, cursor, count):
    out = bytearray(count)
    filled = 0
    while filled < count:
        if cursor["v"] >= len(data):
            break
        opcode = data[cursor["v"]]
        cursor["v"] += 1
        if opcode <= 126:
            run = opcode + 1
            value = data[cursor["v"]] if cursor["v"] < len(data) else 0
            cursor["v"] += 1
            end = min(count, filled + run)
            out[filled:end] = bytes((value,)) * (end - filled)
            filled += run
        elif opcode == 127:
            if cursor["v"] + 2 >= len(data):
                break
            run = data[cursor["v"]] * 256 + data[cursor["v"] + 1]
            value = data[cursor["v"] + 2]
            cursor["v"] += 3
            end = min(count, filled + run)
            out[filled:end] = bytes((value,)) * (end - filled)
            filled += run
        elif opcode == 128:
            if cursor["v"] + 1 >= len(data):
                break
            run = data[cursor["v"]] * 256 + data[cursor["v"] + 1]
            cursor["v"] += 2
            out[filled:filled + run] = data[cursor["v"]:cursor["v"] + run]
            cursor["v"] += run
            filled += run
        else:
            run = 256 - opcode
            out[filled:filled + run] = data[cursor["v"]:cursor["v"] + run]
            cursor["v"] += run
            filled += run
    return out


def _properties(data, offset):
    props = {}
    for _guard in range(10000):
        kind = _u32(data, offset)
        size = _u32(data, offset)
        if offset["v"] + size > len(data):
            raise XcfError("This GIMP file stopped in the middle.")
        payload = data[offset["v"]:offset["v"] + size]
        offset["v"] += size
        if kind == PROP_END:
            break
        props[kind] = payload
    return props


def _prop_u32(payload, fallback):
    if not payload or len(payload) < 4:
        return fallback
    return struct.unpack_from(">I", payload, 0)[0]


def _prop_offsets(payload):
    if not payload or len(payload) < 8:
        return 0, 0
    return _signed(payload, 0), _signed(payload, 4)


def _prop_path(payload, fallback):
    if not payload or len(payload) < 4:
        return [fallback]
    path = [struct.unpack_from(">I", payload, i)[0] for i in range(0, len(payload) - 3, 4)]
    return path or [fallback]


def _signed(payload, at):
    number = struct.unpack_from(">I", payload, at)[0]
    return number - 0x100000000 if number > 0x7FFFFFFF else number


def _string(data, offset):
    length = _u32(data, offset)
    if length == 0:
        return ""
    if offset["v"] + length > len(data):
        raise XcfError("This GIMP file stopped in the middle.")
    raw = data[offset["v"]:offset["v"] + length]
    offset["v"] += length
    if raw.endswith(b"\x00"):
        raw = raw[:-1]
    return raw.decode("utf-8", "replace")


def _u32(data, offset):
    if offset["v"] + 4 > len(data):
        raise XcfError("This GIMP file stopped in the middle.")
    number = struct.unpack_from(">I", data, offset["v"])[0]
    offset["v"] += 4
    return number


def _pointer(data, offset, wide):
    if not wide:
        return _u32(data, offset)
    if offset["v"] + 8 > len(data):
        raise XcfError("This GIMP file stopped in the middle.")
    number = struct.unpack_from(">Q", data, offset["v"])[0]
    offset["v"] += 8
    return number
