//! Dump one .clip file's vector strokes as JSON.
//!
//! The pixels are not baked here. VMI reads this dump and stamps a preview.
//! Official SVG export is not used: it drops per-point taper and anti-alias.

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::process::ExitCode;

use clipfile::{
    ClipFile, Database, Document, FillStyle, Limits, VectorCurveKind, VectorData,
    VectorStrokeKind,
};
use serde_json::{Map, Value, json};

mod images;

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(first) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    if first == "images" {
        let Some(path) = args.next() else {
            usage();
            return ExitCode::from(2);
        };
        let Some(directory) = args.next() else {
            usage();
            return ExitCode::from(2);
        };
        if args.next().is_some() {
            usage();
            return ExitCode::from(2);
        }
        return images::write_images(&path, &directory);
    }
    if args.next().is_some() {
        usage();
        return ExitCode::from(2);
    }
    let path = first;
    match dump(&path) {
        Ok(value) => {
            let mut out = io::stdout().lock();
            if serde_json::to_writer(&mut out, &value).is_err() || out.write_all(b"\n").is_err() {
                eprintln!("vector: could not write json");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("vector: {err}");
            ExitCode::from(1)
        }
    }
}

fn usage() {
    eprintln!("usage: vmi-clip-vector <file.clip>");
    eprintln!("       vmi-clip-vector images <file.clip> <directory>");
}

fn dump(path: &std::ffi::OsStr) -> Result<Value, Box<dyn std::error::Error>> {
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
    let color_columns = color_columns(database.connection())?;
    let mut brush_cache: HashMap<u32, Option<(i64, f64, Option<i64>)>> = HashMap::new();
    let mut fill_cache: HashMap<u32, Option<i64>> = HashMap::new();
    let mut brush_warned = false;
    let mut seen_alias: HashMap<i64, u32> = HashMap::new();
    let mut seen_composite: HashMap<i64, u32> = HashMap::new();
    let root = canvas.root_layer_id();
    let layers = children(&document, root)
        .into_iter()
        .map(|id| {
            layer_json(
                &mut clip,
                &database,
                &document,
                id,
                limits,
                &color_columns,
                &mut brush_cache,
                &mut fill_cache,
                &mut brush_warned,
                &mut seen_alias,
                &mut seen_composite,
            )
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    if seen_alias.is_empty() {
        eprintln!("vector alias: no BrushStyle or FillStyle anti_alias on this file");
    } else {
        let mut pairs: Vec<_> = seen_alias.into_iter().collect();
        pairs.sort_by_key(|(value, _)| *value);
        for (value, count) in pairs {
            eprintln!("vector alias raw={value} count={count}");
        }
    }
    if !seen_composite.is_empty() {
        let mut pairs: Vec<_> = seen_composite.into_iter().collect();
        pairs.sort_by_key(|(value, _)| *value);
        for (value, count) in pairs {
            eprintln!("vector composite raw={value} count={count}");
        }
    }
    Ok(json!({
        "width": canvas.width(),
        "height": canvas.height(),
        "resolution": canvas.resolution(),
        "layers": layers,
    }))
}

fn children(document: &Document, id: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let mut current = document.layer(id).and_then(|layer| layer.first_child_id());
    let mut guard = 0u32;
    while let Some(next) = current {
        guard += 1;
        if guard > 100_000 {
            eprintln!("vector: layer sibling list did not end");
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
    limits: Limits,
    color_columns: &[String],
    brush_cache: &mut HashMap<u32, Option<(i64, f64, Option<i64>)>>,
    fill_cache: &mut HashMap<u32, Option<i64>>,
    brush_warned: &mut bool,
    seen_alias: &mut HashMap<i64, u32>,
    seen_composite: &mut HashMap<i64, u32>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let Some(layer) = document.layer(id) else {
        return Ok(json!({
            "id": id,
            "name": "",
            "folder": false,
            "monochrome": false,
            "color": {},
            "vector": null,
            "children": [],
        }));
    };
    let color = layer_color(database.connection(), id, color_columns)?;
    let monochrome = is_monochrome(&color);
    if let Some(index) = color.get("LayerColorTypeIndex").and_then(Value::as_i64) {
        if index != 0 {
            eprintln!("vector expression layer={id} LayerColorTypeIndex={index} monochrome={monochrome}");
        }
    }
    let vector = if layer.is_folder() {
        Value::Null
    } else {
        vector_json(
            clip,
            database,
            id,
            limits,
            brush_cache,
            fill_cache,
            brush_warned,
            seen_alias,
            seen_composite,
        )?
    };
    let children = children(document, id)
        .into_iter()
        .map(|child| {
            layer_json(
                clip,
                database,
                document,
                child,
                limits,
                color_columns,
                brush_cache,
                fill_cache,
                brush_warned,
                seen_alias,
                seen_composite,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "id": id,
        "name": layer.name().unwrap_or(""),
        "folder": layer.is_folder(),
        "monochrome": monochrome,
        "color": color,
        "vector": vector,
        "children": children,
    }))
}

fn vector_json(
    clip: &mut ClipFile<File>,
    database: &Database,
    layer_id: i64,
    limits: Limits,
    brush_cache: &mut HashMap<u32, Option<(i64, f64, Option<i64>)>>,
    fill_cache: &mut HashMap<u32, Option<i64>>,
    brush_warned: &mut bool,
    seen_alias: &mut HashMap<i64, u32>,
    seen_composite: &mut HashMap<i64, u32>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let sources = database.vector_data_sources(layer_id, limits)?;
    if sources.is_empty() {
        return Ok(Value::Null);
    }
    let mut strokes = Vec::new();
    for source in &sources {
        let data = match clip.read_vector(database, source, limits) {
            Ok(data) => data,
            Err(err) => {
                let message = err.to_string();
                eprintln!(
                    "vector: layer {layer_id} object {} rejected: {message}",
                    source.id()
                );
                return Ok(json!({
                    "error": message,
                    "object_id": source.id(),
                    "strokes": [],
                }));
            }
        };
        push_strokes(
            &data,
            database,
            limits,
            brush_cache,
            fill_cache,
            brush_warned,
            seen_alias,
            seen_composite,
            layer_id,
            &mut strokes,
        )?;
    }
    Ok(json!({
        "error": null,
        "strokes": strokes,
    }))
}

fn push_strokes(
    data: &VectorData,
    database: &Database,
    limits: Limits,
    brush_cache: &mut HashMap<u32, Option<(i64, f64, Option<i64>)>>,
    fill_cache: &mut HashMap<u32, Option<i64>>,
    brush_warned: &mut bool,
    seen_alias: &mut HashMap<i64, u32>,
    seen_composite: &mut HashMap<i64, u32>,
    layer_id: i64,
    strokes: &mut Vec<Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut varied_scale = 0u32;
    let mut counted_scale = 0u32;
    for (index, stroke) in data.strokes().enumerate() {
        let Some(style) = stroke.style() else {
            eprintln!("vector: layer {layer_id} stroke {index} has no supported style");
            strokes.push(json!({
                "error": "unsupported stroke layout",
                "index": index,
            }));
            continue;
        };
        if matches!(style.kind(), VectorStrokeKind::Ruler) {
            eprintln!("vector: layer {layer_id} stroke {index} is a ruler");
            continue;
        }
        let Some(curve) = stroke.curve_kind() else {
            eprintln!("vector: layer {layer_id} stroke {index} has no curve kind");
            strokes.push(json!({
                "error": "unsupported curve",
                "index": index,
            }));
            continue;
        };
        // brush_style_id is BrushStyle.MainId on modern headers. Older
        // headers only have the common brush id.
        let brush_style_id = style.brush_style_id().unwrap_or(style.brush_id());
        let brush = brush_alias(database, brush_style_id, limits, brush_cache, brush_warned)?;
        let fill_id = style.fill_style_id();
        let fill_alias = match fill_id {
            Some(id) => fill_alias(database, id, fill_cache)?,
            None => None,
        };
        let (anti_alias, alias_source, hardness) = match (fill_alias, brush) {
            (Some(alias), Some((_, hardness, _))) => (Some(alias), "fill", Some(hardness)),
            (Some(alias), None) => (Some(alias), "fill", None),
            (None, Some((alias, hardness, _))) => (Some(alias), "brush", Some(hardness)),
            (None, None) => (None, "missing", None),
        };
        let composite = brush.and_then(|(_, _, mode)| mode);
        if let Some(mode) = composite {
            if mode != 0 {
                *seen_composite.entry(mode).or_insert(0) += 1;
            }
        }
        if let Some(alias) = anti_alias {
            *seen_alias.entry(alias).or_insert(0) += 1;
        }
        let mut points = Vec::new();
        let mut point_error = false;
        let mut scale_min = f32::MAX;
        let mut scale_max = 0f32;
        let mut scale_count = 0u32;
        for point in stroke.points() {
            let (Some(position), Some(width), Some(opacity)) =
                (point.position(), point.width_factor(), point.opacity_factor())
            else {
                point_error = true;
                break;
            };
            let scale = width_scale(point.raw());
            if let Some(scale) = scale {
                scale_min = scale_min.min(scale);
                scale_max = scale_max.max(scale);
                scale_count += 1;
            }
            let controls: Vec<Value> = point
                .control_points()
                .map(|control| json!([control.x(), control.y()]))
                .collect();
            points.push(json!({
                "x": position.x(),
                "y": position.y(),
                "width_factor": width,
                "width_scale": scale,
                "opacity_factor": opacity,
                "controls": controls,
            }));
        }
        if point_error {
            eprintln!("vector: layer {layer_id} stroke {index} has an unsupported point");
            strokes.push(json!({
                "error": "unsupported point",
                "index": index,
            }));
            continue;
        }
        let color = style.main_color();
        strokes.push(json!({
            "error": null,
            "index": index,
            "kind": kind_name(style.kind()),
            "curve": curve_name(curve),
            "closed": stroke.is_closed(),
            "brush_radius": style.brush_radius(),
            "brush_style_id": brush_style_id,
            "brush_id": style.brush_id(),
            "fill_style_id": fill_id,
            "color": [color.red_8bit(), color.green_8bit(), color.blue_8bit()],
            "opacity": style.opacity(),
            "composite": composite,
            "anti_alias": anti_alias,
            "hardness": hardness,
            "alias_source": alias_source,
            "points": points,
        }));
        if scale_count > 0 {
            counted_scale += 1;
            if scale_max - scale_min > 0.001 {
                varied_scale += 1;
            }
        }
    }
    if counted_scale > 0 {
        eprintln!("vector width scale layer={layer_id} varied={varied_scale} of {counted_scale}");
    }
    Ok(())
}

/// Stored per-point width envelope at byte 36.
///
/// Untapered strokes keep this at 1. Pen strokes drop toward 0 at the ends.
/// It multiplies `width_factor`; it is not a pixel width.
fn width_scale(raw: &[u8]) -> Option<f32> {
    let bytes = raw.get(36..40)?;
    let value = f32::from_bits(u32::from_be_bytes(bytes.try_into().ok()?));
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn kind_name(kind: VectorStrokeKind) -> &'static str {
    match kind {
        VectorStrokeKind::Normal => "normal",
        VectorStrokeKind::Ruler => "ruler",
        VectorStrokeKind::Frame => "frame",
        _ => "other",
    }
}

fn curve_name(kind: VectorCurveKind) -> &'static str {
    match kind {
        VectorCurveKind::Straight => "straight",
        VectorCurveKind::Quadratic => "quadratic",
        VectorCurveKind::Cubic => "cubic",
        VectorCurveKind::Spline => "spline",
        _ => "other",
    }
}

fn brush_alias(
    database: &Database,
    id: u32,
    limits: Limits,
    cache: &mut HashMap<u32, Option<(i64, f64, Option<i64>)>>,
    warned: &mut bool,
) -> Result<Option<(i64, f64, Option<i64>)>, Box<dyn std::error::Error>> {
    if let Some(hit) = cache.get(&id) {
        return Ok(*hit);
    }
    let typed = match database.brush_style(id, limits) {
        Ok(Some(style)) => Some((
            style.anti_alias(),
            style.hardness(),
            Some(style.composite_mode()),
        )),
        Ok(None) => None,
        Err(err) => {
            if !*warned {
                eprintln!("vector: BrushStyle reader: {err}; reading AntiAlias directly");
                *warned = true;
            }
            None
        }
    };
    // Older files omit columns the typed reader requires. AntiAlias,
    // Hardness, and CompositeMode are still on the row. CompositeMode 27
    // is the ink Erase mode: a transparent vector stroke, not white paint.
    let found = if typed.is_some() {
        typed
    } else {
        sql_brush(database.connection(), id)?
    };
    cache.insert(id, found);
    Ok(found)
}

fn sql_brush(
    connection: &rusqlite::Connection,
    id: u32,
) -> Result<Option<(i64, f64, Option<i64>)>, Box<dyn std::error::Error>> {
    let mut statement = match connection.prepare(
        "SELECT AntiAlias, Hardness, CompositeMode FROM BrushStyle WHERE MainId = ?1",
    ) {
        Ok(statement) => statement,
        Err(err) => {
            eprintln!("vector: BrushStyle anti_alias: {err}");
            return Ok(None);
        }
    };
    let mut rows = statement.query([id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let alias: Option<i64> = row.get(0)?;
    let hardness: Option<f64> = row.get(1)?;
    let composite: Option<i64> = row.get(2)?;
    Ok(alias.map(|alias| (alias, hardness.unwrap_or(1.0), composite)))
}

fn fill_alias(
    database: &Database,
    id: u32,
    cache: &mut HashMap<u32, Option<i64>>,
) -> Result<Option<i64>, Box<dyn std::error::Error>> {
    if let Some(hit) = cache.get(&id) {
        return Ok(*hit);
    }
    let found = match database.fill_style(id) {
        Ok(Some(style)) => Some(FillStyle::anti_alias(style)),
        Ok(None) => None,
        Err(err) => {
            eprintln!("vector: fill style {id}: {err}");
            None
        }
    };
    cache.insert(id, found);
    Ok(found)
}

fn color_columns(connection: &rusqlite::Connection) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut statement = connection.prepare("PRAGMA table_info(Layer)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut names = Vec::new();
    for row in rows {
        let name = row?;
        let low = name.to_ascii_lowercase();
        if low.contains("color") || low.contains("mono") || low.contains("grey") || low.contains("gray") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn layer_color(
    connection: &rusqlite::Connection,
    id: i64,
    columns: &[String],
) -> Result<Map<String, Value>, Box<dyn std::error::Error>> {
    let mut out = Map::new();
    if columns.is_empty() {
        return Ok(out);
    }
    let selected = columns.join(", ");
    let sql = format!("SELECT {selected} FROM Layer WHERE MainId = ?1");
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query([id])?;
    let Some(row) = rows.next()? else {
        return Ok(out);
    };
    for (index, name) in columns.iter().enumerate() {
        out.insert(name.clone(), sql_value(row.get_ref(index)?)?);
    }
    Ok(out)
}

fn sql_value(value: rusqlite::types::ValueRef<'_>) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(match value {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(number) => json!(number),
        rusqlite::types::ValueRef::Real(number) => json!(number),
        rusqlite::types::ValueRef::Text(bytes) => json!(String::from_utf8_lossy(bytes)),
        rusqlite::types::ValueRef::Blob(_) => json!("blob"),
    })
}

fn is_monochrome(color: &Map<String, Value>) -> bool {
    // Expression color is LayerColorTypeIndex. Ordinary color layers store 0,
    // even when the black and white checks are both set. MonochromeFillInfo is
    // a separate blob (paper uses one) and is not the expression mode.
    // 2 is the monochrome expression. Gray (1) keeps the stroke's anti-alias.
    matches!(color.get("LayerColorTypeIndex"), Some(Value::Number(number)) if number.as_i64() == Some(2))
}
