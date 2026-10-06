"""Read a Krita .kra. RGBA tiles are planar blue, green, red, then alpha."""

import os
import subprocess
import zipfile
import xml.etree.ElementTree as ET
from concurrent.futures import ThreadPoolExecutor

from vmi_studio.document import ArtFile, Ids, Node, Raster, refresh


def load(path, only=None, progress=None):
    """Open a .kra. `only` decodes just the layer with that name."""
    path = os.path.abspath(path)
    with zipfile.ZipFile(path) as archive:
        try:
            xml = archive.read("maindoc.xml")
        except KeyError as exc:
            raise RuntimeError("This Krita file has no layer list.") from exc
        image = _image(xml)
        if image is None:
            raise RuntimeError("This Krita file has no canvas.")
        width = int(image.attrib.get("width") or 0)
        height = int(image.attrib.get("height") or 0)
        image_name = image.attrib.get("name") or "image"
        ids = Ids()
        layers = _read_box(_layers_box(image), ids)
        names = set(archive.namelist())
        payloads = {}
        targets = [node for node in _paint(layers) if node.source_key and (only is None or node.name == only)]
        for node in targets:
            key = "%s/layers/%s" % (image_name, node.source_key)
            if key not in names:
                suffix = "/" + node.source_key
                key = next((name for name in names if name.endswith(suffix) and "." not in name.rsplit("/", 1)[-1]), "")
            blob = archive.read(key) if key in names else b""
            default = archive.read(key + ".defaultpixel") if key and (key + ".defaultpixel") in names else b""
            if not blob and not default:
                continue
            payloads[node.id] = (blob, default)
    if progress:
        progress("Reading %d layers" % len(payloads))
    _decode_all(payloads, targets, width, height)
    missing = [node.name for node in targets if node.source_key and node.raster is None and node.id in payloads]
    note = "Every layer's paint is loaded."
    if only:
        note = "Opened %s." % only
    elif missing:
        note = "The stack is loaded. Some layers did not include a picture this reader knows."
    art = ArtFile(os.path.basename(path), path, "kra", width, height, refresh(layers), note)
    return art


def _decode_all(payloads, nodes, width, height):
    by_id = {node.id: node for node in nodes}

    def one(item):
        ident, packed = item
        blob, default = packed
        try:
            return ident, decode_paint(blob, width, height, default)
        except Exception:
            return ident, None

    workers = min(4, max(1, len(payloads)))
    if not payloads:
        return
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for ident, raster in pool.map(one, payloads.items()):
            node = by_id.get(ident)
            if node is not None:
                node.raster = raster


def decode_paint(data, canvas_w, canvas_h, default=b""):
    """One Krita layer file to an RGBA raster. None when the tiles are not this format.

    Empty tiles use the layer's default pixel. Krita stores that pixel in the
    same order as the planes: blue, green, red, alpha.
    """
    if not data.startswith(b"VERSION 2"):
        return _solid(default, canvas_w, canvas_h)
    data_at = data.find(b"DATA ")
    if data_at < 0:
        return _solid(default, canvas_w, canvas_h)
    header = data[:data_at].decode("latin1", "replace")
    tile_w = _number_after(header, "TILEWIDTH") or 64
    tile_h = _number_after(header, "TILEHEIGHT") or 64
    pixel_size = _number_after(header, "PIXELSIZE") or 4
    if pixel_size != 4 or tile_w <= 0 or tile_h <= 0:
        return None
    tile_bytes = tile_w * tile_h * pixel_size
    pos = data.find(b"\n", data_at)
    if pos < 0:
        return None
    pos += 1
    tiles = []
    while pos < len(data):
        end = data.find(b"\n", pos)
        if end < 0:
            break
        line = data[pos:end].decode("latin1", "replace").strip()
        parts = line.split(",")
        if len(parts) != 4:
            break
        try:
            x = int(parts[0])
            y = int(parts[1])
            size = int(parts[3])
        except ValueError:
            break
        codec = parts[2]
        start = end + 1
        if size < 0 or start + size > len(data):
            break
        blob = data[start:start + size]
        if codec == "LZF":
            raw = _lzf_tile(blob, tile_bytes)
            if raw is not None:
                tiles.append((x, y, _planar_bgra_to_rgba(raw, tile_w * tile_h)))
        pos = start + size
    if canvas_w <= 0 or canvas_h <= 0:
        return None
    if not tiles:
        return _solid(default, canvas_w, canvas_h)
    min_x, min_y = canvas_w, canvas_h
    max_x, max_y = 0, 0
    for x, y, _rgba in tiles:
        min_x = min(min_x, max(0, x))
        min_y = min(min_y, max(0, y))
        max_x = max(max_x, min(canvas_w, x + tile_w))
        max_y = max(max_y, min(canvas_h, y + tile_h))
    if max_x <= min_x or max_y <= min_y:
        return None
    from PIL import Image

    image = Image.new("RGBA", (max_x - min_x, max_y - min_y), (0, 0, 0, 0))
    for x, y, rgba in tiles:
        if len(rgba) != tile_w * tile_h * 4:
            continue
        tile = Image.frombytes("RGBA", (tile_w, tile_h), rgba)
        left = max(0, -x)
        top = max(0, -y)
        right = min(tile_w, canvas_w - x)
        bottom = min(tile_h, canvas_h - y)
        if right <= left or bottom <= top:
            continue
        if left or top or right != tile_w or bottom != tile_h:
            tile = tile.crop((left, top, right, bottom))
        image.paste(tile, (x + left - min_x, y + top - min_y))
    return Raster(min_x, min_y, image.width, image.height, image.tobytes())


def _solid(default, width, height):
    if not default or len(default) < 4 or width <= 0 or height <= 0 or default[3] == 0:
        return None
    from PIL import Image

    blue, green, red, alpha = default[0], default[1], default[2], default[3]
    image = Image.new("RGBA", (width, height), (red, green, blue, alpha))
    return Raster(0, 0, width, height, image.tobytes())


def _planar_bgra_to_rgba(tile, count):
    if len(tile) < count * 4:
        count = len(tile) // 4
    blue = tile[0:count]
    green = tile[count:count * 2]
    red = tile[count * 2:count * 3]
    alpha = tile[count * 3:count * 4]
    out = bytearray(count * 4)
    out[0::4] = red
    out[1::4] = green
    out[2::4] = blue
    out[3::4] = alpha
    return bytes(out)


def _lzf_tile(blob, tile_bytes):
    if blob and blob[0] == 1:
        decoded = _lzf_decode(blob[1:], tile_bytes)
        if decoded is not None:
            return decoded
    return _lzf_decode(blob, tile_bytes)


_LIB = None
_LIB_TRIED = False


def _lzf_decode(src, size):
    lib = _library()
    if lib is not None:
        import ctypes

        source = (ctypes.c_char * len(src)).from_buffer_copy(src)
        out = (ctypes.c_char * size)()
        got = lib.lzf_decode(source, len(src), out, size)
        if got >= size:
            return bytes(out)
        return None
    return _lzf_decode_py(src, size)


def _library():
    global _LIB, _LIB_TRIED
    if _LIB_TRIED:
        return _LIB
    _LIB_TRIED = True
    import ctypes

    here = os.path.dirname(os.path.abspath(__file__))
    source = os.path.join(here, "lzf_d.c")
    library = os.path.join(here, "lzf_d.so")
    try:
        stale = os.path.isfile(source) and (
            not os.path.isfile(library) or os.path.getmtime(source) > os.path.getmtime(library)
        )
        if stale:
            subprocess.run(
                ["gcc", "-O3", "-shared", "-fPIC", "-o", library, source],
                check=True,
                capture_output=True,
            )
        if not os.path.isfile(library):
            return None
        lib = ctypes.CDLL(library)
        lib.lzf_decode.argtypes = [
            ctypes.POINTER(ctypes.c_char),
            ctypes.c_int,
            ctypes.POINTER(ctypes.c_char),
            ctypes.c_int,
        ]
        lib.lzf_decode.restype = ctypes.c_int
        _LIB = lib
    except (OSError, subprocess.CalledProcessError):
        _LIB = None
    return _LIB


def _lzf_decode_py(src, max_out):
    out = bytearray(max_out)
    ip = 0
    op = 0
    length_src = len(src)
    while ip < length_src and op < max_out:
        bite = src[ip]
        ctrl = bite + 1
        ofs = (bite & 31) << 8
        length = bite >> 5
        ip += 1
        if ctrl < 33:
            if ip + ctrl > length_src:
                return None
            take = min(ctrl, max_out - op)
            out[op:op + take] = src[ip:ip + take]
            op += take
            ip += ctrl
        else:
            length -= 1
            if length == 6:
                if ip >= length_src:
                    return None
                length += src[ip]
                ip += 1
            if ip >= length_src:
                return None
            ref = op - ofs - 1 - src[ip]
            ip += 1
            count = length + 3
            if ref < 0:
                return None
            for _ in range(count):
                if op >= max_out:
                    break
                if ref < 0 or ref >= op:
                    return None
                out[op] = out[ref]
                op += 1
                ref += 1
    if op < max_out:
        return None
    return bytes(out)


def _number_after(header, key):
    token = key + " "
    at = header.find(token)
    if at < 0:
        return 0
    start = at + len(token)
    end = start
    while end < len(header) and header[end].isdigit():
        end += 1
    if end == start:
        return 0
    return int(header[start:end])


def _local(tag):
    return tag.rsplit("}", 1)[-1]


def _image(xml):
    root = ET.fromstring(xml)
    if _local(root.tag) == "IMAGE":
        return root
    for element in root.iter():
        if _local(element.tag) == "IMAGE":
            return element
    return None


def _layers_box(element):
    for child in element:
        if _local(child.tag) == "layers":
            return child
    return None


def _read_box(box, ids):
    nodes = []
    if box is None:
        return nodes
    for element in box:
        if _local(element.tag) != "layer":
            continue
        nodetype = element.attrib.get("nodetype", "")
        if "mask" in nodetype:
            continue
        group = nodetype == "grouplayer"
        name = element.attrib.get("name") or ("group" if group else "layer")
        visible = element.attrib.get("visible", "1") != "0"
        opacity = 255
        try:
            opacity = int(float(element.attrib.get("opacity", "255")))
        except ValueError:
            opacity = 255
        blend = element.attrib.get("compositeop") or "normal"
        if element.attrib.get("passthrough") == "1":
            blend = "pass through"
        # Krita inherit alpha is the clipping mask. "1" clips to the layers below.
        inherit = (element.attrib.get("inheritalpha") or "").strip().lower()
        node = Node(
            ids.next(),
            name,
            "group" if group else "layer",
            visible=visible,
            opacity=max(0, min(255, opacity)),
            blend=blend,
            clip=inherit in ("1", "true"),
            source_key="" if group else element.attrib.get("filename", ""),
        )
        if group:
            node.children = _read_box(_layers_box(element), ids)
        nodes.append(node)
    nodes.reverse()
    return nodes


def _paint(nodes):
    out = []
    for node in nodes:
        if node.kind == "group":
            out.extend(_paint(node.children))
        else:
            out.append(node)
    return out
