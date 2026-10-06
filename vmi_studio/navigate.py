"""Picture navigation, the same gestures KOOLDRAW uses.

Scroll zooms. Space+drag and the middle button pan. Shift+drag rotates a
mouse (a pen still picks the layer under it). Pinch zooms and twists.
+ and - step by 1.25, the same amount as the KOOLDRAW zoom buttons.
"""

import math

ZOOM_MIN = 0.05
ZOOM_MAX = 64.0
ZOOM_STEP = 1.25
WHEEL_LIMIT = 1.35
ROTATE_PER_PIXEL = 0.4


def clamp_zoom(value):
    try:
        number = float(value)
    except (TypeError, ValueError):
        return 1.0
    if not math.isfinite(number) or number <= 0:
        return 1.0
    return min(ZOOM_MAX, max(ZOOM_MIN, number))


def wheel_factor(delta_y, delta_mode=0):
    """KOOLDRAW's canvas wheel. A positive delta_y zooms out."""
    try:
        dy = float(delta_y)
    except (TypeError, ValueError):
        return 1.0
    if not math.isfinite(dy) or dy == 0:
        return 1.0
    if delta_mode == 1:
        sensitivity = 0.08
    elif delta_mode == 2:
        sensitivity = 0.25
    else:
        sensitivity = 0.0018
    factor = math.exp(-dy * sensitivity)
    return min(WHEEL_LIMIT, max(1.0 / WHEEL_LIMIT, factor))


def _half_up(value):
    return int(math.floor(float(value) + 0.5))


def format_zoom(value):
    z = clamp_zoom(value)
    if z >= 10:
        return str(_half_up(z))
    if z >= 1:
        shown = _half_up(z * 10) / 10
        text = "%.1f" % shown
        return text[:-2] if text.endswith(".0") else text
    if z >= 0.1:
        return "%.2f" % (_half_up(z * 100) / 100)
    return "%.3f" % (_half_up(z * 1000) / 1000)


def gesture(button, space=False, shift=False, alt=False, ctrl=False, meta=False, pointer="mouse"):
    """What a press does. button is 0 left, 1 middle, 2 right.

    Right-click, Ctrl, and Meta are left alone (on a Mac, Ctrl-click is a
    right-click). Space or the middle button pans. Shift rotates a mouse.
    A pen never rotates. The left button picks the layer under the cursor.
    """
    if button == 2 or ctrl or meta:
        return "none"
    if space or button == 1:
        return "pan"
    if shift and not alt and pointer == "mouse" and button == 0:
        return "rotate"
    if button == 0:
        return "pick"
    return "none"


def view_to_image(vx, vy, view_w, view_h, image_w, image_h, scale, pan_x, pan_y, rotation):
    """Widget pixel to image pixel. None when the point misses the picture.

    The picture is centered, shifted by pan, then turned clockwise (y down).
    scale is how many screen pixels one image pixel covers.
    """
    if image_w <= 0 or image_h <= 0 or view_w <= 0 or view_h <= 0 or scale <= 0:
        return None
    dx = float(vx) - (float(view_w) / 2.0 + float(pan_x))
    dy = float(vy) - (float(view_h) / 2.0 + float(pan_y))
    rad = math.radians(float(rotation))
    cos = math.cos(rad)
    sin = math.sin(rad)
    ux = dx * cos + dy * sin
    uy = -dx * sin + dy * cos
    ix = ux / float(scale) + float(image_w) / 2.0
    iy = uy / float(scale) + float(image_h) / 2.0
    if ix < 0 or iy < 0 or ix >= image_w or iy >= image_h:
        return None
    return ix, iy
