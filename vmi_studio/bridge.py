"""Leaf rasters for the Rust desk.

The window asks Python to open a drawing it cannot read itself:

    python -m vmi_studio.bridge open FILE --manifest PATH --rasters DIR

Each leaf layer with paint gets a tight RGBA file. Strokes are copied
through as JSON, including any extra trait keys the file already stored.
"""

import argparse
import json
import os
import sys


def _safe(ident):
    text = "".join(ch if ch.isalnum() or ch in "-_" else "_" for ch in str(ident)) or "layer"
    return text


def _plain(value):
    if isinstance(value, tuple):
        return [_plain(item) for item in value]
    if isinstance(value, list):
        return [_plain(item) for item in value]
    if isinstance(value, dict):
        return {str(key): _plain(item) for key, item in value.items()}
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return str(value)


def _node(node, rasters, used):
    row = {
        "id": node.id,
        "name": node.name,
        "kind": node.kind,
        "visible": bool(node.visible),
        "opacity": int(node.opacity),
        "blend": node.blend or "normal",
        "clip": bool(node.clip),
        "mute": bool(getattr(node, "mute", False)),
        "solo": bool(getattr(node, "solo", False)),
        "marker": getattr(node, "marker", "") or "",
        "color": getattr(node, "color", "") or "",
        "vectors": _plain(getattr(node, "vectors", []) or []),
        "strokes": _plain(getattr(node, "strokes", []) or []),
        "children": [_node(child, rasters, used) for child in node.children],
    }
    raster = getattr(node, "raster", None)
    if node.kind != "group" and raster is not None and getattr(raster, "rgba", None):
        width = int(raster.w)
        height = int(raster.h)
        data = bytes(raster.rgba)
        if width > 0 and height > 0 and len(data) >= width * height * 4:
            name = _safe(node.id) + ".rgba"
            number = 2
            while name in used:
                name = "%s_%d.rgba" % (_safe(node.id), number)
                number += 1
            used.add(name)
            with open(os.path.join(rasters, name), "wb") as handle:
                handle.write(data[: width * height * 4])
            row["raster"] = {
                "file": name,
                "x": int(raster.x),
                "y": int(raster.y),
                "w": width,
                "h": height,
            }
    return row


def dump_art(art, manifest, rasters):
    """Write one manifest and one RGBA file per painted leaf."""

    rasters = os.path.abspath(rasters)
    manifest = os.path.abspath(manifest)
    os.makedirs(rasters, exist_ok=True)
    parent = os.path.dirname(manifest)
    if parent:
        os.makedirs(parent, exist_ok=True)
    used = set()
    document = {
        "width": int(art.width),
        "height": int(art.height),
        "file_name": art.file_name or "",
        "path": art.path or "",
        "rasters_dir": rasters,
        "layers": [_node(node, rasters, used) for node in art.layers],
    }
    with open(manifest, "w", encoding="utf-8") as handle:
        json.dump(document, handle)


def main(argv=None):
    parser = argparse.ArgumentParser(prog="vmi_studio.bridge")
    sub = parser.add_subparsers(dest="command", required=True)
    open_cmd = sub.add_parser("open")
    open_cmd.add_argument("file")
    open_cmd.add_argument("--manifest", required=True)
    open_cmd.add_argument("--rasters", required=True)
    args = parser.parse_args(argv)
    if not os.path.isfile(args.file):
        print("That file is missing.", file=sys.stderr)
        return 1
    try:
        from vmi_studio.openers import open_drawing

        art = open_drawing(args.file)
        dump_art(art, args.manifest, args.rasters)
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
