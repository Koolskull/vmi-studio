"""Warp targets are the pink quads a picture, video, or gif is bent into.

Names vary. Some are vectors with no pixels, some are hot-pink polygons.
A window is the screen, monitor, or mask those quads belong to.
"""

import re

from vmi_studio.document import find_node, walk

# Unselected names stay as dark as the grey text. The pink is only a hue shift.
WARP_PINK = "#5c3e4a"
WARP_PURPLE = "#8b6bb8"

_WARP_RE = re.compile(
    r"(?i)(?:^|[^a-z0-9])(?:wt(?:[-_ ]*\d+)?|warp[-_ ]*targets?)(?:$|[^a-z0-9])"
)
_WINDOW_RE = re.compile(r"(?i)(?:screen\s*mask|sceen\s*mask|screenmask|\bwindows?\b)")
_WINDOW_FOLDER_RE = re.compile(
    r"(?i)^(?:monitor|screen|screens|tv|tvs|poster|posters|window|windows)$"
)
_CONTENT_RE = re.compile(r"(?i)photo|image|picture|nft|screenshot|gif|video")


def name_is_warp(name):
    text = name or ""
    if "screenshot" in text.lower():
        return False
    return _WARP_RE.search(" %s " % text) is not None


def name_is_window(name):
    return _WINDOW_RE.search(name or "") is not None


def folder_is_window(name):
    return _WINDOW_FOLDER_RE.match((name or "").strip()) is not None


def pink_raster(raster):
    """True when the opaque paint is the hot-pink warp marker."""
    if raster is None or not raster.rgba or raster.w < 1 or raster.h < 1:
        return False
    data = raster.rgba
    count = raster.w * raster.h
    if len(data) < count * 4:
        return False
    step = max(1, count // 80000)
    opaque = 0
    pink = 0
    index = 0
    while index < count:
        offset = index * 4
        red, green, blue, alpha = data[offset], data[offset + 1], data[offset + 2], data[offset + 3]
        if alpha >= 40:
            opaque += 1
            if red >= 180 and blue >= 120 and green <= int(min(red, blue) * 0.65) and red - green >= 70:
                pink += 1
        index += step
    if opaque == 0:
        return False
    need = 1 if count <= 64 else 3
    return pink >= need and pink / opaque >= 0.55


def mark_warps(nodes):
    """Tag each node warp as target, window, or empty. Returns (targets, windows)."""
    for node in walk(nodes):
        node.warp = ""

    def visit(lst):
        for node in lst:
            if node.kind == "layer" and (name_is_warp(node.name) or pink_raster(node.raster)):
                node.warp = "target"
            elif node.kind == "group" and name_is_warp(node.name):
                node.warp = "target"
            visit(node.children)

    def visit_empty(lst, parent):
        for node in lst:
            if (
                node.kind == "layer"
                and not node.warp
                and node.raster is None
                and not node.omit
                and parent is not None
                and folder_is_window(parent.name)
                and _CONTENT_RE.search(node.name or "") is None
            ):
                node.warp = "target"
            visit_empty(node.children, node)

    def visit_windows(lst):
        for node in lst:
            visit_windows(node.children)
            if node.warp == "target":
                continue
            if name_is_window(node.name):
                node.warp = "window"
            elif node.kind == "group" and folder_is_window(node.name):
                if any(getattr(child, "warp", "") == "target" for child in walk(node.children)):
                    node.warp = "window"

    visit(nodes)
    visit_empty(nodes, None)
    visit_windows(nodes)
    return warp_counts(nodes)


def warp_counts(nodes):
    targets = 0
    windows = 0
    for node in walk(nodes):
        role = getattr(node, "warp", "")
        if role == "target":
            targets += 1
        elif role == "window":
            windows += 1
    return targets, windows


def _under_window(nodes, ident):
    found = {"yes": False}

    def visit(lst, under):
        for node in lst:
            here = under or getattr(node, "warp", "") == "window"
            if node.id == ident:
                if getattr(node, "warp", "") == "window" or under:
                    found["yes"] = True
                return True
            if visit(node.children, here):
                return True
        return False

    visit(nodes, False)
    return found["yes"]


def warp_system_folder(node):
    """A folder that is a warp target, a warp window, or holds one directly."""
    if node is None or node.kind != "group":
        return False
    if getattr(node, "warp", "") in ("target", "window"):
        return True
    return any(getattr(child, "warp", "") in ("target", "window") for child in node.children)


def warp_system_layers(node):
    """Layers that belong to this warp system, hidden quads included."""
    from vmi_studio.document import paint_layers

    layers = []
    if node is None:
        return layers
    for layer in paint_layers([node]):
        if layer.kind != "layer":
            continue
        role = getattr(layer, "warp", "")
        if role in ("target", "window") or (layer.visible and not layer.omit):
            layers.append(layer)
    return layers


def warp_layer_kind(layers):
    """A warp target is a warp map. A window with no target is a mask."""
    saw_window = False
    for layer in layers or []:
        role = getattr(layer, "warp", "")
        if role == "target":
            return "warp map"
        if role == "window":
            saw_window = True
    return "mask" if saw_window else ""


def object_role(art, obj):
    """A prepared warp target stays pink. A window object is purple."""
    if art is None or obj is None:
        return ""
    forced = getattr(obj, "role", "") or ""
    if forced in ("target", "window"):
        return forced
    saw_window = False
    saw_target = False
    for ident in obj.layer_ids:
        layer = find_node(art.layers, ident)
        if layer is None:
            continue
        role = getattr(layer, "warp", "")
        if role == "window" or _under_window(art.layers, ident):
            saw_window = True
        elif role == "target":
            saw_target = True
    if saw_window:
        return "window"
    if saw_target:
        return "target"
    return ""
