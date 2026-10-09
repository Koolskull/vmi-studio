"""Show Clip Studio image materials, warped at their four stored corners.

The sidecar reads ResizableOriginalMipmap and the corner list. This module
maps that unwarped image onto the quad. Corner order is the source image:
top-left, top-right, bottom-left, bottom-right. A layer mask the same size
as the image is multiplied in before the warp, so it moves with the picture.
Clipping masks already on the stack still clip the result. There are no edit
handles. The drawing file is not rewritten.
"""

import json
import math
import os
import shutil
import subprocess
import sys
import tempfile

import numpy as np

from vmi_studio.document import Raster
from vmi_studio.vectorbake import sidecar_path

_CANVAS_CAP = 8192 * 8192


def apply_image_previews(art, path):
    """Attach one warped preview to each image material that imported empty."""
    payload = read_images(path)
    if not payload:
        return 0
    width = int(getattr(art, "width", 0) or payload.get("width") or 0)
    height = int(getattr(art, "height", 0) or payload.get("height") or 0)
    count = _attach(art.layers, payload.get("layers") or [], width, height)
    if count:
        _log("%d image material%s" % (count, "" if count == 1 else "s"))
    return count


def read_images(path):
    """Run the sidecar. A missing binary or a rejected file leaves layers blank."""
    binary = sidecar_path()
    if not binary:
        _log("sidecar is not built; image materials stay blank")
        return None
    directory = tempfile.mkdtemp(prefix="vmi-image-")
    try:
        try:
            proc = subprocess.run(
                [binary, "images", path, directory],
                capture_output=True,
                timeout=300,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            _log("sidecar failed: %s" % exc)
            return None
        if proc.stderr:
            sys.stderr.write(proc.stderr.decode("utf-8", "replace"))
        if proc.returncode != 0:
            _log("sidecar exited %s; image materials stay blank" % proc.returncode)
            return None
        try:
            payload = json.loads(proc.stdout.decode("utf-8"))
        except ValueError as exc:
            _log("sidecar json: %s" % exc)
            return None
        if not isinstance(payload, dict):
            _log("sidecar json was not an object")
            return None
        _load_files(payload.get("layers") or [], directory)
        return payload
    finally:
        shutil.rmtree(directory, ignore_errors=True)


def place_image(canvas_w, canvas_h, image):
    """Warp one RGBA image through its four corners. Returns a Raster or None."""
    if not isinstance(image, dict) or image.get("error"):
        return None
    try:
        sw = int(image.get("width") or 0)
        sh = int(image.get("height") or 0)
        canvas_w = int(canvas_w)
        canvas_h = int(canvas_h)
    except (TypeError, ValueError):
        return None
    rgba = image.get("rgba")
    corners = image.get("corners")
    if sw < 1 or sh < 1 or canvas_w < 1 or canvas_h < 1:
        return None
    if not isinstance(rgba, (bytes, bytearray)) or len(rgba) != sw * sh * 4:
        return None
    points = _points(corners)
    if points is None:
        return None
    rgba = _mask_rgba(bytes(rgba), sw, sh, image.get("mask"))
    copied = _integer_copy(rgba, sw, sh, points, canvas_w, canvas_h)
    if copied is False:
        return None
    if copied is not None:
        return copied
    return _resample(rgba, sw, sh, points, canvas_w, canvas_h)


def source_point(sw, sh, corners, x, y):
    """Canvas point mapped back into the source image, or None."""
    points = _points(corners)
    if points is None or sw < 1 or sh < 1:
        return None
    try:
        matrix = _matrix(sw, sh, points)
    except np.linalg.LinAlgError:
        return None
    return _apply(matrix, float(x), float(y))


def _attach(nodes, rows, width, height):
    count = 0
    for node, row in _pair(nodes, rows):
        count += _attach(node.children, row.get("children") or [], width, height)
        if node.kind != "layer" or node.raster is not None:
            continue
        image = row.get("image")
        if not isinstance(image, dict):
            continue
        if image.get("error"):
            _log("layer %s (%s) left blank: %s" % (row.get("id"), node.name, image.get("error")))
            continue
        placed = place_image(width, height, image)
        if placed is None:
            _log("layer %s (%s) left blank" % (row.get("id"), node.name))
            continue
        node.raster = placed
        count += 1
    return count


def _pair(nodes, rows):
    rows = [row for row in rows or [] if isinstance(row, dict)]
    if len(nodes) == len(rows) and all(
        (node.kind == "group") == bool(row.get("folder")) for node, row in zip(nodes, rows)
    ):
        return list(zip(nodes, rows))
    if rows:
        _log("image tree does not match imported layers; pairing by name")
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
            _log("no imported layer for image %s %r" % (row.get("id"), name))
            continue
        used.add(found)
        pairs.append((nodes[found], row))
    return pairs


def _load_files(rows, directory):
    for row in rows or []:
        if not isinstance(row, dict):
            continue
        _load_files(row.get("children") or [], directory)
        image = row.get("image")
        if not isinstance(image, dict) or image.get("error"):
            continue
        name = image.get("file")
        if not isinstance(name, str) or not name or os.path.basename(name) != name:
            image["error"] = "missing pixels"
            continue
        try:
            with open(os.path.join(directory, name), "rb") as handle:
                image["rgba"] = handle.read()
        except OSError:
            image["error"] = "missing pixels"
            continue
        mask = image.get("mask")
        if not isinstance(mask, dict):
            continue
        mask_name = mask.get("file")
        if not isinstance(mask_name, str) or os.path.basename(mask_name) != mask_name:
            image["mask"] = None
            continue
        try:
            with open(os.path.join(directory, mask_name), "rb") as handle:
                mask["gray"] = handle.read()
        except OSError:
            image["mask"] = None


def _points(corners):
    if not isinstance(corners, (list, tuple)) or len(corners) != 4:
        return None
    points = []
    for point in corners:
        if not isinstance(point, (list, tuple)) or len(point) < 2:
            return None
        try:
            x = float(point[0])
            y = float(point[1])
        except (TypeError, ValueError):
            return None
        if not math.isfinite(x) or not math.isfinite(y):
            return None
        points.append((x, y))
    return points


def _mask_rgba(rgba, sw, sh, mask):
    if not isinstance(mask, dict):
        return rgba
    try:
        mw = int(mask.get("width") or 0)
        mh = int(mask.get("height") or 0)
    except (TypeError, ValueError):
        return rgba
    gray = mask.get("gray")
    if mw != sw or mh != sh or not isinstance(gray, (bytes, bytearray)) or len(gray) != mw * mh:
        _log("mask is %sx%s and the image is %sx%s; the image stays unmasked" % (mw, mh, sw, sh))
        return rgba
    image = np.frombuffer(rgba, dtype=np.uint8).reshape(sh, sw, 4).copy()
    cover = np.frombuffer(gray, dtype=np.uint8).reshape(sh, sw)
    alpha = image[:, :, 3].astype(np.uint16) * cover.astype(np.uint16)
    image[:, :, 3] = ((alpha + 127) // 255).astype(np.uint8)
    return image.tobytes()


def _integer_copy(rgba, sw, sh, points, canvas_w, canvas_h):
    """Exact copy when the quad is the source rectangle, flipped or not.

    Returns a Raster, False when that rectangle misses the canvas, or None
    when the quad needs the sampler.
    """
    tl, tr, bl, br = points
    if any(abs(value - round(value)) > 1e-6 for point in points for value in point):
        return None
    tl = (int(round(tl[0])), int(round(tl[1])))
    tr = (int(round(tr[0])), int(round(tr[1])))
    bl = (int(round(bl[0])), int(round(bl[1])))
    br = (int(round(br[0])), int(round(br[1])))
    if tr[1] != tl[1] or bl[0] != tl[0] or br != (tr[0], bl[1]):
        return None
    span_x = tr[0] - tl[0]
    span_y = bl[1] - tl[1]
    if abs(span_x) != sw or abs(span_y) != sh or span_x == 0 or span_y == 0:
        return None
    image = np.frombuffer(rgba, dtype=np.uint8).reshape(sh, sw, 4)
    if span_x < 0:
        image = np.flip(image, axis=1)
    if span_y < 0:
        image = np.flip(image, axis=0)
    origin_x = min(tl[0], tr[0])
    origin_y = min(tl[1], bl[1])
    src_x = max(0, -origin_x)
    src_y = max(0, -origin_y)
    dst_x = max(0, origin_x)
    dst_y = max(0, origin_y)
    copy_w = min(sw - src_x, canvas_w - dst_x)
    copy_h = min(sh - src_y, canvas_h - dst_y)
    if copy_w <= 0 or copy_h <= 0:
        return False
    crop = np.ascontiguousarray(image[src_y:src_y + copy_h, src_x:src_x + copy_w])
    return Raster(dst_x, dst_y, int(copy_w), int(copy_h), crop.tobytes())


def _resample(rgba, sw, sh, points, canvas_w, canvas_h):
    try:
        matrix = _matrix(sw, sh, points)
    except np.linalg.LinAlgError:
        _log("corners do not form a warp")
        return None
    xs = [point[0] for point in points]
    ys = [point[1] for point in points]
    x0 = max(0, math.floor(min(xs)))
    y0 = max(0, math.floor(min(ys)))
    x1 = min(canvas_w, math.ceil(max(xs)))
    y1 = min(canvas_h, math.ceil(max(ys)))
    if x1 <= x0 or y1 <= y0:
        return None
    width = x1 - x0
    height = y1 - y0
    if width * height > _CANVAS_CAP:
        _log("warped image is too large")
        return None
    src = np.frombuffer(rgba, dtype=np.uint8).reshape(sh, sw, 4)
    out = np.zeros((height, width, 4), dtype=np.uint8)
    columns = np.arange(x0, x1, dtype=np.float64) + 0.5
    covered = False
    for top in range(0, height, 128):
        bottom = min(height, top + 128)
        rows = np.arange(y0 + top, y0 + bottom, dtype=np.float64) + 0.5
        grid_x = columns[None, :]
        grid_y = rows[:, None]
        den = matrix[2, 0] * grid_x + matrix[2, 1] * grid_y + matrix[2, 2]
        with np.errstate(divide="ignore", invalid="ignore"):
            u = (matrix[0, 0] * grid_x + matrix[0, 1] * grid_y + matrix[0, 2]) / den
            v = (matrix[1, 0] * grid_x + matrix[1, 1] * grid_y + matrix[1, 2]) / den
        inside = np.isfinite(u) & np.isfinite(v) & (u >= 0.0) & (v >= 0.0) & (u < sw) & (v < sh)
        if not np.any(inside):
            continue
        covered = True
        su = np.clip(u - 0.5, 0.0, sw - 1)
        sv = np.clip(v - 0.5, 0.0, sh - 1)
        xi = np.floor(su).astype(np.int32)
        yi = np.floor(sv).astype(np.int32)
        xj = np.minimum(xi + 1, sw - 1)
        yj = np.minimum(yi + 1, sh - 1)
        wx = (su - xi).astype(np.float32)[..., None]
        wy = (sv - yi).astype(np.float32)[..., None]
        upper = src[yi, xi].astype(np.float32) * (1.0 - wx) + src[yi, xj].astype(np.float32) * wx
        lower = src[yj, xi].astype(np.float32) * (1.0 - wx) + src[yj, xj].astype(np.float32) * wx
        mixed = upper * (1.0 - wy) + lower * wy
        mixed[~inside] = 0
        out[top:bottom] = np.clip(np.rint(mixed), 0, 255).astype(np.uint8)
    if not covered:
        return None
    return Raster(x0, y0, width, height, out.tobytes())


def _matrix(sw, sh, points):
    """Canvas (x, y) to continuous source (u, v). The source rectangle is the image."""
    source = ((0.0, 0.0), (float(sw), 0.0), (0.0, float(sh)), (float(sw), float(sh)))
    return _homography(points, source)


def _homography(src, dst):
    rows = []
    values = []
    for (x, y), (u, v) in zip(src, dst):
        rows.append((x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y))
        rows.append((0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y))
        values.append(u)
        values.append(v)
    solved = np.linalg.solve(np.asarray(rows, dtype=np.float64), np.asarray(values, dtype=np.float64))
    return np.array((
        (solved[0], solved[1], solved[2]),
        (solved[3], solved[4], solved[5]),
        (solved[6], solved[7], 1.0),
    ), dtype=np.float64)


def _apply(matrix, x, y):
    den = matrix[2, 0] * x + matrix[2, 1] * y + matrix[2, 2]
    if den == 0 or not math.isfinite(float(den)):
        return None
    u = (matrix[0, 0] * x + matrix[0, 1] * y + matrix[0, 2]) / den
    v = (matrix[1, 0] * x + matrix[1, 1] * y + matrix[1, 2]) / den
    if not math.isfinite(float(u)) or not math.isfinite(float(v)):
        return None
    return (float(u), float(v))


def _log(message):
    sys.stderr.write("image: %s\n" % message)
