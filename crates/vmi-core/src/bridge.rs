//! Leaf rasters written by `python -m vmi_studio.bridge`.
//!
//! The manifest points at a directory of tight RGBA files. A layer that
//! already has both a picture and strokes keeps `embedded`, so the stroke
//! cache is not drawn on top of the bake.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::doc::{Art, DeskObject, Node, NodeKind};
use crate::vector::Raster;
use crate::vmib::strokes_from_value;

pub fn load_manifest(path: &Path) -> Result<Art, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let document: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let width = document.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
    let height = document.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        return Err("This manifest has no canvas.".into());
    }
    let rasters = rasters_dir(path, &document)?;
    let mut art = Art::blank(width, height);
    art.file_name = document
        .get("file_name")
        .and_then(Value::as_str)
        .unwrap_or("opened")
        .to_string();
    art.path = document.get("path").and_then(Value::as_str).unwrap_or("").to_string();
    let mut max_id = 0u64;
    if let Some(layers) = document.get("layers").and_then(Value::as_array) {
        for row in layers {
            art.layers.push(node_from(row, &rasters, width, height, &mut max_id)?);
        }
    }
    art.next_id = max_id + 1;
    art.next_stroke = max_stroke(&art.layers) + 1;
    art.kind = document.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
    if let Some(objects) = document.get("objects").and_then(Value::as_array) {
        for row in objects {
            let id = row.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let mut object = DeskObject::new(id, row.get("name").and_then(Value::as_str).unwrap_or("Object"), Vec::new());
            object.kind = row.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
            art.objects.push(object);
        }
    }
    Ok(art)
}

fn rasters_dir(manifest: &Path, document: &Value) -> Result<PathBuf, String> {
    let raw = document.get("rasters_dir").and_then(Value::as_str).unwrap_or("rasters");
    let path = PathBuf::from(raw);
    let full = if path.is_absolute() {
        path
    } else {
        manifest.parent().unwrap_or_else(|| Path::new(".")).join(path)
    };
    Ok(full)
}

fn node_from(row: &Value, rasters: &Path, width: u32, height: u32, max_id: &mut u64) -> Result<Node, String> {
    let id = row.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    if id.is_empty() {
        return Err("This manifest has a broken layer.".into());
    }
    if let Some(number) = id.strip_prefix('n').and_then(|text| text.parse::<u64>().ok()) {
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
    node.opacity = row.get("opacity").and_then(Value::as_u64).unwrap_or(255).min(255) as u8;
    node.blend = row.get("blend").and_then(Value::as_str).unwrap_or("normal").to_string();
    node.clip = row.get("clip").and_then(Value::as_bool).unwrap_or(false);
    node.mute = row.get("mute").and_then(Value::as_bool).unwrap_or(false);
    node.solo = row.get("solo").and_then(Value::as_bool).unwrap_or(false);
    let marker = row.get("marker").and_then(Value::as_str).unwrap_or("");
    node.marker = if marker == "target" || marker == "mask" { marker.to_string() } else { String::new() };
    node.color = row.get("color").and_then(Value::as_str).unwrap_or("").to_string();
    node.vectors = read_vectors(row.get("vectors"));
    if let Some(strokes) = row.get("strokes") {
        node.strokes = strokes_from_value(strokes);
    }
    if kind == NodeKind::Layer {
        if let Some(raster) = read_raster(rasters, row.get("raster").unwrap_or(&Value::Null))? {
            node.pixels = crate::doc::expand_raster(&raster, width, height);
            node.embedded = !node.strokes.is_empty();
        }
    }
    if let Some(children) = row.get("children").and_then(Value::as_array) {
        for child in children {
            node.children.push(node_from(child, rasters, width, height, max_id)?);
        }
    }
    Ok(node)
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
                    Some([pair.first()?.as_f64()? as f32, pair.get(1)?.as_f64()? as f32])
                })
                .collect::<Vec<_>>();
            (points.len() >= 3).then_some(points)
        })
        .collect()
}

fn read_raster(dir: &Path, value: &Value) -> Result<Option<Raster>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let Some(row) = value.as_object() else {
        return Ok(None);
    };
    let Some(file_name) = row.get("file").and_then(Value::as_str) else {
        return Ok(None);
    };
    if file_name.is_empty() || file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("A layer picture is missing.".into());
    }
    let path = dir.join(file_name);
    let data = fs::read(&path).map_err(|_| format!("A layer picture is missing: {}", path.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn a_leaf_raster_and_its_stroke_load_from_a_manifest() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("vmi-bridge-{stamp}"));
        let rasters = root.join("rasters");
        fs::create_dir_all(&rasters).unwrap();
        fs::write(rasters.join("n1.rgba"), [9u8, 0, 0, 255]).unwrap();
        let manifest = root.join("manifest.json");
        let body = format!(
            r##"{{
                "width": 2,
                "height": 1,
                "file_name": "leaf",
                "path": "leaf.clip",
                "rasters_dir": "{}",
                "layers": [{{
                    "id": "n0",
                    "name": "Folder",
                    "kind": "group",
                    "children": [{{
                        "id": "n1",
                        "name": "Ink",
                        "kind": "layer",
                        "raster": {{"file": "n1.rgba", "x": 1, "y": 0, "w": 1, "h": 1}},
                        "strokes": [{{
                            "id": 4,
                            "color": "#FF3EB8",
                            "size": 8,
                            "softness": 0,
                            "antialias": 0.5,
                            "points": [[1, 0, 1], [2, 0, 0.5]],
                            "edge": "dither",
                            "chunk": 2,
                            "grain": "hatch",
                            "seed": 9
                        }}]
                    }}]
                }}]
            }}"##,
            rasters.display().to_string().replace('\\', "\\\\")
        );
        fs::write(&manifest, body).unwrap();
        let art = load_manifest(&manifest).unwrap();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(art.width, 2);
        assert_eq!(art.layers[0].children[0].pixels[7], 255);
        assert_eq!(art.layers[0].children[0].pixels[4], 9);
        assert!(art.layers[0].children[0].embedded);
        assert_eq!(art.layers[0].children[0].strokes[0].id, 4);
        assert_eq!(art.layers[0].children[0].strokes[0].seed, 9);
        assert_eq!(art.next_id, 2);
        assert_eq!(art.next_stroke, 5);
    }
}
