"""Layer tree. Children are stored back to front: index 0 is the back of the picture."""

from vmi_studio.naming import is_omitted_name


class Raster:
    def __init__(self, x, y, w, h, rgba):
        self.x = int(x)
        self.y = int(y)
        self.w = int(w)
        self.h = int(h)
        self.rgba = rgba


class Node:
    def __init__(self, id, name, kind, visible=True, opacity=255, blend="normal", clip=False, raster=None, source_key="", animation=False):
        self.id = id
        self.name = name
        self.kind = kind
        self.visible = bool(visible)
        self.opacity = opacity
        self.blend = blend or "normal"
        self.clip = bool(clip)
        self.raster = raster
        self.source_key = source_key or ""
        self.children = []
        self.omit = ""
        self.group = ""
        self.animation = bool(animation)
        self.timeline = []
        self.mute = False
        self.solo = False
        self.warp = ""


class ArtFile:
    def __init__(self, file_name, path, kind, width, height, layers, note=""):
        self.file_name = file_name
        self.path = path
        self.kind = kind
        self.width = int(width)
        self.height = int(height)
        self.layers = layers
        self.note = note
        self.project = None


class Ids:
    def __init__(self):
        self.n = 0

    def next(self):
        ident = "n%d" % self.n
        self.n += 1
        return ident


def clamp_opacity(value):
    try:
        number = int(round(float(value)))
    except (TypeError, ValueError):
        return 255
    return max(0, min(255, number))


def refresh(nodes):
    """Recompute omit reasons and folder paths after a rename, a hide, or a move."""

    def visit(lst, group, ancestor_hidden, ancestor_ref):
        for node in lst:
            ref = ancestor_ref or is_omitted_name(node.name)
            if ancestor_hidden:
                node.omit = "hidden folder"
            elif not node.visible:
                node.omit = "hidden"
            elif ref:
                node.omit = "reference name"
            else:
                node.omit = ""
            node.group = group
            hidden_folder = ancestor_hidden or (node.kind == "group" and not node.visible)
            if node.kind == "group":
                child_group = "%s/%s" % (group, node.name) if group else node.name
            else:
                child_group = group
            visit(node.children, child_group, hidden_folder, ref)

    visit(nodes, "", False, False)
    return nodes


def find_node(nodes, ident):
    for node in nodes:
        if node.id == ident:
            return node
        found = find_node(node.children, ident)
        if found:
            return found
    return None


def paint_layers(nodes):
    out = []

    def visit(lst):
        for node in lst:
            if node.kind == "group":
                visit(node.children)
            else:
                out.append(node)

    visit(nodes)
    return out


def panel_order(nodes):
    return list(reversed(nodes))


def layer_index(art):
    return {layer.id: index for index, layer in enumerate(paint_layers(art.layers))}


def view_of(group):
    for part in (group or "").split("/"):
        if part.lower() == "exterior":
            return "Exterior"
        if part.lower() == "interior":
            return "Interior"
    return ""


def walk(nodes):
    for node in nodes:
        yield node
        yield from walk(node.children)


def visibility_map(nodes):
    return {node.id: bool(node.visible) for node in walk(nodes)}


def apply_visibility(nodes, flags):
    if not flags:
        return nodes
    for node in walk(nodes):
        if node.id in flags:
            node.visible = bool(flags[node.id])
    return refresh(nodes)


def rename_node(nodes, ident, name):
    trimmed = (name or "").strip()
    if not trimmed:
        return nodes
    node = find_node(nodes, ident)
    if node:
        node.name = trimmed
    return refresh(nodes)


def set_visible(nodes, ident, visible):
    node = find_node(nodes, ident)
    if node:
        node.visible = bool(visible)
    return refresh(nodes)


def set_mute(nodes, ident, mute):
    node = find_node(nodes, ident)
    if node:
        node.mute = bool(mute)
    return nodes


def set_solo(nodes, ident, solo):
    node = find_node(nodes, ident)
    if node:
        node.solo = bool(solo)
    return nodes


def set_clip(nodes, ident, clip):
    node = find_node(nodes, ident)
    if node:
        node.clip = bool(clip)
    return nodes


def apply_clips(nodes, clip_ids):
    """Session clip flags. Missing ids are not clipping masks."""
    chosen = set(clip_ids or [])
    for node in walk(nodes):
        node.clip = node.id in chosen
    return nodes


def track_ids(nodes, flag):
    return [node.id for node in walk(nodes) if getattr(node, flag, False)]


def apply_blends(nodes, rows):
    """Put session blend choices back onto the same layers."""
    if not rows:
        return nodes
    by_id = {node.id: node for node in walk(nodes)}
    for row in rows:
        if not isinstance(row, dict):
            continue
        node = by_id.get(row.get("id"))
        if node is None:
            continue
        blend = row.get("blend")
        if isinstance(blend, str) and blend.strip():
            node.blend = blend.strip()
        if "shapes" in row:
            node.shapes = bool(row["shapes"])
    return nodes


def blend_rows(nodes):
    return [
        {
            "id": node.id,
            "blend": node.blend or "normal",
            "shapes": bool(getattr(node, "shapes", True)),
        }
        for node in walk(nodes)
    ]


def apply_track_flags(nodes, mute_ids, solo_ids):
    muted = set(mute_ids or [])
    soloed = set(solo_ids or [])
    for node in walk(nodes):
        node.mute = node.id in muted
        node.solo = node.id in soloed
    return nodes


def gate_of(visible, mute, solo, ancestor_hidden, ancestor_muted, ancestor_solo, solo_on):
    """drop skips the row and everything inside it. seek keeps looking for a soloed child. in can paint."""
    hidden = ancestor_hidden or not visible
    muted = ancestor_muted or bool(mute)
    soloed = ancestor_solo or bool(solo)
    if hidden or muted:
        return "drop"
    if solo_on and not soloed:
        return "seek"
    return "in"


def paint_gate(node, ancestor_hidden, ancestor_muted, ancestor_solo, solo_on):
    return gate_of(
        node.visible,
        getattr(node, "mute", False),
        getattr(node, "solo", False),
        ancestor_hidden,
        ancestor_muted,
        ancestor_solo,
        solo_on,
    )


def capture_flags(nodes):
    """Copy hide, mute, and solo so a click during the picture build cannot tear it."""
    return {
        node.id: (
            bool(node.visible),
            bool(getattr(node, "mute", False)),
            bool(getattr(node, "solo", False)),
        )
        for node in walk(nodes)
    }


def _solo_on(nodes):
    return any(getattr(node, "solo", False) for node in walk(nodes))


def _contains_solo(node):
    if getattr(node, "solo", False):
        return True
    return any(_contains_solo(child) for child in node.children)


def animation_folders(nodes):
    """Marked folders that are not already a frame inside another animation."""
    found = []

    def visit(lst, under):
        for node in lst:
            marked = node.kind == "group" and node.animation and not under
            if marked:
                found.append(node)
            visit(node.children, under or marked)

    visit(nodes, False)
    return found


def timeline_cels(node):
    """Direct children in timeline order. The top row is frame 0 until that order is moved."""
    kids = list(panel_order(node.children))
    if not node.timeline:
        return kids
    by_id = {child.id: child for child in kids}
    ordered = [by_id[ident] for ident in node.timeline if ident in by_id]
    seen = {child.id for child in ordered}
    for child in kids:
        if child.id not in seen:
            ordered.append(child)
    return ordered


def move_cel(node, cel_id, delta):
    """Move one frame along the timeline. The layer stack stays where it is."""
    order = [cel.id for cel in timeline_cels(node)]
    if cel_id not in order:
        return False
    index = order.index(cel_id)
    nxt = index + delta
    if nxt < 0 or nxt >= len(order):
        return False
    order[index], order[nxt] = order[nxt], order[index]
    node.timeline = order
    return True


def _holds(node, ident):
    for cel in node.children:
        if cel.id == ident:
            return True
        if cel.kind == "group" and find_node(cel.children, ident):
            return True
    return False


def owning_animation(nodes, ident):
    def visit(lst):
        for node in lst:
            if node.kind == "group" and node.animation:
                if node.id == ident or _holds(node, ident):
                    return node
            found = visit(node.children)
            if found:
                return found
        return None

    return visit(nodes)


def cel_of(folder, ident):
    if not folder or ident == folder.id:
        return None
    for cel in timeline_cels(folder):
        if cel.id == ident:
            return cel
        if cel.kind == "group" and find_node(cel.children, ident):
            return cel
    return None


def set_animation(nodes, ident, on):
    """Mark a folder as a timeline. A folder already inside one stays a frame."""
    node = find_node(nodes, ident)
    if node is None or node.kind != "group":
        return None
    owner = owning_animation(nodes, ident)
    if on and owner is not None and owner.id != ident:
        return owner
    node.animation = bool(on)
    if on:
        for desc in walk(node.children):
            desc.animation = False
            desc.timeline = []
        node.timeline = [cel.id for cel in panel_order(node.children)]
    else:
        node.timeline = []
    return node


def apply_animations(nodes, rows):
    if not rows:
        return nodes
    for row in rows:
        if not isinstance(row, dict):
            continue
        node = find_node(nodes, row.get("id"))
        if node is None or node.kind != "group":
            continue
        node.animation = True
        kids = {child.id for child in node.children}
        node.timeline = [ident for ident in row.get("timeline") or [] if ident in kids]
    return nodes


def preview_paint(nodes, focus_id=None):
    """The visible stack, with one cel showing from each animation folder.

    Mute and solo change what is in the picture. A muted cel stays on the
    timeline and contributes no paint. Solo brings that track forward.
    """
    solo_on = _solo_on(nodes)
    roots = {folder.id for folder in animation_folders(nodes)}
    out = []

    def visit(lst, ancestor_hidden, ancestor_muted, ancestor_solo):
        for node in lst:
            gate = paint_gate(node, ancestor_hidden, ancestor_muted, ancestor_solo, solo_on)
            if gate == "drop":
                continue
            child_solo = ancestor_solo or bool(node.solo)
            if node.kind == "group" and node.id in roots:
                cels = [cel for cel in timeline_cels(node) if cel.visible]
                if solo_on and not child_solo:
                    cels = [cel for cel in cels if cel.solo or _contains_solo(cel)]
                if not cels:
                    continue
                chosen = cels[0]
                if focus_id and focus_id != node.id:
                    for cel in cels:
                        holds = cel.id == focus_id or (cel.kind == "group" and find_node(cel.children, focus_id))
                        if holds:
                            chosen = cel
                            break
                chosen_gate = paint_gate(chosen, False, False, child_solo, solo_on)
                if chosen_gate == "drop":
                    continue
                if chosen.kind == "group":
                    visit(chosen.children, False, False, child_solo or bool(chosen.solo))
                elif chosen_gate == "in":
                    out.append(chosen)
                continue
            if node.kind == "group":
                visit(node.children, False, False, child_solo)
            elif gate == "in":
                out.append(node)

    visit(nodes, False, False, False)
    return out


def _next_id(nodes):
    highest = -1
    for node in walk(nodes):
        if node.id.startswith("n") and node.id[1:].isdigit():
            highest = max(highest, int(node.id[1:]))
    return "n%d" % (highest + 1)


def _blank_folder(ident):
    return Node(ident, "Folder", "group")


def _topmost(nodes, ids):
    top = set()

    def visit(lst, under):
        for node in lst:
            hit = node.id in ids
            if hit and not under:
                top.add(node.id)
            visit(node.children, under or hit)

    visit(nodes, False)
    return top


def _collect(nodes, ids):
    out = []

    def visit(lst):
        for node in lst:
            if node.id in ids:
                out.append(node)
            else:
                visit(node.children)

    visit(nodes)
    return out


def _visual_first(nodes, ids):
    def visit(lst):
        for node in panel_order(lst):
            if node.id in ids:
                return node.id
            found = visit(node.children)
            if found:
                return found
        return None

    return visit(nodes)


def _is_ancestor(nodes, ancestor_id, ident):
    ancestor = find_node(nodes, ancestor_id)
    return bool(ancestor and find_node(ancestor.children, ident))


def _rewrite(nodes, remove, rebuild):
    def visit(lst):
        out = []
        for node in lst:
            if node.id in remove:
                continue
            node.children = visit(node.children)
            out.append(rebuild(node))
        return out

    return visit(nodes)


def _add_empty(nodes, parent_id):
    folder = _blank_folder(_next_id(nodes))
    if not parent_id:
        return list(nodes) + [folder], folder.id
    parent = find_node(nodes, parent_id)
    if parent:
        parent.children = list(parent.children) + [folder]
    return refresh(nodes), folder.id


def _place_folder(nodes, remove, anchor_id, folder):
    parked = {"yes": False}

    def visit(lst):
        out = []
        for node in lst:
            if node.id in remove:
                if node.id == anchor_id:
                    out.append(folder)
                    parked["yes"] = True
                continue
            node.children = visit(node.children)
            out.append(node)
        return out

    nxt = visit(nodes)
    if parked["yes"]:
        return nxt
    return list(nxt) + [folder]


def new_folder(nodes, context_id, picked_ids):
    context = find_node(nodes, context_id) if context_id else None
    picked = [ident for ident in picked_ids if ident != context_id and find_node(nodes, ident)]
    if context and context.kind == "group" and not picked:
        layers, focus = _add_empty(nodes, context.id)
        return layers, focus
    ids = set(picked)
    if context and context.kind != "group":
        ids.add(context.id)
    if not ids:
        layers, focus = _add_empty(nodes, None)
        return layers, focus
    top = _topmost(nodes, ids)
    taken = _collect(nodes, top)
    anchor = _visual_first(nodes, top)
    folder = _blank_folder(_next_id(nodes))
    folder.children = taken
    placed = _place_folder(nodes, top, anchor, folder)
    return refresh(placed), folder.id


def _containing(nodes, ident):
    if any(node.id == ident for node in nodes):
        return nodes
    for node in walk(nodes):
        if any(child.id == ident for child in node.children):
            return node.children
    return None


def _plan(nodes, ids, target_id, place):
    """The nodes to move, back to front, and the row they move against."""
    if place not in ("before", "after", "into"):
        return None
    target = find_node(nodes, target_id)
    if target is None:
        return None
    if place == "into" and target.kind != "group":
        return None
    known = [ident for ident in ids if find_node(nodes, ident)]
    top = _topmost(nodes, set(known))
    if not top or target_id in top:
        return None
    for ident in top:
        node = find_node(nodes, ident)
        if node is not None and find_node(node.children, target_id):
            return None
    visual = []

    def walk_front(lst):
        for node in panel_order(lst):
            if node.id in top:
                visual.append(node)
            else:
                walk_front(node.children)

    walk_front(nodes)
    if not visual:
        return None
    return list(reversed(visual)), target


def can_arrange(nodes, ids, target_id, place):
    return _plan(nodes, ids, target_id, place) is not None


def arrange(nodes, ids, target_id, place):
    """Move layers and folders. before is in front of the row, after is behind it, into is inside a folder.

    Children stay stored back to front. The picture paints in that order.
    """
    plan = _plan(nodes, ids, target_id, place)
    if plan is None:
        return None
    block, target = plan
    moving = {node.id for node in block}

    def detach(lst):
        kept = []
        for node in lst:
            if node.id in moving:
                continue
            node.children = detach(node.children)
            kept.append(node)
        return kept

    layers = detach(list(nodes))
    if place == "into":
        target.children = list(target.children) + block
    else:
        parent = _containing(layers, target.id)
        if parent is None:
            layers.extend(block)
        else:
            index = next(item for item, node in enumerate(parent) if node.id == target.id)
            at = index + 1 if place == "before" else index
            parent[at:at] = block
    return refresh(layers), block[-1].id


def move_into(nodes, ids, folder_id):
    folder = find_node(nodes, folder_id)
    if not folder or folder.kind != "group":
        return None
    wanted = []
    for ident in ids:
        if ident == folder_id or not find_node(nodes, ident):
            continue
        if find_node(folder.children, ident):
            continue
        if _is_ancestor(nodes, ident, folder_id):
            continue
        wanted.append(ident)
    if not wanted:
        return None
    top = _topmost(nodes, set(wanted))
    taken = _collect(nodes, top)

    def rebuild(node):
        if node.id == folder_id:
            node.children = list(node.children) + taken
        return node

    layers = _rewrite(nodes, top, rebuild)
    return refresh(layers), folder_id
