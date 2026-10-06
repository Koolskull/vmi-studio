"""Stack the picture back to front.

A Clip Studio clipping mask stays inside the layer it clips onto. Later
clipped layers use that same base, each with its own blend mode. A Normal
folder keeps those blends inside the folder. Pass through lets them reach
the paint underneath.

A folder plate is only as large as the paint inside it. Blend work runs in
short bands. The fit view can ask for a smaller edge so an 8K drawing is
shown at screen size instead of being rebuilt at full resolution.

A folder that did not change is reused. Mute and solo rebuild the folders
on that branch. Pass-through folders still paint onto the picture below
them, and a clipping mask is stamped onto a copy of the cached plate.
"""

import math
import threading

import numpy as np
from PIL import Image

from vmi_studio.document import (
    animation_folders,
    find_node,
    gate_of,
    paint_gate,
    paint_layers,
    timeline_cels,
    _solo_on,
)


_MODES = {
    "normal": "normal",
    "norm": "normal",
    "pass": "pass",
    "pass through": "pass",
    "passthrough": "pass",
    "darken": "darken",
    "dark": "darken",
    "multiply": "multiply",
    "mul": "multiply",
    "color burn": "color_burn",
    "idiv": "color_burn",
    "burn": "color_burn",
    "linear burn": "linear_burn",
    "lbrn": "linear_burn",
    "subtract": "subtract",
    "fsub": "subtract",
    "darker color": "darker_color",
    "dkcl": "darker_color",
    "lighten": "lighten",
    "lite": "lighten",
    "screen": "screen",
    "scrn": "screen",
    "color dodge": "color_dodge",
    "div": "color_dodge",
    "dodge": "color_dodge",
    "glow dodge": "glow_dodge",
    "glowdodge": "glow_dodge",
    "linear dodge": "linear_dodge",
    "lddg": "linear_dodge",
    "add": "linear_dodge",
    "addition": "linear_dodge",
    "add glow": "add_glow",
    "add (glow)": "add_glow",
    "lighter color": "lighter_color",
    "lgcl": "lighter_color",
    "overlay": "overlay",
    "over": "overlay",
    "soft light": "soft_light",
    "slit": "soft_light",
    "hard light": "hard_light",
    "hlit": "hard_light",
    "vivid light": "vivid_light",
    "vlit": "vivid_light",
    "linear light": "linear_light",
    "llit": "linear_light",
    "pin light": "pin_light",
    "plit": "pin_light",
    "hard mix": "hard_mix",
    "hmix": "hard_mix",
    "difference": "difference",
    "diff": "difference",
    "exclusion": "exclusion",
    "smud": "exclusion",
    "hue": "hue",
    "saturation": "saturation",
    "sat": "saturation",
    "color": "color",
    "colr": "color",
    "luminosity": "luminosity",
    "lum": "luminosity",
    "brightness": "luminosity",
    "divide": "divide",
    "fdiv": "divide",
}

_SHOWN = {
    "multiply": "multiply",
    "color_burn": "color burn",
    "linear_burn": "linear burn",
    "subtract": "subtract",
    "darker_color": "darker color",
    "darken": "darken",
    "lighten": "lighten",
    "screen": "screen",
    "color_dodge": "color dodge",
    "glow_dodge": "glow dodge",
    "linear_dodge": "add",
    "add_glow": "add (glow)",
    "lighter_color": "lighter color",
    "overlay": "overlay",
    "soft_light": "soft light",
    "hard_light": "hard light",
    "vivid_light": "vivid light",
    "linear_light": "linear light",
    "pin_light": "pin light",
    "hard_mix": "hard mix",
    "difference": "difference",
    "exclusion": "exclusion",
    "hue": "hue",
    "saturation": "saturation",
    "color": "color",
    "luminosity": "luminosity",
    "divide": "divide",
}


def _mode_key(blend):
    return " ".join((blend or "normal").strip().lower().replace("_", " ").split())


def resolve_mode(blend, shapes=True):
    """Return (mode, glow, unknown). Unknown modes paint as normal."""
    key = _mode_key(blend)
    mode = _MODES.get(key)
    if mode is None:
        return "normal", False, True
    if not shapes and mode == "linear_dodge":
        mode = "add_glow"
    elif not shapes and mode == "color_dodge":
        mode = "glow_dodge"
    return mode, mode in ("add_glow", "glow_dodge"), False


# Order matches a paint program's blend list. The stored word is what resolve_mode reads.
BLEND_CHOICES = (
    ("normal", "Normal"),
    ("pass through", "Pass through"),
    ("darken", "Darken"),
    ("multiply", "Multiply"),
    ("color burn", "Color burn"),
    ("linear burn", "Linear burn"),
    ("subtract", "Subtract"),
    ("darker color", "Darker color"),
    ("lighten", "Lighten"),
    ("screen", "Screen"),
    ("color dodge", "Color dodge"),
    ("glow dodge", "Glow dodge"),
    ("add", "Add"),
    ("add (glow)", "Add (glow)"),
    ("lighter color", "Lighter color"),
    ("overlay", "Overlay"),
    ("soft light", "Soft light"),
    ("hard light", "Hard light"),
    ("vivid light", "Vivid light"),
    ("linear light", "Linear light"),
    ("pin light", "Pin light"),
    ("hard mix", "Hard mix"),
    ("difference", "Difference"),
    ("exclusion", "Exclusion"),
    ("hue", "Hue"),
    ("saturation", "Saturation"),
    ("color", "Color"),
    ("luminosity", "Luminosity"),
    ("divide", "Divide"),
)


def blend_label(blend, shapes=True):
    """The menu name for a layer's stored blend."""
    mode, _glow, unknown = resolve_mode(blend, shapes)
    if unknown:
        text = (blend or "").strip()
        return text or "Normal"
    for key, label in BLEND_CHOICES:
        resolved, _glow, _missed = resolve_mode(key, True)
        if resolved == mode:
            return label
    return "Normal"


def assign_blend(node, key):
    """Store a menu choice. Glow modes are the ones Clip Studio marks with shapes off."""
    node.blend = key
    node.shapes = key not in ("add (glow)", "glow dodge")
    return node


def blend_summary(nodes):
    """One sentence when the drawing uses clips or a blend other than normal."""
    clips = 0
    modes = []
    unknown = []

    def visit(lst, ancestor_hidden):
        nonlocal clips
        for node in lst:
            hidden = ancestor_hidden or not node.visible or node.omit in ("hidden", "hidden folder")
            if not hidden and node.clip:
                clips += 1
            if not hidden:
                mode, _glow, missed = resolve_mode(node.blend, getattr(node, "shapes", True))
                if missed:
                    label = (node.blend or "").strip() or "unknown"
                    if label not in unknown:
                        unknown.append(label)
                elif mode not in ("normal", "pass"):
                    shown = _SHOWN.get(mode, mode)
                    if shown not in modes:
                        modes.append(shown)
            if node.kind == "group":
                visit(node.children, hidden)

    visit(nodes, False)
    parts = []
    if clips:
        parts.append("%d clipping mask%s." % (clips, "" if clips == 1 else "s"))
    if modes:
        parts.append("Blend modes: %s." % ", ".join(modes))
    if unknown:
        parts.append("Drawn as normal: %s." % ", ".join(unknown))
    return " ".join(parts)


def visible_paint(nodes):
    """Paint layers that are still showing. A hidden or muted folder drops its contents.

    When any track is soloed, only soloed tracks remain.
    """
    solo_on = _solo_on(nodes)
    out = []

    def visit(lst, ancestor_hidden, ancestor_muted, ancestor_solo):
        for node in lst:
            gate = paint_gate(node, ancestor_hidden, ancestor_muted, ancestor_solo, solo_on)
            if gate == "drop":
                continue
            child_solo = ancestor_solo or bool(node.solo)
            if node.kind == "group":
                visit(node.children, False, False, child_solo)
            elif gate == "in":
                out.append(node)

    visit(nodes, False, False, False)
    return out


def has_hidden(nodes):
    for node in nodes:
        if not node.visible:
            return True
        if node.kind == "group" and has_hidden(node.children):
            return True
    return False


def ink_count(image):
    if image is None:
        return 0
    hist = image.getchannel("A").histogram()
    return sum(hist[1:])


def composite_image(width, height, layers):
    """Full-canvas RGBA of this list, back to front. None when nothing has paint."""
    return _composite(width, height, layers, gated=False, focus_id=None, max_edge=None)


def composite_scene(width, height, nodes, focus_id=None, max_edge=None, flags=None):
    """The picture. Hides, mute, solo, and animation cels follow the tree.

    Clipping masks and blend modes stay with the folders they were drawn in.
    max_edge is the fit view: the long side of the document is shrunk to it.
    flags is a capture_flags snapshot. The build uses that copy, not later clicks.
    """
    return _composite(width, height, nodes, gated=True, focus_id=focus_id, max_edge=max_edge, flags=flags)


def _composite(width, height, nodes, gated, focus_id, max_edge, flags=None):
    if width <= 0 or height <= 0:
        return None
    scale = 1.0
    if max_edge and max(width, height) > max_edge > 0:
        scale = float(max_edge) / float(max(width, height))
    sw = max(1, int(round(width * scale)))
    sh = max(1, int(round(height * scale)))
    if sw * sh > 8192 * 8192:
        return None
    if not _any_raster(nodes):
        return None
    image = Image.new("RGBA", (sw, sh), (0, 0, 0, 0))
    solo = _solo_from(nodes, flags) if gated else False
    ctx = _Ctx(gated, solo, nodes, focus_id)
    ctx.scale = scale
    ctx.flags = flags
    scale_key = int(round(scale * 100000))
    ctx.plates = _Plates(_take_store(scale_key))
    try:
        _paint_list(_Surface(image, 0, 0, (sw, sh)), nodes, ctx)
    finally:
        _keep_store(scale_key, ctx.plates)
    return image


def _any_raster(nodes):
    for node in nodes:
        raster = getattr(node, "raster", None)
        if raster is not None and raster.w > 0 and raster.h > 0 and len(raster.rgba) == raster.w * raster.h * 4:
            return True
        if getattr(node, "kind", "") == "group" and _any_raster(node.children):
            return True
    return False


class _Ctx:
    def __init__(self, gated, solo_on, nodes, focus_id, hidden=False, muted=False, solo=False):
        self.gated = gated
        self.solo_on = solo_on
        self.roots = {folder.id for folder in animation_folders(nodes)} if gated else set()
        self.focus = focus_id
        self.hidden = hidden
        self.muted = muted
        self.solo = solo

    def child(self, node):
        _visible, _mute, solo = _trio(node, self)
        nxt = _Ctx(self.gated, self.solo_on, [], self.focus, False, False, self.solo or solo)
        nxt.roots = self.roots
        nxt.scale = getattr(self, "scale", 1)
        nxt.flags = getattr(self, "flags", None)
        nxt.plates = getattr(self, "plates", None)
        return nxt


class _View:
    """One animation cel standing in the folder's place in the stack."""

    def __init__(self, node, clip):
        self.id = node.id
        self.kind = node.kind
        self.children = node.children
        self.raster = node.raster
        self.blend = node.blend
        self.opacity = node.opacity
        self.shapes = getattr(node, "shapes", True)
        self.clip = clip
        self.visible = True
        self.mute = False
        self.solo = False
        self.animation = False
        self.omit = ""


def _trio(node, ctx):
    """Hide, mute, and solo. A snapshot wins over a click that lands mid-build."""
    if isinstance(node, _View):
        return bool(node.visible), bool(node.mute), bool(node.solo)
    flags = getattr(ctx, "flags", None)
    if flags and node.id in flags:
        return flags[node.id]
    return (
        bool(getattr(node, "visible", True)),
        bool(getattr(node, "mute", False)),
        bool(getattr(node, "solo", False)),
    )


def _solo_from(nodes, flags):
    if not flags:
        return _solo_on(nodes)
    return any(row[2] for row in flags.values())


def _branch_solo(node, ctx):
    if _trio(node, ctx)[2]:
        return True
    return any(_branch_solo(child, ctx) for child in getattr(node, "children", []))


def _slot(node, ctx):
    """The node to paint for this stack slot, or None when the slot is empty."""
    if not ctx.gated:
        return node
    visible, mute, solo = _trio(node, ctx)
    gate = gate_of(visible, mute, solo, ctx.hidden, ctx.muted, ctx.solo, ctx.solo_on)
    if gate == "drop":
        return None
    if node.kind == "group" and node.id in ctx.roots:
        cel = _chosen_cel(node, ctx)
        if cel is None:
            return None
        _cel_visible, _cel_mute, cel_solo = _trio(cel, ctx)
        view = _View(cel, node.clip or cel.clip)
        view.solo = bool(ctx.solo or solo or cel_solo)
        return view
    if gate == "seek" and node.kind != "group":
        return None
    return node


def _chosen_cel(folder, ctx):
    child_solo = ctx.solo or _trio(folder, ctx)[2]
    cels = [cel for cel in timeline_cels(folder) if _trio(cel, ctx)[0]]
    if ctx.solo_on and not child_solo:
        cels = [cel for cel in cels if _trio(cel, ctx)[2] or _branch_solo(cel, ctx)]
    if not cels:
        return None
    chosen = cels[0]
    focus = ctx.focus
    if focus and focus != folder.id:
        for cel in cels:
            holds = cel.id == focus or (cel.kind == "group" and find_node(cel.children, focus))
            if holds:
                chosen = cel
                break
    visible, mute, solo = _trio(chosen, ctx)
    if gate_of(visible, mute, solo, False, False, child_solo, ctx.solo_on) == "drop":
        return None
    return chosen


def pick_layer(width, height, nodes, x, y, focus_id=None):
    """The front-most painted layer at one document pixel, or None.

    Hide, mute, solo, the current animation cel, and clipping follow the
    picture. A transparent pixel falls through. A clip counts only inside
    its base.
    """
    if width <= 0 or height <= 0 or not nodes:
        return None
    try:
        px = math.floor(float(x))
        py = math.floor(float(y))
    except (TypeError, ValueError):
        return None
    if px < 0 or py < 0 or px >= width or py >= height:
        return None
    ctx = _Ctx(True, _solo_on(nodes), nodes, focus_id)
    ctx.scale = 1
    return _pick_list(nodes, ctx, int(px), int(py))


def _alpha_at(node, x, y):
    raster = getattr(node, "raster", None)
    if raster is None or raster.w <= 0 or raster.h <= 0:
        return 0
    if len(raster.rgba) != raster.w * raster.h * 4:
        return 0
    try:
        opacity = int(getattr(node, "opacity", 255) or 0)
    except (TypeError, ValueError):
        opacity = 255
    if opacity <= 0:
        return 0
    lx = int(x) - int(raster.x)
    ly = int(y) - int(raster.y)
    if lx < 0 or ly < 0 or lx >= raster.w or ly >= raster.h:
        return 0
    alpha = raster.rgba[(ly * raster.w + lx) * 4 + 3]
    if alpha <= 0:
        return 0
    return max(1, alpha * min(opacity, 255) // 255)


def _covers(node, ctx, x, y):
    if getattr(node, "kind", "") != "group":
        return _alpha_at(node, x, y) > 0
    return _covers_list(getattr(node, "children", []), ctx.child(node), x, y)


def _covers_list(nodes, ctx, x, y):
    for raw in nodes:
        if getattr(raw, "clip", False):
            continue
        item = _slot(raw, ctx)
        if item is not None and _covers(item, ctx, x, y):
            return True
    return False


def _pick_list(nodes, ctx, x, y):
    index = len(nodes) - 1
    while index >= 0:
        raw = nodes[index]
        if getattr(raw, "clip", False):
            index -= 1
            continue
        item = _slot(raw, ctx)
        clips = []
        nxt = index + 1
        while nxt < len(nodes) and getattr(nodes[nxt], "clip", False):
            clipped = _slot(nodes[nxt], ctx)
            if clipped is not None:
                clips.append(clipped)
            nxt += 1
        if item is not None:
            covered = _covers(item, ctx, x, y)
            for clipped in reversed(clips):
                if not covered:
                    break
                hit = _pick_one(clipped, ctx, x, y)
                if hit:
                    return hit
            hit = _pick_one(item, ctx, x, y)
            if hit:
                return hit
        index -= 1
    return None


def _pick_one(node, ctx, x, y):
    if getattr(node, "kind", "") == "group":
        return _pick_list(node.children, ctx.child(node), x, y)
    if _alpha_at(node, x, y) > 0:
        return node.id
    return None


def _is_pass(node):
    mode, _glow, _unknown = resolve_mode(getattr(node, "blend", "normal"), True)
    return mode == "pass"


# Folder plates from the last picture at this scale. A mute reuses the folders
# that did not change. Two scales stay (the fit view and a full export).
_PLATE_LOCK = threading.Lock()
_PLATE_STORES = {}
_PLATE_BUDGET = 768 * 1024 * 1024
_plate_stats = {"hits": 0, "misses": 0}


def reset_plate_cache():
    """Drop remembered folder plates. Tests use this so a hit is earned."""
    global _PLATE_STORES
    with _PLATE_LOCK:
        _PLATE_STORES = {}
    _plate_stats["hits"] = 0
    _plate_stats["misses"] = 0


def plate_stats():
    return dict(_plate_stats)


def _take_store(scale_key):
    with _PLATE_LOCK:
        return _PLATE_STORES.get(scale_key, {})


def _keep_store(scale_key, plates):
    """Remember this pass, and the plates it did not need, while they fit.

    Mute then unmute asks for the previous plate. Dropping it would rebuild
    the folder that the click just put back.
    """
    _plate_stats["hits"] = plates.hits
    _plate_stats["misses"] = plates.misses
    kept = dict(plates.used)
    size = plates.bytes
    for key, found in plates.previous.items():
        if key in kept:
            continue
        image = found[0]
        extra = image.size[0] * image.size[1] * 4
        if size + extra > _PLATE_BUDGET:
            continue
        kept[key] = found
        size += extra
    with _PLATE_LOCK:
        _PLATE_STORES[scale_key] = kept
        if len(_PLATE_STORES) > 2:
            for key in list(_PLATE_STORES):
                if key != scale_key:
                    _PLATE_STORES.pop(key, None)
                    break


class _Plates:
    def __init__(self, previous):
        self.previous = previous or {}
        self.used = {}
        self.hits = 0
        self.misses = 0
        self.bytes = 0

    def lookup(self, key):
        found = self.previous.get(key)
        if found is None:
            found = self.used.get(key)
        if found is None:
            self.misses += 1
            return None
        self.hits += 1
        if key not in self.used:
            image = found[0]
            self.bytes += image.size[0] * image.size[1] * 4
            self.used[key] = found
        return found

    def store(self, key, image, ox, oy):
        if key in self.used:
            return
        size = image.size[0] * image.size[1] * 4
        if self.bytes + size > _PLATE_BUDGET:
            return
        self.used[key] = (image, ox, oy)
        self.bytes += size


def _raster_sig(raster):
    """Identity of a layer's pixels. Small fixtures store the bytes themselves."""
    if raster is None or raster.w <= 0 or raster.h <= 0:
        return None
    data = raster.rgba
    n = len(data) if data is not None else 0
    if n <= 256:
        blob = data if isinstance(data, (bytes, bytearray)) else bytes(data)
        return (raster.x, raster.y, raster.w, raster.h, blob)
    return (raster.x, raster.y, raster.w, raster.h, n, id(data))


def _animation_in(node):
    if getattr(node, "animation", False):
        return True
    for child in getattr(node, "children", []):
        if _animation_in(child):
            return True
    return False


def _focus_key(node, focus):
    """A selection changes the picture only by choosing an animation cel."""
    if not focus or focus == getattr(node, "id", None):
        return ""
    if not _animation_in(node):
        return ""
    if find_node(getattr(node, "children", []), focus):
        return focus
    return ""


def _tree_sig(nodes, ctx):
    rows = []
    for node in nodes:
        visible, mute, solo = _trio(node, ctx)
        row = (
            node.id,
            node.kind,
            visible,
            mute,
            solo,
            int(getattr(node, "opacity", 255) or 0),
            getattr(node, "blend", "") or "",
            bool(getattr(node, "shapes", True)),
            bool(getattr(node, "clip", False)),
            bool(getattr(node, "animation", False)),
            _raster_sig(getattr(node, "raster", None)),
        )
        if getattr(node, "kind", "") == "group":
            row = row + (_tree_sig(node.children, ctx.child(node)),)
        rows.append(row)
    return tuple(rows)


def _plate_key(node, ctx, canvas, scale):
    child = ctx.child(node)
    # Gated and full export walk the same pixels differently. A hidden child
    # stays in the signature either way, so the gate has to be part of the key.
    return (
        int(round(float(scale) * 100000)),
        canvas[0],
        canvas[1],
        bool(child.gated),
        bool(child.solo_on),
        bool(child.solo),
        bool(child.hidden),
        bool(child.muted),
        _focus_key(node, child.focus),
        _tree_sig(node.children, child),
    )


class _Surface:
    """A piece of the document. ox, oy is where this image sits on the canvas."""

    def __init__(self, image, ox, oy, canvas):
        self.image = image
        self.ox = int(ox)
        self.oy = int(oy)
        self.canvas = canvas


def _paint_list(dest, nodes, ctx):
    index = 0
    count = len(nodes)
    while index < count:
        raw = nodes[index]
        item = _slot(raw, ctx)
        if item is None:
            index += 1
            if not getattr(raw, "clip", False):
                while index < count and nodes[index].clip:
                    index += 1
            continue
        if item.clip:
            index += 1
            continue
        clips = []
        nxt = index + 1
        while nxt < count and nodes[nxt].clip:
            clipped = _slot(nodes[nxt], ctx)
            if clipped is not None:
                clips.append(clipped)
            nxt += 1
        opacity = int(getattr(item, "opacity", 255) or 0)
        direct = item.kind == "group" and _is_pass(item) and not clips and opacity >= 255
        if direct:
            _paint_list(dest, item.children, ctx.child(item))
        elif item.kind != "group" and not clips:
            _stamp(dest, item, ctx, clip_to_dest=False)
        else:
            plate = _appearance(dest.canvas, item, ctx)
            if plate is not None:
                # Clips draw onto the plate. A cached folder plate stays untouched.
                if clips:
                    plate = _Surface(plate.image.copy(), plate.ox, plate.oy, dest.canvas)
                for clipped in clips:
                    _stamp(plate, clipped, ctx, clip_to_dest=True)
                _blend(dest, plate.image, (plate.ox, plate.oy), item, clip_to_dest=False)
        index = nxt


def _stamp(dest, node, ctx, clip_to_dest):
    """Paint one node onto dest. A group is flattened first, then blended."""
    if node.kind == "group":
        nested = _appearance(dest.canvas, node, ctx)
        if nested is not None:
            _blend(dest, nested.image, (nested.ox, nested.oy), node, clip_to_dest)
        return
    placed = _raster_view(node.raster, getattr(ctx, "scale", 1), dest.canvas)
    if placed is None:
        return
    src, origin = placed
    _blend(dest, src, origin, node, clip_to_dest)


def _appearance(canvas, node, ctx):
    """This node's paint before its blend hits the parent.

    The plate covers the paint, not the whole document. The base of a clipping
    stack is pasted as itself. Its blend mode is applied later, to the whole stack.
    """
    scale = getattr(ctx, "scale", 1)
    if node.kind != "group":
        placed = _raster_view(node.raster, scale, canvas)
        if placed is None:
            return None
        src, origin = placed
        return _Surface(src, origin[0], origin[1], canvas)
    plates = getattr(ctx, "plates", None)
    key = _plate_key(node, ctx, canvas, scale) if plates is not None else None
    if plates is not None:
        found = plates.lookup(key)
        if found is not None:
            image, ox, oy = found
            return _Surface(image, ox, oy, canvas)
    bounds = _list_bounds(node.children, ctx.child(node), canvas, scale)
    if bounds is None:
        return None
    x, y, w, h = bounds
    plate = _Surface(Image.new("RGBA", (w, h), (0, 0, 0, 0)), x, y, canvas)
    _paint_list(plate, node.children, ctx.child(node))
    if plates is not None:
        plates.store(key, plate.image, plate.ox, plate.oy)
    return plate


def _list_bounds(nodes, ctx, canvas, scale):
    total = None
    index = 0
    count = len(nodes)
    while index < count:
        raw = nodes[index]
        item = _slot(raw, ctx)
        if item is None:
            index += 1
            if not getattr(raw, "clip", False):
                while index < count and nodes[index].clip:
                    index += 1
            continue
        if item.clip:
            index += 1
            continue
        nxt = index + 1
        while nxt < count and nodes[nxt].clip:
            nxt += 1
        total = _union(total, _item_bounds(item, ctx, canvas, scale))
        index = nxt
    return total


def _item_bounds(item, ctx, canvas, scale):
    if item.kind != "group":
        return _raster_box(item.raster, scale, canvas)
    return _list_bounds(item.children, ctx.child(item), canvas, scale)


def _union(a, b):
    if a is None:
        return b
    if b is None:
        return a
    x0 = min(a[0], b[0])
    y0 = min(a[1], b[1])
    x1 = max(a[0] + a[2], b[0] + b[2])
    y1 = max(a[1] + a[3], b[1] + b[3])
    return x0, y0, x1 - x0, y1 - y0


def _raster_box(raster, scale, canvas):
    """Where a raster lands on the canvas, or None when it misses."""
    if raster is None or raster.w <= 0 or raster.h <= 0:
        return None
    if len(raster.rgba) != raster.w * raster.h * 4:
        return None
    width, height = canvas
    if scale == 1:
        left = max(0, -raster.x)
        top = max(0, -raster.y)
        right = min(raster.w, width - raster.x)
        bottom = min(raster.h, height - raster.y)
        if right <= left or bottom <= top:
            return None
        return raster.x + left, raster.y + top, right - left, bottom - top
    x0 = math.floor(raster.x * scale)
    y0 = math.floor(raster.y * scale)
    x1 = math.ceil((raster.x + raster.w) * scale)
    y1 = math.ceil((raster.y + raster.h) * scale)
    if x1 <= x0:
        x1 = x0 + 1
    if y1 <= y0:
        y1 = y0 + 1
    left = max(0, -x0)
    top = max(0, -y0)
    right = min(x1 - x0, width - x0)
    bottom = min(y1 - y0, height - y0)
    if right <= left or bottom <= top:
        return None
    return x0 + left, y0 + top, right - left, bottom - top


def _scaled_layer(raster, scale):
    """One scaled copy of a layer, kept for the next hide, show, or frame change.

    The fit view asks for the same scale over and over. Resizing every full
    canvas layer on each click is what froze the window.
    """
    key = int(round(float(scale) * 100000))
    cached = getattr(raster, "preview", None)
    if isinstance(cached, tuple) and len(cached) == 2 and cached[0] == key:
        return cached[1]
    src = Image.frombytes("RGBA", (raster.w, raster.h), raster.rgba)
    full_w = max(1, math.ceil((raster.x + raster.w) * scale) - math.floor(raster.x * scale))
    full_h = max(1, math.ceil((raster.y + raster.h) * scale) - math.floor(raster.y * scale))
    src = src.resize((full_w, full_h), Image.Resampling.BOX)
    raster.preview = (key, src)
    return src


def _raster_view(raster, scale, canvas):
    box = _raster_box(raster, scale, canvas)
    if box is None:
        return None
    x, y, w, h = box
    if scale == 1:
        src = Image.frombytes("RGBA", (raster.w, raster.h), raster.rgba)
        left = x - raster.x
        top = y - raster.y
        if left or top or w != raster.w or h != raster.h:
            src = src.crop((left, top, left + w, top + h))
        return src, (x, y)
    scaled = _scaled_layer(raster, scale)
    left = x - math.floor(raster.x * scale)
    top = y - math.floor(raster.y * scale)
    # A copy, so a clipping stack can draw onto this plate without touching the cache.
    src = scaled.crop((left, top, left + w, top + h))
    return src, (x, y)


def _blend(dest, src, origin, node, clip_to_dest):
    """Blend src onto dest. A clip keeps the destination's coverage."""
    mode, glow, _unknown = resolve_mode(getattr(node, "blend", "normal"), getattr(node, "shapes", True))
    if mode == "pass":
        mode = "normal"
    opacity = max(0, min(255, int(getattr(node, "opacity", 255) or 0)))
    if opacity <= 0:
        return
    sx = origin[0] - dest.ox
    sy = origin[1] - dest.oy
    box = _overlap(dest.image.size, src.size, sx, sy)
    if box is None:
        return
    dx, dy, dw, dh = box
    src_box = (dx - sx, dy - sy, dx - sx + dw, dy - sy + dh)
    src_crop = src.crop(src_box)
    if mode == "normal" and not glow and not clip_to_dest:
        if opacity < 255:
            src_crop = _scale_alpha(src_crop, opacity)
        dest_crop = dest.image.crop((dx, dy, dx + dw, dy + dh))
        dest.image.paste(Image.alpha_composite(dest_crop, src_crop), (dx, dy))
        return
    # Bands keep a large multiply or glow from building a full-frame float buffer.
    band = 128
    y = 0
    while y < dh:
        bh = min(band, dh - y)
        src_band = src_crop.crop((0, y, dw, y + bh))
        dest_box = (dx, dy + y, dx + dw, dy + y + bh)
        dest_band = dest.image.crop(dest_box)
        dest.image.paste(_mix_band(dest_band, src_band, mode, glow, opacity, clip_to_dest), (dx, dy + y))
        y += bh


def _mix_band(dest_band, src_band, mode, glow, opacity, clip_to_dest):
    base = np.asarray(dest_band).astype(np.float32) / 255.0
    over = np.asarray(src_band).astype(np.float32) / 255.0
    cb = base[:, :, :3]
    ca = base[:, :, 3:4]
    cs = over[:, :, :3]
    sa = over[:, :, 3:4] * (opacity / 255.0)
    if glow:
        sa = np.clip(sa * (2.0 - sa), 0.0, 1.0)
    mixed = _apply_mode(mode, cb, cs)
    ao = sa + ca * (1.0 - sa)
    premul = sa * (1.0 - ca) * cs + sa * ca * mixed + (1.0 - sa) * ca * cb
    co = np.zeros_like(premul)
    np.divide(premul, ao, out=co, where=ao > 1e-6)
    if clip_to_dest:
        visible = ca > 0
        co = np.where(visible, co, cb)
        ao = ca
    out = np.empty_like(base)
    out[:, :, :3] = np.clip(co, 0.0, 1.0)
    out[:, :, 3:4] = np.clip(ao, 0.0, 1.0)
    return Image.fromarray(np.rint(out * 255.0).astype(np.uint8), "RGBA")


def _scale_alpha(image, opacity):
    arr = np.asarray(image).astype(np.uint16)
    arr[:, :, 3] = (arr[:, :, 3] * opacity) // 255
    return Image.fromarray(arr.astype(np.uint8), "RGBA")


def _overlap(dest_size, src_size, sx, sy):
    width, height = dest_size
    sw, sh = src_size
    dx = max(0, sx)
    dy = max(0, sy)
    right = min(width, sx + sw)
    bottom = min(height, sy + sh)
    if right <= dx or bottom <= dy:
        return None
    return dx, dy, right - dx, bottom - dy


def _apply_mode(mode, cb, cs):
    if mode == "multiply":
        return cb * cs
    if mode == "screen":
        return 1.0 - (1.0 - cb) * (1.0 - cs)
    if mode == "overlay":
        return np.where(cb <= 0.5, 2.0 * cb * cs, 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs))
    if mode == "darken":
        return np.minimum(cb, cs)
    if mode == "lighten":
        return np.maximum(cb, cs)
    if mode == "color_dodge" or mode == "glow_dodge":
        return _color_dodge(cb, cs)
    if mode == "color_burn":
        return _color_burn(cb, cs)
    if mode == "linear_burn":
        return np.clip(cb + cs - 1.0, 0.0, 1.0)
    if mode == "linear_dodge" or mode == "add_glow":
        return np.clip(cb + cs, 0.0, 1.0)
    if mode == "subtract":
        return np.clip(cb - cs, 0.0, 1.0)
    if mode == "difference":
        return np.abs(cb - cs)
    if mode == "exclusion":
        return cb + cs - 2.0 * cb * cs
    if mode == "hard_light":
        return np.where(cs <= 0.5, 2.0 * cs * cb, 1.0 - 2.0 * (1.0 - cs) * (1.0 - cb))
    if mode == "soft_light":
        return _soft_light(cb, cs)
    if mode == "vivid_light":
        return np.where(cs <= 0.5, _color_burn(cb, np.clip(2.0 * cs, 0.0, 1.0)), _color_dodge(cb, np.clip(2.0 * cs - 1.0, 0.0, 1.0)))
    if mode == "linear_light":
        return np.clip(cb + 2.0 * cs - 1.0, 0.0, 1.0)
    if mode == "pin_light":
        return np.where(cs <= 0.5, np.minimum(cb, 2.0 * cs), np.maximum(cb, 2.0 * cs - 1.0))
    if mode == "hard_mix":
        return np.where(cb + cs >= 1.0, 1.0, 0.0)
    if mode == "divide":
        out = np.ones_like(cb)
        np.divide(cb, cs, out=out, where=cs > 1e-6)
        return np.clip(out, 0.0, 1.0)
    if mode == "darker_color":
        return _pick_color(cb, cs, lighter=False)
    if mode == "lighter_color":
        return _pick_color(cb, cs, lighter=True)
    if mode == "hue":
        return _hsl_keep(cb, cs, "hue")
    if mode == "saturation":
        return _hsl_keep(cb, cs, "saturation")
    if mode == "color":
        return _hsl_keep(cb, cs, "color")
    if mode == "luminosity":
        return _hsl_keep(cb, cs, "luminosity")
    return cs


def _color_dodge(cb, cs):
    out = np.ones_like(cb)
    safe = cs < 1.0
    np.divide(cb, np.maximum(1.0 - cs, 1e-8), out=out, where=safe)
    out = np.where(np.logical_and(cb <= 0.0, safe), 0.0, out)
    return np.clip(out, 0.0, 1.0)


def _color_burn(cb, cs):
    out = np.zeros_like(cb)
    out = np.where(cb >= 1.0, 1.0, out)
    safe = np.logical_and(cs > 0.0, cb < 1.0)
    burned = 1.0 - np.minimum(1.0, (1.0 - cb) / np.maximum(cs, 1e-8))
    return np.clip(np.where(safe, burned, out), 0.0, 1.0)


def _soft_light(cb, cs):
    deep = ((16.0 * cb - 12.0) * cb + 4.0) * cb
    curve = np.where(cb <= 0.25, deep, np.sqrt(np.clip(cb, 0.0, 1.0)))
    low = cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    high = cb + (2.0 * cs - 1.0) * (curve - cb)
    return np.clip(np.where(cs <= 0.5, low, high), 0.0, 1.0)


def _pick_color(cb, cs, lighter):
    weight = np.array([0.3, 0.59, 0.11], dtype=np.float32)
    lb = np.tensordot(cb, weight, axes=([-1], [0]))
    ls = np.tensordot(cs, weight, axes=([-1], [0]))
    pick = (ls > lb) if lighter else (ls < lb)
    return np.where(pick[:, :, None], cs, cb)


def _hsl_keep(cb, cs, which):
    hb, sb, lb = _rgb_hsl(cb)
    hs, ss, ls = _rgb_hsl(cs)
    if which == "hue":
        h, s, l = hs, sb, lb
    elif which == "saturation":
        h, s, l = hb, ss, lb
    elif which == "color":
        h, s, l = hs, ss, lb
    else:
        h, s, l = hb, sb, ls
    return _hsl_rgb(h, s, l)


def _rgb_hsl(rgb):
    r = rgb[:, :, 0]
    g = rgb[:, :, 1]
    b = rgb[:, :, 2]
    mx = np.maximum(np.maximum(r, g), b)
    mn = np.minimum(np.minimum(r, g), b)
    light = (mx + mn) * 0.5
    delta = mx - mn
    sat = np.zeros_like(light)
    span = mx + mn
    wide = 2.0 - mx - mn
    sat = np.where(delta <= 1e-8, 0.0, np.where(light <= 0.5, delta / np.maximum(span, 1e-8), delta / np.maximum(wide, 1e-8)))
    hue = np.zeros_like(light)
    usable = delta > 1e-8
    hue = np.where(usable & (mx == r), ((g - b) / np.maximum(delta, 1e-8)) % 6.0, hue)
    hue = np.where(usable & (mx == g), (b - r) / np.maximum(delta, 1e-8) + 2.0, hue)
    hue = np.where(usable & (mx == b), (r - g) / np.maximum(delta, 1e-8) + 4.0, hue)
    return hue / 6.0, np.clip(sat, 0.0, 1.0), np.clip(light, 0.0, 1.0)


def _hsl_rgb(hue, sat, light):
    hue = np.mod(hue, 1.0)
    q = np.where(light < 0.5, light * (1.0 + sat), light + sat - light * sat)
    p = 2.0 * light - q

    def channel(shift):
        t = np.mod(hue + shift, 1.0)
        out = p
        out = np.where(t < 1.0 / 6.0, p + (q - p) * 6.0 * t, out)
        out = np.where((t >= 1.0 / 6.0) & (t < 0.5), q, out)
        out = np.where((t >= 0.5) & (t < 2.0 / 3.0), p + (q - p) * (2.0 / 3.0 - t) * 6.0, out)
        return out

    rgb = np.empty(hue.shape + (3,), dtype=np.float32)
    rgb[:, :, 0] = channel(1.0 / 3.0)
    rgb[:, :, 1] = channel(0.0)
    rgb[:, :, 2] = channel(-1.0 / 3.0)
    return np.clip(np.where(sat[:, :, None] <= 1e-8, light[:, :, None], rgb), 0.0, 1.0)


def present(image):
    """Dark checker under the picture so clear pixels stay visible on a black field."""
    if image is None:
        return None
    width, height = image.size
    tile = 16
    xs = np.arange(width, dtype=np.int32)[None, :]
    ys = np.arange(height, dtype=np.int32)[:, None]
    light = ((xs // tile) + (ys // tile)) % 2 == 0
    tone = np.where(light, np.uint8(34), np.uint8(17))
    bg = np.empty((height, width, 4), np.uint8)
    bg[:, :, 0] = tone
    bg[:, :, 1] = tone
    bg[:, :, 2] = tone
    bg[:, :, 3] = 255
    checker = Image.fromarray(bg, "RGBA")
    checker.alpha_composite(image)
    return checker


def loaded_paint(nodes):
    return [layer for layer in paint_layers(nodes) if layer.raster is not None]
