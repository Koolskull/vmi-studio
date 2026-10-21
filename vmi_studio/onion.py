"""Onion skin for the picture only.

Past cels are a purple tone. Future cels are a green tone. The current cel
stays its own color. Nothing here is written into a layer, and export does
not call this.
"""

import numpy as np
from PIL import Image

from vmi_studio.composite import composite_image
from vmi_studio.document import animation_folders, cel_at, frame_map

PAST = (176, 64, 255)
FUTURE = (40, 210, 90)
_STRENGTH = (0.55, 0.34, 0.20)
_LIMIT = 3


def next_frame_slot(frames, playhead):
    """Where a new cel goes so the drawing under the playhead stays in the past.

    A playhead that already has a key steps forward. A later key moves only
    when it occupies that new slot.
    """
    frames = [int(frame) for frame in frames]
    playhead = int(playhead)
    target = playhead + 1 if playhead in frames else playhead
    if target in frames:
        frames = [frame + 1 if frame >= target else frame for frame in frames]
    return target, frames


def neighbors(folder, frame):
    """Past and future cels, nearest first, and the cel the playhead is showing."""
    frame = int(frame)
    past = []
    future = []
    keyed = None
    for at, cel in frame_map(folder):
        at = int(at)
        if at < frame:
            past.append(cel)
        elif at > frame:
            future.append(cel)
        else:
            keyed = cel
    current = keyed if keyed is not None else cel_at(folder, frame)
    current_id = getattr(current, "id", None)
    if current_id:
        past = [cel for cel in past if cel.id != current_id]
        future = [cel for cel in future if cel.id != current_id]
    past = list(reversed(past))[:_LIMIT]
    future = future[:_LIMIT]
    return past, future, current


def apply_onion(scene, width, height, layers, frame, max_edge):
    """Tint neighbor cels over the preview where the current cel is clear."""
    if scene is None or not layers:
        return scene
    folders = [folder for folder in animation_folders(layers) if getattr(folder, "frames", None)]
    if not folders:
        return scene
    plate = np.array(scene.convert("RGBA"), dtype=np.uint8, copy=True)
    changed = False
    for folder in folders:
        past, future, current = neighbors(folder, frame)
        if not past and not future:
            continue
        onion = _stack(width, height, past, future, max_edge, plate.shape)
        if onion is None or onion.shape != plate.shape:
            continue
        cover = _cover(width, height, current, max_edge, plate.shape)
        plate = _mask(plate, onion, cover)
        changed = True
    if not changed:
        return scene
    return Image.fromarray(plate, "RGBA")


def _strength(distance):
    distance = int(distance)
    if distance < 0:
        distance = 0
    if distance >= len(_STRENGTH):
        return _STRENGTH[-1]
    return _STRENGTH[distance]


def _ordered(cels, tone):
    count = len(cels)
    rows = []
    for index, cel in enumerate(reversed(cels)):
        rows.append((cel, tone, _strength(count - 1 - index)))
    return rows


def _stack(width, height, past, future, max_edge, shape):
    canvas = None
    for cel, tone, strength in _ordered(future, FUTURE) + _ordered(past, PAST):
        shot = composite_image(width, height, [cel], max_edge=max_edge)
        if shot is None:
            continue
        tinted = _tint(shot, tone, strength)
        if tinted.shape != shape:
            continue
        canvas = tinted if canvas is None else _over(canvas, tinted)
    return canvas


def _tint(image, tone, strength):
    src = np.asarray(image.convert("RGBA"), dtype=np.float32)
    out = np.zeros_like(src)
    out[:, :, 0] = float(tone[0])
    out[:, :, 1] = float(tone[1])
    out[:, :, 2] = float(tone[2])
    out[:, :, 3] = src[:, :, 3] * float(strength)
    return out


def _over(base, top):
    ta = top[:, :, 3:4] / 255.0
    ba = base[:, :, 3:4] / 255.0
    out_a = ta + ba * (1.0 - ta)
    rgb = top[:, :, :3] * ta + base[:, :, :3] * ba * (1.0 - ta)
    rgb = np.where(out_a > 1e-6, rgb / np.maximum(out_a, 1e-6), 0.0)
    out = np.empty_like(base)
    out[:, :, :3] = rgb
    out[:, :, 3:4] = out_a * 255.0
    return out


def _cover(width, height, cel, max_edge, shape):
    blank = np.zeros(shape[:2], np.float32)
    if cel is None:
        return blank
    shot = composite_image(width, height, [cel], max_edge=max_edge)
    if shot is None:
        return blank
    alpha = np.asarray(shot.convert("RGBA"), dtype=np.float32)[:, :, 3] / 255.0
    if alpha.shape != shape[:2]:
        return blank
    return alpha


def _mask(scene, onion, cover):
    mix = (1.0 - cover)[:, :, None] * (onion[:, :, 3:4] / 255.0)
    base = scene.astype(np.float32)
    out = base.copy()
    out[:, :, :3] = onion[:, :, :3] * mix + base[:, :, :3] * (1.0 - mix)
    return np.clip(np.rint(out), 0, 255).astype(np.uint8)
