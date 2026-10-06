"""VMI blueprint files (.vmib).

A blueprint is a zip. It keeps the layer arrangement, the clipping masks,
and the object list, plus the paint those layers already hold. Opening one
skips the Clip Studio, Photoshop, Krita, and GIMP readers.

The zip is the file a later 2kool.tv upload would send. This module does
not upload anything. The cloud key in the manifest only reserves that step.
"""

import json
import os
import zipfile

from vmi_studio.desk import (
    DeskObject,
    clean_kind,
    clean_playlist,
    clean_sound,
    clean_warp_layer,
)
from vmi_studio.document import ArtFile, Node, Raster, refresh


FORMAT = "vmib"
VERSION = 1
MIME = "application/x-vmi-blueprint"
CLOUD = {
    "service": "2kool.tv",
    "upload": "later",
    "note": (
        "One zip per project. A later upload sends this file to the signed-in "
        "user on 2kool.tv. The studio opens that same zip."
    ),
}


def _source(art):
    project = getattr(art, "project", None) or {}
    if art.kind == FORMAT and project.get("source_file"):
        return project.get("source_file") or "", project.get("source_kind") or ""
    if art.kind == FORMAT:
        return "", ""
    return art.file_name or "", art.kind or ""


def _raster_file(ident, used):
    safe = "".join(ch if ch.isalnum() or ch in "-_" else "_" for ch in str(ident)) or "layer"
    name = "rasters/%s.rgba" % safe
    number = 2
    while name in used:
        name = "rasters/%s_%d.rgba" % (safe, number)
        number += 1
    used.add(name)
    return name


def _layer_row(node, used, blobs):
    row = {
        "id": node.id,
        "name": node.name,
        "kind": node.kind,
        "visible": bool(node.visible),
        "opacity": int(node.opacity),
        "blend": node.blend or "normal",
        "clip": bool(node.clip),
        "shapes": bool(getattr(node, "shapes", True)),
        "mute": bool(node.mute),
        "solo": bool(node.solo),
        "animation": bool(node.animation),
        "timeline": list(node.timeline or []),
        "warp": getattr(node, "warp", "") or "",
        "raster": None,
        "children": [_layer_row(child, used, blobs) for child in node.children],
    }
    raster = node.raster
    if node.kind == "layer" and raster is not None and raster.w > 0 and raster.h > 0:
        if isinstance(raster.rgba, bytes):
            data = raster.rgba
        else:
            data = bytes(raster.rgba)
        expect = raster.w * raster.h * 4
        if len(data) != expect:
            raise RuntimeError("%s has a broken picture." % node.name)
        filename = _raster_file(node.id, used)
        blobs.append((filename, data))
        row["raster"] = {
            "x": int(raster.x),
            "y": int(raster.y),
            "w": int(raster.w),
            "h": int(raster.h),
            "file": filename,
        }
    return row


def _object_row(obj):
    return {
        "id": obj.id,
        "name": obj.name,
        "layer_ids": list(obj.layer_ids),
        "assign": {ident: [int(pos[0]) & 0xFFF, int(pos[1]) & 0xFFF] for ident, pos in obj.assign.items()},
        "origin": obj.origin or "",
        "labels": {str(key): str(value) for key, value in (obj.labels or {}).items() if str(value).strip()},
        "explicit": bool(obj.explicit),
        "parent": obj.parent or "",
        "stack": int(obj.stack or 0),
        "role": obj.role or "",
        "kind": clean_kind(obj.kind),
        "warp_layer": clean_warp_layer(obj.warp_layer),
        "sound": clean_sound(obj.sound),
    }


def prepare_blueprint(art, objects, playlist="", arrange_locked=True):
    """JSON document plus the raster blobs. The blobs alias the layer bytes."""
    if art is None:
        raise RuntimeError("Open a drawing first.")
    blobs = []
    used = set()
    source_file, source_kind = _source(art)
    document = {
        "format": FORMAT,
        "version": VERSION,
        "name": os.path.splitext(art.file_name or "project")[0],
        "width": int(art.width),
        "height": int(art.height),
        "playlist": clean_playlist(playlist),
        "arrange_locked": bool(arrange_locked),
        "source": {"file": source_file, "kind": source_kind},
        "cloud": dict(CLOUD),
        "layers": [_layer_row(node, used, blobs) for node in art.layers],
        "objects": [_object_row(obj) for obj in objects or []],
    }
    return document, blobs


def write_blueprint(path, art, objects, playlist="", arrange_locked=True):
    document, blobs = prepare_blueprint(art, objects, playlist, arrange_locked)
    write_parts(path, document, blobs)
    return document


def write_parts(path, document, blobs):
    """Zip a prepared blueprint. Raster entries are deflated once, not twice."""
    parent = os.path.dirname(os.path.abspath(path)) or "."
    os.makedirs(parent, exist_ok=True)
    temporary = path + ".saving"
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=1) as archive:
            mime = zipfile.ZipInfo("mimetype")
            mime.compress_type = zipfile.ZIP_STORED
            archive.writestr(mime, MIME)
            archive.writestr(
                "blueprint.json",
                json.dumps(document, ensure_ascii=False, separators=(",", ":")).encode("utf-8"),
            )
            for name, data in blobs:
                archive.writestr(name, data)
        os.replace(temporary, path)
    except Exception:
        try:
            os.remove(temporary)
        except OSError:
            pass
        raise


def _read_json(archive):
    try:
        raw = archive.read("blueprint.json")
    except KeyError as exc:
        raise RuntimeError("This blueprint has no project.") from exc
    try:
        document = json.loads(raw.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise RuntimeError("This blueprint could not be read.") from exc
    if not isinstance(document, dict) or document.get("format") != FORMAT:
        raise RuntimeError("This is not a VMI blueprint.")
    version = document.get("version")
    if version != VERSION:
        raise RuntimeError("This blueprint is from a newer VMI STUDIO.")
    return document


def _raster_from(archive, row):
    raster = row.get("raster")
    if not raster:
        return None
    if not isinstance(raster, dict):
        raise RuntimeError("A layer picture is missing.")
    filename = raster.get("file") or ""
    if not filename.startswith("rasters/") or ".." in filename.replace("\\", "/"):
        raise RuntimeError("A layer picture is missing.")
    try:
        data = archive.read(filename)
    except KeyError as exc:
        raise RuntimeError("A layer picture is missing.") from exc
    width = int(raster.get("w") or 0)
    height = int(raster.get("h") or 0)
    if width < 1 or height < 1 or len(data) != width * height * 4:
        raise RuntimeError("A layer picture is the wrong size.")
    return Raster(int(raster.get("x") or 0), int(raster.get("y") or 0), width, height, data)


def _node_from(row, archive, seen):
    if not isinstance(row, dict):
        raise RuntimeError("This blueprint has a broken layer.")
    ident = str(row.get("id") or "")
    if not ident or ident in seen:
        raise RuntimeError("This blueprint has a broken layer.")
    seen.add(ident)
    kind = "group" if row.get("kind") == "group" else "layer"
    node = Node(
        ident,
        row.get("name") or ("group" if kind == "group" else "layer"),
        kind,
        visible=bool(row.get("visible", True)),
        opacity=max(0, min(255, int(row.get("opacity", 255)))),
        blend=row.get("blend") or "normal",
        clip=bool(row.get("clip")),
        raster=_raster_from(archive, row) if kind == "layer" else None,
        animation=bool(row.get("animation")),
    )
    node.shapes = bool(row.get("shapes", True))
    node.mute = bool(row.get("mute"))
    node.solo = bool(row.get("solo"))
    node.warp = row.get("warp") or ""
    node.children = [_node_from(child, archive, seen) for child in row.get("children") or []]
    kids = {child.id for child in node.children}
    node.timeline = [ident for ident in row.get("timeline") or [] if ident in kids]
    return node


def _objects_from(rows, known):
    objects = []
    seen = set()
    for row in rows or []:
        if not isinstance(row, dict):
            continue
        ident = str(row.get("id") or "")
        if not ident or ident in seen:
            continue
        seen.add(ident)
        ids = [item for item in row.get("layer_ids") or [] if item in known]
        assign = {}
        for item, pos in (row.get("assign") or {}).items():
            if item in known and isinstance(pos, (list, tuple)) and len(pos) >= 2:
                assign[item] = (int(pos[0]) & 0xFFF, int(pos[1]) & 0xFFF)
        labels = {}
        for key, value in (row.get("labels") or {}).items():
            if isinstance(key, str) and isinstance(value, str) and value.strip():
                labels[key] = value.strip()
        objects.append(DeskObject(
            ident,
            row.get("name") or "object",
            ids,
            assign,
            row.get("origin") or "",
            labels,
            explicit=bool(row.get("explicit", True)),
            parent=row.get("parent") or "",
            stack=int(row.get("stack") or 0),
            role=row.get("role") or "",
            kind=row.get("kind") or "",
            warp_layer=row.get("warp_layer") or "",
            sound=row.get("sound") or "",
        ))
    return objects


def load_blueprint(path, progress=None):
    path = os.path.abspath(path)
    if progress:
        progress("Reading %s" % os.path.basename(path))
    try:
        archive = zipfile.ZipFile(path, "r")
    except zipfile.BadZipFile as exc:
        raise RuntimeError("This blueprint could not be read.") from exc
    with archive:
        document = _read_json(archive)
        seen = set()
        layers = [_node_from(row, archive, seen) for row in document.get("layers") or []]
        layers = refresh(layers)
        known = set()

        def collect(nodes):
            for node in nodes:
                known.add(node.id)
                collect(node.children)

        collect(layers)
        objects = _objects_from(document.get("objects"), known)
    source = document.get("source") if isinstance(document.get("source"), dict) else {}
    art = ArtFile(
        os.path.basename(path),
        path,
        FORMAT,
        int(document.get("width") or 0),
        int(document.get("height") or 0),
        layers,
        "VMI blueprint. The arrangement and the objects are in this file.",
    )
    art.project = {
        "objects": objects,
        "playlist": clean_playlist(document.get("playlist")),
        "arrange_locked": bool(document.get("arrange_locked", True)),
        "source_file": source.get("file") or "",
        "source_kind": source.get("kind") or "",
    }
    return art
