"""KOSV on the VMI desk.

The picture is a VMI blueprint. Effects and presets live in that file.
The working plate is 1920 by 1080. 3840 by 2160 is the later plate.
A new stack is empty. A preset fills it.
"""

import numpy as np
from PIL import Image


PRODUCT = "KOSV"
PLATE = (1920, 1080)
LATER_PLATE = (3840, 2160)

_PARAM = {
    "amount": (0.0, 1.0, 0.35),
    "spread": (0.0, 0.25, 0.04),
    "cx": (0.0, 1.0, 0.5),
    "cy": (0.0, 1.0, 0.5),
    "bend_r": (0.5, 1.5, 0.96),
    "bend_g": (0.5, 1.5, 1.0),
    "bend_b": (0.5, 1.5, 1.04),
    "blur": (0.0, 32.0, 6.0),
    "threshold": (0.0, 1.0, 0.7),
    "chroma": (0.0, 1.0, 0.25),
    "size": (1.0, 32.0, 2.0),
    "seed": (0.0, 65535.0, 1.0),
    "shadow": (0.0, 1.0, 0.65),
    "scale": (0.25, 4.0, 1.0),
    "mix": (0.0, 1.0, 0.5),
    "softness": (0.0, 1.0, 0.15),
    "invert": (0.0, 1.0, 0.0),
    "amplitude": (0.0, 0.2, 0.02),
    "frequency": (0.0, 1.0, 0.5),
    "scanline": (0.0, 0.1, 0.01),
    "tear": (0.0, 1.0, 0.15),
    "shift": (0.0, 0.05, 0.008),
    "bar": (0.0, 1.0, 0.4),
    "pitch": (0.05, 1.5, 0.35),
    "angle": (0.0, 3.14, 0.4),
    "gain": (0.0, 4.0, 1.0),
    "block": (1.0, 64.0, 8.0),
    "radius": (0.0, 32.0, 6.0),
    "color_r": (0.0, 1.0, 1.0),
    "color_g": (0.0, 1.0, 0.85),
    "color_b": (0.0, 1.0, 0.4),
    "quality": (1.0, 100.0, 18.0),
    "cutoff": (0.0, 1.0, 0.45),
    "order": (0.0, 5.0, 0.0),
    "rows": (0.0, 1080.0, 12.0),
    "filter": (0.0, 4.0, 1.0),
}

# id -> (label, parameter names)
CATALOG = (
    ("fisheye", "Fisheye", ("amount", "spread", "cx", "cy", "seed")),
    ("lens", "Lens", ("bend_r", "bend_g", "bend_b", "blur", "threshold", "cx", "cy")),
    ("filmic-noise", "Filmic noise", ("amount", "chroma", "size", "seed")),
    ("film-grain", "Film grain", ("amount", "size", "shadow", "seed")),
    ("grain-plate", "Grain plate", ("amount", "scale", "mix", "seed")),
    ("texture-clip", "Texture clip", ("threshold", "softness", "invert")),
    ("luma-transition", "Luma transition", ("softness", "invert")),
    ("jiggle", "Jiggle", ("amplitude", "frequency", "scanline", "seed")),
    ("vhs", "VHS", ("tear", "shift", "bar", "pitch", "angle", "seed")),
    ("datamosh", "Datamosh", ("gain", "block")),
    ("color-glow", "Color glow", ("threshold", "radius", "color_r", "color_g", "color_b", "mix")),
    ("jpeg-glitch", "JPEG glitch", ("quality", "block", "cutoff", "order", "seed")),
    ("png-glitch", "PNG glitch", ("filter", "rows", "seed")),
)

_BY_ID = {item[0]: item for item in CATALOG}
_FILTERS = ("none", "sub", "up", "average", "paeth")
_ORDERS = ((0, 1, 2), (0, 2, 1), (1, 0, 2), (1, 2, 0), (2, 0, 1), (2, 1, 0))
_REFS = ("plate", "shape", "clip", "matte", "other")

# Factory stacks. Names are ours. Each entry is effect id, then param overrides.
STACKS = {
    "lens": (("fisheye", {"amount": 0.45, "spread": 0.06}), ("lens", {"blur": 8})),
    "stock": (("filmic-noise", {"amount": 0.2}), ("film-grain", {"amount": 0.45})),
    "tape": (("vhs", {}), ("jiggle", {"amplitude": 0.015})),
    "melt": (("datamosh", {"gain": 1.2, "block": 16}),),
    "break": (("jpeg-glitch", {"quality": 12}), ("png-glitch", {"rows": 24})),
    "glow": (("color-glow", {}),),
}


def plate_size(later=False):
    """Working plate, or the later 4K plate."""
    return LATER_PLATE if later else PLATE


def effect_ids():
    return tuple(item[0] for item in CATALOG)


def make_effect(ident, params=None, preset="", enabled=True):
    """One effect entry. Unknown ids are refused."""
    if ident not in _BY_ID:
        raise KeyError(ident)
    clean = {}
    for name in _BY_ID[ident][2]:
        if params and name in params:
            clean[name] = _store_param(name, params[name])
    return {
        "id": ident,
        "enabled": bool(enabled),
        "preset": str(preset or ident),
        "params": clean,
    }


def make_stack(name):
    """A factory preset as a list of effect entries."""
    rows = []
    for ident, overrides in STACKS[name]:
        rows.append(make_effect(ident, overrides, preset=name))
    return rows


def clean_effects(raw):
    """Drop broken entries. A missing list is an empty stack."""
    if not isinstance(raw, list):
        return []
    out = []
    for row in raw:
        if not isinstance(row, dict):
            continue
        ident = row.get("id")
        if ident not in _BY_ID:
            continue
        params = {}
        given = row.get("params") if isinstance(row.get("params"), dict) else {}
        for name in _BY_ID[ident][2]:
            if name in given:
                params[name] = _store_param(name, given[name])
        for name in _REFS:
            if isinstance(given.get(name), str) and given[name].strip():
                params[name] = given[name].strip()
        out.append({
            "id": ident,
            "enabled": bool(row.get("enabled", True)),
            "preset": str(row.get("preset") or ""),
            "params": params,
        })
    return out


def clean_marks(raw, known=None):
    """I and P marks keyed by cel id. Unknown cels are dropped when known is set."""
    if not isinstance(raw, dict):
        return {}
    out = {}
    for key, row in raw.items():
        if not isinstance(key, str) or not key:
            continue
        if known is not None and key not in known:
            continue
        if not isinstance(row, dict):
            continue
        kind = row.get("type") if row.get("type") in ("I", "P") else "I"
        mark = {
            "type": kind,
            "reference": str(row.get("reference") or ""),
            "origin": str(row.get("origin") or ""),
        }
        # Omitted gain and block stay omitted. A stored 1 would hide the
        # effect's own gain, including a gain that is keyed on the timeline.
        if "gain" in row and row.get("gain") is not None:
            mark["gain"] = _unit("gain", row.get("gain"), 1.0)
        if "block" in row and row.get("block") is not None:
            mark["block"] = _unit("block", row.get("block"), 8.0)
        out[key] = mark
    return out


def clean_presets(raw):
    """User presets saved in the blueprint. Factory names can be saved too."""
    if not isinstance(raw, list):
        return []
    out = []
    seen = set()
    for row in raw:
        if not isinstance(row, dict):
            continue
        name = str(row.get("name") or "").strip()
        if not name or name in seen:
            continue
        stack = clean_effects(row.get("stack"))
        if not stack:
            continue
        seen.add(name)
        out.append({"name": name, "stack": stack})
    return out


def clean_kosv(raw):
    """The blueprint block. Missing means an empty KOSV section."""
    raw = raw if isinstance(raw, dict) else {}
    plate = raw.get("plate")
    if (
        isinstance(plate, (list, tuple))
        and len(plate) >= 2
        and int(plate[0]) > 0
        and int(plate[1]) > 0
    ):
        size = [int(plate[0]), int(plate[1])]
    else:
        size = [PLATE[0], PLATE[1]]
    return {
        "product": PRODUCT,
        "plate": size,
        "later_plate": [LATER_PLATE[0], LATER_PLATE[1]],
        "presets": clean_presets(raw.get("presets")),
    }


def apply_effects(image, effects, frame=0, maps=None):
    """Run an effect stack. The source image is not modified.

    maps may hold 'plate', 'shape', 'clip', 'matte', 'other' as PIL images,
    plus 'frames', 'marks', and 'index' for datamosh.
    """
    effects = [row for row in clean_effects(effects) if row.get("enabled", True)]
    if image is None or not effects:
        return image
    base = image if image.mode == "RGBA" else image.convert("RGBA")
    arr = np.asarray(base).astype(np.float32)
    maps = maps or {}
    frame = int(frame or 0)
    for effect in effects:
        arr = _run(effect, arr, frame, maps)
    return Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), "RGBA")


def _run(effect, arr, frame, maps):
    ident = effect["id"]
    got = lambda name, default=None: _at(effect, name, frame, default)
    if ident == "fisheye":
        return _fisheye(arr, got("amount"), got("spread"), got("cx"), got("cy"))
    if ident == "lens":
        return _lens(arr, effect, frame, maps.get("shape"))
    if ident == "filmic-noise":
        return _noise(arr, got("amount"), got("chroma"), got("size"), got("seed"), frame, fine=True)
    if ident == "film-grain":
        return _noise(arr, got("amount"), 0.0, got("size"), got("seed"), frame, fine=False, shadow=got("shadow"))
    if ident == "grain-plate":
        return _grain_plate(arr, maps.get("plate"), got("amount"), got("scale"), got("mix"))
    if ident == "texture-clip":
        return _clip(arr, maps.get("clip"), got("threshold"), got("softness"), got("invert") >= 0.5)
    if ident == "luma-transition":
        return _luma(arr, maps.get("other"), maps.get("matte"), got("softness"), got("invert") >= 0.5)
    if ident == "jiggle":
        return _jiggle(arr, got("amplitude"), got("scanline"), got("seed"), frame)
    if ident == "vhs":
        return _vhs(arr, effect, frame)
    if ident == "datamosh":
        return _datamosh(arr, maps, got("gain"), got("block"))
    if ident == "color-glow":
        return _glow(arr, effect, frame)
    if ident == "jpeg-glitch":
        return _jpeg(arr, got("quality"), got("block"), got("cutoff"), got("order"), got("seed"), frame)
    if ident == "png-glitch":
        return _png(arr, int(round(got("filter"))), got("rows"), got("seed"), frame)
    return arr


def _store_param(name, raw):
    if isinstance(raw, dict) and isinstance(raw.get("keys"), list):
        keys = []
        for row in raw["keys"]:
            if not isinstance(row, dict):
                continue
            try:
                frame = int(row.get("frame"))
            except (TypeError, ValueError):
                continue
            keys.append({"frame": frame, "value": _unit(name, row.get("value"), _PARAM[name][2])})
        keys.sort(key=lambda item: item["frame"])
        return {"keys": keys}
    return _unit(name, raw, _PARAM[name][2])


def _unit(name, raw, default):
    low, high, fallback = _PARAM[name]
    if raw is None:
        raw = default if default is not None else fallback
    try:
        number = float(raw)
    except (TypeError, ValueError):
        number = fallback
    if number != number or number in (float("inf"), float("-inf")):
        number = fallback
    return max(low, min(high, number))


def _at(effect, name, frame, default=None):
    fallback = _PARAM[name][2] if default is None else default
    raw = (effect.get("params") or {}).get(name, fallback)
    if isinstance(raw, dict):
        keys = raw.get("keys") or []
        chosen = fallback
        for row in keys:
            if int(row["frame"]) <= int(frame):
                chosen = row["value"]
            else:
                break
        return _unit(name, chosen, fallback)
    return _unit(name, raw, fallback)


def _sample(channel, sx, sy):
    height, width = channel.shape
    x0 = np.floor(sx).astype(np.int32)
    y0 = np.floor(sy).astype(np.int32)
    wx = (sx - x0).astype(np.float32)
    wy = (sy - y0).astype(np.float32)

    def take(x, y):
        ok = (x >= 0) & (x < width) & (y >= 0) & (y < height)
        xc = np.clip(x, 0, width - 1)
        yc = np.clip(y, 0, height - 1)
        return np.where(ok, channel[yc, xc], 0.0)

    top = take(x0, y0) * (1 - wx) + take(x0 + 1, y0) * wx
    bot = take(x0, y0 + 1) * (1 - wx) + take(x0 + 1, y0 + 1) * wx
    return top * (1 - wy) + bot * wy


def _bend(arr, nx, ny, cx, cy, scale):
    height, width = arr.shape[:2]
    sx = (nx * scale + cx) * width - 0.5
    sy = (ny * scale + cy) * height - 0.5
    out = np.zeros_like(arr)
    for channel in range(4):
        out[..., channel] = _sample(arr[..., channel], sx, sy)
    return out


def _fisheye(arr, amount, spread, cx, cy):
    height, width = arr.shape[:2]
    ys, xs = np.mgrid[0:height, 0:width]
    nx = (xs + 0.5) / float(width) - cx
    ny = (ys + 0.5) / float(height) - cy
    radius = np.sqrt(nx * nx + ny * ny)
    factor = 1.0 + float(amount) * radius * radius
    out = np.zeros_like(arr)
    for channel, extra in enumerate((1.0 - spread, 1.0, 1.0 + spread, 1.0)):
        sx = (nx * factor * extra + cx) * width - 0.5
        sy = (ny * factor * extra + cy) * height - 0.5
        out[..., channel] = _sample(arr[..., channel], sx, sy)
    return out


def _lens(arr, effect, frame, shape):
    bent = _fisheye(
        arr,
        0.15,
        0.0,
        _at(effect, "cx", frame),
        _at(effect, "cy", frame),
    )
    # Wavelength bend: resample each color on its own scale around the center.
    height, width = bent.shape[:2]
    ys, xs = np.mgrid[0:height, 0:width]
    cx = _at(effect, "cx", frame)
    cy = _at(effect, "cy", frame)
    nx = (xs + 0.5) / float(width) - cx
    ny = (ys + 0.5) / float(height) - cy
    out = np.zeros_like(bent)
    for channel, name in enumerate(("bend_r", "bend_g", "bend_b")):
        out[..., channel] = _bend(bent, nx, ny, cx, cy, _at(effect, name, frame))[..., channel]
    out[..., 3] = bent[..., 3]
    blur = int(round(_at(effect, "blur", frame)))
    if blur < 1:
        return out
    luma = _luma_of(out)
    mask = luma >= _at(effect, "threshold", frame) * 255.0
    kernel = _kernel(shape, max(1, min(16, blur)))
    bloom = _stamp_mask(mask, kernel, out)
    return np.clip(out + bloom, 0, 255)


def _kernel(shape, size):
    size = max(1, int(size))
    if shape is None:
        yy, xx = np.mgrid[0:size, 0:size]
        circle = (xx - size / 2.0) ** 2 + (yy - size / 2.0) ** 2 <= (size / 2.0) ** 2
        kern = circle.astype(np.float32)
    else:
        plate = shape if shape.mode == "L" else shape.convert("L")
        plate = plate.resize((size, size), Image.Resampling.BOX)
        kern = np.asarray(plate).astype(np.float32) / 255.0
    total = float(kern.sum())
    if total <= 0:
        kern = np.ones((size, size), np.float32)
        total = float(kern.size)
    return kern / total


def _stamp_mask(mask, kernel, color):
    """Stamp the kernel on bright pixels. Stride keeps a 1080p plate from stalling."""
    height, width = mask.shape
    bloom = np.zeros_like(color)
    kh, kw = kernel.shape
    stride = max(1, kh // 2)
    ys, xs = np.nonzero(mask[::stride, ::stride])
    if len(ys) > 4000:
        ys = ys[:4000]
        xs = xs[:4000]
    for y, x in zip(ys, xs):
        py = int(y) * stride
        px = int(x) * stride
        y0 = py - kh // 2
        x0 = px - kw // 2
        tint = color[py, px]
        for ky in range(kh):
            iy = y0 + ky
            if iy < 0 or iy >= height:
                continue
            row = kernel[ky]
            for kx in range(kw):
                ix = x0 + kx
                if ix < 0 or ix >= width or row[kx] == 0:
                    continue
                bloom[iy, ix, :3] += tint[:3] * row[kx]
                bloom[iy, ix, 3] = max(bloom[iy, ix, 3], tint[3] * row[kx])
    return bloom


def _rng(seed, frame):
    return np.random.default_rng((int(seed) + int(frame) * 997) & 0xFFFFFFFF)


def _noise(arr, amount, chroma, size, seed, frame, fine, shadow=0.0):
    if amount <= 0:
        return arr
    height, width = arr.shape[:2]
    cell = max(1, int(round(size if not fine else max(1.0, size * 0.5))))
    rng = _rng(seed, frame)
    gh = max(1, (height + cell - 1) // cell)
    gw = max(1, (width + cell - 1) // cell)
    field = rng.standard_normal((gh, gw)).astype(np.float32)
    field = np.repeat(np.repeat(field, cell, axis=0), cell, axis=1)[:height, :width]
    out = arr.copy()
    luma = _luma_of(arr) / 255.0
    weight = (1.0 - luma) * float(shadow) + (1.0 - float(shadow))
    if fine:
        weight = 1.0
    gain = float(amount) * 40.0 * weight
    out[..., :3] += field[..., None] * gain
    if chroma > 0:
        tint = rng.standard_normal((gh, gw, 3)).astype(np.float32)
        tint = np.repeat(np.repeat(tint, cell, axis=0), cell, axis=1)[:height, :width]
        out[..., :3] += tint * (float(chroma) * 30.0)
    return out


def _fit(image, width, height):
    if image is None:
        return None
    plate = image if image.mode == "RGBA" else image.convert("RGBA")
    if plate.size != (width, height):
        plate = plate.resize((width, height), Image.Resampling.BOX)
    return np.asarray(plate).astype(np.float32)


def _grain_plate(arr, plate, amount, scale, mix):
    if plate is None or amount <= 0:
        return arr
    height, width = arr.shape[:2]
    thumb = plate if plate.mode == "RGBA" else plate.convert("RGBA")
    tw = max(1, int(round(thumb.size[0] * float(scale))))
    th = max(1, int(round(thumb.size[1] * float(scale))))
    thumb = thumb.resize((tw, th), Image.Resampling.BOX)
    tile = np.asarray(thumb).astype(np.float32)
    reps_y = int(np.ceil(height / float(th)))
    reps_x = int(np.ceil(width / float(tw)))
    tiled = np.tile(tile, (reps_y, reps_x, 1))[:height, :width]
    strength = float(amount) * float(mix)
    out = arr.copy()
    out[..., :3] = arr[..., :3] * (1.0 - strength) + tiled[..., :3] * strength
    return out


def _luma_of(arr):
    return arr[..., 0] * 0.2126 + arr[..., 1] * 0.7152 + arr[..., 2] * 0.0722


def _mask_from(arr, threshold, softness, invert):
    luma = _luma_of(arr) / 255.0
    if invert:
        luma = 1.0 - luma
    soft = max(0.001, float(softness))
    edge = float(threshold)
    mask = np.clip((luma - (edge - soft)) / soft, 0.0, 1.0)
    return mask.astype(np.float32)


def _clip(arr, texture, threshold, softness, invert):
    if texture is None:
        return arr
    fitted = _fit(texture, arr.shape[1], arr.shape[0])
    mask = _mask_from(fitted, threshold, softness, invert)
    out = arr.copy()
    out[..., 3] *= mask
    return out


def _luma(arr, other, matte, softness, invert):
    if other is None or matte is None:
        return arr
    height, width = arr.shape[:2]
    mix = _mask_from(_fit(matte, width, height), 0.5, softness, invert)[..., None]
    right = _fit(other, width, height)
    return arr * (1.0 - mix) + right * mix


def _jiggle(arr, amplitude, scanline, seed, frame):
    if amplitude <= 0 and scanline <= 0:
        return arr
    height, width = arr.shape[:2]
    rng = _rng(seed, frame)
    out = arr
    if amplitude > 0:
        dx = int(rng.integers(-max(1, int(amplitude * width)), max(2, int(amplitude * width) + 1)))
        dy = int(rng.integers(-max(1, int(amplitude * height)), max(2, int(amplitude * height) + 1)))
        out = np.roll(np.roll(arr, dy, axis=0), dx, axis=1)
    if scanline > 0:
        out = out.copy()
        limit = max(1, int(scanline * width))
        for y in range(0, height, 2):
            out[y] = np.roll(out[y], int(rng.integers(-limit, limit + 1)), axis=0)
    return out


def _vhs(arr, effect, frame):
    out = arr.copy()
    height, width = out.shape[:2]
    shift = int(round(_at(effect, "shift", frame) * width))
    if shift:
        out[..., 0] = np.roll(out[..., 0], shift, axis=1)
        out[..., 2] = np.roll(out[..., 2], -shift, axis=1)
    rng = _rng(_at(effect, "seed", frame), frame)
    tear = _at(effect, "tear", frame)
    if tear > 0:
        band = int(rng.integers(0, height))
        thick = max(1, int(tear * 8))
        slip = int(tear * width * 0.15)
        y0 = max(0, band - thick)
        y1 = min(height, band + thick)
        out[y0:y1] = np.roll(out[y0:y1], slip, axis=1)
    bar = _at(effect, "bar", frame)
    if bar > 0:
        y = int((frame * 17 + int(_at(effect, "seed", frame))) % max(1, height))
        out[y:y + 1, :, :3] *= 1.0 - bar
    pitch = _at(effect, "pitch", frame)
    angle = _at(effect, "angle", frame)
    xs = np.arange(width, dtype=np.float32)[None, :]
    ys = np.arange(height, dtype=np.float32)[:, None]
    wave = np.sin((xs * pitch + ys * angle) * 0.35) * np.sin((xs * pitch * 1.13 - ys * angle) * 0.35)
    out[..., :3] += wave[..., None] * (18.0 * max(pitch, 0.05))
    return out


def _block_mean(diff, block):
    block = max(1, int(round(block)))
    height, width = diff.shape[:2]
    out = diff.copy()
    hh = height - height % block
    ww = width - width % block
    if hh < block or ww < block:
        return out
    view = out[:hh, :ww].reshape(hh // block, block, ww // block, block, 4)
    mean = view.mean(axis=(1, 3), keepdims=True)
    out[:hh, :ww] = np.broadcast_to(mean, view.shape).reshape(hh, ww, 4)
    return out


def _datamosh(arr, maps, gain, block):
    frames = maps.get("frames") or []
    if len(frames) < 2:
        return arr
    marks = list(maps.get("marks") or [])
    index = int(maps.get("index") or 0)
    index = max(0, min(len(frames) - 1, index))
    while len(marks) < len(frames):
        marks.append({"type": "I", "reference": ""})
    mark = marks[index]
    # An I cel is the buffer already painted, so earlier effects in the stack
    # stay. Replacing it with the raw frame would throw those away.
    if mark.get("type") != "P":
        return arr
    ids = list(maps.get("ids") or [])
    last_i = index
    for cursor in range(index, -1, -1):
        if marks[cursor].get("type") != "P":
            last_i = cursor
            break

    def slot(name, fallback):
        named = str(mark.get(name) or "")
        if named and named in ids:
            return ids.index(named)
        return fallback

    # origin is the I this cel was differenced against. reference is the I it
    # plays on. They match on a clean frame. Point reference somewhere else,
    # or change gain, and the picture melts.
    origin_at = slot("origin", last_i)
    ref_at = slot("reference", origin_at)
    base = _fit(frames[ref_at], arr.shape[1], arr.shape[0])
    origin = _fit(frames[origin_at], arr.shape[1], arr.shape[0])
    # The residual is this buffer, not the raw cel. Fisheye-then-datamosh
    # keeps the fisheye. Origin and reference stay the other cels.
    residual = arr
    if base is None or origin is None:
        return arr
    diff = _block_mean(residual - origin, mark.get("block", block))
    return np.clip(base + diff * float(mark.get("gain", gain)), 0, 255)


def _glow(arr, effect, frame):
    luma = _luma_of(arr) / 255.0
    mask = np.clip((luma - _at(effect, "threshold", frame)) / 0.15, 0.0, 1.0)
    radius = int(round(_at(effect, "radius", frame)))
    if radius < 1 or float(mask.max()) <= 0:
        return arr
    soft = _box(mask, radius)
    color = np.array([
        _at(effect, "color_r", frame),
        _at(effect, "color_g", frame),
        _at(effect, "color_b", frame),
    ], dtype=np.float32) * 255.0
    mix = float(_at(effect, "mix", frame))
    out = arr.copy()
    out[..., :3] += color[None, None, :] * soft[..., None] * mix
    return out


def _box(mask, radius):
    radius = max(1, int(radius))
    padded = np.pad(mask, radius, mode="edge")
    integral = np.pad(padded, ((1, 0), (1, 0)), mode="constant").cumsum(0).cumsum(1)
    height, width = mask.shape
    y0 = np.arange(height)
    x0 = np.arange(width)
    y1 = y0 + 2 * radius + 1
    x1 = x0 + 2 * radius + 1
    total = (
        integral[y1[:, None], x1[None, :]]
        - integral[y0[:, None], x1[None, :]]
        - integral[y1[:, None], x0[None, :]]
        + integral[y0[:, None], x0[None, :]]
    )
    return total / float((2 * radius + 1) ** 2)


def _jpeg(arr, quality, block, cutoff, order, seed, frame):
    step = max(1.0, (100.0 - float(quality)) / 4.0)
    block = max(1, int(round(block)))
    if cutoff > 0.5:
        block = max(block, int(block * (1.0 + cutoff)))
    out = _block_mean(arr, block)
    out = np.round(out / step) * step
    perm = _ORDERS[int(round(order)) % len(_ORDERS)]
    # Seed picks an extra channel swap on some blocks so the frame can move.
    rng = _rng(seed, frame)
    if int(rng.integers(0, 2)) == 1 and order == 0:
        perm = _ORDERS[int(rng.integers(1, len(_ORDERS)))]
    rgb = out[..., :3]
    out = out.copy()
    out[..., 0] = rgb[..., perm[0]]
    out[..., 1] = rgb[..., perm[1]]
    out[..., 2] = rgb[..., perm[2]]
    return out


def _png(arr, filt, rows, seed, frame):
    kind = _FILTERS[int(filt) % len(_FILTERS)]
    if kind == "none" or rows <= 0:
        return arr
    out = arr.copy()
    height, width = out.shape[:2]
    count = max(1, min(height, int(round(rows))))
    chosen = _rng(seed, frame).choice(height, size=count, replace=False)
    rgb = out[..., :3]
    for y in chosen:
        if kind == "sub":
            acc = rgb[y, 0].astype(np.int16)
            for x in range(1, width):
                acc = (rgb[y, x].astype(np.int16) + acc) & 255
                rgb[y, x] = acc
        elif kind == "up" and y > 0:
            rgb[y] = (rgb[y].astype(np.int16) + rgb[y - 1].astype(np.int16)) & 255
        elif kind == "average":
            left = np.zeros_like(rgb[y])
            left[1:] = rgb[y, :-1]
            up = rgb[y - 1] if y > 0 else 0
            rgb[y] = (rgb[y].astype(np.int16) + ((left.astype(np.int16) + up) // 2)) & 255
        elif kind == "paeth" and y > 0:
            rgb[y, 1:] = (rgb[y, 1:].astype(np.int16) + rgb[y - 1, :-1].astype(np.int16)) & 255
    out[..., :3] = rgb
    return out
