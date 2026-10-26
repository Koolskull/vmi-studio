//! `.vmib` zip. Python reads `blueprint.json` and `rasters/`. Ink that is
//! separate from the vector cache is stored under `inks/` so a reload can
//! rebuild the strokes without painting them twice.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use serde_json::{json, Value};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::color::hex_of;
use crate::coverage::Edge;
use crate::doc::{expand_raster, present, tight_pixels, Art, DeskObject, Node, NodeKind};
use crate::tool::{CurveKind, GrainKind};
use crate::vector::{Bone, Raster, VectorStroke, Vertex};

const FORMAT: &str = "vmib";
const VERSION: u64 = 1;

pub fn save(art: &mut Art, path: &Path) -> Result<(), String> {
    let plate = present(art);
    let _ = plate;
    let mut document = json!({
        "format": FORMAT,
        "version": VERSION,
        "name": art.file_name,
        "width": art.width,
        "height": art.height,
        "playlist": "",
        "arrange_locked": true,
        "source": {"file": "", "kind": ""},
        "cloud": {"service": "2kool.tv", "upload": "later"},
        "layers": [],
        "objects": art.objects.iter().map(object_row).collect::<Vec<_>>(),
        "tasks": [],
        "task_colors": {},
    });
    let mut blobs: Vec<(String, Vec<u8>)> = Vec::new();
    document["layers"] = Value::Array(art.layers.iter().map(|node| layer_row(art, node, &mut blobs)).collect());
    write_zip(path, &document, &blobs)
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
        "animation": false,
        "timeline": [],
        "warp": "",
        "marker": node.marker,
        "color": node.color,
        "vectors": node.vectors,
        "frames": [],
        "outline": false,
        "outline_px": 0,
        "outline_rgb": [0, 0, 0],
        "raster": Value::Null,
        "children": node.children.iter().map(|child| layer_row(art, child, blobs)).collect::<Vec<_>>(),
        "traits": !node.strokes.is_empty(),
    });
    if !node.strokes.is_empty() {
        row["strokes"] = Value::Array(node.strokes.iter().map(stroke_row).collect());
    }
    if let Some(raster) = tight_pixels(&node.pixels, art.width, art.height) {
        let file = format!("inks/{}.rgba", node.id);
        blobs.push((file.clone(), raster.rgba.clone()));
        row["ink"] = json!({"x": raster.x, "y": raster.y, "w": raster.w, "h": raster.h, "file": file});
    }
    let shown = layer_picture(art, node);
    if let Some(raster) = shown {
        let file = format!("rasters/{}.rgba", node.id);
        blobs.push((file.clone(), raster.rgba.clone()));
        row["raster"] = json!({"x": raster.x, "y": raster.y, "w": raster.w, "h": raster.h, "file": file});
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
    let plate = present(&mut scratch);
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

fn grain_name(grain: GrainKind) -> &'static str {
    match grain {
        GrainKind::None => "none",
        GrainKind::Stipple => "stipple",
        GrainKind::Hatch => "hatch",
        GrainKind::Liquid => "liquid",
    }
}

fn object_row(object: &DeskObject) -> Value {
    json!({
        "id": object.id,
        "name": object.name,
        "kind": object.kind,
        "layer_ids": [],
        "assign": {},
        "explicit": true,
    })
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
    zip.start_file("mimetype", stored).map_err(|err| err.to_string())?;
    zip.write_all(b"application/x-vmi-blueprint").map_err(|err| err.to_string())?;
    zip.start_file("blueprint.json", deflate).map_err(|err| err.to_string())?;
    zip.write_all(document.to_string().as_bytes()).map_err(|err| err.to_string())?;
    for (name, data) in blobs {
        zip.start_file(name, deflate).map_err(|err| err.to_string())?;
        zip.write_all(data).map_err(|err| err.to_string())?;
    }
    zip.finish().map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Art, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|err| err.to_string())?;
    let mut json_file = archive.by_name("blueprint.json").map_err(|_| "This blueprint has no project.".to_string())?;
    let mut raw = String::new();
    json_file.read_to_string(&mut raw).map_err(|err| err.to_string())?;
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
    art.file_name = document.get("name").and_then(Value::as_str).unwrap_or("project").to_string();
    art.path = path.display().to_string();
    let mut max_id = 0u64;
    if let Some(layers) = document.get("layers").and_then(Value::as_array) {
        for row in layers {
            art.layers.push(node_from(row, &mut archive, width, height, &mut max_id)?);
        }
    }
    art.next_id = max_id + 1;
    art.next_stroke = max_stroke(&art.layers) + 1;
    if let Some(objects) = document.get("objects").and_then(Value::as_array) {
        for row in objects {
            let id = row.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            art.objects.push(DeskObject {
                id,
                name: row.get("name").and_then(Value::as_str).unwrap_or("Object").to_string(),
                kind: row.get("kind").and_then(Value::as_str).unwrap_or("static").to_string(),
            });
        }
    }
    Ok(art)
}

fn max_stroke(nodes: &[Node]) -> u64 {
    nodes.iter().map(|node| {
        node.strokes.iter().map(|stroke| stroke.id).max().unwrap_or(0).max(max_stroke(&node.children))
    }).max().unwrap_or(0)
}

fn node_from(row: &Value, archive: &mut ZipArchive<File>, width: u32, height: u32, max_id: &mut u64) -> Result<Node, String> {
    let id = row.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    if id.is_empty() {
        return Err("This blueprint has a broken layer.".into());
    }
    if let Some(number) = id.strip_prefix('n').and_then(|text| text.parse::<u64>().ok()) {
        *max_id = (*max_id).max(number);
    }
    let kind = if row.get("kind").and_then(Value::as_str) == Some("group") { NodeKind::Group } else { NodeKind::Layer };
    let name = row.get("name").and_then(Value::as_str).unwrap_or("Layer");
    let mut node = if kind == NodeKind::Group { Node::group(id, name) } else { Node::layer(id, name, width, height) };
    node.visible = row.get("visible").and_then(Value::as_bool).unwrap_or(true);
    node.opacity = row.get("opacity").and_then(Value::as_u64).unwrap_or(255).min(255) as u8;
    node.blend = row.get("blend").and_then(Value::as_str).unwrap_or("normal").to_string();
    node.clip = row.get("clip").and_then(Value::as_bool).unwrap_or(false);
    node.mute = row.get("mute").and_then(Value::as_bool).unwrap_or(false);
    node.solo = row.get("solo").and_then(Value::as_bool).unwrap_or(false);
    node.marker = row.get("marker").and_then(Value::as_str).unwrap_or("").to_string();
    node.color = row.get("color").and_then(Value::as_str).unwrap_or("").to_string();
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
            node.children.push(node_from(child, archive, width, height, max_id)?);
        }
    }
    Ok(node)
}

fn read_vectors(value: Option<&Value>) -> Vec<Vec<[f32; 2]>> {
    let Some(polys) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    polys.iter().filter_map(|poly| {
        let points = poly.as_array()?.iter().filter_map(|point| {
            let pair = point.as_array()?;
            Some([pair.first()?.as_f64()? as f32, pair.get(1)?.as_f64()? as f32])
        }).collect::<Vec<_>>();
        (points.len() >= 3).then_some(points)
    }).collect()
}

pub fn strokes_from_value(value: &Value) -> Vec<VectorStroke> {
    read_strokes(Some(value))
}

fn read_strokes(value: Option<&Value>) -> Vec<VectorStroke> {
    let Some(rows) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter().enumerate().filter_map(|(index, row)| stroke_from(row, index as u64 + 1)).collect()
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
        let width = widths.and_then(|rows| rows.get(index)).and_then(Value::as_f64).unwrap_or(4.0) as f32;
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
    let color = crate::color::parse_hex(row.get("color").and_then(Value::as_str).unwrap_or("#FFFFFF")).unwrap_or([255, 255, 255]);
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
    Some([pair.first()?.as_f64()? as f32, pair.get(1)?.as_f64()? as f32])
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
    if !(file_name.starts_with("rasters/") || file_name.starts_with("inks/")) || file_name.contains("..") {
        return Err("A layer picture is missing.".into());
    }
    let mut file = archive.by_name(file_name).map_err(|_| "A layer picture is missing.".to_string())?;
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
        let stroke = VectorStroke::from_gesture(art.alloc_stroke(), Tool::Vector, &[(2.0, 2.0, 1.0), (12.0, 8.0, 1.0)], &settings).unwrap();
        art.layers.last_mut().unwrap().strokes.push(stroke);
        art.layers.last_mut().unwrap().pixels[0] = 10;
        art.layers.last_mut().unwrap().pixels[3] = 255;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
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
}
