//! `.vmib` zip. Python reads `blueprint.json` and `rasters/`. Ink that is
//! separate from the vector cache is stored under `inks/` so a reload can
//! rebuild the strokes without painting them twice.
//!
//! The animation keys match `vmi_studio/vmib.py`: every layer row carries
//! `animation`, `timeline`, and `frames`; a layer with an exposure sheet
//! carries `exposure` (Python's `to_row`), the others omit the key; the
//! document carries `clip_time` (`name`, `fps`, `start`, `end`, `current`)
//! whenever the drawing has a cut to keep. Each raster is the layer's own
//! picture, composited without onion skin.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use serde_json::{json, Value};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::anim::{animation_folders, cut_end, ExposureSheet, DEFAULT_FPS};
use crate::color::hex_of;
use crate::coverage::{DitherPattern, Edge};
use crate::doc::{
    expand_raster, present_plain, tight_pixels, Art, ClipTime, DeskObject, Node, NodeKind,
};
use crate::kosv::{clean_effects, clean_kosv, clean_marks, effects_value, kosv_value, marks_value};
use crate::tasks::{colors_value, pack_colors, pack_tasks, tasks_value};
use crate::tool::{CurveKind, GrainKind};
use crate::vector::{Bone, Raster, VectorStroke, Vertex};

const FORMAT: &str = "vmib";
const VERSION: u64 = 1;

/// Python's `CLOUD` block: a note for the reader, not a setting.
const CLOUD_SERVICE: &str = "2kool.tv";

/// The name without its extension, the way Python writes `name`.
fn stem_name(file_name: &str) -> String {
    let name = if file_name.trim().is_empty() {
        "project"
    } else {
        file_name
    };
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].to_string(),
        _ => name.to_string(),
    }
}

/// Python's `_source`: a blueprint keeps the drawing it came from; a
/// drawing that is not yet a blueprint is its own source.
fn source_row(art: &Art) -> Value {
    if art.kind == FORMAT {
        return json!({"file": art.source_file, "kind": art.source_kind});
    }
    json!({"file": art.file_name, "kind": art.kind})
}

pub fn save(art: &mut Art, path: &Path) -> Result<(), String> {
    let mut document = json!({
        "format": FORMAT,
        "version": VERSION,
        "name": stem_name(&art.file_name),
        "width": art.width,
        "height": art.height,
        "playlist": art.playlist,
        "arrange_locked": art.arrange_locked,
        "source": source_row(art),
        "cloud": {
            "service": CLOUD_SERVICE,
            "upload": "later",
            "note": "One zip per project. A later upload sends this file to the signed-in user on 2kool.tv. The studio opens that same zip.",
        },
        "layers": [],
        "objects": art.objects.iter().map(object_row).collect::<Vec<_>>(),
        "tasks": tasks_value(&art.tasks),
        "task_colors": colors_value(&art.task_colors),
        "kosv": kosv_value(&art.kosv),
    });
    let mut blobs: Vec<(String, Vec<u8>)> = Vec::new();
    document["layers"] = Value::Array(
        art.layers
            .iter()
            .map(|node| layer_row(art, node, &mut blobs))
            .collect(),
    );
    if let Some(clip_time) = clip_time_row(art) {
        document["clip_time"] = clip_time;
    }
    write_zip(path, &document, &blobs)
}

/// Python writes `clip_time` only when the cut has a name. A file that came
/// with one keeps it; a drawing that grew an animation here, or runs at
/// another rate, gets the default name so the rate and playhead survive.
/// `end` never shrinks below the last key.
fn clip_time_row(art: &Art) -> Option<Value> {
    let animated = !animation_folders(&art.layers).is_empty();
    if art.clip_time.is_none() && !animated && art.fps == DEFAULT_FPS {
        return None;
    }
    let clip = art.clip_time.clone().unwrap_or_default();
    let name = if clip.name.trim().is_empty() {
        "Timeline".to_string()
    } else {
        clip.name.clone()
    };
    let last_key = cut_end(&art.layers).saturating_sub(1);
    Some(json!({
        "name": name,
        "fps": art.fps,
        "start": clip.start,
        "end": clip.end.max(last_key),
        "current": art.playhead,
    }))
}

fn layer_row(art: &Art, node: &Node, blobs: &mut Vec<(String, Vec<u8>)>) -> Value {
    let mut row = json!({
        "id": node.id,
        "name": node.name,
        "kind": if node.kind == NodeKind::Group { "group" } else { "layer" },
        "visible": node.visible,
        "opacity": node.opacity,
        "blend": node.blend,
        "clip": node.clip,
        "shapes": true,
        "mute": node.mute,
        "solo": node.solo,
        "animation": node.animation,
        "timeline": node.timeline,
        "warp": "",
        "marker": node.marker,
        "color": node.color,
        "vectors": node.vectors,
        "frames": node.frames,
        "outline": false,
        "outline_px": 0,
        "outline_rgb": [0, 0, 0],
        "raster": Value::Null,
        "children": node.children.iter().map(|child| layer_row(art, child, blobs)).collect::<Vec<_>>(),
        "traits": !node.strokes.is_empty(),
    });
    if let Some(sheet) = &node.exposure {
        row["exposure"] = sheet.to_row();
    }
    if !node.effects.is_empty() {
        row["effects"] = effects_value(&node.effects);
    }
    if !node.cel_marks.is_empty() {
        row["cel_marks"] = marks_value(&node.cel_marks);
    }
    if !node.strokes.is_empty() {
        row["strokes"] = Value::Array(node.strokes.iter().map(stroke_row).collect());
    }
    if let Some(raster) = tight_pixels(&node.pixels, art.width, art.height) {
        let file = format!("inks/{}.rgba", node.id);
        blobs.push((file.clone(), raster.rgba.clone()));
        row["ink"] =
            json!({"x": raster.x, "y": raster.y, "w": raster.w, "h": raster.h, "file": file});
    }
    let shown = layer_picture(art, node);
    if let Some(raster) = shown {
        let file = format!("rasters/{}.rgba", node.id);
        blobs.push((file.clone(), raster.rgba.clone()));
        row["raster"] =
            json!({"x": raster.x, "y": raster.y, "w": raster.w, "h": raster.h, "file": file});
    }
    row
}

fn layer_picture(art: &Art, node: &Node) -> Option<Raster> {
    if node.kind != NodeKind::Layer {
        return None;
    }
    let mut scratch = Art::blank(art.width, art.height);
    scratch.layers = vec![node.clone()];
    scratch.layers[0].visible = true;
    scratch.layers[0].opacity = 255;
    scratch.layers[0].mute = false;
    scratch.layers[0].solo = false;
    scratch.layers[0].clip = false;
    let plate = present_plain(&mut scratch);
    tight_pixels(&plate, art.width, art.height)
}

fn stroke_row(stroke: &VectorStroke) -> Value {
    json!({
        "id": stroke.id,
        "color": hex_of(stroke.color),
        "size": stroke.size,
        "softness": stroke.softness,
        "antialias": stroke.fringe,
        "points": stroke.vertices.iter().map(|vertex| [vertex.x, vertex.y, vertex.pressure]).collect::<Vec<_>>(),
        "edge": match stroke.edge { Edge::Smooth => "smooth", Edge::Hard => "hard", Edge::Dither => "dither" },
        "chunk": stroke.chunk,
        "tones": stroke.tones,
        "dither_pattern": pattern_name(stroke.pattern),
        "dither_bias": stroke.bias,
        "curve": curve_name(stroke.curve),
        "grain": grain_name(stroke.grain),
        "seed": stroke.seed,
        "closed": stroke.closed,
        "bone": stroke.bone.map(|bone| json!({"x0": bone.x0, "y0": bone.y0, "x1": bone.x1, "y1": bone.y1, "reach": bone.reach})),
        "widths": stroke.vertices.iter().map(|vertex| vertex.width).collect::<Vec<_>>(),
        "handles": stroke.vertices.iter().map(|vertex| json!({"in": vertex.handle_in, "out": vertex.handle_out})).collect::<Vec<_>>(),
    })
}

fn curve_name(curve: CurveKind) -> &'static str {
    match curve {
        CurveKind::Poly => "poly",
        CurveKind::Quadratic => "quadratic",
        CurveKind::Cubic => "cubic",
        CurveKind::Spline => "spline",
    }
}

fn pattern_name(pattern: DitherPattern) -> &'static str {
    match pattern {
        DitherPattern::Bayer => "bayer",
        DitherPattern::Bayer8 => "bayer8",
        DitherPattern::Checker => "checker",
        DitherPattern::Lines => "lines",
        DitherPattern::Dots => "dots",
    }
}

fn pattern_of(value: Option<&Value>) -> DitherPattern {
    match value.and_then(Value::as_str).unwrap_or("bayer") {
        "bayer8" => DitherPattern::Bayer8,
        "checker" => DitherPattern::Checker,
        "lines" => DitherPattern::Lines,
        "dots" => DitherPattern::Dots,
        _ => DitherPattern::Bayer,
    }
}

fn grain_name(grain: GrainKind) -> &'static str {
    match grain {
        GrainKind::None => "none",
        GrainKind::Stipple => "stipple",
        GrainKind::Hatch => "hatch",
        GrainKind::Liquid => "liquid",
    }
}

/// Python's `_object_row`.
pub fn object_row(object: &DeskObject) -> Value {
    let assign: serde_json::Map<String, Value> = object
        .assign
        .iter()
        .map(|(id, (state, frame))| (id.clone(), json!([state & 0xFFF, frame & 0xFFF])))
        .collect();
    let labels: serde_json::Map<String, Value> = object
        .labels
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    let samples: serde_json::Map<String, Value> = object
        .samples
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    json!({
        "id": object.id,
        "name": object.name,
        "layer_ids": object.layer_ids,
        "assign": Value::Object(assign),
        "origin": object.origin,
        "labels": Value::Object(labels),
        "explicit": object.explicit,
        "parent": object.parent,
        "stack": object.stack,
        "role": object.role,
        "kind": object.kind,
        "warp_layer": object.warp_layer,
        "sound": object.sound,
        "folder": object.folder,
        "samples": Value::Object(samples),
    })
}

/// Python's `_objects_from`. Layer ids the drawing no longer has are dropped.
pub fn objects_from(rows: Option<&Value>, known: &HashSet<String>) -> Vec<DeskObject> {
    let mut objects: Vec<DeskObject> = Vec::new();
    let Some(rows) = rows.and_then(Value::as_array) else {
        return objects;
    };
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if id.is_empty() || objects.iter().any(|object| object.id == id) {
            continue;
        }
        let layer_ids: Vec<String> = row
            .get("layer_ids")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .filter(|id| known.contains(*id))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut assign = BTreeMap::new();
        if let Some(rows) = row.get("assign").and_then(Value::as_object) {
            for (layer, pos) in rows {
                let Some(pos) = pos.as_array().filter(|pos| pos.len() >= 2) else {
                    continue;
                };
                if !known.contains(layer) {
                    continue;
                }
                let state = pos[0].as_f64().unwrap_or(0.0) as i64 & 0xFFF;
                let frame = pos[1].as_f64().unwrap_or(0.0) as i64 & 0xFFF;
                assign.insert(layer.clone(), (state as u16, frame as u16));
            }
        }
        let mut labels = BTreeMap::new();
        if let Some(rows) = row.get("labels").and_then(Value::as_object) {
            for (key, value) in rows {
                if let Some(text) = value
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    labels.insert(key.clone(), text.to_string());
                }
            }
        }
        let mut samples = BTreeMap::new();
        if let Some(rows) = row.get("samples").and_then(Value::as_object) {
            for (key, value) in rows {
                if let Some(text) = value
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    samples.insert(key.clone(), text.to_string());
                }
            }
        }
        let text = |name: &str| {
            row.get(name)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        objects.push(DeskObject {
            id,
            name: row
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .unwrap_or("object")
                .to_string(),
            kind: text("kind"),
            layer_ids,
            assign,
            origin: text("origin"),
            labels,
            explicit: row.get("explicit").and_then(Value::as_bool).unwrap_or(true),
            parent: text("parent"),
            stack: row.get("stack").and_then(Value::as_f64).unwrap_or(0.0) as i64,
            role: text("role"),
            warp_layer: text("warp_layer"),
            sound: text("sound"),
            folder: text("folder"),
            samples,
        });
    }
    objects
}

fn collect_ids(nodes: &[Node], known: &mut HashSet<String>) {
    for node in nodes {
        known.insert(node.id.clone());
        collect_ids(&node.children, known);
    }
}

fn write_zip(path: &Path, document: &Value, blobs: &[(String, Vec<u8>)]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    let file = File::create(path).map_err(|err| err.to_string())?;
    let mut zip = ZipWriter::new(file);
    let stored = FileOptions::default().compression_method(CompressionMethod::Stored);
    let deflate = FileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("mimetype", stored)
        .map_err(|err| err.to_string())?;
    zip.write_all(b"application/x-vmi-blueprint")
        .map_err(|err| err.to_string())?;
    zip.start_file("blueprint.json", deflate)
        .map_err(|err| err.to_string())?;
    zip.write_all(document.to_string().as_bytes())
        .map_err(|err| err.to_string())?;
    for (name, data) in blobs {
        zip.start_file(name, deflate)
            .map_err(|err| err.to_string())?;
        zip.write_all(data).map_err(|err| err.to_string())?;
    }
    zip.finish().map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Art, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|err| err.to_string())?;
    let mut json_file = archive
        .by_name("blueprint.json")
        .map_err(|_| "This blueprint has no project.".to_string())?;
    let mut raw = String::new();
    json_file
        .read_to_string(&mut raw)
        .map_err(|err| err.to_string())?;
    drop(json_file);
    let document: Value = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    if document.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err("This is not a VMI blueprint.".into());
    }
    let width = document.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
    let height = document.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        return Err("This blueprint has no canvas.".into());
    }
    let mut art = Art::blank(width, height);
    // Python names the open drawing after the file on disk. The document's
    // `name` is the stem it was saved under.
    art.file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            document
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("project")
                .to_string()
        });
    art.path = path.display().to_string();
    art.kind = FORMAT.to_string();
    let mut max_id = 0u64;
    if let Some(layers) = document.get("layers").and_then(Value::as_array) {
        for row in layers {
            art.layers
                .push(node_from(row, &mut archive, width, height, &mut max_id)?);
        }
    }
    art.next_id = max_id + 1;
    art.next_stroke = max_stroke(&art.layers) + 1;
    let mut known = HashSet::new();
    collect_ids(&art.layers, &mut known);
    art.objects = objects_from(document.get("objects"), &known);
    art.playlist = match document.get("playlist").and_then(Value::as_str) {
        Some(name @ ("menu" | "battle")) => name.to_string(),
        _ => String::new(),
    };
    art.arrange_locked = document
        .get("arrange_locked")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if let Some(source) = document.get("source").and_then(Value::as_object) {
        art.source_file = source
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        art.source_kind = source
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    }
    art.tasks = pack_tasks(document.get("tasks").unwrap_or(&Value::Null));
    art.task_colors = pack_colors(document.get("task_colors").unwrap_or(&Value::Null));
    art.kosv = clean_kosv(document.get("kosv").unwrap_or(&Value::Null));
    if let Some(clip_time) = document.get("clip_time").and_then(Value::as_object) {
        let number = |key: &str| {
            clip_time
                .get(key)
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .max(0.0)
                .round() as u32
        };
        let fps = number("fps");
        art.fps = if fps == 0 { DEFAULT_FPS } else { fps };
        art.playhead = number("current");
        let name = clip_time
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        art.clip_time = Some(ClipTime {
            name: if name.is_empty() {
                "Timeline".to_string()
            } else {
                name.to_string()
            },
            start: number("start"),
            end: number("end"),
        });
    }
    Ok(art)
}

fn max_stroke(nodes: &[Node]) -> u64 {
    nodes
        .iter()
        .map(|node| {
            node.strokes
                .iter()
                .map(|stroke| stroke.id)
                .max()
                .unwrap_or(0)
                .max(max_stroke(&node.children))
        })
        .max()
        .unwrap_or(0)
}

fn node_from(
    row: &Value,
    archive: &mut ZipArchive<File>,
    width: u32,
    height: u32,
    max_id: &mut u64,
) -> Result<Node, String> {
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return Err("This blueprint has a broken layer.".into());
    }
    if let Some(number) = id
        .strip_prefix('n')
        .and_then(|text| text.parse::<u64>().ok())
    {
        *max_id = (*max_id).max(number);
    }
    let kind = if row.get("kind").and_then(Value::as_str) == Some("group") {
        NodeKind::Group
    } else {
        NodeKind::Layer
    };
    let name = row.get("name").and_then(Value::as_str).unwrap_or("Layer");
    let mut node = if kind == NodeKind::Group {
        Node::group(id, name)
    } else {
        Node::layer(id, name, width, height)
    };
    node.visible = row.get("visible").and_then(Value::as_bool).unwrap_or(true);
    node.opacity = row
        .get("opacity")
        .and_then(Value::as_u64)
        .unwrap_or(255)
        .min(255) as u8;
    node.blend = row
        .get("blend")
        .and_then(Value::as_str)
        .unwrap_or("normal")
        .to_string();
    node.clip = row.get("clip").and_then(Value::as_bool).unwrap_or(false);
    node.mute = row.get("mute").and_then(Value::as_bool).unwrap_or(false);
    node.solo = row.get("solo").and_then(Value::as_bool).unwrap_or(false);
    node.marker = row
        .get("marker")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    node.color = row
        .get("color")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    node.vectors = read_vectors(row.get("vectors"));
    node.strokes = read_strokes(row.get("strokes"));
    if let Some(ink) = row.get("ink") {
        if let Some(raster) = read_raster(archive, ink)? {
            node.pixels = expand_raster(&raster, width, height);
            node.embedded = false;
        }
    } else if let Some(raster) = read_raster(archive, row.get("raster").unwrap_or(&Value::Null))? {
        node.pixels = expand_raster(&raster, width, height);
        node.embedded = !node.strokes.is_empty();
    }
    if let Some(children) = row.get("children").and_then(Value::as_array) {
        for child in children {
            node.children
                .push(node_from(child, archive, width, height, max_id)?);
        }
    }
    node.animation = row
        .get("animation")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    node.timeline = row
        .get("timeline")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter(|id| node.children.iter().any(|child| child.id == *id))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    node.frames = row
        .get("frames")
        .and_then(Value::as_array)
        .map(|frames| {
            frames
                .iter()
                .filter_map(Value::as_f64)
                .map(|frame| frame.max(0.0) as u32)
                .collect()
        })
        .unwrap_or_default();
    node.exposure = load_sheet(row.get("exposure"));
    node.effects = clean_effects(row.get("effects").unwrap_or(&Value::Null));
    let kids: HashSet<String> = node.children.iter().map(|child| child.id.clone()).collect();
    node.cel_marks = clean_marks(row.get("cel_marks").unwrap_or(&Value::Null), Some(&kids));
    Ok(node)
}

/// None when a layer has no exposure row or the row is not one. A row is
/// rebuilt, not allocated.
pub fn load_sheet(value: Option<&Value>) -> Option<ExposureSheet> {
    let row = value?.as_object()?;
    if row.is_empty() {
        return None;
    }
    ExposureSheet::from_row(value?)
}

fn read_vectors(value: Option<&Value>) -> Vec<Vec<[f32; 2]>> {
    let Some(polys) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    polys
        .iter()
        .filter_map(|poly| {
            let points = poly
                .as_array()?
                .iter()
                .filter_map(|point| {
                    let pair = point.as_array()?;
                    Some([
                        pair.first()?.as_f64()? as f32,
                        pair.get(1)?.as_f64()? as f32,
                    ])
                })
                .collect::<Vec<_>>();
            (points.len() >= 3).then_some(points)
        })
        .collect()
}

pub fn strokes_from_value(value: &Value) -> Vec<VectorStroke> {
    read_strokes(Some(value))
}

fn read_strokes(value: Option<&Value>) -> Vec<VectorStroke> {
    let Some(rows) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| stroke_from(row, index as u64 + 1))
        .collect()
}

fn stroke_from(row: &Value, fallback_id: u64) -> Option<VectorStroke> {
    let points = row.get("points")?.as_array()?;
    let widths = row.get("widths").and_then(Value::as_array);
    let handles = row.get("handles").and_then(Value::as_array);
    let mut vertices = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let pair = point.as_array()?;
        let x = pair.first()?.as_f64()? as f32;
        let y = pair.get(1)?.as_f64()? as f32;
        let pressure = pair.get(2).and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let width = widths
            .and_then(|rows| rows.get(index))
            .and_then(Value::as_f64)
            .unwrap_or(4.0) as f32;
        let handle = handles.and_then(|rows| rows.get(index));
        vertices.push(Vertex {
            x,
            y,
            pressure: pressure.clamp(0.0, 1.0),
            width,
            handle_in: handle.and_then(|item| pair_of(item.get("in")?)),
            handle_out: handle.and_then(|item| pair_of(item.get("out")?)),
        });
    }
    if vertices.is_empty() {
        return None;
    }
    let color = crate::color::parse_hex(
        row.get("color")
            .and_then(Value::as_str)
            .unwrap_or("#FFFFFF"),
    )
    .unwrap_or([255, 255, 255]);
    Some(VectorStroke {
        id: row.get("id").and_then(Value::as_u64).unwrap_or(fallback_id),
        vertices,
        closed: row.get("closed").and_then(Value::as_bool).unwrap_or(false),
        curve: match row.get("curve").and_then(Value::as_str).unwrap_or("poly") {
            "quadratic" => CurveKind::Quadratic,
            "cubic" => CurveKind::Cubic,
            "spline" => CurveKind::Spline,
            _ => CurveKind::Poly,
        },
        color,
        softness: row.get("softness").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        fringe: fringe_of(row.get("antialias")),
        edge: match row.get("edge").and_then(Value::as_str).unwrap_or("smooth") {
            "hard" => Edge::Hard,
            "dither" => Edge::Dither,
            _ => Edge::Smooth,
        },
        chunk: row.get("chunk").and_then(Value::as_u64).unwrap_or(1).max(1) as u32,
        tones: row.get("tones").and_then(Value::as_bool).unwrap_or(false),
        pattern: pattern_of(row.get("dither_pattern")),
        bias: row
            .get("dither_bias")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32,
        grain: match row.get("grain").and_then(Value::as_str).unwrap_or("none") {
            "stipple" => GrainKind::Stipple,
            "hatch" => GrainKind::Hatch,
            "liquid" => GrainKind::Liquid,
            _ => GrainKind::None,
        },
        seed: row.get("seed").and_then(Value::as_u64).unwrap_or(1),
        bone: row.get("bone").and_then(bone_of),
        size: row.get("size").and_then(Value::as_f64).unwrap_or(8.0) as f32,
        cache: None,
        dirty: true,
    })
}

fn pair_of(value: &Value) -> Option<[f32; 2]> {
    let pair = value.as_array()?;
    Some([
        pair.first()?.as_f64()? as f32,
        pair.get(1)?.as_f64()? as f32,
    ])
}

fn fringe_of(value: Option<&Value>) -> f32 {
    match value {
        Some(Value::Bool(true)) => 0.5,
        Some(Value::Bool(false)) => 0.0,
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.5) as f32,
        _ => 0.5,
    }
}

fn bone_of(value: &Value) -> Option<Bone> {
    Some(Bone {
        x0: value.get("x0")?.as_f64()? as f32,
        y0: value.get("y0")?.as_f64()? as f32,
        x1: value.get("x1")?.as_f64()? as f32,
        y1: value.get("y1")?.as_f64()? as f32,
        reach: value.get("reach").and_then(Value::as_f64).unwrap_or(160.0) as f32,
    })
}

fn read_raster(archive: &mut ZipArchive<File>, value: &Value) -> Result<Option<Raster>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let Some(row) = value.as_object() else {
        return Ok(None);
    };
    let Some(file_name) = row.get("file").and_then(Value::as_str) else {
        return Ok(None);
    };
    if !(file_name.starts_with("rasters/") || file_name.starts_with("inks/"))
        || file_name.contains("..")
    {
        return Err("A layer picture is missing.".into());
    }
    let mut file = archive
        .by_name(file_name)
        .map_err(|_| "A layer picture is missing.".to_string())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|err| err.to_string())?;
    let w = row.get("w").and_then(Value::as_u64).unwrap_or(0) as u32;
    let h = row.get("h").and_then(Value::as_u64).unwrap_or(0) as u32;
    if w == 0 || h == 0 || data.len() != (w as usize) * (h as usize) * 4 {
        return Err("A layer picture is the wrong size.".into());
    }
    Ok(Some(Raster {
        x: row.get("x").and_then(Value::as_i64).unwrap_or(0) as i32,
        y: row.get("y").and_then(Value::as_i64).unwrap_or(0) as i32,
        w,
        h,
        rgba: data,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Tool, ToolSettings};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn a_stroke_and_its_pixels_round_trip() {
        let mut art = Art::blank(16, 16);
        art.file_name = "ink".into();
        let id = art.add_layer(None, "Ink");
        let settings = ToolSettings::default_for(Tool::Vector);
        let stroke = VectorStroke::from_gesture(
            art.alloc_stroke(),
            Tool::Vector,
            &[(2.0, 2.0, 1.0), (12.0, 8.0, 1.0)],
            &settings,
        )
        .unwrap();
        art.layers.last_mut().unwrap().strokes.push(stroke);
        art.layers.last_mut().unwrap().pixels[0] = 10;
        art.layers.last_mut().unwrap().pixels[3] = 255;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-vmib-{stamp}.vmib"));
        save(&mut art, &path).unwrap();
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let layer = loaded.layers.iter().find(|node| node.id == id).unwrap();
        assert_eq!(layer.strokes.len(), 1);
        assert_eq!(layer.strokes[0].vertices.len(), 2);
        assert_eq!(layer.pixels[0], 10);
        assert!(!layer.embedded);
        assert_eq!(loaded.objects.len(), 0);
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vmi-vmib-{tag}-{stamp}.vmib"))
    }

    #[test]
    fn effects_marks_tasks_objects_and_the_kosv_block_round_trip() {
        use crate::kosv::{make_stack, CelMark, Preset};
        use crate::tasks::{Task, TaskColors};
        let mut art = Art::blank(8, 8);
        art.file_name = "shop.vmib".into();
        let door = art.add_layer(None, "door");
        let folder = art.add_group(None, "wave");
        let cel = art.add_layer(Some(&folder), "a");
        crate::find_mut(&mut art.layers, &door).unwrap().effects = make_stack("tape");
        let wave = crate::find_mut(&mut art.layers, &folder).unwrap();
        wave.animation = true;
        wave.cel_marks.insert(
            cel.clone(),
            CelMark {
                kind: "P".into(),
                reference: "".into(),
                origin: cel.clone(),
                gain: Some(1.5),
                block: None,
            },
        );
        wave.cel_marks.insert(
            "ghost".into(),
            CelMark {
                kind: "I".into(),
                reference: "".into(),
                origin: "".into(),
                gain: None,
                block: None,
            },
        );
        art.playlist = "battle".into();
        art.arrange_locked = false;
        art.tasks = vec![Task {
            id: "t1".into(),
            text: "ink the door".into(),
            done: true,
        }];
        art.task_colors = TaskColors {
            background: "#101010".into(),
            text: "#FFEEDD".into(),
        };
        art.kosv.presets.push(Preset {
            name: "mine".into(),
            stack: make_stack("glow"),
        });
        let mut object = DeskObject::new("o1", "door", vec![door.clone(), "gone".into()]);
        object.kind = "button".into();
        object.assign.insert(door.clone(), (1, 0));
        object.assign.insert("gone".into(), (2, 0));
        object.labels.insert("001000".into(), "lit".into());
        object.parent = "o9".into();
        object.stack = 3;
        object.warp_layer = "mask".into();
        object.sound = "sfx:tik".into();
        object.origin = "slots:n1".into();
        art.objects.push(object);
        let path = temp_path("kosv");
        save(&mut art, &path).unwrap();
        let document = blueprint(&path);
        assert_eq!(document["name"], "shop");
        assert_eq!(document["playlist"], "battle");
        assert_eq!(document["arrange_locked"], false);
        assert_eq!(document["source"]["file"], "");
        assert_eq!(document["tasks"][0]["text"], "ink the door");
        assert_eq!(document["kosv"]["product"], "KOSV");
        assert_eq!(document["kosv"]["plate"], json!([1920, 1080]));
        assert_eq!(document["objects"][0]["assign"][&door], json!([1, 0]));
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            loaded.file_name,
            path.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(loaded.kind, "vmib");
        let door_node = crate::find(&loaded.layers, &door).unwrap();
        assert_eq!(door_node.effects.len(), 2);
        assert_eq!(door_node.effects[0].id, "vhs");
        assert_eq!(door_node.effects[0].preset, "tape");
        let wave = crate::find(&loaded.layers, &folder).unwrap();
        assert_eq!(
            wave.cel_marks.len(),
            1,
            "a mark on a cel the folder lacks is dropped"
        );
        assert_eq!(wave.cel_marks[&cel].gain, Some(1.5));
        assert_eq!(loaded.playlist, "battle");
        assert!(!loaded.arrange_locked);
        assert_eq!(loaded.tasks, art.tasks);
        assert_eq!(loaded.task_colors, art.task_colors);
        assert_eq!(loaded.kosv.presets.len(), 1);
        assert_eq!(loaded.kosv.presets[0].stack[0].id, "color-glow");
        assert_eq!(loaded.objects.len(), 1);
        let object = &loaded.objects[0];
        assert_eq!(
            object.layer_ids,
            vec![door.clone()],
            "a layer the drawing lacks is dropped"
        );
        assert_eq!(object.assign.len(), 1);
        assert_eq!(object.assign[&door], (1, 0));
        assert_eq!(object.labels["001000"], "lit");
        assert_eq!(object.kind, "button");
        assert_eq!(object.parent, "o9");
        assert_eq!(object.stack, 3);
        assert_eq!(object.warp_layer, "mask");
        assert_eq!(object.sound, "sfx:tik");
        assert_eq!(object.origin, "slots:n1");
        assert!(object.explicit);
        assert!(object.folder.is_empty());
        assert!(object.samples.is_empty());
    }

    #[test]
    fn a_library_keeps_its_folder_and_a_state_sample_and_an_old_row_still_loads() {
        let mut art = Art::blank(4, 4);
        art.file_name = "board.vmib".into();
        let layer = art.add_layer(None, "mouth");
        let mut folder = DeskObject::new("f", "Faces", Vec::new());
        folder.kind = "folder".into();
        let mut mouth = DeskObject::new("m", "Mouth", vec![layer.clone()]);
        mouth.kind = "mouth".into();
        mouth.folder = "f".into();
        mouth.stack = 2;
        mouth.sound = "samples/rest.wav".into();
        mouth.set_sample(1, 0, "samples/ah.wav");
        mouth.labels.insert("001000".into(), "AH".into());
        mouth.assign.insert(layer.clone(), (1, 0));
        art.objects.push(folder);
        art.objects.push(mouth);
        let path = temp_path("library");
        save(&mut art, &path).unwrap();
        let document = blueprint(&path);
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(document["objects"][1]["folder"], "f");
        assert_eq!(
            document["objects"][1]["samples"]["001000"],
            "samples/ah.wav"
        );
        assert_eq!(document["objects"][1]["kind"], "mouth");
        assert_eq!(
            loaded.objects.len(),
            2,
            "opening the file does not invent a third object"
        );
        let loaded_mouth = loaded
            .objects
            .iter()
            .find(|object| object.id == "m")
            .unwrap();
        assert_eq!(loaded_mouth.kind, "mouth");
        assert_eq!(loaded_mouth.folder, "f");
        assert_eq!(loaded_mouth.stack, 2);
        assert_eq!(loaded_mouth.sound, "samples/rest.wav");
        assert_eq!(loaded_mouth.sample_of(1, 0), "samples/ah.wav");
        assert_eq!(loaded_mouth.sample_of(0, 0), "samples/rest.wav");
        assert_eq!(loaded_mouth.label_of(1, 0), "AH");
        assert_eq!(loaded.fps, DEFAULT_FPS);

        let old = json!([{
            "id": "o1",
            "name": "door",
            "kind": "button",
            "sound": "sfx:tik"
        }]);
        let known = std::collections::HashSet::new();
        let objects = objects_from(Some(&old), &known);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].kind, "button");
        assert_eq!(objects[0].sound, "sfx:tik");
        assert!(objects[0].folder.is_empty());
        assert!(objects[0].samples.is_empty());
        assert_eq!(objects[0].sample_of(2, 0), "sfx:tik");
        assert_eq!(objects[0].states(), vec![0, 1, 2, 3]);
    }

    fn blueprint(path: &Path) -> Value {
        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut raw = String::new();
        archive
            .by_name("blueprint.json")
            .unwrap()
            .read_to_string(&mut raw)
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn held_sheet() -> ExposureSheet {
        ExposureSheet {
            slots: vec!["000".into(), "000".into(), "000".into(), "002".into()],
            pool: vec!["000".into(), "001".into(), "002".into()],
            issued: 3,
            base_seed: 11,
            active: 2,
        }
    }

    #[test]
    fn a_three_cel_animation_and_its_sheet_round_trip() {
        let mut art = Art::blank(4, 4);
        art.file_name = "shot".into();
        art.add_layer(None, "Paper");
        let folder_id = art.add_group(None, "Animation");
        let mut ids = Vec::new();
        for (index, name) in ["1", "2", "3"].iter().enumerate() {
            let id = art.add_layer(Some(&folder_id), name);
            let cel = crate::doc::find_mut(&mut art.layers, &id).unwrap();
            cel.pixels[index * 4] = 200;
            cel.pixels[index * 4 + 3] = 255;
            ids.push(id);
        }
        let folder = crate::doc::find_mut(&mut art.layers, &folder_id).unwrap();
        folder.animation = true;
        folder.timeline = ids.clone();
        folder.frames = vec![0, 2, 5];
        folder.exposure = Some(held_sheet());
        art.fps = 30;
        art.playhead = 4;
        art.clip_time = Some(ClipTime {
            name: "Timeline 1".into(),
            start: 0,
            end: 48,
        });
        art.onion = true;

        let path = temp_path("anim");
        save(&mut art, &path).unwrap();
        let loaded = load(&path).unwrap();
        let document = blueprint(&path);
        let _ = std::fs::remove_file(&path);

        let folder = crate::doc::find(&loaded.layers, &folder_id).unwrap();
        assert!(folder.animation);
        assert_eq!(folder.timeline, ids);
        assert_eq!(folder.frames, vec![0, 2, 5]);
        assert_eq!(folder.exposure, Some(held_sheet()));
        assert_eq!(folder.children.len(), 3);
        for (index, id) in ids.iter().enumerate() {
            let cel = crate::doc::find(&loaded.layers, id).unwrap();
            assert!(!cel.animation);
            assert!(cel.timeline.is_empty() && cel.frames.is_empty());
            assert_eq!(cel.exposure, None);
            assert_eq!(cel.pixels[index * 4], 200, "cel {id} keeps its own picture");
            assert_eq!(cel.pixels[index * 4 + 3], 255);
        }
        assert_eq!(loaded.fps, 30);
        assert_eq!(loaded.playhead, 4);
        assert_eq!(
            loaded.clip_time,
            Some(ClipTime {
                name: "Timeline 1".into(),
                start: 0,
                end: 48
            })
        );
        assert!(!loaded.onion, "onion skin is a view, not part of the file");
        assert_eq!(crate::anim::cel_at(folder, 4).unwrap().id, ids[1]);

        let rows = document["layers"].as_array().unwrap();
        let row = rows
            .iter()
            .find(|row| row["id"] == folder_id.as_str())
            .unwrap();
        assert_eq!(row["animation"], json!(true));
        assert_eq!(row["timeline"], json!(ids));
        assert_eq!(row["frames"], json!([0, 2, 5]));
        assert_eq!(row["exposure"], held_sheet().to_row());
        assert_eq!(
            row["exposure"]["slots"],
            json!(["000", "000", "000", "002"])
        );
        for cel in row["children"].as_array().unwrap() {
            assert_eq!(cel["animation"], json!(false));
            assert_eq!(cel["timeline"], json!([]));
            assert_eq!(cel["frames"], json!([]));
            assert!(
                cel.get("exposure").is_none(),
                "a layer without a sheet omits the key"
            );
        }
        let paper = rows.iter().find(|row| row["name"] == "Paper").unwrap();
        assert!(paper.get("exposure").is_none());
        assert_eq!(
            document["clip_time"],
            json!({"name": "Timeline 1", "fps": 30, "start": 0, "end": 48, "current": 4})
        );
    }

    #[test]
    fn clip_time_is_written_for_a_cut_and_skipped_for_a_plain_drawing() {
        let mut plain = Art::new(4, 4);
        let path = temp_path("plain");
        save(&mut plain, &path).unwrap();
        let document = blueprint(&path);
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(document.get("clip_time").is_none());
        assert_eq!(
            (loaded.fps, loaded.playhead, loaded.clip_time),
            (DEFAULT_FPS, 0, None)
        );
        assert_eq!(document["layers"][0]["animation"], json!(false));
        assert_eq!(document["layers"][0]["timeline"], json!([]));
        assert_eq!(document["layers"][0]["frames"], json!([]));

        let mut grown = Art::blank(4, 4);
        let folder_id = grown.add_group(None, "Animation");
        let first = grown.add_layer(Some(&folder_id), "1");
        let second = grown.add_layer(Some(&folder_id), "2");
        let folder = crate::doc::find_mut(&mut grown.layers, &folder_id).unwrap();
        folder.animation = true;
        folder.timeline = vec![first, second];
        folder.frames = vec![0, 7];
        grown.playhead = 3;
        let path = temp_path("grown");
        save(&mut grown, &path).unwrap();
        let document = blueprint(&path);
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            document["clip_time"],
            json!({"name": "Timeline", "fps": 24, "start": 0, "end": 7, "current": 3}),
            "an animated drawing keeps its rate and playhead; end reaches the last key"
        );
        assert_eq!(loaded.playhead, 3);
        assert_eq!(
            loaded.clip_time,
            Some(ClipTime {
                name: "Timeline".into(),
                start: 0,
                end: 7
            })
        );

        let mut slow = Art::new(4, 4);
        slow.fps = 12;
        let path = temp_path("slow");
        save(&mut slow, &path).unwrap();
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.fps, 12, "another rate survives without an animation");
    }

    #[test]
    fn an_old_blueprint_without_animation_keys_still_loads() {
        let document = json!({
            "format": FORMAT,
            "version": VERSION,
            "name": "old",
            "width": 2,
            "height": 2,
            "layers": [
                {"id": "n1", "name": "Ink", "kind": "layer"},
                {"id": "n2", "name": "Folder", "kind": "group", "children": [
                    {"id": "n3", "name": "1", "kind": "layer", "exposure": {}},
                    {"id": "n4", "name": "2", "kind": "layer", "exposure": null},
                ], "animation": true, "timeline": ["n4", "n9", 7, "n3"], "frames": [2.0, -1, 5]},
            ],
            "clip_time": {"name": "", "fps": 0, "start": 0, "end": 9, "current": 6},
        });
        let path = temp_path("old");
        write_zip(&path, &document, &[]).unwrap();
        let loaded = load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.layers.len(), 2);
        assert!(!loaded.layers[0].animation);
        assert!(loaded.layers[0].timeline.is_empty() && loaded.layers[0].frames.is_empty());
        assert_eq!(loaded.layers[0].exposure, None);
        let folder = &loaded.layers[1];
        assert!(folder.animation);
        assert_eq!(
            folder.timeline,
            vec!["n4".to_string(), "n3".to_string()],
            "unknown ids drop out"
        );
        assert_eq!(folder.frames, vec![2, 0, 5], "frames clamp at zero");
        assert_eq!(
            folder.children[0].exposure, None,
            "an empty row is no sheet"
        );
        assert_eq!(folder.children[1].exposure, None);
        assert_eq!(loaded.fps, DEFAULT_FPS, "a zero rate falls back");
        assert_eq!(loaded.playhead, 6);
        assert_eq!(
            loaded.clip_time,
            Some(ClipTime {
                name: "Timeline".into(),
                start: 0,
                end: 9
            })
        );
        assert_eq!(loaded.next_id, 5);

        assert_eq!(load_sheet(None), None);
        assert_eq!(load_sheet(Some(&json!(null))), None);
        assert_eq!(load_sheet(Some(&json!({}))), None);
        assert_eq!(load_sheet(Some(&json!({"note": "no sheet"}))), None);
        assert_eq!(load_sheet(Some(&json!(["000"]))), None);
        assert_eq!(load_sheet(Some(&held_sheet().to_row())), Some(held_sheet()));
    }
}
