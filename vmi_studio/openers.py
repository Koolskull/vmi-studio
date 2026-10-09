"""Open a drawing into one layer tree with paint attached."""

import os
import sys
import tempfile

from vmi_studio.composite import blend_summary
from vmi_studio.document import ArtFile, Ids, Node, Raster, refresh
from vmi_studio import kra, vmib, xcf


def clip2krita_src():
    """Directory that contains the clip2krita package.

    CLIP2KRITA_SRC wins. Otherwise the copy shipped in third_party is used,
    including inside a frozen executable.
    """
    env = os.environ.get("CLIP2KRITA_SRC")
    if env:
        return env
    if getattr(sys, "frozen", False):
        root = getattr(sys, "_MEIPASS", os.path.dirname(os.path.abspath(sys.executable)))
        return os.path.join(root, "third_party", "clip2krita", "src")
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.normpath(os.path.join(here, "..", "third_party", "clip2krita", "src"))


def open_drawing(path, progress=None):
    path = os.path.abspath(path)
    if not os.path.isfile(path):
        raise FileNotFoundError(path)
    kind = _kind(path)
    if progress:
        progress("Reading %s" % os.path.basename(path))
    if kind == "clip":
        art = _open_clip(path, progress)
    elif kind == "psd":
        art = _open_psd(path, "psd")
    elif kind == "kra":
        art = kra.load(path, progress=progress)
    elif kind == "xcf":
        art = xcf.load(path)
    elif kind == "vmib":
        art = vmib.load_blueprint(path, progress=progress)
    else:
        raise RuntimeError("Open a .clip, .psd, .psb, .kra, .xcf, or .vmib file.")
    extra = blend_summary(art.layers)
    if extra:
        art.note = ("%s %s" % (art.note or "", extra)).strip()
    return art


def _kind(path):
    ext = os.path.splitext(path)[1].lower()
    if ext == ".clip":
        return "clip"
    if ext in (".psd", ".psb"):
        return "psd"
    if ext == ".kra":
        return "kra"
    if ext == ".xcf":
        return "xcf"
    if ext == ".vmib":
        return "vmib"
    return ""


def _ensure_clip2krita():
    src = clip2krita_src()
    if src not in sys.path:
        sys.path.insert(0, src)
    existing = sys.modules.get("clip2krita")
    loaded = getattr(existing, "__file__", "") or ""
    if existing is not None and "convert" not in dir(existing):
        del sys.modules["clip2krita"]
    elif loaded and os.path.basename(loaded) == "clip2krita.py":
        del sys.modules["clip2krita"]


def _open_clip(path, progress):
    if progress:
        progress("Converting Clip Studio paint")
    _ensure_clip2krita()
    from clip2krita.convert import convert_clip_to_psd

    handle, dest = tempfile.mkstemp(prefix="vmi-studio-", suffix=".psd")
    os.close(handle)
    try:
        convert_clip_to_psd(path, dest, blank_preview=True)
        art = _open_psd(dest, "clip")
    finally:
        try:
            os.remove(dest)
        except OSError:
            pass
    art.file_name = os.path.basename(path)
    art.path = os.path.abspath(path)
    art.kind = "clip"
    art.note = "Clip Studio paint is loaded."
    try:
        from vmi_studio.vectorbake import apply_vector_previews

        if progress:
            progress("Reading vector layers")
        count = apply_vector_previews(art, path)
        if count:
            art.note = "%s %d vector layer%s." % (
                art.note, count, "" if count == 1 else "s",
            )
    except Exception as exc:
        sys.stderr.write("vector: %s\n" % exc)
    return art


def _open_psd(path, kind):
    _ensure_clip2krita()
    from clip2krita.psdstack import PsdError, load_stack

    try:
        root = load_stack(path, with_pixels=True)
    except PsdError as exc:
        raise RuntimeError(str(exc)) from exc
    ids = Ids()
    layers = [_from_psd(child, ids) for child in root.get("children") or []]
    note = "Every layer's paint is loaded."
    return ArtFile(
        os.path.basename(path),
        os.path.abspath(path),
        kind,
        root.get("width") or 0,
        root.get("height") or 0,
        refresh(layers),
        note,
    )


def _from_psd(raw, ids):
    kind = "group" if raw.get("kind") == "group" else "layer"
    raster = None
    record = raw.get("raster") if kind == "layer" else None
    if record and record.get("w") and record.get("h") and record.get("rgba") is not None:
        raster = Raster(record["x"], record["y"], record["w"], record["h"], record["rgba"])
    node = Node(
        ids.next(),
        raw.get("name") or ("group" if kind == "group" else "layer"),
        kind,
        visible=bool(raw.get("visible", True)),
        opacity=raw.get("opacity", 255),
        blend=raw.get("blend") or "normal",
        clip=bool(raw.get("clip")),
        raster=raster,
    )
    node.shapes = bool(raw.get("shapes", True))
    node.opacity = max(0, min(255, int(node.opacity)))
    node.children = [_from_psd(child, ids) for child in raw.get("children") or []]
    return node
