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


class Tool(object):
    """One picture tool. uses_brush shows the size and pressure row."""

    def __init__(self, ident, label, uses_brush=False):
        self.id = ident
        self.label = label
        self.uses_brush = bool(uses_brush)


TOOLS = (
    Tool("warp-target", "Create warp target", False),
    Tool("warp-mask", "Draw warp mask", True),
)


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


def dab(arr, cx, cy, radius, softness, antialias, color, opacity):
    """Stamp one disc into an HxWx4 uint8 array. Returns True when a pixel changes."""
    if arr is None or radius < 0.5 or opacity <= 0:
        return False
    height, width = arr.shape[:2]
    pad = 1.0 if antialias or softness > 0 else 0.0
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
    if softness <= 0 and not antialias:
        cover = (dist <= edge).astype(np.float32)
    elif softness <= 0:
        cover = np.clip(edge + 0.5 - dist, 0.0, 1.0)
    else:
        inner = edge * (1.0 - max(0.0, min(1.0, float(softness))))
        cover = np.clip((edge - dist) / max(edge - inner, 0.001), 0.0, 1.0)
        if not antialias:
            cover = (cover >= 0.5).astype(np.float32)
    cover *= max(0.0, min(1.0, float(opacity)))
    if not np.any(cover > 0):
        return False
    tile = arr[y0:y1, x0:x1]
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
        return x0 - self.x, y0 - self.y

    def stamp(self, cx, cy, brush, pressure, color):
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
        hit = dab(
            self.arr, local_x, local_y, radius,
            brush.softness, brush.antialias, (rgb[0], rgb[1], rgb[2], 255), opacity,
        )
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


def fresh_name(nodes, base):
    taken = {((node.name or "").strip().lower()) for node in nodes}
    if base.lower() not in taken:
        return base
    number = 2
    while "%s %d" % (base.lower(), number) in taken:
        number += 1
    return "%s %d" % (base, number)
