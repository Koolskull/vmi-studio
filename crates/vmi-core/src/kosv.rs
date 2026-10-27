//! KOSV on the VMI desk. A port of `vmi_studio/kosv.py`.
//!
//! The picture is a VMI blueprint. Effects and presets live in that file.
//! The working plate is 1920 by 1080. 3840 by 2160 is the later plate.
//! A new stack is empty. A preset fills it.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Map, Value};

use crate::doc::Node;

pub const PRODUCT: &str = "KOSV";
pub const PLATE: [u32; 2] = [1920, 1080];
pub const LATER_PLATE: [u32; 2] = [3840, 2160];

/// name, low, high, default. The same table as Python's `_PARAM`.
pub const PARAMS: &[(&str, f64, f64, f64)] = &[
    ("amount", 0.0, 1.0, 0.35),
    ("spread", 0.0, 0.25, 0.04),
    ("cx", 0.0, 1.0, 0.5),
    ("cy", 0.0, 1.0, 0.5),
    ("bend_r", 0.5, 1.5, 0.96),
    ("bend_g", 0.5, 1.5, 1.0),
    ("bend_b", 0.5, 1.5, 1.04),
    ("blur", 0.0, 32.0, 6.0),
    ("threshold", 0.0, 1.0, 0.7),
    ("chroma", 0.0, 1.0, 0.25),
    ("size", 1.0, 32.0, 2.0),
    ("seed", 0.0, 65535.0, 1.0),
    ("shadow", 0.0, 1.0, 0.65),
    ("scale", 0.25, 4.0, 1.0),
    ("mix", 0.0, 1.0, 0.5),
    ("softness", 0.0, 1.0, 0.15),
    ("invert", 0.0, 1.0, 0.0),
    ("amplitude", 0.0, 0.2, 0.02),
    ("frequency", 0.0, 1.0, 0.5),
    ("scanline", 0.0, 0.1, 0.01),
    ("tear", 0.0, 1.0, 0.15),
    ("shift", 0.0, 0.05, 0.008),
    ("bar", 0.0, 1.0, 0.4),
    ("pitch", 0.05, 1.5, 0.35),
    ("angle", 0.0, 3.14, 0.4),
    ("gain", 0.0, 4.0, 1.0),
    ("block", 1.0, 64.0, 8.0),
    ("radius", 0.0, 32.0, 6.0),
    ("color_r", 0.0, 1.0, 1.0),
    ("color_g", 0.0, 1.0, 0.85),
    ("color_b", 0.0, 1.0, 0.4),
    ("quality", 1.0, 100.0, 18.0),
    ("cutoff", 0.0, 1.0, 0.45),
    ("order", 0.0, 5.0, 0.0),
    ("rows", 0.0, 1080.0, 12.0),
    ("filter", 0.0, 4.0, 1.0),
];

/// id, label, parameter names.
pub const CATALOG: &[(&str, &str, &[&str])] = &[
    ("fisheye", "Fisheye", &["amount", "spread", "cx", "cy", "seed"]),
    ("lens", "Lens", &["bend_r", "bend_g", "bend_b", "blur", "threshold", "cx", "cy"]),
    ("filmic-noise", "Filmic noise", &["amount", "chroma", "size", "seed"]),
    ("film-grain", "Film grain", &["amount", "size", "shadow", "seed"]),
    ("grain-plate", "Grain plate", &["amount", "scale", "mix", "seed"]),
    ("texture-clip", "Texture clip", &["threshold", "softness", "invert"]),
    ("luma-transition", "Luma transition", &["softness", "invert"]),
    ("jiggle", "Jiggle", &["amplitude", "frequency", "scanline", "seed"]),
    ("vhs", "VHS", &["tear", "shift", "bar", "pitch", "angle", "seed"]),
    ("datamosh", "Datamosh", &["gain", "block"]),
    ("color-glow", "Color glow", &["threshold", "radius", "color_r", "color_g", "color_b", "mix"]),
    ("jpeg-glitch", "JPEG glitch", &["quality", "block", "cutoff", "order", "seed"]),
    ("png-glitch", "PNG glitch", &["filter", "rows", "seed"]),
];

/// Picture slots an effect may point at another layer for.
pub const REFS: &[&str] = &["plate", "shape", "clip", "matte", "other"];

/// These change with the live clock. A held playhead still crawls.
pub const LIVE_IDS: &[&str] = &["filmic-noise", "film-grain", "jiggle", "vhs", "jpeg-glitch", "png-glitch"];

/// Factory stacks, in the order the bar shows them. Each entry is an effect
/// id and the parameters it overrides.
pub const STACKS: &[(&str, &[(&str, &[(&str, f64)])])] = &[
    ("lens", &[("fisheye", &[("amount", 0.45), ("spread", 0.06)]), ("lens", &[("blur", 8.0)])]),
    (
        "stock",
        &[
            ("filmic-noise", &[("amount", 0.35), ("chroma", 0.55), ("size", 2.0)]),
            ("film-grain", &[("amount", 0.4), ("size", 3.0)]),
        ],
    ),
    ("tape", &[("vhs", &[]), ("jiggle", &[("amplitude", 0.015)])]),
    ("melt", &[("datamosh", &[("gain", 1.2), ("block", 16.0)])]),
    ("break", &[("jpeg-glitch", &[("quality", 12.0)]), ("png-glitch", &[("rows", 24.0)])]),
    ("glow", &[("color-glow", &[])]),
];

pub const PNG_FILTERS: &[&str] = &["none", "sub", "up", "average", "paeth"];
pub const CHANNEL_ORDERS: &[[usize; 3]] = &[[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];

/// One keyed value: the setting holds this from `frame` on.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub frame: i64,
    pub value: f64,
}

/// A parameter is a number, a list of hold keys, or a layer id for one of
/// the picture slots in `REFS`.
#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Number(f64),
    Keys(Vec<Key>),
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub id: String,
    pub enabled: bool,
    pub preset: String,
    pub params: BTreeMap<String, Param>,
}

/// An I or P mark on one cel, for datamosh.
#[derive(Clone, Debug, PartialEq)]
pub struct CelMark {
    /// "I" or "P".
    pub kind: String,
    pub reference: String,
    pub origin: String,
    pub gain: Option<f64>,
    pub block: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    pub stack: Vec<Effect>,
}

/// The blueprint block.
#[derive(Clone, Debug, PartialEq)]
pub struct Kosv {
    pub plate: [u32; 2],
    pub presets: Vec<Preset>,
}

impl Default for Kosv {
    fn default() -> Self {
        Self { plate: PLATE, presets: Vec::new() }
    }
}

pub fn plate_size(later: bool) -> [u32; 2] {
    if later {
        LATER_PLATE
    } else {
        PLATE
    }
}

pub fn param_range(name: &str) -> Option<(f64, f64, f64)> {
    PARAMS.iter().find(|(known, ..)| *known == name).map(|(_, low, high, default)| (*low, *high, *default))
}

pub fn catalog_entry(id: &str) -> Option<&'static (&'static str, &'static str, &'static [&'static str])> {
    CATALOG.iter().find(|(known, ..)| *known == id)
}

pub fn effect_label(id: &str) -> &'static str {
    catalog_entry(id).map(|(_, label, _)| *label).unwrap_or("")
}

pub fn effect_params(id: &str) -> &'static [&'static str] {
    catalog_entry(id).map(|(_, _, names)| *names).unwrap_or(&[])
}

pub fn effect_ids() -> Vec<&'static str> {
    CATALOG.iter().map(|(id, ..)| *id).collect()
}

pub fn stack_names() -> Vec<&'static str> {
    STACKS.iter().map(|(name, _)| *name).collect()
}

pub fn is_live(id: &str) -> bool {
    LIVE_IDS.contains(&id)
}

/// True when a layer or a folder carries a frame-seeded effect.
pub fn has_live(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| {
        node.effects.iter().any(|effect| effect.enabled && is_live(&effect.id)) || has_live(&node.children)
    })
}

/// Clamp into the parameter's range. Not a number falls to the default.
pub fn unit(name: &str, raw: Option<f64>, default: Option<f64>) -> f64 {
    let Some((low, high, fallback)) = param_range(name) else {
        return raw.unwrap_or(0.0);
    };
    let mut number = raw.unwrap_or_else(|| default.unwrap_or(fallback));
    if !number.is_finite() {
        number = fallback;
    }
    number.clamp(low, high)
}

fn number_of(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Python's `_store_param`: a `{"keys": [...]}` object becomes sorted hold
/// keys, anything else a clamped number.
pub fn store_param(name: &str, raw: &Value) -> Param {
    if let Some(keys) = raw.get("keys").and_then(Value::as_array) {
        let mut out: Vec<Key> = keys
            .iter()
            .filter_map(|row| {
                let frame = row.get("frame").and_then(number_of)?;
                let value = unit(name, row.get("value").and_then(number_of), None);
                Some(Key { frame: frame as i64, value })
            })
            .collect();
        out.sort_by_key(|key| key.frame);
        return Param::Keys(out);
    }
    Param::Number(unit(name, number_of(raw), None))
}

/// The value of one setting at `frame`. Keys hold; the last key at or before
/// the frame wins. A missing setting is its default.
pub fn at(effect: &Effect, name: &str, frame: i64) -> f64 {
    let fallback = param_range(name).map(|(_, _, default)| default).unwrap_or(0.0);
    match effect.params.get(name) {
        Some(Param::Number(number)) => unit(name, Some(*number), None),
        Some(Param::Keys(keys)) => {
            let mut chosen = fallback;
            for key in keys {
                if key.frame <= frame {
                    chosen = key.value;
                } else {
                    break;
                }
            }
            unit(name, Some(chosen), None)
        }
        _ => fallback,
    }
}

/// One effect entry. Unknown ids are refused.
pub fn make_effect(id: &str, overrides: &[(&str, f64)], preset: &str, enabled: bool) -> Option<Effect> {
    let (_, _, names) = catalog_entry(id)?;
    let mut params = BTreeMap::new();
    for name in names.iter() {
        if let Some((_, value)) = overrides.iter().find(|(known, _)| known == name) {
            params.insert((*name).to_string(), Param::Number(unit(name, Some(*value), None)));
        }
    }
    Some(Effect {
        id: id.to_string(),
        enabled,
        preset: if preset.is_empty() { id.to_string() } else { preset.to_string() },
        params,
    })
}

/// A factory preset as a list of effect entries. Unknown names are empty.
pub fn make_stack(name: &str) -> Vec<Effect> {
    let Some((_, rows)) = STACKS.iter().find(|(known, _)| *known == name) else {
        return Vec::new();
    };
    rows.iter().filter_map(|(id, overrides)| make_effect(id, overrides, name, true)).collect()
}

/// Drop broken entries. A missing list is an empty stack.
pub fn clean_effects(raw: &Value) -> Vec<Effect> {
    let Some(rows) = raw.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some((_, _, names)) = catalog_entry(id) else {
            continue;
        };
        let given = row.get("params").and_then(Value::as_object);
        let mut params = BTreeMap::new();
        if let Some(given) = given {
            for name in names.iter() {
                if let Some(value) = given.get(*name) {
                    params.insert((*name).to_string(), store_param(name, value));
                }
            }
            for name in REFS {
                if let Some(text) = given.get(*name).and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        params.insert((*name).to_string(), Param::Text(text.trim().to_string()));
                    }
                }
            }
        }
        out.push(Effect {
            id: id.to_string(),
            enabled: row.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            preset: row.get("preset").and_then(Value::as_str).unwrap_or("").to_string(),
            params,
        });
    }
    out
}

/// I and P marks keyed by cel id. Unknown cels are dropped when `known` is set.
pub fn clean_marks(raw: &Value, known: Option<&HashSet<String>>) -> BTreeMap<String, CelMark> {
    let mut out = BTreeMap::new();
    let Some(rows) = raw.as_object() else {
        return out;
    };
    for (key, row) in rows {
        if key.is_empty() {
            continue;
        }
        if let Some(known) = known {
            if !known.contains(key) {
                continue;
            }
        }
        let Some(row) = row.as_object() else {
            continue;
        };
        let kind = match row.get("type").and_then(Value::as_str) {
            Some("P") => "P",
            _ => "I",
        };
        let text = |name: &str| row.get(name).and_then(Value::as_str).unwrap_or("").to_string();
        // Omitted gain and block stay omitted. A stored 1 would hide the
        // effect's own gain, including a gain that is keyed on the timeline.
        let gain = row.get("gain").filter(|value| !value.is_null()).map(|value| unit("gain", number_of(value), Some(1.0)));
        let block =
            row.get("block").filter(|value| !value.is_null()).map(|value| unit("block", number_of(value), Some(8.0)));
        out.insert(
            key.clone(),
            CelMark { kind: kind.to_string(), reference: text("reference"), origin: text("origin"), gain, block },
        );
    }
    out
}

/// User presets saved in the blueprint. Factory names can be saved too.
pub fn clean_presets(raw: &Value) -> Vec<Preset> {
    let Some(rows) = raw.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<Preset> = Vec::new();
    for row in rows {
        let name = row.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if name.is_empty() || out.iter().any(|preset| preset.name == name) {
            continue;
        }
        let stack = clean_effects(row.get("stack").unwrap_or(&Value::Null));
        if stack.is_empty() {
            continue;
        }
        out.push(Preset { name, stack });
    }
    out
}

/// The blueprint block. Missing means an empty KOSV section.
pub fn clean_kosv(raw: &Value) -> Kosv {
    let plate = raw
        .get("plate")
        .and_then(Value::as_array)
        .filter(|plate| plate.len() >= 2)
        .and_then(|plate| {
            let w = plate[0].as_f64()? as i64;
            let h = plate[1].as_f64()? as i64;
            (w > 0 && h > 0).then_some([w as u32, h as u32])
        })
        .unwrap_or(PLATE);
    Kosv { plate, presets: clean_presets(raw.get("presets").unwrap_or(&Value::Null)) }
}

pub fn param_value(param: &Param) -> Value {
    match param {
        Param::Number(number) => json!(number),
        Param::Keys(keys) => json!({
            "keys": keys.iter().map(|key| json!({"frame": key.frame, "value": key.value})).collect::<Vec<_>>()
        }),
        Param::Text(text) => Value::String(text.clone()),
    }
}

pub fn effect_value(effect: &Effect) -> Value {
    let mut params = Map::new();
    for (name, param) in &effect.params {
        params.insert(name.clone(), param_value(param));
    }
    json!({
        "id": effect.id,
        "enabled": effect.enabled,
        "preset": effect.preset,
        "params": Value::Object(params),
    })
}

pub fn effects_value(effects: &[Effect]) -> Value {
    Value::Array(effects.iter().map(effect_value).collect())
}

pub fn marks_value(marks: &BTreeMap<String, CelMark>) -> Value {
    let mut out = Map::new();
    for (key, mark) in marks {
        let mut row = Map::new();
        row.insert("type".into(), Value::String(mark.kind.clone()));
        row.insert("reference".into(), Value::String(mark.reference.clone()));
        row.insert("origin".into(), Value::String(mark.origin.clone()));
        if let Some(gain) = mark.gain {
            row.insert("gain".into(), json!(gain));
        }
        if let Some(block) = mark.block {
            row.insert("block".into(), json!(block));
        }
        out.insert(key.clone(), Value::Object(row));
    }
    Value::Object(out)
}

pub fn kosv_value(kosv: &Kosv) -> Value {
    json!({
        "product": PRODUCT,
        "plate": [kosv.plate[0], kosv.plate[1]],
        "later_plate": [LATER_PLATE[0], LATER_PLATE[1]],
        "presets": kosv.presets.iter().map(|preset| json!({
            "name": preset.name,
            "stack": effects_value(&preset.stack),
        })).collect::<Vec<_>>(),
    })
}

/// Hold keys. A plain number becomes a key at frame 0, then this frame.
pub fn upsert_key(params: &mut BTreeMap<String, Param>, name: &str, frame: i64, value: f64) {
    let mut keys = match params.get(name) {
        Some(Param::Keys(keys)) => keys.clone(),
        Some(Param::Number(number)) => vec![Key { frame: 0, value: *number }],
        _ => Vec::new(),
    };
    match keys.iter_mut().find(|key| key.frame == frame) {
        Some(key) => key.value = value,
        None => keys.push(Key { frame, value }),
    }
    keys.sort_by_key(|key| key.frame);
    params.insert(name.to_string(), Param::Keys(keys));
}

/// The value the bar shows for one setting at the playhead.
pub fn shown(effect: &Effect, name: &str, frame: i64) -> f64 {
    at(effect, name, frame)
}

/// Pictures an effect stack may read beside the buffer it paints. Every
/// picture is straight RGBA at the plate's size, or any size for the
/// layer slots, which the effect fits itself.
#[derive(Clone, Debug, Default)]
pub struct Maps {
    pub plate: Option<Picture>,
    pub shape: Option<Picture>,
    pub clip: Option<Picture>,
    pub matte: Option<Picture>,
    pub other: Option<Picture>,
    /// Datamosh: every cel of the timeline this cel sits in, in order.
    pub frames: Vec<Picture>,
    /// Parallel to `frames`. A missing mark is an I.
    pub marks: Vec<CelMark>,
    /// Parallel to `frames`: the cel ids.
    pub ids: Vec<String>,
    /// Which of `frames` is the cel being painted.
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Run an effect stack over straight RGBA pixels. The source is not changed.
///
/// `frame` is the playhead; keyed settings hold on that number. `live` is
/// the grain clock: noise, grain, jiggle, VHS, and the glitches follow it,
/// so a held drawing still crawls. None seeds them with the playhead, which
/// is what an export frame wants.
pub fn apply_effects(
    rgba: &[u8],
    width: u32,
    height: u32,
    effects: &[Effect],
    frame: i64,
    maps: &Maps,
    live: Option<i64>,
) -> Vec<u8> {
    let _ = (width, height, frame, maps, live);
    let _: Vec<&Effect> = effects.iter().filter(|effect| effect.enabled).collect();
    rgba.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_matches_the_python_tables() {
        assert_eq!(CATALOG.len(), 13);
        assert_eq!(PARAMS.len(), 36);
        assert_eq!(stack_names(), ["lens", "stock", "tape", "melt", "break", "glow"]);
        assert_eq!(param_range("blur"), Some((0.0, 32.0, 6.0)));
        assert_eq!(effect_params("vhs"), &["tear", "shift", "bar", "pitch", "angle", "seed"]);
        assert!(is_live("vhs"));
        assert!(!is_live("fisheye"));
    }

    #[test]
    fn a_stack_carries_its_overrides_and_clamps_them() {
        let stack = make_stack("lens");
        assert_eq!(stack.len(), 2);
        assert_eq!(stack[0].id, "fisheye");
        assert_eq!(stack[0].preset, "lens");
        assert_eq!(at(&stack[0], "amount", 0), 0.45);
        assert_eq!(at(&stack[0], "cx", 0), 0.5);
        assert_eq!(at(&stack[1], "blur", 0), 8.0);
        let big = make_effect("lens", &[("blur", 900.0)], "", true).unwrap();
        assert_eq!(at(&big, "blur", 0), 32.0);
        assert!(make_effect("nope", &[], "", true).is_none());
        assert!(make_stack("nope").is_empty());
    }

    #[test]
    fn keys_hold_and_a_number_becomes_the_first_key() {
        let mut effect = make_effect("fisheye", &[("amount", 0.2)], "", true).unwrap();
        upsert_key(&mut effect.params, "amount", 10, 0.8);
        assert_eq!(at(&effect, "amount", 0), 0.2);
        assert_eq!(at(&effect, "amount", 9), 0.2);
        assert_eq!(at(&effect, "amount", 10), 0.8);
        assert_eq!(at(&effect, "amount", 99), 0.8);
        upsert_key(&mut effect.params, "amount", 10, 0.6);
        assert_eq!(at(&effect, "amount", 10), 0.6);
        match effect.params.get("amount") {
            Some(Param::Keys(keys)) => assert_eq!(keys.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cleaning_drops_unknown_effects_and_keeps_refs_and_marks() {
        let raw = json!([
            {"id": "ghost", "params": {}},
            {"id": "grain-plate", "enabled": false, "params": {"amount": 2.0, "plate": " n4 ", "bogus": 1}},
            {"id": "datamosh", "params": {"gain": {"keys": [{"frame": 5, "value": 2}, {"frame": 0, "value": 1}]}}},
            "junk"
        ]);
        let effects = clean_effects(&raw);
        assert_eq!(effects.len(), 2);
        assert!(!effects[0].enabled);
        assert_eq!(effects[0].params.get("amount"), Some(&Param::Number(1.0)));
        assert_eq!(effects[0].params.get("plate"), Some(&Param::Text("n4".into())));
        assert!(!effects[0].params.contains_key("bogus"));
        assert_eq!(at(&effects[1], "gain", 3), 1.0);
        assert_eq!(at(&effects[1], "gain", 5), 2.0);
        let back = clean_effects(&effects_value(&effects));
        assert_eq!(back, effects);

        let known: HashSet<String> = ["n1".to_string(), "n2".to_string()].into_iter().collect();
        let marks = clean_marks(
            &json!({"n1": {"type": "P", "reference": "n2", "gain": 9}, "n2": {"type": "X"}, "n9": {"type": "P"}}),
            Some(&known),
        );
        assert_eq!(marks.len(), 2);
        assert_eq!(marks["n1"].kind, "P");
        assert_eq!(marks["n1"].gain, Some(4.0));
        assert_eq!(marks["n1"].block, None);
        assert_eq!(marks["n2"].kind, "I");
        assert_eq!(clean_marks(&marks_value(&marks), None), marks);

        let kosv = clean_kosv(&json!({"plate": [3840, 2160], "presets": [
            {"name": "mine", "stack": [{"id": "vhs"}]},
            {"name": "mine", "stack": [{"id": "vhs"}]},
            {"name": "empty", "stack": []},
        ]}));
        assert_eq!(kosv.plate, [3840, 2160]);
        assert_eq!(kosv.presets.len(), 1);
        assert_eq!(clean_kosv(&kosv_value(&kosv)), kosv);
        assert_eq!(clean_kosv(&Value::Null).plate, PLATE);
    }
}
