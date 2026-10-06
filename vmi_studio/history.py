"""Undo and redo for the open project.

A step remembers the layer tree, the object list, and the playlist.
Raster bytes stay shared until a cut replaces them. The old raster object
is what Undo puts back. Twenty steps is the limit, so a big cut does not
keep every earlier copy.
"""

from contextlib import contextmanager

from vmi_studio.document import refresh


LIMIT = 20
TAP_SLOP = 24
TAP_MS = 400


def _layer_props(node):
    return (
        node.name,
        node.kind,
        bool(node.visible),
        int(node.opacity),
        node.blend or "normal",
        bool(node.clip),
        bool(getattr(node, "shapes", True)),
        bool(node.mute),
        bool(node.solo),
        bool(node.animation),
        tuple(node.timeline or []),
        getattr(node, "warp", "") or "",
        node.raster,
    )


def _object_props(obj):
    assign = tuple(sorted(
        (ident, (int(pos[0]) & 0xFFF, int(pos[1]) & 0xFFF))
        for ident, pos in (obj.assign or {}).items()
        if isinstance(pos, (list, tuple)) and len(pos) >= 2
    ))
    labels = tuple(sorted(
        (str(key), str(value))
        for key, value in (obj.labels or {}).items()
        if str(value).strip()
    ))
    return (
        obj.name,
        tuple(obj.layer_ids),
        assign,
        obj.origin or "",
        labels,
        bool(obj.explicit),
        obj.parent or "",
        int(obj.stack or 0),
        obj.role or "",
        obj.kind or "",
        obj.warp_layer or "",
        obj.sound or "",
    )


class Snap:
    def __init__(self):
        self.skeleton = []
        self.nodes = {}
        self.props = {}
        self.objects = []
        self.object_nodes = {}
        self.object_props = {}
        self.playlist = ""

    def same(self, other):
        return (
            self.skeleton == other.skeleton
            and self.props == other.props
            and self.objects == other.objects
            and self.object_props == other.object_props
            and self.playlist == other.playlist
        )


def capture(window):
    snap = Snap()

    def visit(nodes):
        skeleton = []
        for node in nodes:
            snap.nodes[node.id] = node
            snap.props[node.id] = _layer_props(node)
            skeleton.append((node.id, visit(node.children)))
        return skeleton

    snap.skeleton = visit(window.art.layers)
    for obj in window.objects:
        snap.object_nodes[obj.id] = obj
        snap.object_props[obj.id] = _object_props(obj)
        snap.objects.append(obj.id)
    snap.playlist = window.playlist or ""
    return snap


def _build(skeleton, nodes):
    built = []
    for ident, kids in skeleton:
        node = nodes[ident]
        node.children = _build(kids, nodes)
        built.append(node)
    return built


def apply_snap(window, snap):
    for ident, props in snap.props.items():
        node = snap.nodes[ident]
        (
            name, kind, visible, opacity, blend, clip, shapes,
            mute, solo, animation, timeline, warp, raster,
        ) = props
        node.name = name
        node.kind = kind
        node.visible = visible
        node.opacity = opacity
        node.blend = blend
        node.clip = clip
        node.shapes = shapes
        node.mute = mute
        node.solo = solo
        node.animation = animation
        node.timeline = list(timeline)
        node.warp = warp
        node.raster = raster
    window.art.layers = _build(snap.skeleton, snap.nodes)
    refresh(window.art.layers)
    objects = []
    for ident in snap.objects:
        obj = snap.object_nodes[ident]
        (
            name, layer_ids, assign, origin, labels, explicit,
            parent, stack, role, kind, warp_layer, sound,
        ) = snap.object_props[ident]
        obj.name = name
        obj.layer_ids = list(layer_ids)
        obj.assign = {key: pos for key, pos in assign}
        obj.origin = origin
        obj.labels = {key: value for key, value in labels}
        obj.explicit = explicit
        obj.parent = parent
        obj.stack = stack
        obj.role = role
        obj.kind = kind
        obj.warp_layer = warp_layer
        obj.sound = sound
        objects.append(obj)
    window.objects = objects
    window.playlist = snap.playlist


class Edit:
    def __init__(self, label, before, after):
        self.label = label
        self.before = before
        self.after = after


class History:
    def __init__(self):
        self.undo_stack = []
        self.redo_stack = []
        self._before = None
        self._depth = 0
        self._busy = False

    def clear(self):
        self.undo_stack = []
        self.redo_stack = []
        self._before = None
        self._depth = 0

    @contextmanager
    def editing(self, window, label):
        if self._busy or window is None or getattr(window, "art", None) is None:
            yield
            return
        if self._depth == 0:
            self._before = capture(window)
        self._depth += 1
        try:
            yield
        finally:
            self._depth -= 1
            if self._depth == 0:
                self._commit(window, label)

    def _commit(self, window, label):
        before = self._before
        self._before = None
        if before is None or getattr(window, "art", None) is None:
            return
        after = capture(window)
        if before.same(after):
            return
        self.undo_stack.append(Edit(label, before, after))
        del self.undo_stack[:-LIMIT]
        self.redo_stack = []

    def remember(self, window, label, before):
        """Record one edit that already happened, such as a finished knob drag."""
        if self._busy or before is None or getattr(window, "art", None) is None:
            return
        after = capture(window)
        if before.same(after):
            return
        self.undo_stack.append(Edit(label, before, after))
        del self.undo_stack[:-LIMIT]
        self.redo_stack = []

    def undo(self, window):
        if self._busy or not self.undo_stack or getattr(window, "art", None) is None:
            return None
        edit = self.undo_stack.pop()
        self._busy = True
        try:
            apply_snap(window, edit.before)
        finally:
            self._busy = False
        self.redo_stack.append(edit)
        return edit.label

    def redo(self, window):
        if self._busy or not self.redo_stack or getattr(window, "art", None) is None:
            return None
        edit = self.redo_stack.pop()
        self._busy = True
        try:
            apply_snap(window, edit.after)
        finally:
            self._busy = False
        self.undo_stack.append(edit)
        return edit.label


class FingerChords:
    """Two-finger tap undoes. Three-finger tap redoes. A drag is a pinch, not a tap."""

    def __init__(self):
        self.down = {}
        self.max_count = 0
        self.moved = False
        self.pen = False
        self.started = None

    def reset(self):
        self.down = {}
        self.max_count = 0
        self.moved = False
        self.pen = False
        self.started = None

    def update(self, points, now_ms):
        """points are (id, x, y, down, finger). Returns 'undo', 'redo', or None."""
        for ident, x, y, down, finger in points:
            if not finger:
                self.pen = True
            if down:
                prev = self.down.get(ident)
                if prev is None:
                    self.down[ident] = (float(x), float(y))
                    if self.started is None:
                        self.started = now_ms
                else:
                    ox, oy = prev
                    if abs(float(x) - ox) + abs(float(y) - oy) > TAP_SLOP:
                        self.moved = True
            else:
                self.down.pop(ident, None)
        self.max_count = max(self.max_count, len(self.down))
        if self.down or self.started is None:
            return None
        result = None
        if not self.pen and not self.moved and now_ms - self.started <= TAP_MS:
            if self.max_count == 2:
                result = "undo"
            elif self.max_count == 3:
                result = "redo"
        self.reset()
        return result
