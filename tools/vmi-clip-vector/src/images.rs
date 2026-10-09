//! Read Clip Studio image materials and write their unwarped pixels.
//!
//! ResizableImageInfo stores the source size and the four canvas corners, in
//! order top-left, top-right, bottom-left, bottom-right of the source image.
//! The pixels live on ResizableOriginalMipmap. This command does not warp
//! them. VMI maps those corners when it builds the preview.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use clipfile::{ClipFile, Database, Document, PixelFormat, RasterImage};
use serde_json::{Value, json};

struct Material {
    info: Vec<u8>,
    original_mipmap: i64,
    mask_mipmap: i64,
    visibility: i64,
}

struct Placement {
    width: u32,
    height: u32,
    corners: [(f64, f64); 4],
}

pub fn write_images(path: &OsStr, directory: &OsStr) -> ExitCode {
    match dump_images(path, directory) {
        Ok(value) => {
            let mut out = io::stdout().lock();
            if serde_json::to_writer(&mut out, &value).is_err() || out.write_all(b"\n").is_err() {
                eprintln!("image: could not write json");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("image: {err}");
            ExitCode::from(1)
        }
    }
}

fn dump_images(path: &OsStr, directory: &OsStr) -> Result<Value, Box<dyn std::error::Error>> {
    let mut clip = ClipFile::open(File::open(path)?)?;
    let limits = clip.limits();
    let database = clip.open_database()?;
    let document = Document::load(&database, limits)?;
    let canvas = document
        .canvases()
        .iter()
        .find(|canvas| canvas.width() > 0.0 && canvas.height() > 0.0)
        .or_else(|| document.canvases().first())
        .ok_or("clip has no canvas")?;
    let materials = load_materials(database.connection())?;
    let root = canvas.root_layer_id();
    let layers = children(&document, root)
        .into_iter()
        .map(|id| layer_json(&mut clip, &database, &document, id, directory, &materials))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(json!({
        "width": canvas.width(),
        "height": canvas.height(),
        "layers": layers,
    }))
}

fn load_materials(
    connection: &rusqlite::Connection,
) -> Result<HashMap<i64, Material>, Box<dyn std::error::Error>> {
    let ready: i64 = connection.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('Layer') \
         WHERE name IN ('ResizableImageInfo', 'ResizableOriginalMipmap')",
        [],
        |row| row.get(0),
    )?;
    if ready < 2 {
        eprintln!("image: this file has no image-material columns");
        return Ok(HashMap::new());
    }
    let mask_column: i64 = connection.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('Layer') WHERE name = 'LayerLayerMaskMipmap'",
        [],
        |row| row.get(0),
    )?;
    let sql = if mask_column == 0 {
        "SELECT MainId, ResizableImageInfo, ResizableOriginalMipmap, 0, LayerVisibility \
         FROM Layer \
         WHERE ResizableImageInfo IS NOT NULL AND length(ResizableImageInfo) > 0"
    } else {
        "SELECT MainId, ResizableImageInfo, ResizableOriginalMipmap, \
         COALESCE(LayerLayerMaskMipmap, 0), COALESCE(LayerVisibility, 0) \
         FROM Layer \
         WHERE ResizableImageInfo IS NOT NULL AND length(ResizableImageInfo) > 0"
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let info: Vec<u8> = row.get(1)?;
        let original: Option<i64> = row.get(2)?;
        let mask: Option<i64> = row.get(3)?;
        let visibility: Option<i64> = row.get(4)?;
        Ok((
            id,
            Material {
                info,
                original_mipmap: original.unwrap_or(0),
                mask_mipmap: mask.unwrap_or(0),
                visibility: visibility.unwrap_or(0),
            },
        ))
    })?;
    let mut materials = HashMap::new();
    for row in rows {
        let (id, material) = row?;
        materials.insert(id, material);
    }
    Ok(materials)
}

fn children(document: &Document, id: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut current = document.layer(id).and_then(|layer| layer.first_child_id());
    let mut guard = 0u32;
    while let Some(next) = current {
        guard += 1;
        if guard > 100_000 {
            eprintln!("image: layer sibling list did not end");
            break;
        }
        out.push(next);
        current = document.layer(next).and_then(|layer| layer.next_sibling_id());
    }
    out
}

fn layer_json(
    clip: &mut ClipFile<File>,
    database: &Database,
    document: &Document,
    id: i64,
    directory: &OsStr,
    materials: &HashMap<i64, Material>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let Some(layer) = document.layer(id) else {
        return Ok(json!({
            "id": id,
            "name": "",
            "folder": false,
            "image": null,
            "children": [],
        }));
    };
    let image = if layer.is_folder() {
        Value::Null
    } else if let Some(material) = materials.get(&id) {
        image_json(clip, database, id, layer.name().unwrap_or(""), material, directory)
    } else {
        Value::Null
    };
    let children = children(document, id)
        .into_iter()
        .map(|child| layer_json(clip, database, document, child, directory, materials))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "id": id,
        "name": layer.name().unwrap_or(""),
        "folder": layer.is_folder(),
        "image": image,
        "children": children,
    }))
}

fn image_json(
    clip: &mut ClipFile<File>,
    database: &Database,
    id: i64,
    name: &str,
    material: &Material,
    directory: &OsStr,
) -> Value {
    let placement = match parse_placement(&material.info) {
        Ok(placement) => placement,
        Err(err) => {
            eprintln!("image: layer {id} ({name}) left blank: {err}");
            return json!({ "error": err });
        }
    };
    if material.original_mipmap == 0 {
        let err = "no original image";
        eprintln!("image: layer {id} ({name}) left blank: {err}");
        return json!({ "error": err });
    }
    let source = match database.raster_source(material.original_mipmap) {
        Ok(Some(source)) => source,
        Ok(None) => {
            let err = "original mipmap has no pixels";
            eprintln!("image: layer {id} ({name}) left blank: {err}");
            return json!({ "error": err });
        }
        Err(err) => {
            eprintln!("image: layer {id} ({name}) left blank: {err}");
            return json!({ "error": err.to_string() });
        }
    };
    let raster = match clip.decode_raster(database, &source) {
        Ok(raster) => raster,
        Err(err) => {
            eprintln!("image: layer {id} ({name}) left blank: {err}");
            return json!({ "error": err.to_string() });
        }
    };
    if !raster.data_state().is_present() {
        let err = "original image is missing";
        eprintln!("image: layer {id} ({name}) left blank: {err}");
        return json!({ "error": err });
    }
    let rgba = match to_rgba(&raster) {
        Ok(rgba) => rgba,
        Err(err) => {
            eprintln!("image: layer {id} ({name}) left blank: {err}");
            return json!({ "error": err });
        }
    };
    let file_name = format!("{id}.rgba");
    if let Err(err) = fs::write(Path::new(directory).join(&file_name), &rgba) {
        eprintln!("image: layer {id} ({name}) left blank: {err}");
        return json!({ "error": err.to_string() });
    }
    let (corner_text, corners) = corner_json(&placement.corners);
    eprintln!(
        "image: layer {id} ({name}) {}x{} corners {corner_text}",
        raster.width(),
        raster.height()
    );
    if raster.width() != placement.width || raster.height() != placement.height {
        eprintln!(
            "image: layer {id} ({name}) placement says {}x{}, bitmap is {}x{}",
            placement.width,
            placement.height,
            raster.width(),
            raster.height()
        );
    }
    let mask = mask_json(clip, database, id, name, material, directory);
    json!({
        "width": raster.width(),
        "height": raster.height(),
        "corners": corners,
        "file": file_name,
        "mask": mask,
    })
}

fn mask_json(
    clip: &mut ClipFile<File>,
    database: &Database,
    id: i64,
    name: &str,
    material: &Material,
    directory: &OsStr,
) -> Value {
    if material.mask_mipmap == 0 {
        return Value::Null;
    }
    // Clip Studio stores the mask as enabled when LayerVisibility bit 1 is set.
    if material.visibility & 2 == 0 {
        eprintln!("image: layer {id} ({name}) mask is switched off");
        return Value::Null;
    }
    let source = match database.raster_source(material.mask_mipmap) {
        Ok(Some(source)) => source,
        Ok(None) => {
            eprintln!("image: layer {id} ({name}) mask has no pixels");
            return Value::Null;
        }
        Err(err) => {
            eprintln!("image: layer {id} ({name}) mask left off: {err}");
            return Value::Null;
        }
    };
    let raster = match clip.decode_raster(database, &source) {
        Ok(raster) if raster.data_state().is_present() => raster,
        Ok(_) => {
            eprintln!("image: layer {id} ({name}) mask is missing");
            return Value::Null;
        }
        Err(err) => {
            eprintln!("image: layer {id} ({name}) mask left off: {err}");
            return Value::Null;
        }
    };
    let gray = match to_coverage(&raster) {
        Ok(gray) => gray,
        Err(err) => {
            eprintln!("image: layer {id} ({name}) mask left off: {err}");
            return Value::Null;
        }
    };
    let file_name = format!("{id}.mask");
    if let Err(err) = fs::write(Path::new(directory).join(&file_name), &gray) {
        eprintln!("image: layer {id} ({name}) mask left off: {err}");
        return Value::Null;
    }
    eprintln!(
        "image: layer {id} ({name}) mask {}x{}",
        raster.width(),
        raster.height()
    );
    json!({
        "width": raster.width(),
        "height": raster.height(),
        "file": file_name,
    })
}

fn corner_json(corners: &[(f64, f64); 4]) -> (String, Value) {
    let text = corners
        .iter()
        .map(|(x, y)| format!("({x:.2},{y:.2})"))
        .collect::<Vec<_>>()
        .join(" ");
    let value = corners.iter().map(|(x, y)| json!([x, y])).collect();
    (text, Value::Array(value))
}

fn parse_placement(bytes: &[u8]) -> Result<Placement, String> {
    if bytes.len() < 12 {
        return Err(format!("image record is {} bytes", bytes.len()));
    }
    let header = be_u32(bytes, 0) as usize;
    let record = be_u32(bytes, 4) as usize;
    let count = be_u32(bytes, 8);
    if count != 4 {
        return Err(format!("corner count is {count}"));
    }
    if header < 120 || record < 16 {
        return Err(format!("image record header {header} record {record}"));
    }
    let end = header
        .checked_add(record.checked_mul(4).ok_or("corner list overflowed")?)
        .ok_or("corner list overflowed")?;
    if bytes.len() < end {
        return Err(format!("image record is {} bytes, need {end}", bytes.len()));
    }
    let width = be_u32(bytes, 32);
    let height = be_u32(bytes, 36);
    if width == 0 || height == 0 {
        return Err(format!("source size {width}x{height}"));
    }
    let mut corners = [(0.0, 0.0); 4];
    for index in 0..4 {
        let at = header + index * record;
        let x = be_f64(bytes, at);
        let y = be_f64(bytes, at + 8);
        if !x.is_finite() || !y.is_finite() {
            return Err("a corner is not finite".to_string());
        }
        corners[index] = (x, y);
    }
    Ok(Placement {
        width,
        height,
        corners,
    })
}

fn to_rgba(image: &RasterImage) -> Result<Vec<u8>, String> {
    let count = (image.width() as usize)
        .checked_mul(image.height() as usize)
        .ok_or("image is too large")?;
    let pixels = image.pixels();
    match image.format() {
        PixelFormat::Rgba8 => {
            if pixels.len() != count.saturating_mul(4) {
                return Err("rgba length does not match the bitmap".to_string());
            }
            Ok(pixels.to_vec())
        }
        PixelFormat::Gray8 => {
            if pixels.len() != count {
                return Err("gray length does not match the bitmap".to_string());
            }
            let mut out = vec![0_u8; count * 4];
            for (index, gray) in pixels.iter().copied().enumerate() {
                let at = index * 4;
                out[at] = gray;
                out[at + 1] = gray;
                out[at + 2] = gray;
                out[at + 3] = 255;
            }
            Ok(out)
        }
        PixelFormat::GrayAlpha8 => {
            if pixels.len() != count.saturating_mul(2) {
                return Err("gray-alpha length does not match the bitmap".to_string());
            }
            let mut out = vec![0_u8; count * 4];
            for (index, pair) in pixels.chunks_exact(2).enumerate() {
                let at = index * 4;
                out[at] = pair[0];
                out[at + 1] = pair[0];
                out[at + 2] = pair[0];
                out[at + 3] = pair[1];
            }
            Ok(out)
        }
        _ => Err("pixel format is not supported".to_string()),
    }
}

fn to_coverage(image: &RasterImage) -> Result<Vec<u8>, String> {
    let count = (image.width() as usize)
        .checked_mul(image.height() as usize)
        .ok_or("mask is too large")?;
    let pixels = image.pixels();
    match image.format() {
        PixelFormat::Gray8 => {
            if pixels.len() != count {
                return Err("mask length does not match the bitmap".to_string());
            }
            Ok(pixels.to_vec())
        }
        PixelFormat::GrayAlpha8 => {
            if pixels.len() != count.saturating_mul(2) {
                return Err("mask length does not match the bitmap".to_string());
            }
            Ok(pixels.chunks_exact(2).map(|pair| pair[0]).collect())
        }
        PixelFormat::Rgba8 => {
            if pixels.len() != count.saturating_mul(4) {
                return Err("mask length does not match the bitmap".to_string());
            }
            Ok(pixels.chunks_exact(4).map(|pixel| pixel[3]).collect())
        }
        _ => Err("mask format is not supported".to_string()),
    }
}

fn be_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

fn be_f64(bytes: &[u8], at: usize) -> f64 {
    f64::from_be_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
}
