"""Cut or clear a selection on every layer inside one folder.

The drawing file stays the source. These edits live in the open session
and in Export. Opening the file again brings the original pixels back.
"""

import numpy as np
from PIL import Image, ImageDraw

from vmi_studio.document import Node, Raster, find_node, paint_layers, refresh, _next_id


def rect_points(x0, y0, x1, y1):
    left, right = (float(x0), float(x1)) if x0 <= x1 else (float(x1), float(x0))
    top, bottom = (float(y0), float(y1)) if y0 <= y1 else (float(y1), float(y0))
    return [(left, top), (right, top), (right, bottom), (left, bottom)]


def parent_of(nodes, ident):
    """The folder that holds this node, or None when it is a root or missing."""

    def visit(lst, parent):
        for node in lst:
            if node.id == ident:
                return parent, True
            found, ok = visit(node.children, node)
            if ok:
                return found, True
        return None, False

    found, ok = visit(nodes, None)
    return found if ok else None


def polygon_mask(width, height, points):
    image = Image.new("L", (max(1, int(width)), max(1, int(height))), 0)
    if width <= 0 or height <= 0 or len(points) < 3:
        return image
    ImageDraw.Draw(image).polygon([(float(x), float(y)) for x, y in points], fill=255)
    return image


def _layers_in(folder):
    return [layer for layer in paint_layers([folder]) if layer.kind == "layer" and layer.raster is not None]


def _split_raster(raster, mask):
    """Return (cut raster or None, remaining raster). Opaque pixels inside the mask move."""
    if raster is None or raster.w < 1 or raster.h < 1 or len(raster.rgba) != raster.w * raster.h * 4:
        return None, raster
    mw, mh = mask.size
    x0 = max(0, int(raster.x))
    y0 = max(0, int(raster.y))
    x1 = min(mw, int(raster.x) + int(raster.w))
    y1 = min(mh, int(raster.y) + int(raster.h))
    if x1 <= x0 or y1 <= y0:
        return None, raster
    src = np.frombuffer(raster.rgba, dtype=np.uint8).copy().reshape(raster.h, raster.w, 4)
    sx = x0 - int(raster.x)
    sy = y0 - int(raster.y)
    piece = src[sy:sy + (y1 - y0), sx:sx + (x1 - x0)]
    selected = np.asarray(mask.crop((x0, y0, x1, y1))) > 127
    hit = selected & (piece[..., 3] > 0)
    if not hit.any():
        return None, raster
    cut = piece.copy()
    cut[..., 3] = np.where(selected, piece[..., 3], 0)
    piece[..., 3] = np.where(selected, 0, piece[..., 3])
    src[sy:sy + (y1 - y0), sx:sx + (x1 - x0)] = piece
    remain = Raster(raster.x, raster.y, raster.w, raster.h, src.tobytes())
    tight = _tight(cut, x0, y0)
    return tight, remain


def _tight(rgba, origin_x, origin_y):
    alpha = rgba[..., 3]
    ys, xs = np.nonzero(alpha)
    if xs.size == 0:
        return None
    left, right = int(xs.min()), int(xs.max()) + 1
    top, bottom = int(ys.min()), int(ys.max()) + 1
    cropped = np.ascontiguousarray(rgba[top:bottom, left:right])
    return Raster(origin_x + left, origin_y + top, right - left, bottom - top, cropped.tobytes())


def _copy_common(node, source):
    node.shapes = getattr(source, "shapes", True)
    node.mute = bool(getattr(source, "mute", False))
    node.solo = bool(getattr(source, "solo", False))
    marker = getattr(source, "marker", "") or ""
    node.marker = marker if marker in ("target", "mask") else ""
    node.color = getattr(source, "color", "") or ""
    node.vectors = [
        [(float(x), float(y)) for x, y in poly]
        for poly in (getattr(source, "vectors", None) or [])
    ]
    node.frames = [int(frame) for frame in (getattr(source, "frames", None) or [])]
    return node


def _copy_layer(source, ident, raster):
    node = Node(
        ident, source.name, "layer",
        visible=source.visible, opacity=source.opacity, blend=source.blend,
        clip=bool(source.clip), raster=raster,
    )
    return _copy_common(node, source)


def _copy_group(source, ident, children):
    node = Node(
        ident, source.name, "group",
        visible=source.visible, opacity=source.opacity, blend=source.blend,
        clip=bool(source.clip),
    )
    node.children = children
    return _copy_common(node, source)


class _CutIds:
    def __init__(self, nodes):
        start = _next_id(nodes)
        self.number = int(start[1:]) if start.startswith("n") and start[1:].isdigit() else 1

    def next(self):
        ident = "n%d" % self.number
        self.number += 1
        return ident


def next_cut_name(nodes):
    taken = set()

    def visit(lst):
        for node in lst:
            taken.add((node.name or "").lower())
            visit(node.children)

    visit(nodes)
    if "cut" not in taken:
        return "Cut"
    number = 2
    while ("cut %d" % number) in taken:
        number += 1
    return "Cut %d" % number


def _insert_in_front(nodes, folder_id, made):
    def visit(lst):
        for index, node in enumerate(lst):
            if node.id == folder_id:
                lst.insert(index + 1, made)
                return True
            if visit(node.children):
                return True
        return False

    if not visit(nodes):
        nodes.append(made)


def _draft(node, mask, ids, changed):
    """A copy of this node for the cut, or (None, node) when the mask misses it.

    `changed` remembers the raster from before this cut so a clip with no base
    can be put back instead of sticking to the wrong layer.
    """
    if node.kind == "group":
        children = _draft_list(node.children, mask, ids, changed)
        if not children:
            return None, node
        return _copy_group(node, ids.next(), children), node
    if node.raster is None:
        return None, node
    cut, remain = _split_raster(node.raster, mask)
    if cut is None:
        return None, node
    changed.setdefault(node.id, node.raster)
    node.raster = remain
    return _copy_layer(node, ids.next(), cut), node


def _revert(node, changed):
    if node.id in changed:
        node.raster = changed.pop(node.id)
    for child in node.children:
        _revert(child, changed)


def _draft_list(children, mask, ids, changed):
    rows = [_draft(child, mask, ids, changed) for child in children]
    _drop_clips_without_a_base(children, rows, changed)
    return [copy for copy, _source in rows if copy is not None]


def _drop_clips_without_a_base(children, rows, changed):
    """A clipping mask stays on the layer below it. Without that layer, put the pixels back."""
    index = 0
    count = len(children)
    while index < count:
        if children[index].clip:
            copy, source = rows[index]
            if copy is not None:
                _revert(source, changed)
                rows[index] = (None, source)
            index += 1
            continue
        base = index
        index += 1
        run = []
        while index < count and children[index].clip:
            run.append(index)
            index += 1
        if rows[base][0] is not None or not any(rows[item][0] is not None for item in run):
            continue
        for item in run:
            copy, source = rows[item]
            if copy is None:
                continue
            _revert(source, changed)
            rows[item] = (None, source)


def _layer_count(node):
    if node.kind != "group":
        return 1
    return sum(_layer_count(child) for child in node.children)


def cut_folder(nodes, folder_id, points, width, height, name=""):
    """Move the selection out of this folder into a new sibling folder in front of it.

    Folders inside the selection stay folders. A clipping mask stays a clipping
    mask, on the same layer below it. `name` comes from the dialog. An empty
    name uses Cut, then Cut 2.
    Returns (new folder or None, number of layers that gave up pixels).
    """
    folder = find_node(nodes, folder_id)
    if folder is None or folder.kind != "group" or len(points) < 3:
        return None, 0
    mask = polygon_mask(width, height, points)
    ids = _CutIds(nodes)
    children = _draft_list(folder.children, mask, ids, {})
    if not children:
        return None, 0
    chosen = (name or "").strip() or next_cut_name(nodes)
    made = Node(ids.next(), chosen, "group")
    made.children = children
    _insert_in_front(nodes, folder.id, made)
    refresh(nodes)
    return made, _layer_count(made)


def erase_folder(nodes, folder_id, points, width, height):
    """Clear the selection on every layer in the folder. Returns how many layers changed."""
    folder = find_node(nodes, folder_id)
    if folder is None or folder.kind != "group" or len(points) < 3:
        return 0
    mask = polygon_mask(width, height, points)
    changed = 0
    for layer in _layers_in(folder):
        _cut, remain = _split_raster(layer.raster, mask)
        if _cut is None:
            continue
        layer.raster = remain
        changed += 1
    if changed:
        refresh(nodes)
    return changed
