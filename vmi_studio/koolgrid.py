"""The 2kool grid, measured from the Clip Studio template.

The template is 3840×3840. Paper is a separate layer. The grid art is a
3589×3589 block of 5px lines placed at (125, 125), which centers it.
Solid lines are #2CB0FF. The lighter guides are the same blue at alpha 158.
Coordinates below are in that 3589 block, before the placement offset.
"""

import numpy as np

from vmi_studio.document import Raster

PLATE = 3840
OFFSET = 125
ART = 3589
THICK = 5
BLUE = (44, 176, 255)
SOLID = 255
FAINT = 158

# (x, y, w, h, alpha) inside the 3589 block. Faint guides first, then solid.
_BLOCK = (
    (0, 1280, ART, THICK, FAINT),
    (0, 2304, ART, THICK, FAINT),
    (1280, 0, THICK, ART, FAINT),
    (2304, 0, THICK, ART, FAINT),
    (1280, 0, 1029, THICK, FAINT),
    (1280, 3584, 1029, THICK, FAINT),
    (0, 1280, THICK, 1029, FAINT),
    (3584, 1280, THICK, 1029, FAINT),
    (1152, 256, 1285, THICK, SOLID),
    (1024, 640, 1541, THICK, SOLID),
    (640, 1024, 2309, THICK, SOLID),
    (256, 1152, 3077, THICK, SOLID),
    (640, 1792, 2309, THICK, SOLID),
    (256, 2432, 3077, THICK, SOLID),
    (640, 2560, 2309, THICK, SOLID),
    (1024, 2944, 1541, THICK, SOLID),
    (1152, 3328, 1285, THICK, SOLID),
    (256, 1152, THICK, 1285, SOLID),
    (640, 1024, THICK, 1541, SOLID),
    (1024, 640, THICK, 2309, SOLID),
    (1152, 256, THICK, 3077, SOLID),
    (1792, 640, THICK, 2309, SOLID),
    (2432, 256, THICK, 3077, SOLID),
    (2560, 640, THICK, 2309, SOLID),
    (2944, 1024, THICK, 1541, SOLID),
    (3328, 1152, THICK, 1285, SOLID),
)


def _canvas_rects():
    """Line rectangles on the 3840 plate. Solid rows win over the guides."""
    placed = []
    for x, y, wide, high, alpha in _BLOCK:
        placed.append((x + OFFSET, y + OFFSET, wide, high, alpha))
    return placed


def pixel(x, y):
    """RGBA at one plate pixel. Outside a line this is empty."""
    ix = int(x) - OFFSET
    iy = int(y) - OFFSET
    if ix < 0 or iy < 0 or ix >= ART or iy >= ART:
        return (0, 0, 0, 0)
    found = 0
    for left, top, wide, high, alpha in _BLOCK:
        if left <= ix < left + wide and top <= iy < top + high:
            found = alpha
            if alpha == SOLID:
                break
    if found == 0:
        return (0, 0, 0, 0)
    return (BLUE[0], BLUE[1], BLUE[2], found)


def render_crop(x, y, width, height):
    """A small RGBA array. Tests use this instead of the full plate."""
    arr = np.zeros((int(height), int(width), 4), dtype=np.uint8)
    x0, y0 = int(x), int(y)
    for left, top, wide, high, alpha in _canvas_rects():
        col0 = max(left, x0)
        row0 = max(top, y0)
        col1 = min(left + wide, x0 + int(width))
        row1 = min(top + high, y0 + int(height))
        if col1 <= col0 or row1 <= row0:
            continue
        view = arr[row0 - y0:row1 - y0, col0 - x0:col1 - x0]
        view[..., 0] = BLUE[0]
        view[..., 1] = BLUE[1]
        view[..., 2] = BLUE[2]
        view[..., 3] = alpha
    return arr


def grid_raster():
    """The full 3840 plate. Only the 2kool preset asks for this."""
    arr = np.zeros((PLATE, PLATE, 4), dtype=np.uint8)
    for left, top, wide, high, alpha in _canvas_rects():
        view = arr[top:top + high, left:left + wide]
        view[..., 0] = BLUE[0]
        view[..., 1] = BLUE[1]
        view[..., 2] = BLUE[2]
        view[..., 3] = alpha
    return Raster(0, 0, PLATE, PLATE, np.ascontiguousarray(arr).tobytes())
