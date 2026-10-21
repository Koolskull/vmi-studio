"""Illustration kernel for warp masks, and the vector fill for a warp target.

A tool is a small object with an id and a label. The warp target and the
warp mask are the first two. Another tool is another object in TOOLS plus
the stroke or click behavior in the window. Brush settings live on Brush
so a later pencil can reuse size, softness, and pressure.
"""

import math

import numpy as np
from PIL import Image, ImageDraw

DEFAULT_WARP_COLOR = "#FF3EB8"

# Raster fringe, same steps as the vector line. 0.5 is the old checked AA button.
AA_STEPS = (0.0, 0.5, 1.0, 2.0)


class Tool(object):
    """One picture tool. uses_brush shows the size and pressure row."""

    def __init__(self, ident, label, uses_brush=False):
        self.id = ident
        self.label = label
        self.uses_brush = bool(uses_brush)


TOOLS = (
    Tool("pen", "Pen", True),
    Tool("paint", "Paint", True),
    Tool("eraser", "Eraser", True),
    Tool("vector", "Vector", True),
    Tool("line", "Line", True),
    Tool("warp-target", "Create warp target", False),
    Tool("warp-mask", "Draw warp mask", True),
)

# Brush tools that paint a layer. The warp tools stay on their own path.
INK_MODES = ("pen", "paint", "eraser", "vector", "line")


def tool_by_id(ident):
    for tool in TOOLS:
        if tool.id == ident:
            return tool
    return None


def parse_hex(text):
    """#RGB or #RRGGBB. Returns #RRGGBB, or None when the text is not a color."""
    raw = (text or "").strip()
    if raw.startswith("#"):
        raw = raw[1:]
    if len(raw) == 3:
        raw = "".join(ch * 2 for ch in raw)
    if len(raw) != 6 or any(ch not in "0123456789abcdefABCDEF" for ch in raw):
        return None
    channels = tuple(int(raw[index:index + 2], 16) for index in (0, 2, 4))
    return "#%02X%02X%02X" % channels


def hex_rgb(text, fallback=DEFAULT_WARP_COLOR):
    color = parse_hex(text) or parse_hex(fallback) or DEFAULT_WARP_COLOR
    return tuple(int(color[index:index + 2], 16) for index in (1, 3, 5))


def aa_pixels(antialias):
    """Fringe in pixels. True keeps the old 0.5 edge. False is a hard disc.

    A bool is not a width. True would otherwise become 1 and move the fringe.
    """
    if isinstance(antialias, bool):
        return 0.5 if antialias else 0.0
    try:
        value = float(antialias)
    except (TypeError, ValueError):
        return 0.0
    if not math.isfinite(value) or value <= 0:
        return 0.0
    return float(value)


def aa_label(antialias):
    pixels = aa_pixels(antialias)
    if pixels <= 0:
        return "AA off"
    for step, label in ((0.5, "AA .5"), (1.0, "AA 1"), (2.0, "AA 2")):
        if abs(pixels - step) < 0.05:
            return label
    return "AA %g" % pixels


def stored_antialias(antialias):
    """A bool stays a bool so an old stroke does not change in the file."""
    if isinstance(antialias, bool):
        return antialias
    pixels = aa_pixels(antialias)
    for step in AA_STEPS:
        if abs(pixels - step) < 0.05:
            return step
    return pixels


def quiet_color(text):
    """The same hue, darkened to sit with the grey names."""
    color = parse_hex(text)
    if color is None:
        return ""
    channels = hex_rgb(color)
    peak = max(channels)
    if peak < 1:
        return "#000000"
    scale = 92.0 / float(peak)
    return "#%02X%02X%02X" % tuple(max(0, min(255, int(round(channel * scale)))) for channel in channels)


def curve_value(curve, t):
    """Piecewise linear. t and the stored values are 0 to 1."""
    amount = max(0.0, min(1.0, float(t)))
    points = list(curve or ((0.0, 0.0), (1.0, 1.0)))
    points.sort(key=lambda item: item[0])
    if amount <= points[0][0]:
        return max(0.0, min(1.0, float(points[0][1])))
    for left, right in zip(points, points[1:]):
        if amount <= right[0]:
            span = float(right[0]) - float(left[0])
            mix = 0.0 if span <= 0 else (amount - float(left[0])) / span
            return max(0.0, min(1.0, float(left[1]) + (float(right[1]) - float(left[1])) * mix))
    return max(0.0, min(1.0, float(points[-1][1])))


class Brush(object):
    """One round brush. Pressure can drive size and opacity through a curve."""

    def __init__(self):
        self.size = 24.0
        self.size_min = 1.0
        self.size_max = 128.0
        self.softness = 0.0
        self.antialias = True
        self.use_size_pressure = False
        self.use_opacity_pressure = False
        self.size_curve = [(0.0, 0.0), (1.0, 1.0)]
        self.opacity_curve = [(0.0, 0.0), (1.0, 1.0)]

    def _limits(self):
        low = max(0.5, float(self.size_min))
        high = max(low, float(self.size_max))
        size = min(max(float(self.size), low), high)
        return low, size

    def diameter_for(self, pressure):
        """Full pressure, and a mouse, use size. Zero pressure uses min when that curve is on."""
        low, size = self._limits()
        if not self.use_size_pressure:
            return size
        mix = curve_value(self.size_curve, pressure)
        return low + (size - low) * mix

    def opacity_for(self, pressure):
        if not self.use_opacity_pressure:
            return 1.0
        return curve_value(self.opacity_curve, pressure)


def dab(arr, cx, cy, radius, softness, antialias, color, opacity, erase=False):
    """Stamp one disc into an HxWx4 uint8 array. Returns True when a pixel changes.

    erase punches alpha and leaves the color. The other path is source-over.
    """
    if arr is None or radius < 0.5 or opacity <= 0:
        return False
    height, width = arr.shape[:2]
    fringe = aa_pixels(antialias)
    pad = max(fringe, 1.0) if fringe > 0 or softness > 0 else 0.0
    reach = float(radius) + pad
    x0 = max(0, int(math.floor(cx - reach)))
    y0 = max(0, int(math.floor(cy - reach)))
    x1 = min(width, int(math.ceil(cx + reach)) + 1)
    y1 = min(height, int(math.ceil(cy + reach)) + 1)
    if x1 <= x0 or y1 <= y0:
        return False
    ys = np.arange(y0, y1, dtype=np.float32) + 0.5
    xs = np.arange(x0, x1, dtype=np.float32) + 0.5
    dist = np.sqrt((xs - float(cx))[None, :] ** 2 + (ys - float(cy))[:, None] ** 2)
    edge = max(float(radius), 0.5)
    if softness <= 0 and fringe <= 0:
        cover = (dist <= edge).astype(np.float32)
    elif softness <= 0:
        cover = np.clip(edge + fringe - dist, 0.0, 1.0)
    else:
        inner = edge * (1.0 - max(0.0, min(1.0, float(softness))))
        cover = np.clip((edge - dist) / max(edge - inner, 0.001), 0.0, 1.0)
        if fringe <= 0:
            cover = (cover >= 0.5).astype(np.float32)
    cover *= max(0.0, min(1.0, float(opacity)))
    if not np.any(cover > 0):
        return False
    tile = arr[y0:y1, x0:x1]
    if erase:
        alpha = tile[:, :, 3].astype(np.float32) * (1.0 - cover)
        tile[:, :, 3] = np.clip(np.rint(alpha), 0, 255).astype(np.uint8)
        return True
    src_a = cover * (float(color[3]) / 255.0 if len(color) > 3 else 1.0)
    dst = tile.astype(np.float32)
    dst_a = dst[:, :, 3] / 255.0
    out_a = src_a + dst_a * (1.0 - src_a)
    for channel, value in enumerate(color[:3]):
        dst[:, :, channel] = np.where(
            out_a > 0,
            (float(value) * src_a + dst[:, :, channel] * dst_a * (1.0 - src_a)) / np.maximum(out_a, 1e-6),
            dst[:, :, channel],
        )
    dst[:, :, 3] = out_a * 255.0
    tile[:] = np.clip(np.rint(dst), 0, 255).astype(np.uint8)
    return True


def stroke_steps(x0, y0, p0, x1, y1, p1, spacing):
    """Points along a move, spaced so a round brush reads as a line."""
    distance = math.hypot(float(x1) - float(x0), float(y1) - float(y0))
    step = max(0.5, float(spacing))
    if distance < step:
        return []
    count = int(distance / step)
    points = []
    for index in range(1, count + 1):
        mix = min(1.0, (index * step) / distance)
        points.append((
            float(x0) + (float(x1) - float(x0)) * mix,
            float(y0) + (float(y1) - float(y0)) * mix,
            float(p0) + (float(p1) - float(p0)) * mix,
        ))
    return points


class MaskCanvas(object):
    """The painted warp mask. It grows to the dabs and stays inside the document."""

    def __init__(self, width, height, raster=None):
        self.doc_w = max(1, int(width))
        self.doc_h = max(1, int(height))
        self.changed = False
        if raster is not None and raster.w > 0 and raster.h > 0 and len(raster.rgba) == raster.w * raster.h * 4:
            self.arr = np.frombuffer(bytes(raster.rgba), dtype=np.uint8).reshape(raster.h, raster.w, 4).copy()
            self.x = int(raster.x)
            self.y = int(raster.y)
        else:
            self.arr = None
            self.x = 0
            self.y = 0
        # Bumped when the buffer moves, so a live preview can rebuild that once.
        self.generation = 0
        self.dirty = None

    def take_dirty(self):
        """Local pixel box touched since the last take, or None."""
        box = self.dirty
        self.dirty = None
        if box is None:
            return None
        return tuple(int(value) for value in box)

    def _touch(self, x0, y0, x1, y1):
        if self.arr is None:
            return
        height, width = self.arr.shape[:2]
        x0 = max(0, int(math.floor(x0)))
        y0 = max(0, int(math.floor(y0)))
        x1 = min(width, int(math.ceil(x1)))
        y1 = min(height, int(math.ceil(y1)))
        if x1 <= x0 or y1 <= y0:
            return
        if self.dirty is None:
            self.dirty = [x0, y0, x1, y1]
            return
        box = self.dirty
        box[0] = min(box[0], x0)
        box[1] = min(box[1], y0)
        box[2] = max(box[2], x1)
        box[3] = max(box[3], y1)

    def _ensure(self, x0, y0, x1, y1):
        x0 = max(0, int(x0))
        y0 = max(0, int(y0))
        x1 = min(self.doc_w, int(x1))
        y1 = min(self.doc_h, int(y1))
        if x1 <= x0 or y1 <= y0:
            return None
        if self.arr is None:
            self.arr = np.zeros((y1 - y0, x1 - x0, 4), dtype=np.uint8)
            self.x = x0
            self.y = y0
            self.generation += 1
            self.dirty = [0, 0, self.arr.shape[1], self.arr.shape[0]]
            return 0, 0
        left = min(self.x, x0)
        top = min(self.y, y0)
        right = max(self.x + self.arr.shape[1], x1)
        bottom = max(self.y + self.arr.shape[0], y1)
        if left == self.x and top == self.y and right == self.x + self.arr.shape[1] and bottom == self.y + self.arr.shape[0]:
            return x0 - self.x, y0 - self.y
        grown = np.zeros((bottom - top, right - left, 4), dtype=np.uint8)
        oy = self.y - top
        ox = self.x - left
        grown[oy:oy + self.arr.shape[0], ox:ox + self.arr.shape[1]] = self.arr
        self.arr = grown
        self.x = left
        self.y = top
        self.generation += 1
        self.dirty = [0, 0, self.arr.shape[1], self.arr.shape[0]]
        return x0 - self.x, y0 - self.y

    def stamp(self, cx, cy, brush, pressure, color, erase=False, softness=None):
        diameter = brush.diameter_for(pressure)
        radius = diameter / 2.0
        if radius < 0.5:
            return False
        pad = 2
        origin = self._ensure(cx - radius - pad, cy - radius - pad, cx + radius + pad + 1, cy + radius + pad + 1)
        if origin is None or self.arr is None:
            return False
        local_x = float(cx) - self.x
        local_y = float(cy) - self.y
        opacity = brush.opacity_for(pressure)
        rgb = hex_rgb(color)
        soft = brush.softness if softness is None else float(softness)
        hit = dab(
            self.arr, local_x, local_y, radius,
            soft, brush.antialias, (rgb[0], rgb[1], rgb[2], 255), opacity, erase=erase,
        )
        if hit:
            self._touch(local_x - radius - 2, local_y - radius - 2, local_x + radius + 3, local_y + radius + 3)
        self.changed = self.changed or hit
        return hit

    def raster(self):
        from vmi_studio.document import Raster

        if self.arr is None or not np.any(self.arr[:, :, 3] > 0):
            return None
        alpha = self.arr[:, :, 3]
        ys, xs = np.nonzero(alpha)
        top, bottom = int(ys.min()), int(ys.max()) + 1
        left, right = int(xs.min()), int(xs.max()) + 1
        cropped = np.ascontiguousarray(self.arr[top:bottom, left:right])
        return Raster(self.x + left, self.y + top, right - left, bottom - top, cropped.tobytes())


def fill_polygon(points, color, width, height):
    """Rasterize one polygon in document pixels. The vectors stay the source."""
    from vmi_studio.document import Raster

    if len(points) < 3 or width < 1 or height < 1:
        return None
    xs = [float(point[0]) for point in points]
    ys = [float(point[1]) for point in points]
    left = max(0, int(math.floor(min(xs))) - 1)
    top = max(0, int(math.floor(min(ys))) - 1)
    right = min(int(width), int(math.ceil(max(xs))) + 2)
    bottom = min(int(height), int(math.ceil(max(ys))) + 2)
    if right <= left or bottom <= top:
        return None
    image = Image.new("RGBA", (right - left, bottom - top), (0, 0, 0, 0))
    rgb = hex_rgb(color)
    local = [(float(point[0]) - left, float(point[1]) - top) for point in points]
    ImageDraw.Draw(image).polygon(local, fill=(rgb[0], rgb[1], rgb[2], 255))
    arr = np.asarray(image)
    if not np.any(arr[:, :, 3] > 0):
        return None
    return Raster(left, top, image.size[0], image.size[1], image.tobytes())


def recolor_raster(raster, color):
    """A new raster in this color. Alpha and the origin stay."""
    from vmi_studio.document import Raster

    if raster is None or raster.w < 1 or raster.h < 1 or not raster.rgba:
        return raster
    if len(raster.rgba) < raster.w * raster.h * 4:
        return raster
    rgb = hex_rgb(color)
    arr = np.frombuffer(bytes(raster.rgba), dtype=np.uint8).reshape(raster.h, raster.w, 4).copy()
    mask = arr[:, :, 3] > 0
    arr[mask, 0] = rgb[0]
    arr[mask, 1] = rgb[1]
    arr[mask, 2] = rgb[2]
    return Raster(raster.x, raster.y, raster.w, raster.h, np.ascontiguousarray(arr).tobytes())


def ink_spacing(diameter, mode):
    """Gap between dabs. A wide brush steps in brush-widths, so a swipe stays cheap."""
    width = max(1.0, float(diameter))
    if mode == "paint":
        return max(1.0, width * 0.18)
    if mode == "vector":
        return max(1.0, width * 0.22)
    return max(0.75, width * 0.15)


def paint_softness(mode, softness):
    """Paint is a soft brush even when the knob is still at zero."""
    value = max(0.0, min(1.0, float(softness)))
    if mode == "paint" and value < 0.35:
        return 0.55
    return value


def sample_stroke(points, spacing_at):
    """Dense samples along pointer points. spacing_at(pressure) is the gap."""
    if not points:
        return []
    sampled = [(float(points[0][0]), float(points[0][1]), float(points[0][2]))]
    for nxt in points[1:]:
        x0, y0, p0 = sampled[-1]
        x1, y1, p1 = float(nxt[0]), float(nxt[1]), float(nxt[2])
        gap = spacing_at((p0 + p1) * 0.5)
        for x, y, pressure in stroke_steps(x0, y0, p0, x1, y1, p1, gap):
            sampled.append((x, y, pressure))
        if abs(sampled[-1][0] - x1) > 0.01 or abs(sampled[-1][1] - y1) > 0.01:
            sampled.append((x1, y1, p1))
    return sampled


def stroke_row(color, points, brush, softness):
    """One vector spine. The baked pixels live on the layer raster."""
    return {
        "color": parse_hex(color) or "#FFFFFF",
        "size": float(brush.size),
        "softness": float(softness),
        "antialias": stored_antialias(brush.antialias),
        "points": [
            [round(float(point[0]), 2), round(float(point[1]), 2), round(float(point[2]), 4)]
            for point in points
        ],
    }


def clean_strokes(raw):
    """Vector spines from a blueprint. A bad row is dropped. Empty is valid."""
    if not isinstance(raw, list):
        return []
    strokes = []
    for row in raw:
        if not isinstance(row, dict):
            continue
        points = []
        for point in row.get("points") or []:
            if not isinstance(point, (list, tuple)) or len(point) < 2:
                continue
            try:
                x = float(point[0])
                y = float(point[1])
                pressure = float(point[2]) if len(point) > 2 else 1.0
            except (TypeError, ValueError):
                continue
            if not (math.isfinite(x) and math.isfinite(y) and math.isfinite(pressure)):
                continue
            points.append([x, y, max(0.0, min(1.0, pressure))])
        if not points:
            continue
        try:
            size = float(row.get("size") or 1)
        except (TypeError, ValueError):
            size = 1.0
        try:
            softness = float(row.get("softness") or 0)
        except (TypeError, ValueError):
            softness = 0.0
        if not math.isfinite(size) or size <= 0:
            size = 1.0
        if not math.isfinite(softness):
            softness = 0.0
        raw_aa = row.get("antialias", True)
        if isinstance(raw_aa, bool):
            antialias = raw_aa
        elif isinstance(raw_aa, (int, float)):
            antialias = stored_antialias(raw_aa)
        else:
            antialias = True
        strokes.append({
            "color": parse_hex(row.get("color") or "") or "#FFFFFF",
            "size": size,
            "softness": max(0.0, min(1.0, softness)),
            "antialias": antialias,
            "points": points,
        })
    return strokes


def _raster_array(raster):
    if raster is None or raster.w < 1 or raster.h < 1 or not raster.rgba:
        return None
    if len(raster.rgba) != raster.w * raster.h * 4:
        return None
    return np.frombuffer(bytes(raster.rgba), dtype=np.uint8).reshape(raster.h, raster.w, 4)


def _cropped_raster(arr, origin_x, origin_y):
    from vmi_studio.document import Raster

    if arr is None or not np.any(arr[:, :, 3] > 0):
        return None
    alpha = arr[:, :, 3]
    ys, xs = np.nonzero(alpha)
    top, bottom = int(ys.min()), int(ys.max()) + 1
    left, right = int(xs.min()), int(xs.max()) + 1
    cropped = np.ascontiguousarray(arr[top:bottom, left:right])
    return Raster(origin_x + left, origin_y + top, right - left, bottom - top, cropped.tobytes())


def raster_over(base, extra):
    """Source-over extra onto base. Either raster may be missing."""
    if extra is None:
        return base
    if base is None:
        return extra
    base_arr = _raster_array(base)
    extra_arr = _raster_array(extra)
    if extra_arr is None:
        return base
    if base_arr is None:
        return extra
    left = min(int(base.x), int(extra.x))
    top = min(int(base.y), int(extra.y))
    right = max(int(base.x) + base_arr.shape[1], int(extra.x) + extra_arr.shape[1])
    bottom = max(int(base.y) + base_arr.shape[0], int(extra.y) + extra_arr.shape[0])
    canvas = np.zeros((bottom - top, right - left, 4), dtype=np.uint8)
    _copy_at(canvas, base_arr, int(base.x) - left, int(base.y) - top)
    _over_at(canvas, extra_arr, int(extra.x) - left, int(extra.y) - top)
    return _cropped_raster(canvas, left, top)


def _copy_at(dst, src, x, y):
    dst[y:y + src.shape[0], x:x + src.shape[1]] = src


def _over_at(dst, src, x, y):
    tile = dst[y:y + src.shape[0], x:x + src.shape[1]]
    src_f = src.astype(np.float32)
    dst_f = tile.astype(np.float32)
    src_a = src_f[:, :, 3] / 255.0
    dst_a = dst_f[:, :, 3] / 255.0
    out_a = src_a + dst_a * (1.0 - src_a)
    for channel in range(3):
        dst_f[:, :, channel] = np.where(
            out_a > 0,
            (src_f[:, :, channel] * src_a + dst_f[:, :, channel] * dst_a * (1.0 - src_a)) / np.maximum(out_a, 1e-6),
            dst_f[:, :, channel],
        )
    dst_f[:, :, 3] = out_a * 255.0
    tile[:] = np.clip(np.rint(dst_f), 0, 255).astype(np.uint8)


def fresh_name(nodes, base):
    taken = {((node.name or "").strip().lower()) for node in nodes}
    if base.lower() not in taken:
        return base
    number = 2
    while "%s %d" % (base.lower(), number) in taken:
        number += 1
    return "%s %d" % (base, number)
