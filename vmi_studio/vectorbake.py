"""Bake a display preview for Clip Studio vector layers.

The stroke body comes from the vmi-clip-vector sidecar (clipfile's
read_vector). This module only stamps that geometry. It does not read SVG,
and it does not add edit handles.

Width at a point is brush_radius * width_factor. That product is the round
stamp's diameter, so the stamp radius is half of it. Opacity at a point is
the stroke opacity times opacity_factor.

Anti-alias comes from BrushStyle.AntiAlias, or FillStyle.AntiAlias on a fill
stroke. 0 is None: a hard edge. Any other value is anti-aliased. Weak,
Middle, and Strong are not separate numbers until a labeled None file and a
labeled Strong file have both been logged. Until then an unmapped value uses
the middle fringe. Monochrome expression color forces the hard edge.
"""

import json
import math
import os
import subprocess
import sys

import numpy as np

from vmi_studio.document import Raster

# Raw anti_alias -> fringe in pixels. 0 is None.
# Unmapped non-zero values use UNMAPPED_FRINGE (the middle fringe).
MAPPED_FRINGE = {0: 0.0}
UNMAPPED_FRINGE = 2.0
_LOGGED_ALIAS = set()


def sidecar_path():
    """Built reader, or VMI_CLIP_VECTOR when that file exists."""
    env = os.environ.get("VMI_CLIP_VECTOR")
    if env and os.path.isfile(env):
        return env
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.normpath(os.path.join(here, "..", "tools", "vmi-clip-vector", "target"))
    for name in ("release", "debug"):
        path = os.path.join(root, name, "vmi-clip-vector")
        if os.path.isfile(path):
            return path
    return ""


def apply_vector_previews(art, path):
    """Stamp a preview onto vector layers that imported with no pixels."""
    payload = read_vectors(path)
    if not payload:
        return 0
    width = int(getattr(art, "width", 0) or payload.get("width") or 0)
    height = int(getattr(art, "height", 0) or payload.get("height") or 0)
    notes = []
    count = _attach(art.layers, payload.get("layers") or [], width, height, notes)
    if notes:
        art.note = ("%s %s" % (art.note or "", " ".join(notes))).strip()
    if count:
        _log("%d vector layer%s previewed" % (count, "" if count == 1 else "s"))
    return count


def read_vectors(path):
    """Run the sidecar. A missing binary or a rejected file leaves layers blank."""
    binary = sidecar_path()
    if not binary:
        _log("sidecar is not built; vector layers stay blank")
        return None
    try:
        proc = subprocess.run(
            [binary, path],
            capture_output=True,
            timeout=180,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        _log("sidecar failed: %s" % exc)
        return None
    if proc.stderr:
        sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
    if proc.returncode != 0:
        _log("sidecar exited %s; vector layers stay blank" % proc.returncode)
        return None
    try:
        payload = json.loads(proc.stdout.decode("utf-8"))
    except ValueError as exc:
        _log("sidecar json: %s" % exc)
        return None
    if not isinstance(payload, dict):
        _log("sidecar json was not an object")
        return None
    return payload


def bake_preview(width, height, strokes, monochrome=False):
    """One tight RGBA preview at document resolution.

    Returns (x, y, w, h, rgba bytes, spline_approximated) or None when nothing
    was drawn. A stroke whose body was rejected is not invented.
    """
    width = int(width)
    height = int(height)
    if width <= 0 or height <= 0:
        return None
    samples = []
    approximated = False
    for stroke in strokes or []:
        if not isinstance(stroke, dict):
            continue
        if stroke.get("error"):
            _log("stroke skipped: %s" % stroke.get("error"))
            continue
        curve = stroke.get("curve") or ""
        if curve == "spline":
            approximated = True
        for sample in _samples(stroke, monochrome):
            samples.append(sample)
    if not samples:
        return None
    bounds = _bounds(samples, width, height)
    if bounds is None:
        return None
    x0, y0, x1, y1 = bounds
    image = np.zeros((y1 - y0, x1 - x0, 4), np.uint8)
    for sample in samples:
        _stamp(image, sample[0] - x0, sample[1] - y0, sample)
    alpha = image[:, :, 3]
    if int(alpha.max()) == 0:
        return None
    rows = np.where(alpha.any(axis=1))[0]
    cols = np.where(alpha.any(axis=0))[0]
    top, bottom = int(rows[0]), int(rows[-1]) + 1
    left, right = int(cols[0]), int(cols[-1]) + 1
    image = np.ascontiguousarray(image[top:bottom, left:right])
    return (
        x0 + left,
        y0 + top,
        int(image.shape[1]),
        int(image.shape[0]),
        image.tobytes(),
        approximated,
    )


def fringe_px(anti_alias, monochrome):
    """Hard edge for None and for monochrome. Otherwise the mapped fringe."""
    if monochrome:
        return 0.0
    if anti_alias is None:
        _log_alias(None)
        return UNMAPPED_FRINGE
    try:
        value = int(anti_alias)
    except (TypeError, ValueError):
        _log_alias(anti_alias)
        return UNMAPPED_FRINGE
    _log_alias(value)
    if value in MAPPED_FRINGE:
        return float(MAPPED_FRINGE[value])
    return UNMAPPED_FRINGE


def _attach(nodes, rows, width, height, notes):
    count = 0
    for node, row in _pair(nodes, rows):
        count += _attach(node.children, row.get("children") or [], width, height, notes)
        if node.kind != "layer" or node.raster is not None:
            continue
        vector = row.get("vector")
        if not isinstance(vector, dict):
            continue
        if vector.get("error"):
            _log("layer %s (%s) left blank: %s" % (row.get("id"), node.name, vector.get("error")))
            continue
        baked = bake_preview(width, height, vector.get("strokes") or [], bool(row.get("monochrome")))
        if baked is None:
            continue
        x, y, w, h, rgba, approximated = baked
        node.raster = Raster(x, y, w, h, rgba)
        count += 1
        if approximated:
            node.vector_mark = "spline"
            notes.append("Spline preview on %s." % node.name)
            _log("spline approximated on %s" % node.name)
    return count


def _pair(nodes, rows):
    rows = [row for row in rows or [] if isinstance(row, dict)]
    if len(nodes) == len(rows) and all(
        (node.kind == "group") == bool(row.get("folder")) for node, row in zip(nodes, rows)
    ):
        return list(zip(nodes, rows))
    if rows:
        _log("vector tree does not match imported layers; pairing by name")
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
            _log("no imported layer for vector %s %r" % (row.get("id"), name))
            continue
        used.add(found)
        pairs.append((nodes[found], row))
    return pairs


def _samples(stroke, monochrome):
    radius = _finite(stroke.get("brush_radius"))
    if radius is None or radius <= 0:
        _log("stroke has no brush radius")
        return
    opacity = _unit(stroke.get("opacity"))
    if opacity <= 0:
        return
    color = _color(stroke.get("color"))
    fringe = fringe_px(stroke.get("anti_alias"), monochrome)
    hardness = _finite(stroke.get("hardness"))
    if hardness is None:
        hardness = 1.0
    points = [point for point in (stroke.get("points") or []) if isinstance(point, dict)]
    decoded = []
    for point in points:
        x = _finite(point.get("x"))
        y = _finite(point.get("y"))
        width_factor = _finite(point.get("width_factor"))
        opacity_factor = _finite(point.get("opacity_factor"))
        if x is None or y is None or width_factor is None or opacity_factor is None:
            _log("stroke point is missing position or factors")
            return
        controls = []
        for control in point.get("controls") or []:
            if isinstance(control, (list, tuple)) and len(control) >= 2:
                cx = _finite(control[0])
                cy = _finite(control[1])
                if cx is None or cy is None:
                    continue
                controls.append((cx, cy))
        decoded.append((x, y, max(0.0, width_factor), _unit(opacity_factor), controls))
    if not decoded:
        return
    curve = stroke.get("curve") or "straight"
    closed = bool(stroke.get("closed"))
    if curve == "spline":
        yield from _spline(decoded, radius, opacity, color, fringe, hardness, monochrome, closed)
        return
    if len(decoded) == 1:
        yield _dab(decoded[0], radius, opacity, color, fringe, hardness, monochrome)
        return
    count = len(decoded)
    segments = count if closed else count - 1
    for index in range(segments):
        start = decoded[index]
        end = decoded[(index + 1) % count]
        yield from _segment(curve, start, end, radius, opacity, color, fringe, hardness, monochrome)


def _segment(curve, start, end, radius, opacity, color, fringe, hardness, monochrome):
    controls = start[4]
    if curve == "quadratic" and len(controls) >= 1:
        position = lambda t, a=start, b=end, c=controls[0]: _quad(a, c, b, t)
    elif curve == "cubic" and len(controls) >= 2:
        position = lambda t, a=start, b=end, c1=controls[0], c2=controls[1]: _cubic(a, c1, c2, b, t)
    elif curve == "straight" or curve == "quadratic" or curve == "cubic":
        if curve != "straight":
            _log("%s segment has no controls; left undrawn" % curve)
            return
        position = lambda t, a=start, b=end: _line(a, b, t)
    else:
        _log("curve %s is not drawn" % curve)
        return
    yield from _trace(position, radius, opacity, color, fringe, hardness, monochrome)


def _spline(points, radius, opacity, color, fringe, hardness, monochrome, closed):
    count = len(points)
    if count == 1:
        yield _dab(points[0], radius, opacity, color, fringe, hardness, monochrome)
        return
    segments = count if closed else count - 1
    for index in range(segments):
        if closed:
            p0 = points[(index - 1) % count]
            p1 = points[index % count]
            p2 = points[(index + 1) % count]
            p3 = points[(index + 2) % count]
        else:
            p0 = points[index - 1] if index > 0 else points[index]
            p1 = points[index]
            p2 = points[index + 1] if index + 1 < count else points[index]
            p3 = points[index + 2] if index + 2 < count else p2
        if index + 1 >= count and not closed:
            break
        position = lambda t, a=p0, b=p1, c=p2, d=p3: _catmull(a, b, c, d, t)
        yield from _trace(position, radius, opacity, color, fringe, hardness, monochrome)


def _trace(position, radius, opacity, color, fringe, hardness, monochrome):
    rough = 0.0
    previous = position(0.0)
    for step in range(1, 9):
        point = position(step / 8.0)
        rough += math.hypot(point[0] - previous[0], point[1] - previous[1])
        previous = point
    r0 = _stamp_radius(radius, position(0.0)[2])
    r1 = _stamp_radius(radius, position(1.0)[2])
    thin = min(r for r in (r0, r1) if r > 0) if (r0 > 0 or r1 > 0) else 0.45
    pace = max(0.45, thin * 0.3)
    count = max(1, int(math.ceil(rough / pace))) if rough > 0 else 1
    count = min(count, 8000)
    for index in range(count + 1):
        x, y, width_factor, opacity_factor = position(index / count)
        stamp_radius = _stamp_radius(radius, width_factor)
        amount = opacity * opacity_factor
        if stamp_radius <= 0 or amount <= 0:
            continue
        yield (x, y, stamp_radius, amount, color, fringe, hardness, monochrome)


def _dab(point, radius, opacity, color, fringe, hardness, monochrome):
    x, y, width_factor, opacity_factor, _controls = point
    return (
        x,
        y,
        _stamp_radius(radius, width_factor),
        opacity * opacity_factor,
        color,
        fringe,
        hardness,
        monochrome,
    )


def _stamp_radius(brush_radius, width_factor):
    """Diameter is brush_radius * width_factor. The stamp uses the radius."""
    return max(0.0, brush_radius * width_factor * 0.5)


def _line(start, end, t):
    return (
        start[0] + (end[0] - start[0]) * t,
        start[1] + (end[1] - start[1]) * t,
        start[2] + (end[2] - start[2]) * t,
        start[3] + (end[3] - start[3]) * t,
    )


def _quad(start, control, end, t):
    u = 1.0 - t
    x = u * u * start[0] + 2 * u * t * control[0] + t * t * end[0]
    y = u * u * start[1] + 2 * u * t * control[1] + t * t * end[1]
    width_factor = start[2] + (end[2] - start[2]) * t
    opacity_factor = start[3] + (end[3] - start[3]) * t
    return x, y, width_factor, opacity_factor


def _cubic(start, control1, control2, end, t):
    u = 1.0 - t
    x = (
        u * u * u * start[0]
        + 3 * u * u * t * control1[0]
        + 3 * u * t * t * control2[0]
        + t * t * t * end[0]
    )
    y = (
        u * u * u * start[1]
        + 3 * u * u * t * control1[1]
        + 3 * u * t * t * control2[1]
        + t * t * t * end[1]
    )
    width_factor = start[2] + (end[2] - start[2]) * t
    opacity_factor = start[3] + (end[3] - start[3]) * t
    return x, y, width_factor, opacity_factor


def _catmull(p0, p1, p2, p3, t):
    t2 = t * t
    t3 = t2 * t
    def mix(i):
        return 0.5 * (
            (2.0 * p1[i])
            + (-p0[i] + p2[i]) * t
            + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * t2
            + (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]) * t3
        )
    width_factor = p1[2] + (p2[2] - p1[2]) * t
    opacity_factor = p1[3] + (p2[3] - p1[3]) * t
    return mix(0), mix(1), width_factor, opacity_factor


def _bounds(samples, width, height):
    left = width
    top = height
    right = 0
    bottom = 0
    for x, y, radius, _opacity, _color, fringe, hardness, monochrome in samples:
        reach = radius + _soft_reach(fringe, hardness, monochrome) + 1.0
        left = min(left, x - reach)
        right = max(right, x + reach)
        top = min(top, y - reach)
        bottom = max(bottom, y + reach)
    x0 = max(0, int(math.floor(left)))
    y0 = max(0, int(math.floor(top)))
    x1 = min(width, int(math.ceil(right)) + 1)
    y1 = min(height, int(math.ceil(bottom)) + 1)
    if x0 >= x1 or y0 >= y1:
        return None
    if (x1 - x0) * (y1 - y0) > 8192 * 8192:
        _log("vector preview is larger than the canvas cap")
        return None
    return x0, y0, x1, y1


def _soft_reach(fringe, hardness, monochrome):
    if monochrome or fringe <= 0:
        return 0.0
    return fringe * (1.0 - 0.75 * _clamp(hardness, 0.0, 1.0))


def _stamp(image, x, y, sample):
    _sx, _sy, radius, opacity, color, fringe, hardness, monochrome = sample
    if radius <= 0 or opacity <= 0:
        return
    reach = radius + _soft_reach(fringe, hardness, monochrome)
    height, width, _channels = image.shape
    x0 = max(0, int(math.floor(x - reach)))
    x1 = min(width, int(math.ceil(x + reach)) + 1)
    y0 = max(0, int(math.floor(y - reach)))
    y1 = min(height, int(math.ceil(y + reach)) + 1)
    if x0 >= x1 or y0 >= y1:
        return
    yy, xx = np.ogrid[y0:y1, x0:x1]
    dist = np.sqrt((xx - x) ** 2 + (yy - y) ** 2)
    tile = image[y0:y1, x0:x1]
    if monochrome or fringe <= 0:
        on = dist <= radius
        if not np.any(on):
            return
        alpha = int(round(opacity * 255.0))
        tile[on, 0] = color[0]
        tile[on, 1] = color[1]
        tile[on, 2] = color[2]
        tile[on, 3] = alpha
        return
    cover = _cover(dist, radius, fringe, hardness, False)
    sa = cover * float(opacity)
    if not np.any(sa > 0):
        return
    dst = tile.astype(np.float32)
    da = dst[:, :, 3] * (1.0 / 255.0)
    out_a = sa + da * (1.0 - sa)
    keep = out_a > 1e-6
    for channel in range(3):
        mixed = color[channel] * sa + dst[:, :, channel] * da * (1.0 - sa)
        dst[:, :, channel] = np.where(keep, mixed / np.maximum(out_a, 1e-6), dst[:, :, channel])
    dst[:, :, 3] = out_a * 255.0
    image[y0:y1, x0:x1] = np.clip(np.rint(dst), 0, 255).astype(np.uint8)


def _cover(dist, radius, fringe, hardness, monochrome):
    if monochrome or fringe <= 0:
        return (dist <= radius).astype(np.float32)
    reach = _soft_reach(fringe, hardness, monochrome)
    if reach <= 0:
        return (dist <= radius).astype(np.float32)
    outer = radius + reach
    span = np.clip((outer - dist) / reach, 0.0, 1.0)
    return span.astype(np.float32)


def _color(value):
    if isinstance(value, (list, tuple)) and len(value) >= 3:
        try:
            return tuple(max(0, min(255, int(channel))) for channel in value[:3])
        except (TypeError, ValueError):
            pass
    return (0, 0, 0)


def _finite(value):
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(number):
        return None
    return number


def _unit(value):
    number = _finite(value)
    if number is None:
        return 0.0
    return _clamp(number, 0.0, 1.0)


def _clamp(value, low, high):
    return max(low, min(high, value))


def _log_alias(value):
    if value in _LOGGED_ALIAS:
        return
    _LOGGED_ALIAS.add(value)
    if value == 0:
        mapped = "none"
    elif value in MAPPED_FRINGE:
        mapped = "fringe %s" % MAPPED_FRINGE[value]
    else:
        mapped = "anti-aliased, unmapped fringe %s" % UNMAPPED_FRINGE
    _log("anti_alias raw=%s -> %s" % (value, mapped))


def _log(message):
    sys.stderr.write("vector: %s\n" % message)
