"""Objects, frames, and the positions those layers export to."""

from vmi_studio.document import animation_folders, find_node, layer_index, paint_layers, panel_order, timeline_cels, view_of
from vmi_studio.naming import folder_name, is_vague_name, parse_sf, sanitize, slot_key, state_label


_counter = 1


def reset_object_ids(start=1):
    global _counter
    _counter = start


def _next_object_id():
    global _counter
    ident = "o%d" % _counter
    _counter += 1
    return ident


# Assigned on the object outliner before export. Empty kind follows the layers.
OBJECT_KINDS = ("static", "button", "sequence", "warp target")
WARP_LAYERS = ("mask", "warp map")
# Beetle Game battlefx ids. Empty means this object triggers no sound.
SOUND_EFFECTS = (
    ("sfx:chord1", "Chord 1"),
    ("sfx:chord2", "Chord 2"),
    ("sfx:chord3", "Chord 3"),
    ("sfx:chord4", "Chord 4"),
    ("sfx:hit1", "Hit 1"),
    ("sfx:hit2", "Hit 2"),
    ("sfx:hit3", "Hit 3"),
    ("sfx:hit4", "Hit 4"),
    ("sfx:hit5", "Hit 5"),
    ("sfx:ring1", "Ring 1"),
    ("sfx:ring2", "Ring 2"),
    ("sfx:ring3", "Ring 3"),
    ("sfx:rise1", "Rise 1"),
    ("sfx:rise2", "Rise 2"),
    ("sfx:tik", "Tik"),
)
SOUND_IDS = tuple(ident for ident, _label in SOUND_EFFECTS)
# Beetle Game Webamp has these two lists. Empty leaves the player alone.
PLAYLISTS = ("menu", "battle")


def clean_kind(value):
    text = value or ""
    return text if text in OBJECT_KINDS else ""


def clean_warp_layer(value):
    text = value or ""
    return text if text in WARP_LAYERS else ""


def clean_sound(value):
    text = value or ""
    return text if text in SOUND_IDS else ""


def clean_playlist(value):
    text = value or ""
    return text if text in PLAYLISTS else ""


class DeskObject:
    def __init__(self, ident, name, layer_ids, assign=None, origin="", labels=None, explicit=False, parent="", stack=0, role="", kind="", warp_layer="", sound=""):
        self.id = ident
        self.name = name
        self.layer_ids = list(layer_ids)
        self.assign = dict(assign or {})
        self.origin = origin or ""
        self.labels = dict(labels or {})
        self.explicit = bool(explicit)
        self.parent = parent or ""
        self.stack = int(stack or 0)
        self.role = role or ""
        self.kind = clean_kind(kind)
        self.warp_layer = clean_warp_layer(warp_layer)
        self.sound = clean_sound(sound)


def position_of(layer, obj):
    saved = obj.assign.get(layer.id)
    if saved is None:
        return parse_sf(layer.name)
    return int(saved[0]) & 0xFFF, int(saved[1]) & 0xFFF


def slots_of(art, obj):
    order = layer_index(art)
    buckets = {}
    for ident in obj.layer_ids:
        layer = find_node(art.layers, ident)
        if not layer or layer.kind != "layer" or layer.omit:
            continue
        state, frame = position_of(layer, obj)
        key = slot_key(state, frame)
        slot = buckets.get(key)
        if slot is None:
            slot = {"key": key, "state": state, "frame": frame, "layers": []}
            buckets[key] = slot
        slot["layers"].append(layer)
    slots = list(buckets.values())
    for slot in slots:
        slot["layers"].sort(key=lambda layer: order.get(layer.id, 0))
    slots.sort(key=lambda slot: slot["key"])
    return slots


def object_view(art, obj):
    views = []
    for ident in obj.layer_ids:
        layer = find_node(art.layers, ident)
        if not layer:
            continue
        view = view_of(layer.group)
        if view and view not in views:
            views.append(view)
    if len(views) == 1:
        return views[0]
    return ""


def object_type(art, obj):
    slots = slots_of(art, obj)
    if any(slot["state"] != 0 for slot in slots):
        return "button"
    if any(slot["frame"] != 0 for slot in slots):
        return "sequence"
    return "static"


def assigned_type(art, obj):
    """The type chosen on the object outliner, or the one the layers imply."""
    kind = clean_kind(getattr(obj, "kind", ""))
    if kind:
        return kind
    return object_type(art, obj)


def object_z(art, obj):
    order = layer_index(art)
    found = [order[ident] for ident in obj.layer_ids if ident in order]
    return min(found) if found else 0


def _unique_name(name, taken):
    base = folder_name(name)
    if base.lower() not in taken:
        return base
    number = 2
    while ("%s %d" % (base, number)).lower() in taken:
        number += 1
    return "%s %d" % (base, number)


def _paintable(node):
    return node.kind == "layer" and not node.omit and node.visible


def static_layers(node):
    """Visible layers in this folder. Hidden and reference names stay out."""
    if node is None:
        return []
    return [layer for layer in paint_layers([node]) if _paintable(layer)]


def static_asset_folder(node):
    """True when the folder is already one rest picture, 000000."""
    if node is None or node.kind != "group" or node.animation or node.omit:
        return False
    layers = static_layers(node)
    if not layers:
        return False
    for layer in layers:
        state, frame = parse_sf(layer.name)
        if state or frame:
            return False
    return True


def _subfolder_position(name):
    """A subfolder called hover, or door#hover, or wave@f2, already has a position."""
    from vmi_studio.naming import FRAME_RE, LABEL_TO_STATE, STATE_RE
    if STATE_RE.search(name or "") or FRAME_RE.search(name or ""):
        return parse_sf(name)
    token = (name or "").strip().lower()
    if token in LABEL_TO_STATE:
        return LABEL_TO_STATE[token], 0
    return None


def slots_from_subfolders(folder):
    """Loose layers stay on 000000. Each subfolder is the next position.

    A subfolder named hover, pressed, or @f2 keeps that position. The others
    take the next free rest, hover, pressed, and after slots, top to bottom.
    """
    layers = []
    assign = {}
    if folder is None:
        return layers, assign
    pending = []
    for child in panel_order(folder.children):
        if child.kind == "layer":
            if not _paintable(child):
                continue
            layers.append(child)
            assign[child.id] = (0, 0)
            continue
        if child.kind != "group" or child.omit or not child.visible:
            continue
        paints = [layer for layer in paint_layers([child]) if _paintable(layer)]
        if not paints:
            continue
        named = _subfolder_position(child.name)
        if named is not None:
            state, frame = named
            for layer in paints:
                layers.append(layer)
                assign[layer.id] = (state, frame)
        else:
            pending.append(paints)
    taken = set(assign.values())
    state = 0
    for paints in pending:
        while (state, 0) in taken:
            state += 1
        for layer in paints:
            layers.append(layer)
            assign[layer.id] = (state, 0)
        taken.add((state, 0))
        state += 1
    return layers, assign


def frames_of(folder):
    """Each direct child is one frame. A child folder, subfolders included, is one picture."""
    state, _frame = parse_sf(folder.name)
    layer_ids = []
    assign = {}
    frame = 0
    for cel in timeline_cels(folder):
        if not cel.visible:
            continue
        layers = paint_layers([cel]) if cel.kind == "group" else [cel]
        used = False
        for layer in layers:
            if layer.kind != "layer" or layer.omit:
                continue
            layer_ids.append(layer.id)
            assign[layer.id] = (state & 0xFFF, frame & 0xFFF)
            used = True
        if used:
            frame += 1
    return layer_ids, assign


def animation_objects(art):
    """One sprite per animation name. Frames stay in timeline order, full canvas, not cropped."""
    buckets = {}
    order = []
    for folder in animation_folders(art.layers):
        if folder.omit:
            continue
        ids, assign = frames_of(folder)
        if not ids:
            continue
        stem = sanitize(folder.name)
        slot = buckets.get(stem)
        if slot is None:
            slot = {"ids": [], "assign": {}}
            buckets[stem] = slot
            order.append(stem)
        slot["ids"].extend(ids)
        slot["assign"].update(assign)
    objects = []
    taken = set()
    for stem in order:
        slot = buckets[stem]
        name = _unique_name(stem, taken)
        taken.add(name.lower())
        objects.append(DeskObject(
            _next_object_id(), name, slot["ids"], slot["assign"], origin="anim:%s" % stem,
        ))
    return objects


def merge_animations(art, objects):
    """Refresh animation sprites and leave hand-made objects in place."""
    fresh = animation_objects(art)
    fresh_ids = {ident for obj in fresh for ident in obj.layer_ids}
    previous = {obj.origin: obj for obj in objects if obj.origin}
    kept = []
    for obj in objects:
        if obj.origin:
            continue
        remain = [ident for ident in obj.layer_ids if ident not in fresh_ids]
        if not remain:
            continue
        if remain != list(obj.layer_ids):
            assign = {ident: pos for ident, pos in obj.assign.items() if ident in set(remain)}
            obj = DeskObject(obj.id, obj.name, remain, assign)
        kept.append(obj)
    for obj in fresh:
        prev = previous.get(obj.origin)
        if prev:
            obj.id = prev.id
            obj.name = prev.name
        kept.append(obj)
    return kept


def read_names(art):
    """One object per stem. door and door#hover become one object with two positions."""
    taken = set()
    objects = []
    covered = set()
    for obj in animation_objects(art):
        objects.append(obj)
        covered.update(obj.layer_ids)
        taken.add(obj.name.lower())
    groups = {}
    for layer in paint_layers(art.layers):
        if layer.omit or layer.id in covered:
            continue
        stem = sanitize(layer.name)
        groups.setdefault(stem, []).append(layer.id)
    for stem, layer_ids in groups.items():
        name = _unique_name(stem, taken)
        taken.add(name.lower())
        objects.append(DeskObject(_next_object_id(), name, layer_ids))
    objects.sort(key=lambda obj: object_z(art, obj))
    return objects


def extra_frames(art, obj):
    stems = {sanitize(obj.name)}
    for ident in obj.layer_ids:
        layer = find_node(art.layers, ident)
        if layer:
            stems.add(sanitize(layer.name))
    have = set(obj.layer_ids)
    return [
        layer for layer in paint_layers(art.layers)
        if not layer.omit and layer.id not in have and sanitize(layer.name) in stems
    ]


def label_of(obj, state, frame):
    """The name written for one position. A custom name wins. Otherwise the button word."""
    key = slot_key(state, frame)
    custom = (getattr(obj, "labels", None) or {}).get(key)
    if isinstance(custom, str) and custom.strip():
        return custom.strip()
    return state_label(state)


def slot_rows(art, obj):
    """Button positions first, then any other named or assigned position."""
    by_key = {slot["key"]: slot for slot in slots_of(art, obj)}
    head = [slot_key(state, 0) for state in (0, 1, 2, 3)]
    extra = []
    seen = set(head)
    for key in list(by_key) + list(getattr(obj, "labels", {}) or {}):
        if key in seen or not isinstance(key, str) or len(key) != 6:
            continue
        seen.add(key)
        extra.append(key)
    extra.sort()
    rows = []
    for key in head + extra:
        state = int(key[:3], 16)
        frame = int(key[3:], 16)
        slot = by_key.get(key)
        layers = [layer.name for layer in slot["layers"]] if slot else []
        rows.append({
            "key": key,
            "state": state,
            "frame": frame,
            "label": label_of(obj, state, frame),
            "layers": layers,
        })
    return rows


def object_from_layers(layers, name, taken_names):
    used = {item.lower() for item in taken_names}
    ids = [layer.id for layer in layers if layer.kind == "layer"]
    return DeskObject(_next_object_id(), _unique_name(name, used), ids)


def apply_frames(obj, layer_ids, state, frame, mode, ordered):
    have = set(obj.layer_ids)
    nxt = list(obj.layer_ids)
    for ident in layer_ids:
        if ident not in have:
            have.add(ident)
            nxt.append(ident)
    assign = dict(obj.assign)
    if mode == "stack":
        for ident in layer_ids:
            assign[ident] = (state & 0xFFF, frame & 0xFFF)
    else:
        rank = {layer.id: index for index, layer in enumerate(ordered)}
        ordered_ids = sorted(layer_ids, key=lambda ident: rank.get(ident, 0))
        for index, ident in enumerate(ordered_ids):
            assign[ident] = (state & 0xFFF, (frame + index) & 0xFFF)
    made = DeskObject(
        obj.id, obj.name, nxt, assign, obj.origin, obj.labels, obj.explicit,
        parent=getattr(obj, "parent", ""), stack=getattr(obj, "stack", 0),
        role=getattr(obj, "role", ""), kind=getattr(obj, "kind", ""),
        warp_layer=getattr(obj, "warp_layer", ""), sound=getattr(obj, "sound", ""),
    )
    return made


def vague_object(art, obj):
    if not is_vague_name(obj.name):
        return False
    for ident in obj.layer_ids:
        layer = find_node(art.layers, ident)
        if layer and not is_vague_name(layer.name):
            return False
    return True


def origin_is_animation(origin):
    """Timelines are anim: and folder:. A static folder export is not one."""
    text = origin or ""
    return text.startswith("anim:") or text.startswith("folder:")


def paint_order(objects):
    """Back to front. A child stacks in front of its parent. Low stack is the back."""
    by_id = {obj.id: obj for obj in objects}
    kids = {}
    roots = []
    for obj in objects:
        parent = getattr(obj, "parent", "") or ""
        if parent in by_id and parent != obj.id:
            kids.setdefault(parent, []).append(obj)
        else:
            roots.append(obj)

    def rank(obj):
        return (int(getattr(obj, "stack", 0) or 0), obj.id)

    for group in kids.values():
        group.sort(key=rank)
    roots.sort(key=rank)
    ordered = []

    def walk(group, ancestors):
        for obj in group:
            if obj.id in ancestors:
                continue
            ordered.append(obj)
            walk(kids.get(obj.id, []), ancestors | {obj.id})

    walk(roots, set())
    return ordered


def scene_objects(objects):
    """Every object, in the order the VMI scene draws them. Nothing is dropped."""
    ordered = list(paint_order(objects))
    seen = {obj.id for obj in ordered}
    for obj in objects:
        if obj.id not in seen:
            ordered.append(obj)
            seen.add(obj.id)
    return ordered


def set_scene_order(objects, ordered_ids):
    """The list is the scene. The first id is behind. A drag clears nesting."""
    by_id = {obj.id: obj for obj in objects}
    seen = set()
    stack = 0
    for ident in ordered_ids or []:
        obj = by_id.get(ident)
        if obj is None or ident in seen:
            continue
        obj.parent = ""
        obj.stack = stack
        stack += 1
        seen.add(ident)
    for obj in objects:
        if obj.id in seen:
            continue
        obj.parent = ""
        obj.stack = stack
        stack += 1


def scene_plate(art, objects, max_edge=480):
    """One still of the export. Back to front, each object's rest picture.

    The paint is already at the preview size. A radio-station canvas is not
    built at full resolution just to be thrown away.
    """
    from PIL import Image

    from vmi_studio.composite import composite_image

    width = int(getattr(art, "width", 0) or 0)
    height = int(getattr(art, "height", 0) or 0)
    rows = []
    if width <= 0 or height <= 0:
        return None, rows
    edge = int(max_edge) if max_edge else 0
    plate_edge = edge if edge > 0 else None
    long_edge = max(width, height)
    if plate_edge and long_edge > plate_edge:
        scale = float(plate_edge) / float(long_edge)
        pw = max(1, int(round(width * scale)))
        ph = max(1, int(round(height * scale)))
    else:
        pw, ph = width, height
    plate = Image.new("RGBA", (pw, ph), (0, 0, 0, 0))
    for obj in scene_objects(objects):
        slots = slots_of(art, obj)
        chosen = None
        for slot in slots:
            if slot["key"] == "000000":
                chosen = slot
                break
        if chosen is None and slots:
            chosen = slots[0]
        image = composite_image(width, height, chosen["layers"], max_edge=plate_edge) if chosen else None
        if image is not None:
            if image.size != (pw, ph):
                image = image.resize((pw, ph), Image.Resampling.NEAREST)
            plate.alpha_composite(image.convert("RGBA"))
        rows.append({"name": obj.name, "painted": image is not None})
    field = Image.new("RGBA", plate.size, (0, 0, 0, 255))
    field.alpha_composite(plate)
    return field, rows


def arrange_objects(objects, rows):
    """Apply the outliner. rows is visual order, the top row in front.

    Each row is {"id", "children"}. An object dropped onto another becomes its child.
    """
    by_id = {obj.id: obj for obj in objects}
    seen = set()

    def walk(group, parent, ancestors):
        count = len(group)
        for index, row in enumerate(group):
            ident = row.get("id")
            obj = by_id.get(ident)
            if obj is None or ident in seen or ident in ancestors:
                continue
            obj.parent = parent or ""
            obj.stack = count - 1 - index
            seen.add(ident)
            walk(row.get("children") or [], ident, ancestors | {ident})

    walk(rows or [], "", set())
    for obj in objects:
        if obj.id not in seen:
            obj.parent = ""


def plan_scene(art, objects, playlist=""):
    taken = set()
    folders = {}
    drafted = []
    for obj in scene_objects(objects):
        folder = _unique_name(obj.name, taken)
        taken.add(folder.lower())
        folders[obj.id] = folder
        slots = []
        for slot in slots_of(art, obj):
            slots.append({
                "key": slot["key"],
                "state": slot["state"],
                "frame": slot["frame"],
                "label": label_of(obj, slot["state"], slot["frame"]),
                "layers": [layer.name for layer in slot["layers"]],
                "nodes": slot["layers"],
            })
        drafted.append((obj, folder, slots))
    planned = []
    for index, (obj, folder, slots) in enumerate(drafted):
        parent = getattr(obj, "parent", "") or ""
        planned.append({
            "id": obj.id,
            "name": obj.name,
            "folder": folder,
            "type": assigned_type(art, obj),
            "zIndex": index,
            "parent": folders.get(parent, ""),
            "view": object_view(art, obj),
            "animation": origin_is_animation(obj.origin),
            "warpLayer": clean_warp_layer(getattr(obj, "warp_layer", "")),
            "sound": clean_sound(getattr(obj, "sound", "")),
            "labels": {row["key"]: row["label"] for row in slot_rows(art, obj)},
            "slots": slots,
        })
    scene = folder_name(os_stem(art.file_name))
    return {
        "scene": scene,
        "canvas": {"w": art.width, "h": art.height},
        "source": {"file": art.file_name, "kind": art.kind},
        "playlist": clean_playlist(playlist),
        "objects": planned,
    }


def os_stem(file_name):
    base = os_path_stem(file_name)
    return base or "scene"


def os_path_stem(file_name):
    name = file_name or ""
    dot = name.rfind(".")
    if dot > 0:
        name = name[:dot]
    return name
