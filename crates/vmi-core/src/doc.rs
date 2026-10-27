//! The open drawing. Index 0 is the back of a list. The front is the last child.
//!
//! The picture is built the way `vmi_studio/composite.py` builds it: a hidden
//! or muted row drops with everything inside it, solo keeps only soloed
//! branches, a clipping mask stays inside the row it clips onto, a pass
//! through folder paints its children on the picture below, and an animation
//! folder shows one cel, the one `anim::cel_at` holds at the playhead. Onion
//! skin (`vmi_studio/onion.py`) is a view: `present` tints the neighbour cels
//! when `Art::onion` is on, `present_plain` never does.

use std::collections::BTreeMap;

use crate::anim::{
    animation_folder_ids, cel_at, onion_neighbors, onion_strength, ExposureSheet, DEFAULT_FPS, FUTURE, ONION_LIMIT,
    ONION_TINT, PAST,
};
use crate::blend::{mix, mix_hsl};
use crate::color::Rgba;
use crate::coverage::source_over;
use crate::kosv::{CelMark, Effect, Kosv};
use crate::tasks::{Task, TaskColors};
use crate::vector::{Raster, VectorStroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Layer,
    Group,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    pub visible: bool,
    pub opacity: u8,
    pub blend: String,
    pub clip: bool,
    pub mute: bool,
    pub solo: bool,
    pub marker: String,
    pub color: String,
    pub vectors: Vec<Vec<[f32; 2]>>,
    pub strokes: Vec<VectorStroke>,
    /// Raster-tool pixels, full canvas. Empty means clear.
    pub pixels: Vec<u8>,
    /// When set, `pixels` already includes the baked strokes.
    pub embedded: bool,
    pub children: Vec<Node>,
    /// A folder marked as a timeline. Each direct child is one cel.
    pub animation: bool,
    /// Cel ids in timeline order. Empty means the panel order, front first.
    pub timeline: Vec<String>,
    /// Start frame per cel, parallel to `timeline`. Empty means 0, 1, 2…
    pub frames: Vec<u32>,
    /// OpenToonz style exposure column. None when the layer has no sheet.
    pub exposure: Option<ExposureSheet>,
    /// The KOSV stack on this layer or folder. A folder's stack runs after
    /// the layers inside it.
    pub effects: Vec<Effect>,
    /// Datamosh I and P marks keyed by cel id, on an animation folder.
    pub cel_marks: BTreeMap<String, CelMark>,
}

impl Node {
    pub fn layer(id: impl Into<String>, name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: NodeKind::Layer,
            visible: true,
            opacity: 255,
            blend: "normal".into(),
            clip: false,
            mute: false,
            solo: false,
            marker: String::new(),
            color: String::new(),
            vectors: Vec::new(),
            strokes: Vec::new(),
            pixels: vec![0; (width as usize).saturating_mul(height as usize).saturating_mul(4)],
            embedded: false,
            children: Vec::new(),
            animation: false,
            timeline: Vec::new(),
            frames: Vec::new(),
            exposure: None,
            effects: Vec::new(),
            cel_marks: BTreeMap::new(),
        }
    }

    pub fn group(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: NodeKind::Group,
            visible: true,
            opacity: 255,
            blend: "normal".into(),
            clip: false,
            mute: false,
            solo: false,
            marker: String::new(),
            color: String::new(),
            vectors: Vec::new(),
            strokes: Vec::new(),
            pixels: Vec::new(),
            embedded: false,
            children: Vec::new(),
            animation: false,
            timeline: Vec::new(),
            frames: Vec::new(),
            exposure: None,
            effects: Vec::new(),
            cel_marks: BTreeMap::new(),
        }
    }

    pub fn is_layer(&self) -> bool {
        self.kind == NodeKind::Layer
    }

    pub fn is_group(&self) -> bool {
        self.kind == NodeKind::Group
    }
}

/// Python's `clip_time` row without the two values that live on `Art`
/// (`fps` and `playhead`). `end` is the last frame of the cut, inclusive,
/// the way Clip Studio and `filmstrip.cut_range` count it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipTime {
    pub name: String,
    pub start: u32,
    pub end: u32,
}

impl Default for ClipTime {
    fn default() -> Self {
        Self { name: "Timeline".into(), start: 0, end: 0 }
    }
}

/// One thing in the work: a door, a beetle, a shop, a word. The same row
/// Python's `desk.DeskObject` keeps and the blueprint's `objects` carry.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DeskObject {
    pub id: String,
    pub name: String,
    /// Chosen on the outliner: "", "static", "button", "sequence", or
    /// "warp target". Empty follows the layers.
    pub kind: String,
    /// Paint layers in the object.
    pub layer_ids: Vec<String>,
    /// (state, frame) per layer id when the name does not say it.
    pub assign: BTreeMap<String, (u16, u16)>,
    /// "anim:<stem>", "folder:<id>", "slots:<id>", or empty for a hand-made object.
    pub origin: String,
    /// Custom position names keyed by SSSXXX.
    pub labels: BTreeMap<String, String>,
    pub explicit: bool,
    /// The object this one stacks in front of. Empty is a root.
    pub parent: String,
    /// Low stack is the back.
    pub stack: i64,
    pub role: String,
    /// "", "mask", or "warp map".
    pub warp_layer: String,
    /// A Beetle Game battlefx id, or empty.
    pub sound: String,
}

impl DeskObject {
    pub fn new(id: impl Into<String>, name: impl Into<String>, layer_ids: Vec<String>) -> Self {
        Self { id: id.into(), name: name.into(), layer_ids, explicit: true, ..Self::default() }
    }
}

#[derive(Clone, Debug)]
pub struct Art {
    pub width: u32,
    pub height: u32,
    pub file_name: String,
    pub path: String,
    /// "vmib", "clip", "psd", "kra", "xcf". Python's `ArtFile.kind`.
    pub kind: String,
    pub layers: Vec<Node>,
    pub next_id: u64,
    pub next_stroke: u64,
    pub objects: Vec<DeskObject>,
    /// Webamp playlist for the scene: "", "menu", or "battle".
    pub playlist: String,
    pub arrange_locked: bool,
    /// The drawing's task board, written into the blueprint.
    pub tasks: Vec<Task>,
    pub task_colors: TaskColors,
    /// Saved KOSV presets and the plate size.
    pub kosv: Kosv,
    /// The drawing a blueprint was made from, when it was not a blueprint.
    pub source_file: String,
    pub source_kind: String,
    /// What the opener said about the file: "Opened." when it had nothing to add.
    pub note: String,
    /// Playback rate. Python's `clip_time.fps`.
    pub fps: u32,
    /// The current frame, 0-based. Python's `clip_time.current` and the
    /// `show_frame` every specified folder gets stamped with.
    pub playhead: u32,
    /// Onion skin in the picture. A view, never in the file.
    pub onion: bool,
    /// Neighbour cels each way, at most `ONION_LIMIT`.
    pub onion_past: u8,
    pub onion_future: u8,
    /// The cut Clip Studio stored, when the file had one. None derives the
    /// cut from the keys, the way Python does without a `clip_time`.
    pub clip_time: Option<ClipTime>,
}

impl Art {
    pub fn new(width: u32, height: u32) -> Self {
        let mut art = Self::blank(width, height);
        art.add_layer(None, "Ink");
        art
    }

    pub fn blank(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            file_name: "untitled".into(),
            path: String::new(),
            kind: "vmib".into(),
            layers: Vec::new(),
            next_id: 1,
            next_stroke: 1,
            objects: Vec::new(),
            playlist: String::new(),
            arrange_locked: true,
            tasks: Vec::new(),
            task_colors: TaskColors::default(),
            kosv: Kosv::default(),
            source_file: String::new(),
            source_kind: String::new(),
            note: String::new(),
            fps: DEFAULT_FPS,
            playhead: 0,
            onion: false,
            onion_past: ONION_LIMIT,
            onion_future: ONION_LIMIT,
            clip_time: None,
        }
    }

    pub fn alloc_id(&mut self) -> String {
        let id = format!("n{}", self.next_id);
        self.next_id += 1;
        id
    }

    pub fn alloc_stroke(&mut self) -> u64 {
        let id = self.next_stroke;
        self.next_stroke += 1;
        id
    }

    pub fn add_layer(&mut self, parent: Option<&str>, name: &str) -> String {
        let id = self.alloc_id();
        let node = Node::layer(id.clone(), fresh_name(&self.layers, name), self.width, self.height);
        insert_child(&mut self.layers, parent, node);
        id
    }

    pub fn add_group(&mut self, parent: Option<&str>, name: &str) -> String {
        let id = self.alloc_id();
        let node = Node::group(id.clone(), fresh_name(&self.layers, name));
        insert_child(&mut self.layers, parent, node);
        id
    }
}

fn insert_child(nodes: &mut Vec<Node>, parent: Option<&str>, node: Node) {
    if let Some(parent) = parent {
        if let Some(found) = find_mut(nodes, parent) {
            found.children.push(node);
            return;
        }
    }
    nodes.push(node);
}

pub fn fresh_name(nodes: &[Node], base: &str) -> String {
    let mut taken = Vec::new();
    collect_names(nodes, &mut taken);
    if !taken.iter().any(|name| name.eq_ignore_ascii_case(base)) {
        return base.to_string();
    }
    let mut number = 2;
    loop {
        let candidate = format!("{base} {number}");
        if !taken.iter().any(|name| name.eq_ignore_ascii_case(&candidate)) {
            return candidate;
        }
        number += 1;
    }
}

fn collect_names(nodes: &[Node], taken: &mut Vec<String>) {
    for node in nodes {
        taken.push(node.name.clone());
        collect_names(&node.children, taken);
    }
}

pub fn find_mut<'a>(nodes: &'a mut [Node], id: &str) -> Option<&'a mut Node> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

pub fn find<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find(node.children.as_slice(), id) {
            return Some(found);
        }
    }
    None
}

pub fn parent_of(nodes: &[Node], id: &str) -> Option<String> {
    fn walk(nodes: &[Node], id: &str, parent: Option<&str>) -> Option<String> {
        for node in nodes {
            if node.id == id {
                return parent.map(str::to_string);
            }
            if let Some(found) = walk(&node.children, id, Some(&node.id)) {
                return Some(found);
            }
        }
        None
    }
    walk(nodes, id, None)
}

fn any_solo(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| node.solo || any_solo(&node.children))
}

fn branch_solo(node: &Node) -> bool {
    node.solo || node.children.iter().any(branch_solo)
}

/// Paper is clear. The viewfinder draws the checker behind this. Onion skin
/// follows `art.onion`.
pub fn present(art: &mut Art) -> Vec<u8> {
    let width = art.width;
    let height = art.height;
    present_region(art, [0, 0, width as i32, height as i32])
}

/// The picture with onion skin off whatever `art.onion` says. Export and the
/// blueprint composite with this.
pub fn present_plain(art: &mut Art) -> Vec<u8> {
    let width = art.width;
    let height = art.height;
    present_region_plain(art, [0, 0, width as i32, height as i32])
}

/// Composite only `x0, y0, x1, y1` (end exclusive). The buffer is region sized;
/// its first pixel is the document pixel at `(x0, y0)`. A pen move only needs
/// the pixels it touched. Onion skin follows `art.onion`.
pub fn present_region(art: &mut Art, rect: [i32; 4]) -> Vec<u8> {
    let onion = art.onion;
    compose_region(art, rect, onion)
}

/// `present_region` with onion skin off.
pub fn present_region_plain(art: &mut Art, rect: [i32; 4]) -> Vec<u8> {
    compose_region(art, rect, false)
}

fn compose_region(art: &mut Art, rect: [i32; 4], onion: bool) -> Vec<u8> {
    let [x0, y0, x1, y1] = clamp_rect(rect, art.width, art.height);
    let rw = (x1 - x0).max(0) as u32;
    let rh = (y1 - y0).max(0) as u32;
    let mut dst = vec![0u8; (rw as usize).saturating_mul(rh as usize).saturating_mul(4)];
    if rw == 0 || rh == 0 {
        return dst;
    }
    let roots = animation_folder_ids(&art.layers);
    let ctx = Ctx {
        width: art.width,
        height: art.height,
        playhead: art.playhead,
        gated: true,
        solo_on: any_solo(&art.layers),
        roots: &roots,
        solo: false,
    };
    paint_list(&mut art.layers, Target { buf: &mut dst, w: rw, h: rh, ox: x0, oy: y0 }, &ctx);
    if onion {
        apply_onion(art, &roots, Target { buf: &mut dst, w: rw, h: rh, ox: x0, oy: y0 });
    }
    dst
}

/// How the picture reads one row. The same rules as `document.gate_of`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Skip the row and everything inside it.
    Drop,
    /// Solo is on elsewhere: keep looking for a soloed child, paint nothing here.
    Seek,
    In,
}

fn gate_of(visible: bool, mute: bool, solo: bool, ancestor_solo: bool, solo_on: bool) -> Gate {
    if !visible || mute {
        Gate::Drop
    } else if solo_on && !(ancestor_solo || solo) {
        Gate::Seek
    } else {
        Gate::In
    }
}

/// One pass over a list. `gated` off paints every row as it is, the way
/// Python's `composite_image` does for a single cel.
#[derive(Clone, Copy)]
struct Ctx<'a> {
    width: u32,
    height: u32,
    playhead: u32,
    gated: bool,
    solo_on: bool,
    /// Ids of the animation folders that stand for one cel each.
    roots: &'a [String],
    /// An ancestor is soloed, so this branch is in.
    solo: bool,
}

impl Ctx<'_> {
    fn child(&self, solo: bool) -> Self {
        Self { solo: self.solo || solo, ..*self }
    }
}

/// What a stack slot paints: the row itself, or one cel of an animation
/// folder standing in the folder's place (Python's `_View`). The cel keeps
/// its own blend and opacity; the folder lends its clip flag and solo.
#[derive(Clone, Copy)]
struct Item {
    child: Option<usize>,
    clip: bool,
    solo: bool,
}

fn actual<'a>(node: &'a Node, item: &Item) -> &'a Node {
    match item.child {
        Some(index) => &node.children[index],
        None => node,
    }
}

fn actual_mut<'a>(node: &'a mut Node, item: &Item) -> &'a mut Node {
    match item.child {
        Some(index) => &mut node.children[index],
        None => node,
    }
}

/// The cel an animation folder shows, or None when the slot is empty.
fn chosen_cel(folder: &Node, ctx: &Ctx<'_>) -> Option<usize> {
    let child_solo = ctx.solo || folder.solo;
    let held = cel_at(folder, ctx.playhead)?;
    if !held.visible {
        return None;
    }
    if ctx.solo_on && !child_solo && !(held.solo || branch_solo(held)) {
        return None;
    }
    if gate_of(held.visible, held.mute, held.solo, child_solo, ctx.solo_on) == Gate::Drop {
        return None;
    }
    folder.children.iter().position(|cel| std::ptr::eq(cel, held))
}

fn resolve(node: &Node, ctx: &Ctx<'_>) -> Option<Item> {
    if !ctx.gated {
        return Some(Item { child: None, clip: node.clip, solo: node.solo });
    }
    let gate = gate_of(node.visible, node.mute, node.solo, ctx.solo, ctx.solo_on);
    if gate == Gate::Drop {
        return None;
    }
    if node.kind == NodeKind::Group && ctx.roots.contains(&node.id) {
        let index = chosen_cel(node, ctx)?;
        let cel = &node.children[index];
        return Some(Item { child: Some(index), clip: node.clip || cel.clip, solo: ctx.solo || node.solo || cel.solo });
    }
    if gate == Gate::Seek && node.kind != NodeKind::Group {
        return None;
    }
    Some(Item { child: None, clip: node.clip, solo: node.solo })
}

fn is_pass(blend: &str) -> bool {
    let key: String = blend.trim().to_ascii_lowercase().replace('_', " ").split_whitespace().collect::<Vec<_>>().join(" ");
    matches!(key.as_str(), "pass" | "pass through" | "passthrough")
}

fn is_normal(blend: &str) -> bool {
    let key = blend.trim().to_ascii_lowercase();
    key == "normal" || key == "norm" || key.is_empty() || is_pass(&key)
}

/// Back to front. A clipping mask is stamped onto the plate of the row it
/// clips onto, then that stack blends as one. A dropped base takes its masks
/// with it. A mask with no base, or a cel that is itself a mask, paints nothing.
fn paint_list(nodes: &mut [Node], target: Target<'_>, ctx: &Ctx<'_>) {
    let Target { buf: dst, w, h, ox, oy } = target;
    let count = nodes.len();
    let mut index = 0;
    while index < count {
        let raw_clip = nodes[index].clip;
        let Some(item) = resolve(&nodes[index], ctx) else {
            index += 1;
            if !raw_clip {
                while index < count && nodes[index].clip {
                    index += 1;
                }
            }
            continue;
        };
        if item.clip {
            index += 1;
            continue;
        }
        let mut next = index + 1;
        let mut clips: Vec<(usize, Item)> = Vec::new();
        while next < count && nodes[next].clip {
            if let Some(clipped) = resolve(&nodes[next], ctx) {
                clips.push((next, clipped));
            }
            next += 1;
        }
        paint_stack(nodes, index, &item, &clips, Target { buf: &mut *dst, w, h, ox, oy }, ctx);
        index = next;
    }
}

/// One base row and the masks clipped onto it.
fn paint_stack(nodes: &mut [Node], index: usize, item: &Item, clips: &[(usize, Item)], target: Target<'_>, ctx: &Ctx<'_>) {
    let Target { buf: dst, w, h, ox, oy } = target;
    let (kind, opacity, blend) = {
        let node = actual(&nodes[index], item);
        (node.kind, node.opacity, node.blend.clone())
    };
    let inner = ctx.child(item.solo);
    if kind == NodeKind::Group && is_pass(&blend) && clips.is_empty() && opacity == 255 {
        let node = actual_mut(&mut nodes[index], item);
        paint_list(&mut node.children, Target { buf: dst, w, h, ox, oy }, &inner);
        return;
    }
    if kind == NodeKind::Layer && clips.is_empty() && opacity == 255 && is_normal(&blend) {
        let node = actual_mut(&mut nodes[index], item);
        blit_layer(node, Target { buf: dst, w, h, ox, oy }, ctx.width, ctx.height);
        return;
    }
    let mut plate = vec![0u8; dst.len()];
    appearance(actual_mut(&mut nodes[index], item), Target { buf: &mut plate, w, h, ox, oy }, &inner);
    for (at, clipped) in clips {
        stamp(actual_mut(&mut nodes[*at], clipped), Target { buf: &mut plate, w, h, ox, oy }, &ctx.child(clipped.solo), true);
    }
    blit_into(dst, w, h, ox, oy, &plate, w, h, ox, oy, opacity, false, &blend);
}

/// This row's paint before its blend hits the parent: a flattened folder, or a
/// layer's pixels with its strokes on top, at full opacity.
fn appearance(node: &mut Node, target: Target<'_>, inner: &Ctx<'_>) {
    if node.kind == NodeKind::Group {
        paint_list(&mut node.children, target, inner);
    } else {
        blit_layer(node, target, inner.width, inner.height);
    }
}

/// Paint one row onto `target` with its own blend and opacity. `clip` keeps
/// the target's coverage, the way a clipping mask stays inside its base.
fn stamp(node: &mut Node, target: Target<'_>, inner: &Ctx<'_>, clip: bool) {
    let Target { buf: dst, w, h, ox, oy } = target;
    let mut plate = vec![0u8; dst.len()];
    appearance(node, Target { buf: &mut plate, w, h, ox, oy }, inner);
    blit_into(dst, w, h, ox, oy, &plate, w, h, ox, oy, node.opacity, clip, &node.blend);
}

/// A layer's pixels and, unless they are baked in, its stroke rasters.
fn blit_layer(node: &mut Node, target: Target<'_>, width: u32, height: u32) {
    let Target { buf: dst, w, h, ox, oy } = target;
    node.pixels.resize((width as usize) * (height as usize) * 4, 0);
    blit_into(dst, w, h, ox, oy, &node.pixels, width, height, 0, 0, 255, false, "normal");
    if node.embedded {
        return;
    }
    for stroke in &mut node.strokes {
        if let Some(raster) = stroke.ensure_cache(width, height) {
            blit_into(dst, w, h, ox, oy, &raster.rgba, raster.w, raster.h, raster.x, raster.y, 255, false, "normal");
        }
    }
}

/// One cel composited on its own over the target rectangle, as straight
/// RGBA floats with the alpha scaled by its opacity. Python's
/// `composite_image(width, height, [cel])`: not gated, so the cel paints as
/// it is.
fn cel_plate(art: &mut Art, id: &str, w: u32, h: u32, ox: i32, oy: i32) -> Vec<f32> {
    let mut out = vec![0f32; (w as usize) * (h as usize) * 4];
    let ctx = Ctx {
        width: art.width,
        height: art.height,
        playhead: art.playhead,
        gated: false,
        solo_on: false,
        roots: &[],
        solo: false,
    };
    let Some(node) = find_mut(&mut art.layers, id) else {
        return out;
    };
    let mut plate = vec![0u8; out.len()];
    appearance(node, Target { buf: &mut plate, w, h, ox, oy }, &ctx);
    let opacity = f32::from(node.opacity) / 255.0;
    for (index, value) in out.iter_mut().enumerate() {
        *value = f32::from(plate[index]);
        if index % 4 == 3 {
            *value *= opacity;
        }
    }
    out
}

/// Onion skin the way KoolDraw does it. For each animation folder the
/// neighbour cels are tinted (future green under past purple, nearest on top,
/// opacity by distance, each pulled `ONION_TINT` of the way to its tone) and
/// shown over the picture wherever the current cel is clear. Python kept the
/// picture's alpha, so over clear paper the tint vanished behind the checker;
/// here it is composited source-over, which is the same colour wherever the
/// picture is opaque and stays visible over clear paper.
fn apply_onion(art: &mut Art, roots: &[String], target: Target<'_>) {
    let Target { buf: dst, w, h, ox, oy } = target;
    let past_limit = usize::from(art.onion_past.min(ONION_LIMIT));
    let future_limit = usize::from(art.onion_future.min(ONION_LIMIT));
    let playhead = art.playhead;
    let mut plans: Vec<(Vec<String>, Vec<String>, Option<String>)> = Vec::new();
    for id in roots {
        let Some(folder) = find(&art.layers, id) else {
            continue;
        };
        let (past, future, current) = onion_neighbors(folder, playhead, past_limit, future_limit);
        if past.is_empty() && future.is_empty() {
            continue;
        }
        plans.push((
            past.iter().map(|cel| cel.id.clone()).collect(),
            future.iter().map(|cel| cel.id.clone()).collect(),
            current.map(|cel| cel.id.clone()),
        ));
    }
    let pixels = (w as usize) * (h as usize);
    for (past, future, current) in plans {
        // Farthest first, nearest on top. Future goes down before past.
        let mut rows: Vec<(String, [u8; 3], f32)> = Vec::new();
        for (cels, tone) in [(&future, FUTURE), (&past, PAST)] {
            let count = cels.len();
            for (index, id) in cels.iter().rev().enumerate() {
                rows.push((id.clone(), tone, onion_strength(count - 1 - index)));
            }
        }
        let mut canvas = vec![0f32; pixels * 4];
        for (id, tone, strength) in rows {
            let plate = cel_plate(art, &id, w, h, ox, oy);
            for index in 0..pixels {
                let top_a = plate[index * 4 + 3] * strength / 255.0;
                if top_a <= 0.0 {
                    continue;
                }
                let base = &mut canvas[index * 4..index * 4 + 4];
                let base_a = base[3] / 255.0;
                let out_a = top_a + base_a * (1.0 - top_a);
                for channel in 0..3 {
                    let toned = plate[index * 4 + channel] * (1.0 - ONION_TINT) + f32::from(tone[channel]) * ONION_TINT;
                    let mixed = toned * top_a + base[channel] * base_a * (1.0 - top_a);
                    base[channel] = if out_a > 1e-6 { mixed / out_a } else { 0.0 };
                }
                base[3] = out_a * 255.0;
            }
        }
        let cover: Vec<f32> = match current {
            Some(id) => cel_plate(art, &id, w, h, ox, oy).chunks_exact(4).map(|px| px[3]).collect(),
            None => vec![0f32; pixels],
        };
        for index in 0..pixels {
            let mix = (1.0 - cover[index] / 255.0) * (canvas[index * 4 + 3] / 255.0);
            if mix <= 0.0 {
                continue;
            }
            let px = &mut dst[index * 4..index * 4 + 4];
            let base_a = f32::from(px[3]) / 255.0;
            let out_a = mix + base_a * (1.0 - mix);
            for channel in 0..3 {
                let mixed = canvas[index * 4 + channel] * mix + f32::from(px[channel]) * base_a * (1.0 - mix);
                px[channel] = (mixed / out_a).round().clamp(0.0, 255.0) as u8;
            }
            px[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

pub fn clamp_rect(rect: [i32; 4], width: u32, height: u32) -> [i32; 4] {
    [
        rect[0].clamp(0, width as i32),
        rect[1].clamp(0, height as i32),
        rect[2].clamp(0, width as i32),
        rect[3].clamp(0, height as i32),
    ]
}

/// A destination buffer whose first pixel sits at document `(ox, oy)`.
struct Target<'a> {
    buf: &'a mut [u8],
    w: u32,
    h: u32,
    ox: i32,
    oy: i32,
}

fn mul_opacity(parent: u8, child: u8) -> u8 {
    (u16::from(parent) * u16::from(child) / 255) as u8
}

pub fn blit_buffer(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    ox: i32,
    oy: i32,
    opacity: u8,
    clip: bool,
    blend: &str,
) {
    blit_into(dst, dw, dh, 0, 0, src, sw, sh, ox, oy, opacity, clip, blend);
}

/// Blit `src` (whose first pixel is document `(ox, oy)`) onto `dst` (whose
/// first pixel is document `(dst_ox, dst_oy)`). Only the overlap is visited.
#[allow(clippy::too_many_arguments)]
pub fn blit_into(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    dst_ox: i32,
    dst_oy: i32,
    src: &[u8],
    sw: u32,
    sh: u32,
    ox: i32,
    oy: i32,
    opacity: u8,
    clip: bool,
    blend: &str,
) {
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 || opacity == 0 || src.len() < (sw as usize) * (sh as usize) * 4 {
        return;
    }
    // Document-space overlap of the two rectangles.
    let left = ox.max(dst_ox);
    let top = oy.max(dst_oy);
    let right = (ox + sw as i32).min(dst_ox + dw as i32);
    let bottom = (oy + sh as i32).min(dst_oy + dh as i32);
    if right <= left || bottom <= top {
        return;
    }
    for doc_y in top..bottom {
        for doc_x in left..right {
            let x = doc_x - ox;
            let y = doc_y - oy;
            let dx = doc_x - dst_ox;
            let dy = doc_y - dst_oy;
            let si = ((y as u32 * sw + x as u32) * 4) as usize;
            let di = ((dy as u32 * dw + dx as u32) * 4) as usize;
            let src_px = Rgba::new(src[si], src[si + 1], src[si + 2], src[si + 3]);
            if src_px.a == 0 {
                continue;
            }
            let dst_px = Rgba::new(dst[di], dst[di + 1], dst[di + 2], dst[di + 3]);
            if clip && dst_px.a == 0 {
                continue;
            }
            let mixed = if matches!(blend, "hue" | "saturation" | "color" | "luminosity") {
                mix_hsl(dst_px, src_px, blend, opacity)
            } else if blend == "normal" || blend == "pass through" {
                let faded = Rgba { a: mul_opacity(opacity, src_px.a), ..src_px };
                source_over(dst_px, faded, 1.0)
            } else {
                mix(dst_px, src_px, blend, opacity)
            };
            if clip {
                dst[di] = mixed.r;
                dst[di + 1] = mixed.g;
                dst[di + 2] = mixed.b;
                dst[di + 3] = dst_px.a;
            } else {
                dst[di] = mixed.r;
                dst[di + 1] = mixed.g;
                dst[di + 2] = mixed.b;
                dst[di + 3] = mixed.a;
            }
        }
    }
}

pub fn expand_raster(raster: &Raster, width: u32, height: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    blit_buffer(&mut pixels, width, height, &raster.rgba, raster.w, raster.h, raster.x, raster.y, 255, false, "normal");
    pixels
}

/// The painted bounds of a full canvas buffer. None when nothing is painted,
/// or when the buffer is not a full canvas (a folder has no pixels).
pub fn tight_pixels(pixels: &[u8], width: u32, height: u32) -> Option<Raster> {
    if pixels.len() < (width as usize).saturating_mul(height as usize).saturating_mul(4) {
        return None;
    }
    crate::vector::tight(pixels.to_vec(), width, height)
}

pub fn reorder_objects(objects: &mut Vec<DeskObject>, from: usize, to: usize) {
    if from >= objects.len() || to >= objects.len() || from == to {
        return;
    }
    let item = objects.remove(from);
    objects.insert(to, item);
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Art>,
    redo: Vec<Art>,
}

impl History {
    pub const LIMIT: usize = 20;

    pub fn push(&mut self, before: Art) {
        self.undo.push(before);
        if self.undo.len() > Self::LIMIT {
            let extra = self.undo.len() - Self::LIMIT;
            self.undo.drain(0..extra);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: Art) -> Option<Art> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        Some(previous)
    }

    pub fn redo(&mut self, current: Art) -> Option<Art> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        if self.undo.len() > Self::LIMIT {
            let extra = self.undo.len() - Self::LIMIT;
            self.undo.drain(0..extra);
        }
        Some(next)
    }

    pub fn len(&self) -> usize {
        self.undo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_holds_twenty_and_a_hidden_layer_drops_out() {
        let mut history = History::default();
        for index in 0..25 {
            let mut art = Art::blank(2, 2);
            art.file_name = index.to_string();
            history.push(art);
        }
        assert_eq!(history.len(), 20);
        let current = Art::blank(2, 2);
        let undone = history.undo(current).unwrap();
        assert_eq!(undone.file_name, "24");

        let mut art = Art::blank(2, 1);
        let id = art.add_layer(None, "Ink");
        art.layers[0].pixels[3] = 255;
        art.layers[0].visible = false;
        let plate = present(&mut art);
        assert_eq!(plate[3], 0);
        find_mut(&mut art.layers, &id).unwrap().visible = true;
        let plate = present(&mut art);
        assert_eq!(plate[3], 255);
    }

    #[test]
    fn a_region_matches_the_same_pixels_of_the_full_plate() {
        let mut art = Art::blank(8, 6);
        let under = art.add_layer(None, "Under");
        let over = art.add_layer(None, "Over");
        for x in 0..8u32 {
            let i = ((2 * 8 + x) * 4) as usize;
            let node = find_mut(&mut art.layers, &under).unwrap();
            node.pixels[i] = 200;
            node.pixels[i + 3] = 255;
        }
        let node = find_mut(&mut art.layers, &over).unwrap();
        node.opacity = 128;
        node.blend = "multiply".into();
        for y in 0..6u32 {
            let i = ((y * 8 + 5) * 4) as usize;
            node.pixels[i + 2] = 255;
            node.pixels[i + 3] = 255;
        }
        let full = present(&mut art);
        let rect = [3, 1, 7, 5];
        let part = present_region(&mut art, rect);
        assert_eq!(part.len(), 4 * 4 * 4);
        for y in 1..5 {
            for x in 3..7 {
                let fi = ((y * 8 + x) * 4) as usize;
                let pi = (((y - 1) * 4 + (x - 3)) * 4) as usize;
                assert_eq!(&full[fi..fi + 4], &part[pi..pi + 4], "{x},{y}");
            }
        }
        assert_eq!(clamp_rect([-3, 2, 40, 9], 8, 6), [0, 2, 8, 6]);
    }

    #[test]
    fn objects_reorder() {
        let mut objects = vec![DeskObject::new("a", "A", Vec::new()), DeskObject::new("b", "B", Vec::new())];
        reorder_objects(&mut objects, 1, 0);
        assert_eq!(objects[0].id, "b");
    }

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const YELLOW: [u8; 4] = [255, 255, 0, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn paint(node: &mut Node, width: u32, x: u32, y: u32, rgba: [u8; 4]) {
        let i = ((y * width + x) * 4) as usize;
        node.pixels[i..i + 4].copy_from_slice(&rgba);
    }

    fn fill(node: &mut Node, rgba: [u8; 4]) {
        for index in 0..node.pixels.len() / 4 {
            node.pixels[index * 4..index * 4 + 4].copy_from_slice(&rgba);
        }
    }

    fn px(plate: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [plate[i], plate[i + 1], plate[i + 2], plate[i + 3]]
    }

    fn solid(width: u32, height: u32, id: &str, name: &str, rgba: [u8; 4]) -> Node {
        let mut node = Node::layer(id, name, width, height);
        fill(&mut node, rgba);
        node
    }

    /// Cels paint column `x` only; the rest of the canvas stays clear.
    fn folder(id: &str, width: u32, x: u32, cels: &[(&str, [u8; 4])], frames: &[u32]) -> Node {
        let mut folder = Node::group(id, "Animation");
        folder.animation = true;
        for (index, (cel, rgba)) in cels.iter().enumerate() {
            let mut node = Node::layer(*cel, (index + 1).to_string(), width, 1);
            paint(&mut node, width, x, 0, *rgba);
            folder.children.push(node);
            folder.timeline.push((*cel).to_string());
        }
        folder.frames = frames.to_vec();
        folder
    }

    #[test]
    fn an_animation_folder_shows_the_cel_held_at_the_playhead() {
        let mut art = Art::blank(3, 1);
        let mut loose = Node::layer("loose", "Loose", 3, 1);
        paint(&mut loose, 3, 1, 0, YELLOW);
        art.layers = vec![
            solid(3, 1, "paper", "Paper", WHITE),
            folder("one", 3, 0, &[("a", RED), ("b", GREEN), ("c", BLUE)], &[0, 2, 5]),
            loose,
            folder("two", 3, 2, &[("d", RED), ("e", BLUE)], &[0, 1]),
        ];
        let expect = [(0, RED, RED), (1, RED, BLUE), (2, GREEN, BLUE), (4, GREEN, BLUE), (5, BLUE, BLUE), (9, BLUE, BLUE)];
        for (frame, one, two) in expect {
            art.playhead = frame;
            let plate = present(&mut art);
            assert_eq!(px(&plate, 3, 0, 0), one, "folder one at frame {frame}");
            assert_eq!(px(&plate, 3, 1, 0), YELLOW, "a plain layer shows at every frame");
            assert_eq!(px(&plate, 3, 2, 0), two, "folder two at frame {frame}");
            assert_eq!(present_plain(&mut art), plate, "onion is off, plain is the same picture");
        }

        find_mut(&mut art.layers, "one").unwrap().frames = vec![1, 2, 5];
        art.playhead = 0;
        assert_eq!(px(&present(&mut art), 3, 0, 0), WHITE, "no cel before the first key");

        find_mut(&mut art.layers, "one").unwrap().frames = vec![0, 2, 5];
        find_mut(&mut art.layers, "b").unwrap().visible = false;
        art.playhead = 3;
        assert_eq!(px(&present(&mut art), 3, 0, 0), WHITE, "a hidden cel shows nothing, not another cel");
        find_mut(&mut art.layers, "b").unwrap().visible = true;
        find_mut(&mut art.layers, "b").unwrap().opacity = 0;
        let plate = present(&mut art);
        assert_eq!(px(&plate, 3, 0, 0), WHITE, "a cel keeps its own opacity");

        find_mut(&mut art.layers, "one").unwrap().visible = false;
        for frame in [0, 3, 7] {
            art.playhead = frame;
            assert_eq!(px(&present(&mut art), 3, 0, 0), WHITE, "a hidden folder drops every cel");
        }
        find_mut(&mut art.layers, "one").unwrap().visible = true;
        find_mut(&mut art.layers, "one").unwrap().animation = false;
        art.playhead = 0;
        assert_eq!(px(&present(&mut art), 3, 0, 0), BLUE, "a plain folder stacks its children, front on top");
    }

    #[test]
    fn onion_tints_the_neighbours_only_when_it_is_on() {
        let mut art = Art::blank(8, 8);
        let mut past = Node::layer("past", "1", 8, 8);
        paint(&mut past, 8, 1, 1, RED);
        let mut now = Node::layer("now", "2", 8, 8);
        paint(&mut now, 8, 3, 3, RED);
        let mut next = Node::layer("next", "3", 8, 8);
        paint(&mut next, 8, 5, 5, BLUE);
        let mut folder = Node::group("anim", "Animation");
        folder.animation = true;
        folder.children = vec![past, now, next];
        folder.timeline = vec!["past".into(), "now".into(), "next".into()];
        folder.frames = vec![0, 1, 2];
        art.layers = vec![solid(8, 8, "paper", "Paper", WHITE), folder];
        art.playhead = 1;

        let plain = present(&mut art);
        assert_eq!(px(&plain, 8, 1, 1), WHITE);
        assert_eq!(px(&plain, 8, 3, 3), RED);
        assert_eq!(px(&plain, 8, 5, 5), WHITE);

        art.onion = true;
        let skinned = present(&mut art);
        let purple = px(&skinned, 8, 1, 1);
        let green = px(&skinned, 8, 5, 5);
        assert!(purple[2] > purple[0] && purple[0] > purple[1], "past is purple: {purple:?}");
        assert!(green[1] > green[0] && green[1] > green[2], "future is green: {green:?}");
        assert_eq!(purple[3], 255);
        assert_eq!(px(&skinned, 8, 3, 3), RED, "the current cel covers the tint");
        assert_eq!(px(&skinned, 8, 0, 0), WHITE, "paper away from the cels is untouched");
        assert_eq!(present_plain(&mut art), plain, "plain never shows onion skin");
        let region = present_region(&mut art, [0, 0, 4, 4]);
        assert_eq!(px(&region, 4, 1, 1), purple, "a region carries the same tint");
        let past = find(&art.layers, "past").unwrap();
        assert_eq!(px(&past.pixels, 8, 1, 1), RED, "the stored cel is not recoloured");

        art.onion_past = 0;
        let no_past = present(&mut art);
        assert_eq!(px(&no_past, 8, 1, 1), WHITE, "zero past neighbours");
        assert_eq!(px(&no_past, 8, 5, 5), green);
        art.onion_past = ONION_LIMIT;
        art.onion = false;
        assert_eq!(present(&mut art), plain, "off again");

        art.layers.remove(0);
        art.onion = true;
        let bare = present(&mut art);
        let tint = px(&bare, 8, 1, 1);
        assert!(tint[3] > 0 && tint[2] > tint[0] && tint[0] > tint[1], "visible over clear paper: {tint:?}");
        assert_eq!(px(&bare, 8, 0, 0), CLEAR);
        art.onion = false;
        assert_eq!(px(&present(&mut art), 8, 1, 1), CLEAR);
    }

    #[test]
    fn hide_mute_solo_and_clip_read_like_python() {
        let mut art = Art::blank(2, 1);
        let mut base = Node::layer("base", "Base", 2, 1);
        paint(&mut base, 2, 0, 0, RED);
        let mut mask = solid(2, 1, "mask", "Mask", BLUE);
        mask.clip = true;
        art.layers = vec![base, mask];
        let plate = present(&mut art);
        assert_eq!(px(&plate, 2, 0, 0), BLUE, "a mask paints inside its base");
        assert_eq!(px(&plate, 2, 1, 0), CLEAR, "and nowhere else");

        find_mut(&mut art.layers, "base").unwrap().visible = false;
        assert_eq!(present(&mut art), vec![0; 8], "a dropped base takes its masks with it");
        find_mut(&mut art.layers, "base").unwrap().visible = true;
        find_mut(&mut art.layers, "base").unwrap().mute = true;
        assert_eq!(present(&mut art), vec![0; 8], "mute drops like hide");
        find_mut(&mut art.layers, "base").unwrap().mute = false;

        art.layers.remove(0);
        assert_eq!(present(&mut art), vec![0; 8], "a mask with no base paints nothing");

        let mut art = Art::blank(1, 1);
        let mut group = Node::group("g", "Folder");
        group.solo = true;
        group.children = vec![solid(1, 1, "inner", "Inner", GREEN)];
        art.layers = vec![solid(1, 1, "loose", "Loose", RED), group, solid(1, 1, "top", "Top", BLUE)];
        assert_eq!(present(&mut art), GREEN.to_vec(), "solo keeps the soloed folder and all its children");
        find_mut(&mut art.layers, "g").unwrap().solo = false;
        assert_eq!(present(&mut art), BLUE.to_vec(), "no solo, the front shows");
        find_mut(&mut art.layers, "inner").unwrap().solo = true;
        assert_eq!(present(&mut art), GREEN.to_vec(), "a soloed child is sought through its folder");

        let mut art = Art::blank(1, 1);
        let mut cel = solid(1, 1, "cel", "1", RED);
        cel.clip = true;
        let mut folder = Node::group("anim", "Animation");
        folder.animation = true;
        folder.children = vec![cel];
        art.layers = vec![solid(1, 1, "paper", "Paper", WHITE), folder];
        assert_eq!(present(&mut art), WHITE.to_vec(), "a cel that is itself a mask paints nothing");
        find_mut(&mut art.layers, "cel").unwrap().clip = false;
        assert_eq!(present(&mut art), RED.to_vec());
        find_mut(&mut art.layers, "anim").unwrap().clip = true;
        art.layers.remove(0);
        assert_eq!(present(&mut art), CLEAR.to_vec(), "a clipping animation folder needs a base too");
    }
}
