"""Clip Studio's border edge, the outline around non-transparent pixels.

The .clip file stores it as EffectEdge: on or off, a width in pixels, and a
color. The paint stays as it was. The outline is drawn underneath, out to
that width, including the inner edge of a hole. The same module runs on
macOS, Windows, and Linux.
"""

import io
import math
import sqlite3
import struct
import tempfile

import numpy as np

from vmi_studio.csptime import _sqlite_bytes
from vmi_studio.document import Raster, walk


MAX_PX = 250.0
_FAR = 1.0e20


def clamp_px(value):
    try:
        number = float(value)
    except (TypeError, ValueError):
        return 0.0
    if not math.isfinite(number) or number <= 0:
        return 0.0
    return min(MAX_PX, number)


def clamp_rgb(value):
    if not isinstance(value, (list, tuple)) or len(value) < 3:
        return (0, 0, 0)
    channels = []
    for channel in list(value)[:3]:
        try:
            number = int(round(float(channel)))
        except (TypeError, ValueError):
            number = 0
        channels.append(max(0, min(255, number)))
    return tuple(channels)


def active_outline(node):
    """(pixels, rgb) when this layer's outline should draw. Otherwise None."""
    if node is None or not getattr(node, "outline", False):
        return None
    px = clamp_px(getattr(node, "outline_px", 0))
    if px <= 0:
        return None
    return px, clamp_rgb(getattr(node, "outline_rgb", (0, 0, 0)))


def outline_key(node):
    """A cache token. A width or color change has to miss the old plate."""
    px = clamp_px(getattr(node, "outline_px", 0)) if node is not None else 0.0
    rgb = clamp_rgb(getattr(node, "outline_rgb", (0, 0, 0))) if node is not None else (0, 0, 0)
    return (bool(getattr(node, "outline", False)), round(px, 4), rgb)


def parse_effect_edge(blob):
    """EffectEdge from a LayerEffectInfo blob, or None when it is absent.

    Enabled is the first integer after the name. Width is the double after
    that. Red, green, and blue follow, each with the color in the top byte.
    """
    if not blob:
        return None
    data = bytes(blob)
    name = "EffectEdge"
    needle = len(name).to_bytes(4, "big") + name.encode("utf-16-be")
    at = data.find(needle)
    if at < 0:
        return None
    handle = io.BytesIO(data[at + len(needle):])
    raw_on = handle.read(4)
    raw_px = handle.read(8)
    if len(raw_on) < 4 or len(raw_px) < 8:
        return None
    enabled = int.from_bytes(raw_on, "big") != 0
    px = struct.unpack(">d", raw_px)[0]
    rgb = []
    for _channel in range(3):
        raw = handle.read(4)
        if len(raw) < 4:
            return None
        rgb.append(int.from_bytes(raw, "big") >> 24)
    return {"on": enabled, "px": clamp_px(px) if math.isfinite(px) and px > 0 else 0.0, "rgb": tuple(rgb)}


def read_clip_outlines(path):
    """Layer tree of outline settings, in the same order the studio stacks them."""
    blob = _sqlite_bytes(path)
    if not blob:
        return []
    handle = tempfile.NamedTemporaryFile(prefix="vmi-edge-", suffix=".sqlite", delete=False)
    try:
        handle.write(blob)
        handle.close()
        return _outlines_from_sqlite(handle.name)
    except (OSError, sqlite3.Error, ValueError):
        return []
    finally:
        try:
            import os
            os.remove(handle.name)
        except OSError:
            pass


def apply_clip_outlines(art, path):
    """Copy EffectEdge onto the matching layers. Returns how many are on."""
    if art is None or not path:
        return 0
    rows = read_clip_outlines(path)
    if not rows:
        return 0
    return _attach(art.layers, rows)


def outline_rows(nodes):
    return [
        {
            "id": node.id,
            "on": bool(getattr(node, "outline", False)),
            "px": clamp_px(getattr(node, "outline_px", 0)) if getattr(node, "outline_px", 0) else 0.0,
            "rgb": list(clamp_rgb(getattr(node, "outline_rgb", (0, 0, 0)))),
        }
        for node in walk(nodes)
    ]


def apply_outlines(nodes, rows):
    """Session or blueprint choices. A missing id keeps what the file said."""
    if not rows:
        return nodes
    by_id = {node.id: node for node in walk(nodes)}
    for row in rows:
        if not isinstance(row, dict):
            continue
        node = by_id.get(row.get("id"))
        if node is None:
            continue
        if "on" in row:
            node.outline = bool(row.get("on"))
        if "px" in row:
            node.outline_px = clamp_px(row.get("px")) if row.get("px") else 0.0
        if "rgb" in row:
            node.outline_rgb = clamp_rgb(row.get("rgb"))
        if hasattr(node, "_outline_cache"):
            del node._outline_cache
    return nodes


def outlined_raster(raster, node):
    """The layer's paint with its outline, or the paint alone when the outline is off."""
    if raster is None or raster.w < 1 or raster.h < 1 or not raster.rgba:
        return raster
    spec = active_outline(node)
    key = (
        id(raster.rgba),
        int(raster.w),
        int(raster.h),
        int(raster.x),
        int(raster.y),
        None if spec is None else (round(spec[0], 4), spec[1]),
    )
    cached = getattr(node, "_outline_cache", None)
    if cached is not None and cached[0] == key:
        return cached[1]
    made = raster if spec is None else render_outline(raster, spec[0], spec[1])
    try:
        node._outline_cache = (key, made)
    except Exception:
        pass
    return made


def render_outline(raster, px, rgb):
    """A new raster. The outline sits under the original pixels and grows the bounds."""
    px = clamp_px(px)
    rgb = clamp_rgb(rgb)
    if raster is None or px <= 0 or raster.w < 1 or raster.h < 1:
        return raster
    if len(raster.rgba) != raster.w * raster.h * 4:
        return raster
    src = np.frombuffer(bytes(raster.rgba), dtype=np.uint8).reshape(raster.h, raster.w, 4)
    disc = _expand_disc(src[:, :, 3], px, rgb)
    if disc is None:
        return raster
    plate, dx, dy = disc
    pad = -int(dx)
    canvas = np.zeros_like(plate)
    canvas[pad:pad + raster.h, pad:pad + raster.w] = src
    merged = _over(plate, canvas)
    cropped = _crop(merged, dx, dy)
    if cropped is None:
        return raster
    arr, shift_x, shift_y = cropped
    return Raster(
        int(raster.x) + int(shift_x),
        int(raster.y) + int(shift_y),
        int(arr.shape[1]),
        int(arr.shape[0]),
        np.ascontiguousarray(arr).tobytes(),
    )


def outline_image(image, px, rgb):
    """Same outline for a folder plate. Returns (image, dx, dy)."""
    from PIL import Image

    arr = np.asarray(image.convert("RGBA"))
    fake = Raster(0, 0, int(arr.shape[1]), int(arr.shape[0]), np.ascontiguousarray(arr).tobytes())
    made = render_outline(fake, px, rgb)
    if made is fake or made is None:
        return image, 0, 0
    out = Image.frombytes("RGBA", (made.w, made.h), made.rgba)
    return out, int(made.x), int(made.y)


def outline_disc_image(image, px, rgb):
    """The outline alone, including the area under the paint. (image, dx, dy) or None."""
    from PIL import Image

    arr = np.asarray(image.convert("RGBA"))
    disc = _expand_disc(arr[:, :, 3], px, rgb)
    if disc is None:
        return None
    plate, dx, dy = disc
    cropped = _crop(plate, dx, dy)
    if cropped is None:
        return None
    out, shift_x, shift_y = cropped
    return Image.frombytes("RGBA", (int(out.shape[1]), int(out.shape[0])), np.ascontiguousarray(out).tobytes()), int(shift_x), int(shift_y)


def _attach(nodes, rows):
    count = 0
    for node, row in _pair(nodes, rows):
        count += _attach(node.children, row.get("children") or [])
        edge = row.get("edge")
        if not isinstance(edge, dict):
            continue
        node.outline = bool(edge.get("on"))
        node.outline_px = clamp_px(edge.get("px")) if edge.get("px") else 0.0
        node.outline_rgb = clamp_rgb(edge.get("rgb"))
        if hasattr(node, "_outline_cache"):
            del node._outline_cache
        if node.outline and node.outline_px > 0:
            count += 1
    return count


def _pair(nodes, rows):
    rows = [row for row in rows or [] if isinstance(row, dict)]
    if len(nodes) == len(rows) and all(
        (node.kind == "group") == bool(row.get("folder")) for node, row in zip(nodes, rows)
    ):
        return list(zip(nodes, rows))
    used = set()
    pairs = []
    for row in rows:
        folder = bool(row.get("folder"))
        name = row.get("name") or ""
        found = None
        for index, node in enumerate(nodes):
            if index in used:
                continue
            if (node.kind == "group") != folder:
                continue
            if node.name == name:
                found = index
                break
        if found is None:
            continue
        used.add(found)
        pairs.append((nodes[found], row))
    return pairs


def _outlines_from_sqlite(db_path):
    connection = sqlite3.connect(db_path)
    try:
        columns = {row[1] for row in connection.execute("PRAGMA table_info(Layer)")}
        needed = {"MainId", "LayerName", "LayerFolder", "LayerFirstChildIndex", "LayerNextIndex", "LayerEffectInfo"}
        if not needed.issubset(columns):
            return []
        root = connection.execute("SELECT CanvasRootFolder FROM Canvas").fetchone()
        if not root:
            return []
        layers = {}
        for row in connection.execute(
            "SELECT MainId, LayerName, LayerFolder, LayerFirstChildIndex, "
            "LayerNextIndex, LayerEffectInfo FROM Layer"
        ):
            layers[row[0]] = {
                "name": row[1] or "",
                "folder": bool(row[2]),
                "first": row[3] or 0,
                "next": row[4] or 0,
                "effect": row[5],
            }
    finally:
        connection.close()
    root_layer = layers.get(root[0])
    if root_layer is None:
        return []
    return _node_row(root_layer, layers).get("children") or []


def _node_row(layer, layers):
    children = []
    current = layer.get("first") or 0
    seen = set()
    while current and current in layers and current not in seen:
        seen.add(current)
        child = layers[current]
        children.append(_node_row(child, layers))
        current = child.get("next") or 0
    return {
        "name": layer.get("name") or "",
        "folder": bool(layer.get("folder")),
        "edge": parse_effect_edge(layer.get("effect")),
        "children": children,
    }


def _expand_disc(alpha, px, rgb):
    px = clamp_px(px)
    rgb = clamp_rgb(rgb)
    if px <= 0 or alpha.size == 0 or int(alpha.max()) == 0:
        return None
    pad = int(math.ceil(px))
    height, width = alpha.shape
    sites = np.full((height + 2 * pad, width + 2 * pad), _FAR, np.float64)
    inside = alpha > 0
    if not bool(inside.any()):
        return None
    sites[pad:pad + height, pad:pad + width] = np.where(inside, 0.0, _FAR)
    dist = np.sqrt(np.maximum(_edt_sq(sites), 0.0))
    cover = np.clip(px + 1.0 - dist, 0.0, 1.0)
    disc = np.zeros(sites.shape + (4,), np.uint8)
    disc[:, :, 0] = rgb[0]
    disc[:, :, 1] = rgb[1]
    disc[:, :, 2] = rgb[2]
    disc[:, :, 3] = np.clip(np.rint(cover * 255.0), 0, 255).astype(np.uint8)
    return disc, -pad, -pad


def _over(dst, src):
    """src over dst. Both are straight RGBA uint8 of the same shape."""
    base = dst.astype(np.float64)
    paint = src.astype(np.float64)
    src_a = paint[:, :, 3:4] / 255.0
    dst_a = base[:, :, 3:4] / 255.0
    out_a = src_a + dst_a * (1.0 - src_a)
    premul = paint[:, :, :3] * src_a + base[:, :, :3] * dst_a * (1.0 - src_a)
    rgb = np.where(out_a > 1.0e-6, premul / np.maximum(out_a, 1.0e-6), 0.0)
    out = np.zeros_like(dst)
    out[:, :, :3] = np.clip(np.rint(rgb), 0, 255).astype(np.uint8)
    out[:, :, 3] = np.clip(np.rint(out_a[:, :, 0] * 255.0), 0, 255).astype(np.uint8)
    return out


def _crop(arr, dx, dy):
    alpha = arr[:, :, 3]
    rows = np.flatnonzero(alpha.any(axis=1))
    cols = np.flatnonzero(alpha.any(axis=0))
    if len(rows) == 0 or len(cols) == 0:
        return None
    top, bottom = int(rows[0]), int(rows[-1]) + 1
    left, right = int(cols[0]), int(cols[-1]) + 1
    cropped = np.ascontiguousarray(arr[top:bottom, left:right])
    return cropped, int(dx) + left, int(dy) + top


def _edt_sq(field):
    """Squared Euclidean distance to the nearest 0 in field. Other cells are far."""
    height, width = field.shape
    work = np.empty_like(field)
    for x in range(width):
        work[:, x] = _dt1(field[:, x])
    out = np.empty_like(work)
    for y in range(height):
        out[y, :] = _dt1(work[y, :])
    return out


def _dt1(values):
    """Squared distance transform of one row. The lower hull starts at -infinity, so k stays put."""
    count = int(values.shape[0])
    if count == 0:
        return values
    parabola = np.zeros(count, np.int32)
    border = np.empty(count + 1, np.float64)
    k = 0
    parabola[0] = 0
    border[0] = -np.inf
    border[1] = np.inf
    for q in range(1, count):
        s = _intersect(values, q, int(parabola[k]))
        while s <= border[k]:
            k -= 1
            s = _intersect(values, q, int(parabola[k]))
        k += 1
        parabola[k] = q
        border[k] = s
        border[k + 1] = np.inf
    k = 0
    out = np.empty(count, np.float64)
    for q in range(count):
        while border[k + 1] < q:
            k += 1
        span = q - int(parabola[k])
        out[q] = span * span + values[int(parabola[k])]
    return out


def _intersect(values, q, p):
    return ((values[q] + q * q) - (values[p] + p * p)) / (2.0 * q - 2.0 * p)
