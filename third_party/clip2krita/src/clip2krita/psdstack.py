"""Read a PSD layer stack, including the clipping flag Krita ignores.

Children are bottom to top, the same order as Krita's childNodes():
index 0 is the lowest layer. Krita's PSD loader adds each file record
on top of the previous one, and the file stores the bottom layer first.
Groups follow that loader: section-divider type 3 opens a group, and
type 1 or 2 names it.
"""

import struct


_SIGS = (b"8BIM", b"8B64")


class PsdError(Exception):
    pass


def load_stack(path, with_pixels=False):
    """Return a root dict with width, height, and bottom-to-top children.

    with_pixels attaches each layer's own paint as raster {x, y, w, h, rgba}.
    Report leaves this off. Channel id -2 is a layer mask and is not applied.
    """
    with open(path, "rb") as handle:
        data = handle.read()
    if len(data) < 26 or data[:4] != b"8BPS":
        raise PsdError("not a PSD or PSB file")
    version = struct.unpack_from(">H", data, 4)[0]
    if version not in (1, 2):
        raise PsdError("unsupported PSD version %s" % version)
    height, width = struct.unpack_from(">II", data, 14)
    depth = struct.unpack_from(">H", data, 22)[0]
    if with_pixels and depth != 8:
        raise PsdError("This file is %d-bit. Export needs an 8-bit drawing." % depth)
    long_len = version == 2
    pos = 26
    pos = _skip_section(data, pos, 4)
    pos = _skip_section(data, pos, 4)
    mask_len_size = 8 if long_len else 4
    mask_len, pos = _read_len(data, pos, mask_len_size)
    mask_end = pos + mask_len
    if mask_len == 0 or pos >= len(data):
        return _root(width, height, [])
    info_len_size = 8 if long_len else 4
    info_len, pos = _read_len(data, pos, info_len_size)
    info_end = pos + info_len
    if info_len == 0 or pos + 2 > mask_end:
        return _root(width, height, [])
    count = struct.unpack_from(">h", data, pos)[0]
    pos += 2
    count = abs(count)
    channel_len_size = 8 if long_len else 4
    records = []
    for _ in range(count):
        record, pos = _read_record(data, pos, channel_len_size, mask_end)
        records.append(record)
    if with_pixels:
        # Image data follows the records, in the same order, including the
        # empty blobs on folder markers. PSD row sizes are 2 bytes; PSB uses 4.
        _attach_pixels(data, pos, info_end, records, 2 if version == 1 else 4)
    children = _groups_top_to_bottom(records)
    return _root(width, height, children)


def flip_siblings(node):
    """Reverse every sibling list. Used when Krita's stack order differs."""
    flipped = dict(node)
    flipped["children"] = [flip_siblings(child) for child in reversed(node["children"])]
    return flipped


def format_stack(node):
    """Plain-text layer list, top of the stack first, with clip targets named."""
    lines = ["%d x %d" % (node["width"], node["height"])]
    body, layer_count, clip_count = _format_children(node["children"], 0)
    lines.append("%d layers, %d clipped onto a layer below" % (layer_count, clip_count))
    lines.extend(body)
    return "\n".join(lines)


def _root(width, height, children):
    return {
        "name": "",
        "kind": "group",
        "clip": False,
        "blend": "norm",
        "visible": True,
        "opacity": 255,
        "width": width,
        "height": height,
        "children": children,
    }


def _format_children(children, depth):
    lines = []
    layer_count = 0
    clip_count = 0
    # Stored bottom-to-top. Print the top of the stack first.
    ordered = list(reversed(children))
    index = 0
    children = ordered
    while index < len(children):
        child = children[index]
        note = ""
        if child["clip"]:
            clip_count += 1
            base_at = index
            while base_at < len(children) and children[base_at]["clip"]:
                base_at += 1
            target = children[base_at]["name"] if base_at < len(children) else "missing base"
            note = "    clips onto %s" % target
        if not child["visible"]:
            note += "    hidden"
        prefix = "folder  " if child["kind"] == "group" else "layer   "
        lines.append("%s%s%s%s" % ("  " * depth, prefix, child["name"], note))
        layer_count += 1
        if child["kind"] == "group":
            nested, nested_count, nested_clips = _format_children(child["children"], depth + 1)
            lines.extend(nested)
            layer_count += nested_count
            clip_count += nested_clips
        index += 1
    return lines, layer_count, clip_count


def _groups_top_to_bottom(records):
    """Build Krita's child list. The first file record is the bottom layer."""
    root = {"children": []}
    stack = [root]
    for record in records:
        divider = record["divider"]
        if divider == 3:
            group = _node(record, "group")
            group["name"] = ""
            stack[-1]["children"].append(group)
            stack.append(group)
            continue
        if divider in (1, 2):
            if len(stack) == 1:
                continue
            group = stack.pop()
            group["name"] = record["name"] or "Group"
            group["clip"] = record["clip"]
            group["blend"] = record["section_blend"] or record["blend"]
            group["shapes"] = record.get("shapes", True)
            group["visible"] = record["visible"]
            group["opacity"] = record["opacity"]
            continue
        stack[-1]["children"].append(_node(record, "layer"))
    return root["children"]


def _node(record, kind):
    name = record["name"] or ("Group" if kind == "group" else "Layer")
    raster = record.get("raster") if kind == "layer" else None
    bounds = None
    if raster:
        bounds = (raster["x"], raster["y"], raster["w"], raster["h"])
    return {
        "name": name,
        "kind": kind,
        "clip": bool(record["clip"]),
        "blend": record["blend"],
        "shapes": record.get("shapes", True),
        "visible": bool(record["visible"]),
        "opacity": record["opacity"],
        "bounds": bounds,
        "raster": raster,
        "children": [],
    }


def _read_record(data, pos, channel_len_size, limit):
    if pos + 18 > limit:
        raise PsdError("layer record truncated at %d" % pos)
    top, left, bottom, right = struct.unpack_from(">iiii", data, pos)
    pos += 16
    channels = struct.unpack_from(">H", data, pos)[0]
    pos += 2
    channel_info = []
    entry_size = 2 + channel_len_size
    if pos + channels * entry_size > limit:
        raise PsdError("channel table truncated at %d" % pos)
    for _ in range(channels):
        channel_id = struct.unpack_from(">h", data, pos)[0]
        if channel_len_size == 8:
            length = struct.unpack_from(">Q", data, pos + 2)[0]
        else:
            length = struct.unpack_from(">I", data, pos + 2)[0]
        channel_info.append((channel_id, length))
        pos += entry_size
    if pos + 16 > limit:
        raise PsdError("layer header truncated at %d" % pos)
    if data[pos:pos + 4] not in _SIGS:
        raise PsdError("bad blend signature at %d" % pos)
    blend = data[pos + 4:pos + 8].decode("latin1")
    opacity = data[pos + 8]
    clip = data[pos + 9] != 0
    flags = data[pos + 10]
    pos += 16  # signature, blend, opacity, clip, flags, filler, then extra length follows
    # The 16 bytes above were 4+4+1+1+1+1 = 12, plus we still need the extra length.
    # Recalculate from the blend signature to avoid an off-by-four.
    pos -= 4
    extra_len = struct.unpack_from(">I", data, pos)[0]
    pos += 4
    extra_end = pos + extra_len
    if extra_end > len(data):
        raise PsdError("layer extra data overruns the file at %d" % pos)
    name = ""
    divider = 0
    section_blend = ""
    shapes = True
    if extra_len >= 4:
        mask_size = struct.unpack_from(">I", data, pos)[0]
        cursor = pos + 4 + mask_size
        if cursor + 4 <= extra_end:
            ranges_size = struct.unpack_from(">I", data, cursor)[0]
            cursor += 4 + ranges_size
            if cursor < extra_end:
                name_len = data[cursor]
                cursor += 1
                raw_name = data[cursor:cursor + name_len]
                cursor += name_len
                cursor += (4 - ((1 + name_len) % 4)) % 4
                try:
                    name = raw_name.decode("utf-8")
                except UnicodeDecodeError:
                    name = raw_name.decode("latin1", errors="replace")
                luni, divider, section_blend, shapes = _read_tags(data, cursor, extra_end)
                if luni:
                    name = luni
    return {
        "name": name.replace("\x00", ""),
        "clip": clip,
        "blend": blend,
        "section_blend": section_blend,
        "shapes": shapes,
        "visible": (flags & 2) == 0,
        "opacity": opacity,
        "divider": divider,
        "rect": (top, left, bottom, right),
        "channels": channel_info,
    }, extra_end


def _read_tags(data, pos, extra_end):
    luni = ""
    divider = 0
    section_blend = ""
    # Photoshop "transparency shapes layer". Clip Studio writes 0 for Add (Glow)
    # and Glow Dodge, which stay stronger in soft edges than Add and Color Dodge.
    shapes = True
    while pos + 12 <= extra_end:
        if data[pos:pos + 4] not in _SIGS:
            break
        key = data[pos + 4:pos + 8]
        length = struct.unpack_from(">I", data, pos + 8)[0]
        pos += 12
        if length < 0 or pos + length > extra_end:
            break
        payload = data[pos:pos + length]
        pos += length
        if key == b"luni" and len(payload) >= 4:
            chars = struct.unpack_from(">I", payload, 0)[0]
            luni = payload[4:4 + chars * 2].decode("utf-16-be", errors="replace")
        elif key in (b"lsct", b"lsdk") and len(payload) >= 4:
            divider = struct.unpack_from(">I", payload, 0)[0]
            if len(payload) >= 12 and payload[4:8] == b"8BIM":
                section_blend = payload[8:12].decode("latin1")
        elif key == b"tsly" and len(payload) >= 4:
            shapes = struct.unpack_from(">I", payload, 0)[0] != 0
    return luni.replace("\x00", ""), divider, section_blend, shapes


def _skip_section(data, pos, size):
    length, pos = _read_len(data, pos, size)
    return pos + length


def _read_len(data, pos, size):
    if size == 8:
        if pos + 8 > len(data):
            raise PsdError("truncated length at %d" % pos)
        return struct.unpack_from(">Q", data, pos)[0], pos + 8
    if pos + 4 > len(data):
        raise PsdError("truncated length at %d" % pos)
    return struct.unpack_from(">I", data, pos)[0], pos + 4


def _attach_pixels(data, pos, limit, records, row_len_size):
    """Consume every channel blob, then keep A/R/G/B paint on the record."""
    for record in records:
        planes = {}
        for channel_id, length in record.get("channels") or []:
            if length < 0 or pos + length > len(data):
                raise PsdError('channel data for "%s" is truncated' % (record.get("name") or "layer"))
            blob = data[pos:pos + length]
            pos += length
            if channel_id in (-1, 0, 1, 2):
                planes[channel_id] = blob
        record["raster"] = _compose_raster(record, planes, row_len_size)
    if pos > limit:
        raise PsdError("channel data runs past the layer section")


def _compose_raster(record, planes, row_len_size):
    top, left, bottom, right = record.get("rect") or (0, 0, 0, 0)
    width = right - left
    height = bottom - top
    if width <= 0 or height <= 0:
        return None
    missing = [channel_id for channel_id in (-1, 0, 1, 2) if channel_id not in planes]
    if missing:
        raise PsdError('"%s" is missing a color channel' % (record.get("name") or "layer"))
    try:
        red = _decode_plane(planes[0], width, height, row_len_size)
        green = _decode_plane(planes[1], width, height, row_len_size)
        blue = _decode_plane(planes[2], width, height, row_len_size)
        alpha = _decode_plane(planes[-1], width, height, row_len_size)
    except PsdError as error:
        raise PsdError("%s (%s)" % (error, record.get("name") or "layer")) from error
    count = width * height
    rgba = bytearray(count * 4)
    rgba[0::4] = red
    rgba[1::4] = green
    rgba[2::4] = blue
    rgba[3::4] = alpha
    return {"x": left, "y": top, "w": width, "h": height, "rgba": bytes(rgba)}


def _decode_plane(blob, width, height, row_len_size):
    if len(blob) < 2:
        raise PsdError("channel is shorter than a compression word")
    compression = struct.unpack_from(">H", blob, 0)[0]
    payload = blob[2:]
    expected = width * height
    if compression == 0:
        if len(payload) < expected:
            raise PsdError("raw channel is %d bytes for %d pixels" % (len(payload), expected))
        return payload[:expected]
    if compression == 1:
        return _decode_packbits(payload, width, height, row_len_size)
    raise PsdError("channel compression %d is not supported" % compression)


def _decode_packbits(payload, width, height, row_len_size):
    sizes_bytes = height * row_len_size
    if len(payload) < sizes_bytes:
        raise PsdError("PackBits row sizes are truncated")
    sizes = []
    pos = 0
    for _ in range(height):
        if row_len_size == 2:
            size = struct.unpack_from(">H", payload, pos)[0]
        else:
            size = struct.unpack_from(">I", payload, pos)[0]
        sizes.append(size)
        pos += row_len_size
    rows = []
    for size in sizes:
        if pos + size > len(payload):
            raise PsdError("PackBits row runs past the channel")
        rows.append(_unpack_packbits_row(payload[pos:pos + size], width))
        pos += size
    return b"".join(rows)


def _unpack_packbits_row(data, width):
    out = bytearray()
    index = 0
    limit = len(data)
    while index < limit and len(out) < width:
        header = data[index]
        index += 1
        if header < 128:
            count = header + 1
            chunk = data[index:index + count]
            if len(chunk) < count:
                raise PsdError("PackBits literal runs past the row")
            index += count
            out += chunk
        elif header > 128:
            if index >= limit:
                raise PsdError("PackBits repeat runs past the row")
            count = 257 - header
            out += bytes((data[index],)) * count
            index += 1
    if len(out) < width:
        raise PsdError("PackBits row decoded to %d bytes, expected %d" % (len(out), width))
    return bytes(out[:width])
