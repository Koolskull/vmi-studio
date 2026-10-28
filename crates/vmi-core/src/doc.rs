//! The open drawing. Index 0 is the back of a list. The front is the last child.
//!
//! The picture is built the way `vmi_studio/composite.py` builds it: a hidden
//! or muted row drops with everything inside it, solo keeps only soloed
//! branches, a clipping mask stays inside the row it clips onto, a pass
//! through folder paints its children on the picture below, and an animation
//! folder shows one cel, the one `anim::cel_at` holds at the playhead. Onion
//! skin (`vmi_studio/onion.py`) is a view: `present` tints the neighbour cels
//! when `Art::onion` is on, `present_plain` never does.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::anim::{
    animation_folder_ids, cel_at, key_of, onion_neighbors, onion_strength, timeline_cels,
    ExposureSheet, DEFAULT_FPS, FUTURE, ONION_LIMIT, ONION_TINT, PAST,
};
use crate::blend::{mix, mix_hsl};
use crate::color::Rgba;
use crate::coverage::source_over;
use crate::kosv::{self, CelMark, Effect, Kosv, Maps, Param, Picture};
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
            pixels: vec![
                0;
                (width as usize)
                    .saturating_mul(height as usize)
                    .saturating_mul(4)
            ],
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
        Self {
            name: "Timeline".into(),
            start: 0,
            end: 0,
        }
    }
}

/// Library kinds. The first four are the Python desk's list. A folder only
/// groups the library. The rest are symbols: a state swaps the picture, and
/// the sample on that state rides with it. An unknown kind still loads.
pub const OBJECT_KINDS: &[&str] = &[
    "static",
    "sequence",
    "button",
    "warp target",
    "mouth",
    "pose",
    "prop",
    "fx",
    "folder",
];

/// One thing in the work: a door, a beetle, a shop, a word. The same row
/// Python's `desk.DeskObject` keeps and the blueprint's `objects` carry.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DeskObject {
    pub id: String,
    pub name: String,
    /// `OBJECT_KINDS`, or empty, which follows the layers. A custom word loads too.
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
    /// Low stack is the back. The top library row is the back.
    pub stack: i64,
    pub role: String,
    /// "", "mask", or "warp map".
    pub warp_layer: String,
    /// The sample for the whole object: a file path, or a battlefx id such as `sfx:tik`.
    pub sound: String,
    /// Library folder id. Empty is the root. A folder's own value is the folder it sits in.
    pub folder: String,
    /// A sample file per SSSXXX position. Empty falls back to `sound`.
    pub samples: BTreeMap<String, String>,
}

impl DeskObject {
    pub fn new(id: impl Into<String>, name: impl Into<String>, layer_ids: Vec<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            layer_ids,
            explicit: true,
            ..Self::default()
        }
    }

    pub fn is_folder(&self) -> bool {
        self.kind.eq_ignore_ascii_case("folder")
    }

    /// The sample written on this position, with no fallback.
    pub fn own_sample(&self, state: u16, frame: u16) -> String {
        self.samples
            .get(&slot_key(state, frame))
            .cloned()
            .unwrap_or_default()
    }

    /// The sample this position plays. Its own file wins, then the state's
    /// first frame, then the object's sample.
    pub fn sample_of(&self, state: u16, frame: u16) -> String {
        let own = self.own_sample(state, frame);
        if !own.trim().is_empty() {
            return own;
        }
        if frame != 0 {
            let head = self.own_sample(state, 0);
            if !head.trim().is_empty() {
                return head;
            }
        }
        self.sound.trim().to_string()
    }

    /// Remember a sample file for one position. A blank path clears it.
    pub fn set_sample(&mut self, state: u16, frame: u16, path: &str) {
        let key = slot_key(state, frame);
        let path = path.trim();
        if path.is_empty() {
            self.samples.remove(&key);
        } else {
            self.samples.insert(key, path.to_string());
        }
    }

    /// This layer becomes the picture for the state. The first layer is frame
    /// `000`. Another layer on the same state is the next frame.
    pub fn take_layer(&mut self, layer_id: &str, state: u16) -> u16 {
        if self.is_folder() {
            return 0;
        }
        let state = state & 0xFFF;
        if let Some((found, frame)) = self.assign.get(layer_id).copied() {
            if found & 0xFFF == state {
                if !self.layer_ids.iter().any(|id| id == layer_id) {
                    self.layer_ids.push(layer_id.to_string());
                }
                return frame & 0xFFF;
            }
        }
        let mut assigned: Vec<u16> = self
            .assign
            .iter()
            .filter(|(id, (found, _))| id.as_str() != layer_id && found & 0xFFF == state)
            .map(|(_, (_, frame))| frame & 0xFFF)
            .collect();
        assigned.sort_unstable();
        assigned.dedup();
        let frame = if let Some(&last) = assigned.last() {
            if last >= 0xFFF {
                last
            } else {
                let next = last + 1;
                let word = self.label_of(state, 0);
                self.labels.entry(slot_key(state, next)).or_insert(word);
                if self.kind.is_empty() || self.kind.eq_ignore_ascii_case("static") {
                    self.kind = "sequence".into();
                }
                next
            }
        } else {
            0
        };
        if !self.layer_ids.iter().any(|id| id == layer_id) {
            self.layer_ids.push(layer_id.to_string());
        }
        self.assign.insert(layer_id.to_string(), (state, frame));
        frame
    }

    /// The name written for one position. A custom name wins. Otherwise the
    /// button word: `000` idle, `001` hover, `002` click, `003` after.
    pub fn label_of(&self, state: u16, frame: u16) -> String {
        let key = slot_key(state, frame);
        self.labels
            .get(&key)
            .map(|text| text.trim())
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| state_word(state).to_string())
    }

    /// States this object already has. A button always carries idle, hover,
    /// click, and after. Anything else keeps idle, plus states named in the
    /// object file.
    pub fn states(&self) -> Vec<u16> {
        let mut states = vec![0u16];
        if self.kind.eq_ignore_ascii_case("button") {
            states.extend([1, 2, 3]);
        }
        for key in self.labels.keys().chain(self.samples.keys()) {
            if let Some((state, _)) = parse_slot(key) {
                states.push(state);
            }
        }
        for (state, _) in self.assign.values() {
            states.push(*state & 0xFFF);
        }
        states.sort_unstable();
        states.dedup();
        states
    }

    /// Frames of one state, starting at `000`. Later frames are the animation.
    pub fn frames_of(&self, state: u16) -> Vec<u16> {
        let state = state & 0xFFF;
        let mut frames = vec![0u16];
        for key in self.labels.keys().chain(self.samples.keys()) {
            if let Some((found, frame)) = parse_slot(key) {
                if found == state {
                    frames.push(frame);
                }
            }
        }
        for (found, frame) in self.assign.values() {
            if u16::from(*found) & 0xFFF == state {
                frames.push(*frame & 0xFFF);
            }
        }
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// Remember a name for one position. A blank name falls back to the button word.
    pub fn rename_slot(&mut self, state: u16, frame: u16, name: &str) {
        let key = slot_key(state, frame);
        let name = name.trim();
        if name.is_empty() {
            self.labels.remove(&key);
        } else {
            self.labels.insert(key, name.to_string());
        }
    }

    /// The next frame of this state. A still object becomes a sequence.
    /// A folder has no frames.
    pub fn add_frame(&mut self, state: u16) -> u16 {
        if self.is_folder() {
            return 0;
        }
        let frames = self.frames_of(state);
        let last = frames.last().copied().unwrap_or(0);
        if last >= 0xFFF {
            return last;
        }
        let next = last + 1;
        let word = state_word(state).to_string();
        self.labels.entry(slot_key(state, next)).or_insert(word);
        if self.kind.is_empty() || self.kind.eq_ignore_ascii_case("static") {
            self.kind = "sequence".into();
        }
        next
    }

    /// The next free state, named with the button word when it is one of the four.
    /// A folder has no states to add.
    pub fn add_state(&mut self) -> u16 {
        if self.is_folder() {
            return 0;
        }
        let states = self.states();
        let last = states.last().copied().unwrap_or(0);
        if last >= 0xFFF {
            return last;
        }
        let next = last + 1;
        let word = state_word(next).to_string();
        self.labels.entry(slot_key(next, 0)).or_insert(word);
        next
    }
}

/// Three hex digits. Frame 0 is `000`, frame 10 is `00A`. This is the VMI slot.
pub fn slot_hex(n: u32) -> String {
    format!("{:03X}", n & 0xFFF)
}

/// `SSSXXX`. State, then frame, each three hex digits.
pub fn slot_key(state: u16, frame: u16) -> String {
    format!(
        "{:03X}{:03X}",
        u32::from(state) & 0xFFF,
        u32::from(frame) & 0xFFF
    )
}

/// The name a button state wears until someone writes another.
pub fn state_word(state: u16) -> &'static str {
    match state & 0xFFF {
        0 => "idle",
        1 => "hover",
        2 => "click",
        3 => "after",
        _ => "custom",
    }
}

fn parse_slot(key: &str) -> Option<(u16, u16)> {
    if key.len() != 6 || !key.chars().all(|char| char.is_ascii_hexdigit()) {
        return None;
    }
    let state = u16::from_str_radix(&key[..3], 16).ok()?;
    let frame = u16::from_str_radix(&key[3..], 16).ok()?;
    Some((state, frame))
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
    /// Past cels pull toward this colour. A view, never in the file.
    pub onion_past_color: [u8; 3],
    /// Future cels pull toward this colour. A view, never in the file.
    pub onion_future_color: [u8; 3],
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
            onion_past_color: PAST,
            onion_future_color: FUTURE,
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
        let node = Node::layer(
            id.clone(),
            fresh_name(&self.layers, name),
            self.width,
            self.height,
        );
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
        if !taken
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&candidate))
        {
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
    nodes
        .iter()
        .any(|node| node.solo || any_solo(&node.children))
}

fn branch_solo(node: &Node) -> bool {
    node.solo || node.children.iter().any(branch_solo)
}

/// Paper is clear. The viewfinder draws the checker behind this. Onion skin
/// follows `art.onion`.
pub fn present(art: &mut Art) -> Vec<u8> {
    present_live(art, None)
}

/// `present` with the KOSV grain clock. `live` is the window's 84 ms tick
/// while a live effect (grain, jiggle, VHS, the glitches) is on the picture;
/// None keeps the grain locked to the playhead frame.
pub fn present_live(art: &mut Art, live: Option<i64>) -> Vec<u8> {
    let width = art.width;
    let height = art.height;
    present_region_live(art, [0, 0, width as i32, height as i32], live)
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
    present_region_live(art, rect, None)
}

/// `present_region` with the KOSV grain clock.
pub fn present_region_live(art: &mut Art, rect: [i32; 4], live: Option<i64>) -> Vec<u8> {
    let onion = art.onion;
    compose_region(art, rect, onion, live, 1)
}

/// `present_region` with onion skin off.
pub fn present_region_plain(art: &mut Art, rect: [i32; 4]) -> Vec<u8> {
    compose_region(art, rect, false, None, 1)
}

/// A playback plate. Canvases larger than `max_side` are sampled on a stride,
/// so a 3840 picture does not composite every pixel at 24 fps. The buffer is
/// the reduced size. A small picture is unchanged.
pub fn present_proxy(art: &mut Art, max_side: u32) -> (u32, u32, Vec<u8>) {
    let long = art.width.max(art.height).max(1);
    let cap = max_side.max(32);
    let step = if long <= cap {
        1
    } else {
        long.div_ceil(cap).max(1)
    };
    let rgba = compose_region(
        art,
        [0, 0, art.width as i32, art.height as i32],
        art.onion,
        None,
        step,
    );
    let w = (art.width / step).max(1);
    let h = (art.height / step).max(1);
    (w, h, rgba)
}

fn compose_region(
    art: &mut Art,
    rect: [i32; 4],
    onion: bool,
    live: Option<i64>,
    step: u32,
) -> Vec<u8> {
    let [x0, y0, x1, y1] = clamp_rect(rect, art.width, art.height);
    let rw = (x1 - x0).max(0) as u32;
    let rh = (y1 - y0).max(0) as u32;
    if rw == 0 || rh == 0 {
        return Vec::new();
    }
    // A KOSV stack reads the whole plate: the grain, the line shifts, and the
    // datamosh cels do not stop at a pen move's rectangle. With a stack on
    // the picture the whole plate composites, like the Python, then the
    // region is cut from it.
    let jobs = effect_jobs(art);
    let whole = !jobs.is_empty() && (x0 != 0 || y0 != 0 || rw != art.width || rh != art.height);
    if whole {
        let width = art.width;
        let height = art.height;
        let full = compose_whole(
            art,
            [0, 0, width as i32, height as i32],
            onion,
            live,
            &jobs,
            1,
        );
        let mut out = vec![0u8; (rw as usize) * (rh as usize) * 4];
        for row in 0..rh as usize {
            let src = ((y0 as usize + row) * width as usize + x0 as usize) * 4;
            let dst = row * rw as usize * 4;
            out[dst..dst + rw as usize * 4].copy_from_slice(&full[src..src + rw as usize * 4]);
        }
        return out;
    }
    compose_whole(art, [x0, y0, x1, y1], onion, live, &jobs, step)
}

fn compose_whole(
    art: &mut Art,
    rect: [i32; 4],
    onion: bool,
    live: Option<i64>,
    jobs: &HashMap<String, Job>,
    step: u32,
) -> Vec<u8> {
    let step = step.max(1);
    let [x0, y0, x1, y1] = rect;
    let doc_w = (x1 - x0).max(0) as u32;
    let doc_h = (y1 - y0).max(0) as u32;
    let rw = (doc_w / step).max(if doc_w > 0 { 1 } else { 0 });
    let rh = (doc_h / step).max(if doc_h > 0 { 1 } else { 0 });
    let mut dst = vec![0u8; (rw as usize).saturating_mul(rh as usize).saturating_mul(4)];
    let roots = animation_folder_ids(&art.layers);
    let ctx = Ctx {
        width: art.width,
        height: art.height,
        playhead: art.playhead,
        gated: true,
        solo_on: any_solo(&art.layers),
        roots: &roots,
        solo: false,
        frame: i64::from(art.playhead),
        live,
        jobs: Some(jobs),
        step,
    };
    paint_list(
        &mut art.layers,
        Target {
            buf: &mut dst,
            w: rw,
            h: rh,
            ox: x0,
            oy: y0,
        },
        &ctx,
    );
    if onion {
        apply_onion(
            art,
            &roots,
            Target {
                buf: &mut dst,
                w: rw,
                h: rh,
                ox: x0,
                oy: y0,
            },
            step,
        );
    }
    dst
}

/// The KOSV stack to run on one painted row, with the pictures its reference
/// slots and datamosh cels read. Python's `_kosv_paint` builds these as it
/// paints; here they are gathered first, since the painter holds the tree.
struct Job {
    effects: Vec<Effect>,
    maps: Maps,
}

/// One row that paints with a stack: the row's id, the stack, and the
/// animation folder whose cels a datamosh reads.
struct Plan {
    painted: String,
    effects: Vec<Effect>,
    source: Option<String>,
}

/// Every row that paints with a stack this frame. A cel standing in for its
/// animation folder runs the folder's stack before its own, reading the
/// folder's cel marks (Python's `_View`).
fn effect_jobs(art: &mut Art) -> HashMap<String, Job> {
    let roots = animation_folder_ids(&art.layers);
    let mut plans = Vec::new();
    plan_effects(&art.layers, &roots, art.playhead, &mut plans);
    if plans.is_empty() {
        return HashMap::new();
    }
    let mut wanted: HashSet<String> = HashSet::new();
    for plan in &plans {
        for effect in &plan.effects {
            for slot in kosv::REFS {
                if let Some(Param::Text(id)) = effect.params.get(*slot) {
                    if !id.is_empty() {
                        wanted.insert(id.clone());
                    }
                }
            }
        }
    }
    let mut refs: HashMap<String, Picture> = HashMap::new();
    for id in wanted {
        if let Some(picture) = layer_picture(art, &id) {
            refs.insert(id, picture);
        }
    }
    let mut shots: HashMap<String, (Vec<Picture>, Vec<CelMark>, Vec<String>)> = HashMap::new();
    let mut jobs = HashMap::new();
    for plan in plans {
        let mut maps = Maps::default();
        for effect in &plan.effects {
            for slot in kosv::REFS {
                let Some(Param::Text(id)) = effect.params.get(*slot) else {
                    continue;
                };
                let Some(picture) = refs.get(id) else {
                    continue;
                };
                let target = match *slot {
                    "plate" => &mut maps.plate,
                    "shape" => &mut maps.shape,
                    "clip" => &mut maps.clip,
                    "matte" => &mut maps.matte,
                    _ => &mut maps.other,
                };
                *target = Some(picture.clone());
            }
        }
        if plan.effects.iter().any(|effect| effect.id == "datamosh") {
            let source = plan.source.clone().unwrap_or_else(|| plan.painted.clone());
            if !shots.contains_key(&source) {
                shots.insert(source.clone(), datamosh_cels(art, &source));
            }
            if let Some((frames, marks, ids)) = shots.get(&source) {
                maps.frames = frames.clone();
                maps.marks = marks.clone();
                maps.ids = ids.clone();
                // The index is this cel, not the clock. The clock still drives keyed knobs.
                maps.index = ids.iter().position(|id| *id == plan.painted).unwrap_or(0);
            }
        }
        jobs.insert(
            plan.painted,
            Job {
                effects: plan.effects,
                maps,
            },
        );
    }
    jobs
}

fn plan_effects(nodes: &[Node], roots: &[String], playhead: u32, out: &mut Vec<Plan>) {
    for node in nodes {
        if node.kind == NodeKind::Group && roots.contains(&node.id) {
            if let Some(cel) = cel_at(node, playhead) {
                let mut effects = node.effects.clone();
                effects.extend(cel.effects.iter().cloned());
                if !effects.is_empty() {
                    out.push(Plan {
                        painted: cel.id.clone(),
                        effects,
                        source: Some(node.id.clone()),
                    });
                }
            }
            continue;
        }
        if !node.effects.is_empty() {
            out.push(Plan {
                painted: node.id.clone(),
                effects: node.effects.clone(),
                source: None,
            });
        }
        if node.kind == NodeKind::Group {
            plan_effects(&node.children, roots, playhead, out);
        }
    }
}

/// RGBA picture of one layer, for a KOSV map slot. A folder gives nothing,
/// like Python's `_layer_image`.
fn layer_picture(art: &mut Art, id: &str) -> Option<Picture> {
    let width = art.width;
    let height = art.height;
    let node = find_mut(&mut art.layers, id)?;
    if node.kind != NodeKind::Layer {
        return None;
    }
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    blit_layer(
        node,
        Target {
            buf: &mut rgba,
            w: width,
            h: height,
            ox: 0,
            oy: 0,
        },
        width,
        height,
        1,
    );
    Some(Picture {
        width,
        height,
        rgba,
    })
}

/// Every cel of an animation folder as its own picture, with the folder's
/// mark on each (I when it has none), in timeline order.
fn datamosh_cels(art: &mut Art, folder_id: &str) -> (Vec<Picture>, Vec<CelMark>, Vec<String>) {
    let Some(folder) = find(&art.layers, folder_id) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let ids: Vec<String> = timeline_cels(folder)
        .iter()
        .map(|cel| cel.id.clone())
        .collect();
    let marks: Vec<CelMark> = ids
        .iter()
        .map(|id| {
            folder.cel_marks.get(id).cloned().unwrap_or(CelMark {
                kind: "I".into(),
                reference: String::new(),
                origin: String::new(),
                gain: None,
                block: None,
            })
        })
        .collect();
    let frames: Vec<Picture> = ids.iter().filter_map(|id| cel_shot(art, id)).collect();
    if frames.len() != ids.len() {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    (frames, marks, ids)
}

/// One cel's picture: a layer's pixels, or a folder flattened once, as it is.
fn cel_shot(art: &mut Art, id: &str) -> Option<Picture> {
    let width = art.width;
    let height = art.height;
    let node = find_mut(&mut art.layers, id)?;
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    let target = Target {
        buf: &mut rgba,
        w: width,
        h: height,
        ox: 0,
        oy: 0,
    };
    if node.kind == NodeKind::Layer {
        blit_layer(node, target, width, height, 1);
    } else {
        let ctx = Ctx {
            width,
            height,
            playhead: 0,
            gated: false,
            solo_on: false,
            roots: &[],
            solo: false,
            frame: 0,
            live: None,
            jobs: None,
            step: 1,
        };
        paint_list(&mut node.children, target, &ctx);
    }
    Some(Picture {
        width,
        height,
        rgba,
    })
}

/// Run a row's stack on its painted plate. The plate is the region being
/// composited; with a stack on the picture that is the whole plate.
fn run_effects(plate: &mut [u8], target_w: u32, target_h: u32, id: &str, ctx: &Ctx<'_>) {
    let Some(jobs) = ctx.jobs else {
        return;
    };
    let Some(job) = jobs.get(id) else {
        return;
    };
    let out = kosv::apply_effects(
        plate,
        target_w,
        target_h,
        &job.effects,
        ctx.frame,
        &job.maps,
        ctx.live,
    );
    if out.len() == plate.len() {
        plate.copy_from_slice(&out);
    }
}

/// A stack on this row, or on the folder this cel stands in for.
fn has_job(node: &Node, item: &Item, ctx: &Ctx<'_>) -> bool {
    ctx.jobs
        .map(|jobs| jobs.contains_key(&actual(node, item).id))
        .unwrap_or(false)
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
    /// The KOSV frame for keyed knobs: the playhead.
    frame: i64,
    /// The KOSV grain clock, when a live effect is on the picture.
    live: Option<i64>,
    /// The stacks that run this frame, by painted row id. None runs none.
    jobs: Option<&'a HashMap<String, Job>>,
    /// 1 visits every document pixel. A playback plate samples every `step` pixels.
    step: u32,
}

impl Ctx<'_> {
    fn child(&self, solo: bool) -> Self {
        Self {
            solo: self.solo || solo,
            ..*self
        }
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
    folder
        .children
        .iter()
        .position(|cel| std::ptr::eq(cel, held))
}

fn resolve(node: &Node, ctx: &Ctx<'_>) -> Option<Item> {
    if !ctx.gated {
        return Some(Item {
            child: None,
            clip: node.clip,
            solo: node.solo,
        });
    }
    let gate = gate_of(node.visible, node.mute, node.solo, ctx.solo, ctx.solo_on);
    if gate == Gate::Drop {
        return None;
    }
    if node.kind == NodeKind::Group && ctx.roots.contains(&node.id) {
        let index = chosen_cel(node, ctx)?;
        let cel = &node.children[index];
        return Some(Item {
            child: Some(index),
            clip: node.clip || cel.clip,
            solo: ctx.solo || node.solo || cel.solo,
        });
    }
    if gate == Gate::Seek && node.kind != NodeKind::Group {
        return None;
    }
    Some(Item {
        child: None,
        clip: node.clip,
        solo: node.solo,
    })
}

fn is_pass(blend: &str) -> bool {
    let key: String = blend
        .trim()
        .to_ascii_lowercase()
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    matches!(key.as_str(), "pass" | "pass through" | "passthrough")
}

fn is_normal(blend: &str) -> bool {
    let key = blend.trim().to_ascii_lowercase();
    key == "normal" || key == "norm" || key.is_empty() || is_pass(&key)
}

/// The front-most painted layer at one document pixel.
///
/// This is `composite.pick_layer`. The walk matches the picture.
///
/// Index 0 is the back. A clear pixel falls through to the ink behind it.
/// A layer at opacity 0 is clear. Any other opacity with a positive alpha
/// hits, including opacity 1, because Python forces that product to at least 1.
/// Hide and mute drop the row, the rows inside it, and the masks clipped onto
/// it. A mask with no base is skipped. When any solo is on, a row outside that
/// soloed branch is skipped. A soloed folder keeps its children. A soloed child
/// is found by walking the folders above it. A solo on a cel that is not the
/// one at the playhead does not pull that cel forward.
///
/// A folder answers with the layer inside it. The folder's own opacity does
/// not clear those children, which is Python's rule. A clipping mask counts
/// only where its base already has ink. Stacked masks are tested front first.
/// A hole in the front mask falls through to the mask behind it, then to the
/// base. A clipping folder answers with the layer inside it.
///
/// An animation folder stands in as the cel `cel_at` holds at the playhead.
/// A hand-marked folder's frames are 0, 1, 2…, so the playhead still chooses
/// and the click agrees with the pixels this desk paints. Python's older
/// focus rule, which substitutes the outliner selection when a folder has no
/// frame numbers, is the playhead here. Before the first key the folder
/// contributes nothing. A hidden or muted cel contributes nothing. A cel that
/// is itself a mask contributes nothing.
///
/// A studio vector stroke that is not already baked into the pixels counts as
/// that layer's ink. Once it is baked, only the pixels count. Onion-skin
/// ghosts are a view. A KOSV stack is not a second hit. The point is floored
/// onto a document pixel. Off the canvas, a non-number, and an empty document
/// miss.
pub fn pick_layer(art: &Art, x: f32, y: f32) -> Option<String> {
    let width = art.width;
    let height = art.height;
    if width == 0 || height == 0 || !x.is_finite() || !y.is_finite() {
        return None;
    }
    let px = x.floor() as i32;
    let py = y.floor() as i32;
    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
        return None;
    }
    let roots = animation_folder_ids(&art.layers);
    let ctx = Ctx {
        width,
        height,
        playhead: art.playhead,
        gated: true,
        solo_on: any_solo(&art.layers),
        roots: &roots,
        solo: false,
        frame: i64::from(art.playhead),
        live: None,
        jobs: None,
        step: 1,
    };
    pick_list(&art.layers, &ctx, px, py)
}

struct HitStack<'a> {
    node: &'a Node,
    item: Item,
    clips: Vec<(&'a Node, Item)>,
}

/// Bases back to front, each with the masks clipped onto it. A dropped base
/// takes those masks with it. A mask with no base is skipped.
fn hit_stacks<'a>(nodes: &'a [Node], ctx: &Ctx<'_>) -> Vec<HitStack<'a>> {
    let mut out = Vec::new();
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
        let mut clips = Vec::new();
        while next < count && nodes[next].clip {
            if let Some(clipped) = resolve(&nodes[next], ctx) {
                clips.push((&nodes[next], clipped));
            }
            next += 1;
        }
        out.push(HitStack {
            node: &nodes[index],
            item,
            clips,
        });
        index = next;
    }
    out
}

fn pick_list(nodes: &[Node], ctx: &Ctx<'_>, x: i32, y: i32) -> Option<String> {
    for stack in hit_stacks(nodes, ctx).iter().rev() {
        let base = actual(stack.node, &stack.item);
        let base_ctx = ctx.child(stack.item.solo);
        // A nested clip shows one inner cel. A miss falls through to the ink behind it.
        if stack.clips.is_empty() {
            if let Some(local) = nested_start(stack.node, base, ctx) {
                if let Some(path) = clip_path(base, ctx, local) {
                    if let Some(hit) = pick_along(base, &path, &base_ctx, x, y) {
                        return Some(hit);
                    }
                }
                continue;
            }
        }
        if !covers(base, &base_ctx, x, y) {
            continue;
        }
        for (clip_node, clip_item) in stack.clips.iter().rev() {
            let inner = ctx.child(clip_item.solo);
            if let Some(hit) = pick_one(actual(clip_node, clip_item), &inner, x, y) {
                return Some(hit);
            }
        }
        if let Some(hit) = pick_one(base, &base_ctx, x, y) {
            return Some(hit);
        }
    }
    None
}

fn pick_one(node: &Node, ctx: &Ctx<'_>, x: i32, y: i32) -> Option<String> {
    if node.kind == NodeKind::Group {
        return pick_list(&node.children, ctx, x, y);
    }
    ink_at(node, x, y, ctx.width, ctx.height).then(|| node.id.clone())
}

fn covers(node: &Node, ctx: &Ctx<'_>, x: i32, y: i32) -> bool {
    if node.kind != NodeKind::Group {
        return ink_at(node, x, y, ctx.width, ctx.height);
    }
    hit_stacks(&node.children, ctx).iter().any(|stack| {
        let base = actual(stack.node, &stack.item);
        covers(base, &ctx.child(stack.item.solo), x, y)
    })
}

fn ink_at(node: &Node, x: i32, y: i32, width: u32, height: u32) -> bool {
    if node.kind != NodeKind::Layer || node.opacity == 0 {
        return false;
    }
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return false;
    }
    let index = ((y as u32) * width + (x as u32)) as usize * 4 + 3;
    if node.pixels.get(index).copied().unwrap_or(0) > 0 {
        return true;
    }
    if node.embedded {
        return false;
    }
    node.strokes.iter().any(|stroke| {
        crate::vector::bake(stroke, width, height).is_some_and(|raster| {
            let lx = x - raster.x;
            let ly = y - raster.y;
            if lx < 0 || ly < 0 || lx >= raster.w as i32 || ly >= raster.h as i32 {
                return false;
            }
            let at = ((ly as u32) * raster.w + (lx as u32)) as usize * 4 + 3;
            raster.rgba.get(at).copied().unwrap_or(0) > 0
        })
    })
}

/// Back to front. A clipping mask is stamped onto the plate of the row it
/// clips onto, then that stack blends as one. A dropped base takes its masks
/// with it. A mask with no base, or a cel that is itself a mask, paints nothing.
fn paint_list(nodes: &mut [Node], target: Target<'_>, ctx: &Ctx<'_>) {
    let Target {
        buf: dst,
        w,
        h,
        ox,
        oy,
    } = target;
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
        paint_stack(
            nodes,
            index,
            &item,
            &clips,
            Target {
                buf: &mut *dst,
                w,
                h,
                ox,
                oy,
            },
            ctx,
        );
        index = next;
    }
}

/// The local frame of a nested animation cel. None when this row is a root
/// folder (those already stand for one cel) or not an animation.
fn nested_start(parent: &Node, node: &Node, ctx: &Ctx<'_>) -> Option<u32> {
    if node.kind != NodeKind::Group || !node.animation || ctx.roots.contains(&node.id) {
        return None;
    }
    let start = key_of(parent, &node.id).unwrap_or(0);
    Some(ctx.playhead.saturating_sub(start))
}

/// Child indexes from an animation folder down to the cel on screen.
/// A cel that is itself an animation continues at its own local frame.
fn clip_path(node: &Node, ctx: &Ctx<'_>, mut local: u32) -> Option<Vec<usize>> {
    let mut path = Vec::new();
    let mut cursor = node;
    for _ in 0..32 {
        let folder_ids = vec![cursor.id.clone()];
        let nested = Ctx {
            playhead: local,
            frame: i64::from(local),
            roots: &folder_ids,
            ..*ctx
        };
        let index = chosen_cel(cursor, &nested)?;
        path.push(index);
        let held = &cursor.children[index];
        if held.clip {
            return None;
        }
        if held.kind == NodeKind::Group && held.animation && !ctx.roots.contains(&held.id) {
            let start = key_of(cursor, &held.id).unwrap_or(0);
            local = local.saturating_sub(start);
            cursor = held;
            continue;
        }
        return Some(path);
    }
    None
}

fn paint_at_path(node: &mut Node, path: &[usize], target: Target<'_>, ctx: &Ctx<'_>) {
    let Some((&index, rest)) = path.split_first() else {
        return;
    };
    if index >= node.children.len() {
        return;
    }
    if rest.is_empty() {
        let Target {
            buf: dst,
            w,
            h,
            ox,
            oy,
        } = target;
        let cel = &mut node.children[index];
        if cel.kind == NodeKind::Layer && cel.opacity == 255 && is_normal(&cel.blend) {
            blit_layer(
                cel,
                Target {
                    buf: dst,
                    w,
                    h,
                    ox,
                    oy,
                },
                ctx.width,
                ctx.height,
                ctx.step,
            );
        } else {
            stamp(
                cel,
                Target {
                    buf: dst,
                    w,
                    h,
                    ox,
                    oy,
                },
                ctx,
                false,
            );
        }
        return;
    }
    paint_at_path(&mut node.children[index], rest, target, ctx);
}

fn pick_along(node: &Node, path: &[usize], ctx: &Ctx<'_>, x: i32, y: i32) -> Option<String> {
    let (&index, rest) = path.split_first()?;
    let child = node.children.get(index)?;
    if rest.is_empty() {
        return pick_one(child, ctx, x, y);
    }
    pick_along(child, rest, ctx, x, y)
}

/// One base row and the masks clipped onto it.
fn paint_stack(
    nodes: &mut [Node],
    index: usize,
    item: &Item,
    clips: &[(usize, Item)],
    target: Target<'_>,
    ctx: &Ctx<'_>,
) {
    let Target {
        buf: dst,
        w,
        h,
        ox,
        oy,
    } = target;
    let local = {
        let parent = &nodes[index];
        let node = actual(parent, item);
        if clips.is_empty() {
            nested_start(parent, node, ctx)
        } else {
            None
        }
    };
    if let Some(local) = local {
        let (opacity, blend) = {
            let node = actual(&nodes[index], item);
            (node.opacity, node.blend.clone())
        };
        let plain = opacity == 255 && is_normal(&blend);
        if plain {
            let node = actual_mut(&mut nodes[index], item);
            if let Some(path) = clip_path(node, ctx, local) {
                paint_at_path(
                    node,
                    &path,
                    Target {
                        buf: dst,
                        w,
                        h,
                        ox,
                        oy,
                    },
                    ctx,
                );
            }
            return;
        }
        let mut plate = vec![0u8; dst.len()];
        {
            let node = actual_mut(&mut nodes[index], item);
            if let Some(path) = clip_path(node, ctx, local) {
                paint_at_path(
                    node,
                    &path,
                    Target {
                        buf: &mut plate,
                        w,
                        h,
                        ox,
                        oy,
                    },
                    ctx,
                );
            }
        }
        blit_into(
            dst, w, h, ox, oy, &plate, w, h, ox, oy, opacity, false, &blend, 1,
        );
        return;
    }
    let (kind, opacity, blend) = {
        let node = actual(&nodes[index], item);
        (node.kind, node.opacity, node.blend.clone())
    };
    let inner = ctx.child(item.solo);
    // A row with a stack is flattened first. Pass-through would paint the
    // children raw and skip the folder look; a plain blit would skip a
    // layer's. The stack then covers everything that was inside.
    let styled = has_job(&nodes[index], item, ctx);
    if kind == NodeKind::Group && is_pass(&blend) && clips.is_empty() && opacity == 255 && !styled {
        let node = actual_mut(&mut nodes[index], item);
        paint_list(
            &mut node.children,
            Target {
                buf: dst,
                w,
                h,
                ox,
                oy,
            },
            &inner,
        );
        return;
    }
    if kind == NodeKind::Layer && clips.is_empty() && opacity == 255 && is_normal(&blend) && !styled
    {
        let node = actual_mut(&mut nodes[index], item);
        blit_layer(
            node,
            Target {
                buf: dst,
                w,
                h,
                ox,
                oy,
            },
            ctx.width,
            ctx.height,
            ctx.step,
        );
        return;
    }
    let mut plate = vec![0u8; dst.len()];
    appearance(
        actual_mut(&mut nodes[index], item),
        Target {
            buf: &mut plate,
            w,
            h,
            ox,
            oy,
        },
        &inner,
    );
    for (at, clipped) in clips {
        stamp(
            actual_mut(&mut nodes[*at], clipped),
            Target {
                buf: &mut plate,
                w,
                h,
                ox,
                oy,
            },
            &ctx.child(clipped.solo),
            true,
        );
    }
    blit_into(
        dst, w, h, ox, oy, &plate, w, h, ox, oy, opacity, false, &blend, 1,
    );
}

/// This row's paint before its blend hits the parent: a flattened folder, or a
/// layer's pixels with its strokes on top, at full opacity. The row's KOSV
/// stack runs on that paint (Python's `_kosv_paint`).
fn appearance(node: &mut Node, target: Target<'_>, inner: &Ctx<'_>) {
    let Target { buf, w, h, ox, oy } = target;
    if node.kind == NodeKind::Group {
        paint_list(
            &mut node.children,
            Target {
                buf: &mut *buf,
                w,
                h,
                ox,
                oy,
            },
            inner,
        );
    } else {
        blit_layer(
            node,
            Target {
                buf: &mut *buf,
                w,
                h,
                ox,
                oy,
            },
            inner.width,
            inner.height,
            inner.step,
        );
    }
    run_effects(buf, w, h, &node.id, inner);
}

/// Paint one row onto `target` with its own blend and opacity. `clip` keeps
/// the target's coverage, the way a clipping mask stays inside its base.
fn stamp(node: &mut Node, target: Target<'_>, inner: &Ctx<'_>, clip: bool) {
    let Target {
        buf: dst,
        w,
        h,
        ox,
        oy,
    } = target;
    let mut plate = vec![0u8; dst.len()];
    appearance(
        node,
        Target {
            buf: &mut plate,
            w,
            h,
            ox,
            oy,
        },
        inner,
    );
    blit_into(
        dst,
        w,
        h,
        ox,
        oy,
        &plate,
        w,
        h,
        ox,
        oy,
        node.opacity,
        clip,
        &node.blend,
        1,
    );
}

/// A layer's pixels and, unless they are baked in, its stroke rasters.
fn blit_layer(node: &mut Node, target: Target<'_>, width: u32, height: u32, step: u32) {
    let Target {
        buf: dst,
        w,
        h,
        ox,
        oy,
    } = target;
    node.pixels
        .resize((width as usize) * (height as usize) * 4, 0);
    blit_into(
        dst,
        w,
        h,
        ox,
        oy,
        &node.pixels,
        width,
        height,
        0,
        0,
        255,
        false,
        "normal",
        step,
    );
    if node.embedded {
        return;
    }
    for stroke in &mut node.strokes {
        if let Some(raster) = stroke.ensure_cache(width, height) {
            blit_into(
                dst,
                w,
                h,
                ox,
                oy,
                &raster.rgba,
                raster.w,
                raster.h,
                raster.x,
                raster.y,
                255,
                false,
                "normal",
                step,
            );
        }
    }
}

/// One cel composited on its own over the target rectangle, as straight
/// RGBA floats with the alpha scaled by its opacity. Python's
/// `composite_image(width, height, [cel])`: not gated, so the cel paints as
/// it is.
fn cel_plate(art: &mut Art, id: &str, w: u32, h: u32, ox: i32, oy: i32, step: u32) -> Vec<f32> {
    let mut out = vec![0f32; (w as usize) * (h as usize) * 4];
    let ctx = Ctx {
        width: art.width,
        height: art.height,
        playhead: art.playhead,
        gated: false,
        solo_on: false,
        roots: &[],
        solo: false,
        frame: i64::from(art.playhead),
        live: None,
        jobs: None,
        step,
    };
    let Some(node) = find_mut(&mut art.layers, id) else {
        return out;
    };
    let mut plate = vec![0u8; out.len()];
    appearance(
        node,
        Target {
            buf: &mut plate,
            w,
            h,
            ox,
            oy,
        },
        &ctx,
    );
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
fn apply_onion(art: &mut Art, roots: &[String], target: Target<'_>, step: u32) {
    let Target {
        buf: dst,
        w,
        h,
        ox,
        oy,
    } = target;
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
        let past_tone = art.onion_past_color;
        let future_tone = art.onion_future_color;
        for (cels, tone) in [(&future, future_tone), (&past, past_tone)] {
            let count = cels.len();
            for (index, id) in cels.iter().rev().enumerate() {
                rows.push((id.clone(), tone, onion_strength(count - 1 - index)));
            }
        }
        let mut canvas = vec![0f32; pixels * 4];
        for (id, tone, strength) in rows {
            let plate = cel_plate(art, &id, w, h, ox, oy, step);
            for index in 0..pixels {
                let top_a = plate[index * 4 + 3] * strength / 255.0;
                if top_a <= 0.0 {
                    continue;
                }
                let base = &mut canvas[index * 4..index * 4 + 4];
                let base_a = base[3] / 255.0;
                let out_a = top_a + base_a * (1.0 - top_a);
                for channel in 0..3 {
                    let toned = plate[index * 4 + channel] * (1.0 - ONION_TINT)
                        + f32::from(tone[channel]) * ONION_TINT;
                    let mixed = toned * top_a + base[channel] * base_a * (1.0 - top_a);
                    base[channel] = if out_a > 1e-6 { mixed / out_a } else { 0.0 };
                }
                base[3] = out_a * 255.0;
            }
        }
        let cover: Vec<f32> = match current {
            Some(id) => cel_plate(art, &id, w, h, ox, oy, step)
                .chunks_exact(4)
                .map(|px| px[3])
                .collect(),
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
                let mixed = canvas[index * 4 + channel] * mix
                    + f32::from(px[channel]) * base_a * (1.0 - mix);
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
    blit_into(
        dst, dw, dh, 0, 0, src, sw, sh, ox, oy, opacity, clip, blend, 1,
    );
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
    step: u32,
) {
    if sw == 0
        || sh == 0
        || dw == 0
        || dh == 0
        || opacity == 0
        || src.len() < (sw as usize) * (sh as usize) * 4
    {
        return;
    }
    if step > 1 {
        let stride = step as i32;
        for dy in 0..dh {
            for dx in 0..dw {
                let doc_x = dst_ox + dx as i32 * stride;
                let doc_y = dst_oy + dy as i32 * stride;
                let x = doc_x - ox;
                let y = doc_y - oy;
                if x < 0 || y < 0 || x >= sw as i32 || y >= sh as i32 {
                    continue;
                }
                let si = ((y as u32 * sw + x as u32) * 4) as usize;
                let di = ((dy * dw + dx) * 4) as usize;
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
                    let faded = Rgba {
                        a: mul_opacity(opacity, src_px.a),
                        ..src_px
                    };
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
                let faded = Rgba {
                    a: mul_opacity(opacity, src_px.a),
                    ..src_px
                };
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
    blit_buffer(
        &mut pixels,
        width,
        height,
        &raster.rgba,
        raster.w,
        raster.h,
        raster.x,
        raster.y,
        255,
        false,
        "normal",
    );
    pixels
}

/// The painted bounds of a full canvas buffer. None when nothing is painted,
/// or when the buffer is not a full canvas (a folder has no pixels).
pub fn tight_pixels(pixels: &[u8], width: u32, height: u32) -> Option<Raster> {
    if pixels.len()
        < (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    {
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

/// One library row. `depth` is how many folders deep. The top row is the back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryRow {
    pub id: String,
    pub depth: u16,
    pub folder: bool,
}

/// Folders, then the symbols inside them, back to front. A closed folder
/// keeps its children out of the list. A broken folder id sits at the root.
pub fn library_rows(objects: &[DeskObject], closed: &HashSet<String>) -> Vec<LibraryRow> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    walk_library(objects, "", 0, closed, &mut out, &mut seen);
    let mut rest: Vec<&DeskObject> = objects
        .iter()
        .filter(|object| !seen.contains(&object.id))
        .collect();
    rest.sort_by(|a, b| a.stack.cmp(&b.stack).then(a.id.cmp(&b.id)));
    for object in rest {
        out.push(LibraryRow {
            id: object.id.clone(),
            depth: 0,
            folder: object.is_folder(),
        });
    }
    out
}

fn walk_library(
    objects: &[DeskObject],
    parent: &str,
    depth: u16,
    closed: &HashSet<String>,
    out: &mut Vec<LibraryRow>,
    seen: &mut HashSet<String>,
) {
    if depth > 32 {
        return;
    }
    let mut folders: Vec<&DeskObject> = objects
        .iter()
        .filter(|object| object.is_folder() && object.folder == parent)
        .collect();
    folders.sort_by(|a, b| {
        a.stack
            .cmp(&b.stack)
            .then(a.name.cmp(&b.name))
            .then(a.id.cmp(&b.id))
    });
    let mut symbols: Vec<&DeskObject> = objects
        .iter()
        .filter(|object| !object.is_folder() && object.folder == parent)
        .collect();
    symbols.sort_by(|a, b| a.stack.cmp(&b.stack).then(a.id.cmp(&b.id)));
    for folder in folders {
        if !seen.insert(folder.id.clone()) {
            continue;
        }
        out.push(LibraryRow {
            id: folder.id.clone(),
            depth,
            folder: true,
        });
        if closed.contains(&folder.id) {
            hide_inside(objects, &folder.id, seen);
        } else {
            walk_library(objects, &folder.id, depth + 1, closed, out, seen);
        }
    }
    for symbol in symbols {
        if seen.insert(symbol.id.clone()) {
            out.push(LibraryRow {
                id: symbol.id.clone(),
                depth,
                folder: false,
            });
        }
    }
}

fn hide_inside(objects: &[DeskObject], parent: &str, seen: &mut HashSet<String>) {
    for object in objects {
        if object.folder == parent && seen.insert(object.id.clone()) && object.is_folder() {
            hide_inside(objects, &object.id, seen);
        }
    }
}

/// True when `id` sits inside `ancestor`, following folder ids.
pub fn folder_contains(objects: &[DeskObject], ancestor: &str, id: &str) -> bool {
    if ancestor.is_empty() || id.is_empty() {
        return false;
    }
    let mut cursor = id.to_string();
    for _ in 0..32 {
        if cursor == ancestor {
            return true;
        }
        let Some(next) = objects
            .iter()
            .find(|object| object.id == cursor)
            .map(|object| object.folder.clone())
        else {
            return false;
        };
        if next.is_empty() {
            return false;
        }
        cursor = next;
    }
    false
}

/// The next stack in this folder. Low stack is the back, so a new row is in front.
pub fn next_stack(objects: &[DeskObject], folder: &str, folder_row: bool) -> i64 {
    objects
        .iter()
        .filter(|object| object.is_folder() == folder_row && object.folder == folder)
        .map(|object| object.stack)
        .max()
        .unwrap_or(-1)
        + 1
}

/// Move a symbol or a folder. `index` counts from the back among the rows it joins.
/// A folder cannot move into itself. Returns false when nothing changes.
pub fn move_symbol(objects: &mut [DeskObject], id: &str, to_folder: &str, index: usize) -> bool {
    let Some(moving) = objects.iter().find(|object| object.id == id) else {
        return false;
    };
    let moving_folder = moving.is_folder();
    if moving_folder && (to_folder == id || folder_contains(objects, id, to_folder)) {
        return false;
    }
    if !to_folder.is_empty()
        && !objects
            .iter()
            .any(|object| object.id == to_folder && object.is_folder())
    {
        return false;
    }
    let mut order: Vec<(i64, String)> = objects
        .iter()
        .filter(|object| object.is_folder() == moving_folder && object.folder == to_folder)
        .map(|object| (object.stack, object.id.clone()))
        .collect();
    order.sort();
    let previous: Vec<String> = order.into_iter().map(|(_, ident)| ident).collect();
    let mut next: Vec<String> = previous
        .iter()
        .filter(|ident| ident.as_str() != id)
        .cloned()
        .collect();
    let index = index.min(next.len());
    next.insert(index, id.to_string());
    let already = objects
        .iter()
        .any(|object| object.id == id && object.folder == to_folder);
    if already && previous == next {
        return false;
    }
    if let Some(object) = objects.iter_mut().find(|object| object.id == id) {
        object.folder = to_folder.to_string();
    }
    for (stack, ident) in next.iter().enumerate() {
        if let Some(object) = objects.iter_mut().find(|object| &object.id == ident) {
            object.stack = stack as i64;
        }
    }
    true
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
    use std::collections::HashSet;

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
        let mut objects = vec![
            DeskObject::new("a", "A", Vec::new()),
            DeskObject::new("b", "B", Vec::new()),
        ];
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
            folder(
                "one",
                3,
                0,
                &[("a", RED), ("b", GREEN), ("c", BLUE)],
                &[0, 2, 5],
            ),
            loose,
            folder("two", 3, 2, &[("d", RED), ("e", BLUE)], &[0, 1]),
        ];
        let expect = [
            (0, RED, RED),
            (1, RED, BLUE),
            (2, GREEN, BLUE),
            (4, GREEN, BLUE),
            (5, BLUE, BLUE),
            (9, BLUE, BLUE),
        ];
        for (frame, one, two) in expect {
            art.playhead = frame;
            let plate = present(&mut art);
            assert_eq!(px(&plate, 3, 0, 0), one, "folder one at frame {frame}");
            assert_eq!(
                px(&plate, 3, 1, 0),
                YELLOW,
                "a plain layer shows at every frame"
            );
            assert_eq!(px(&plate, 3, 2, 0), two, "folder two at frame {frame}");
            assert_eq!(
                present_plain(&mut art),
                plate,
                "onion is off, plain is the same picture"
            );
        }

        find_mut(&mut art.layers, "one").unwrap().frames = vec![1, 2, 5];
        art.playhead = 0;
        assert_eq!(
            px(&present(&mut art), 3, 0, 0),
            WHITE,
            "no cel before the first key"
        );

        find_mut(&mut art.layers, "one").unwrap().frames = vec![0, 2, 5];
        find_mut(&mut art.layers, "b").unwrap().visible = false;
        art.playhead = 3;
        assert_eq!(
            px(&present(&mut art), 3, 0, 0),
            WHITE,
            "a hidden cel shows nothing, not another cel"
        );
        find_mut(&mut art.layers, "b").unwrap().visible = true;
        find_mut(&mut art.layers, "b").unwrap().opacity = 0;
        let plate = present(&mut art);
        assert_eq!(px(&plate, 3, 0, 0), WHITE, "a cel keeps its own opacity");

        find_mut(&mut art.layers, "one").unwrap().visible = false;
        for frame in [0, 3, 7] {
            art.playhead = frame;
            assert_eq!(
                px(&present(&mut art), 3, 0, 0),
                WHITE,
                "a hidden folder drops every cel"
            );
        }
        find_mut(&mut art.layers, "one").unwrap().visible = true;
        find_mut(&mut art.layers, "one").unwrap().animation = false;
        art.playhead = 0;
        assert_eq!(
            px(&present(&mut art), 3, 0, 0),
            BLUE,
            "a plain folder stacks its children, front on top"
        );
    }

    #[test]
    fn a_nested_clip_plays_its_own_frame_inside_the_parent_hold() {
        let mut art = Art::blank(2, 1);
        let ground = solid(2, 1, "ground", "ground", WHITE);
        let mut red = Node::layer("red", "red", 2, 1);
        paint(&mut red, 2, 0, 0, RED);
        let mut blue = Node::layer("blue", "blue", 2, 1);
        paint(&mut blue, 2, 0, 0, BLUE);
        let mut mouth = Node::group("mouth", "mouth");
        mouth.animation = true;
        mouth.children = vec![red, blue];
        mouth.timeline = vec!["red".into(), "blue".into()];
        mouth.frames = vec![0, 1];
        let mut shot = Node::group("shot", "shot");
        shot.animation = true;
        shot.children = vec![mouth];
        shot.timeline = vec!["mouth".into()];
        shot.frames = vec![2];
        art.layers = vec![ground, shot];
        art.playhead = 0;
        assert_eq!(
            px(&present(&mut art), 2, 0, 0),
            WHITE,
            "before the clip the ground shows"
        );
        art.playhead = 2;
        assert_eq!(
            px(&present(&mut art), 2, 0, 0),
            RED,
            "the clip's first frame sits on its key"
        );
        art.playhead = 3;
        assert_eq!(
            px(&present(&mut art), 2, 0, 0),
            BLUE,
            "the next frame of the clip follows"
        );
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("blue"));
        art.playhead = 2;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("red"));
        art.playhead = 0;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
    }

    #[test]
    fn a_wide_canvas_plays_back_from_a_smaller_plate() {
        let mut art = Art::blank(64, 8);
        let mut ink = Node::layer("ink", "ink", 64, 8);
        paint(&mut ink, 64, 0, 0, RED);
        art.layers = vec![ink];
        let (w, h, rgba) = present_proxy(&mut art, 32);
        assert_eq!((w, h), (32, 4));
        assert_eq!(rgba.len(), 32 * 4 * 4);
        assert_eq!(
            px(&rgba, w, 0, 0),
            RED,
            "the stride keeps the pixel it samples"
        );
        assert_eq!(present(&mut art).len(), 64 * 8 * 4);
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
        assert!(
            purple[2] > purple[0] && purple[0] > purple[1],
            "past is purple: {purple:?}"
        );
        assert!(
            green[1] > green[0] && green[1] > green[2],
            "future is green: {green:?}"
        );
        assert_eq!(purple[3], 255);
        assert_eq!(
            px(&skinned, 8, 3, 3),
            RED,
            "the current cel covers the tint"
        );
        assert_eq!(
            px(&skinned, 8, 0, 0),
            WHITE,
            "paper away from the cels is untouched"
        );
        assert_eq!(
            present_plain(&mut art),
            plain,
            "plain never shows onion skin"
        );
        let region = present_region(&mut art, [0, 0, 4, 4]);
        assert_eq!(
            px(&region, 4, 1, 1),
            purple,
            "a region carries the same tint"
        );
        let past = find(&art.layers, "past").unwrap();
        assert_eq!(
            px(&past.pixels, 8, 1, 1),
            RED,
            "the stored cel is not recoloured"
        );

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
        assert!(
            tint[3] > 0 && tint[2] > tint[0] && tint[0] > tint[1],
            "visible over clear paper: {tint:?}"
        );
        assert_eq!(px(&bare, 8, 0, 0), CLEAR);
        art.onion = false;
        assert_eq!(px(&present(&mut art), 8, 1, 1), CLEAR);

        art.onion = true;
        art.onion_past_color = [255, 0, 0];
        let red = px(&present(&mut art), 8, 1, 1);
        assert!(
            red[0] > red[2] && red[0] > red[1],
            "a chosen past colour replaces the purple: {red:?}"
        );
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
        assert_eq!(
            present(&mut art),
            vec![0; 8],
            "a dropped base takes its masks with it"
        );
        find_mut(&mut art.layers, "base").unwrap().visible = true;
        find_mut(&mut art.layers, "base").unwrap().mute = true;
        assert_eq!(present(&mut art), vec![0; 8], "mute drops like hide");
        find_mut(&mut art.layers, "base").unwrap().mute = false;

        art.layers.remove(0);
        assert_eq!(
            present(&mut art),
            vec![0; 8],
            "a mask with no base paints nothing"
        );

        let mut art = Art::blank(1, 1);
        let mut group = Node::group("g", "Folder");
        group.solo = true;
        group.children = vec![solid(1, 1, "inner", "Inner", GREEN)];
        art.layers = vec![
            solid(1, 1, "loose", "Loose", RED),
            group,
            solid(1, 1, "top", "Top", BLUE),
        ];
        assert_eq!(
            present(&mut art),
            GREEN.to_vec(),
            "solo keeps the soloed folder and all its children"
        );
        find_mut(&mut art.layers, "g").unwrap().solo = false;
        assert_eq!(present(&mut art), BLUE.to_vec(), "no solo, the front shows");
        find_mut(&mut art.layers, "inner").unwrap().solo = true;
        assert_eq!(
            present(&mut art),
            GREEN.to_vec(),
            "a soloed child is sought through its folder"
        );

        let mut art = Art::blank(1, 1);
        let mut cel = solid(1, 1, "cel", "1", RED);
        cel.clip = true;
        let mut folder = Node::group("anim", "Animation");
        folder.animation = true;
        folder.children = vec![cel];
        art.layers = vec![solid(1, 1, "paper", "Paper", WHITE), folder];
        assert_eq!(
            present(&mut art),
            WHITE.to_vec(),
            "a cel that is itself a mask paints nothing"
        );
        find_mut(&mut art.layers, "cel").unwrap().clip = false;
        assert_eq!(present(&mut art), RED.to_vec());
        find_mut(&mut art.layers, "anim").unwrap().clip = true;
        art.layers.remove(0);
        assert_eq!(
            present(&mut art),
            CLEAR.to_vec(),
            "a clipping animation folder needs a base too"
        );
    }

    #[test]
    fn a_stack_gathers_its_reference_plates_and_the_datamosh_cels_first() {
        let mut art = Art::blank(3, 1);
        let mut loose = Node::layer("loose", "Loose", 3, 1);
        paint(&mut loose, 3, 1, 0, YELLOW);
        art.layers = vec![
            solid(3, 1, "paper", "Paper", WHITE),
            folder(
                "one",
                3,
                0,
                &[("a", RED), ("b", GREEN), ("c", BLUE)],
                &[0, 2, 5],
            ),
            loose,
        ];
        assert!(effect_jobs(&mut art).is_empty(), "no stack, no work");

        // A datamosh on the animation folder: the cel at the playhead paints
        // with the folder's stack and reads every cel in timeline order, with
        // the folder's marks (I when it has none).
        let datamosh = kosv::make_effect("datamosh", &[], "", true).unwrap();
        {
            let one = find_mut(&mut art.layers, "one").unwrap();
            one.effects.push(datamosh.clone());
            one.cel_marks.insert(
                "b".into(),
                CelMark {
                    kind: "P".into(),
                    reference: String::new(),
                    origin: String::new(),
                    gain: Some(1.0),
                    block: None,
                },
            );
        }
        art.playhead = 3;
        let jobs = effect_jobs(&mut art);
        assert_eq!(jobs.len(), 1);
        let job = jobs
            .get("b")
            .expect("the cel held at frame 3 runs the folder's stack");
        assert_eq!(job.effects.len(), 1);
        assert_eq!(job.effects[0].id, "datamosh");
        assert_eq!(job.maps.ids, ["a", "b", "c"]);
        assert_eq!(job.maps.index, 1);
        assert_eq!(job.maps.frames.len(), 3);
        assert_eq!(px(&job.maps.frames[2].rgba, 3, 0, 0), BLUE);
        let kinds: Vec<&str> = job
            .maps
            .marks
            .iter()
            .map(|mark| mark.kind.as_str())
            .collect();
        assert_eq!(kinds, ["I", "P", "I"]);

        // A reference slot names a layer: its picture rides along. A folder
        // in a slot gives nothing. The cel's own stack runs after the folder's.
        let mut grain = kosv::make_effect("grain-plate", &[], "", true).unwrap();
        grain
            .params
            .insert("plate".into(), Param::Text("loose".into()));
        grain
            .params
            .insert("shape".into(), Param::Text("one".into()));
        find_mut(&mut art.layers, "b").unwrap().effects.push(grain);
        let jobs = effect_jobs(&mut art);
        let job = jobs.get("b").unwrap();
        assert_eq!(
            job.effects
                .iter()
                .map(|effect| effect.id.as_str())
                .collect::<Vec<_>>(),
            ["datamosh", "grain-plate"]
        );
        let plate = job
            .maps
            .plate
            .as_ref()
            .expect("the loose layer is the plate");
        assert_eq!((plate.width, plate.height), (3, 1));
        assert_eq!(px(&plate.rgba, 3, 1, 0), YELLOW);
        assert!(job.maps.shape.is_none());

        // With a stack on the picture a pen move's region still comes from
        // the whole plate, and reads the same.
        let full = present(&mut art);
        let part = present_region(&mut art, [1, 0, 3, 1]);
        assert_eq!(part.len(), 2 * 4);
        assert_eq!(&part[..], &full[4..12]);
    }

    #[test]
    fn a_click_finds_the_front_layer_and_a_hole_falls_through() {
        let mut art = Art::blank(4, 4);
        let mut back = Node::layer("back", "back", 4, 4);
        fill(&mut back, [255, 0, 0, 255]);
        let mut front = Node::layer("front", "front", 4, 4);
        paint(&mut front, 4, 1, 1, [0, 0, 255, 255]);
        art.layers = vec![back, front];
        assert_eq!(pick_layer(&art, 1.9, 1.2).as_deref(), Some("front"));
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("back"));
        assert_eq!(pick_layer(&art, 9.0, 0.0), None);
        art.layers[1].opacity = 0;
        assert_eq!(pick_layer(&art, 1.0, 1.0).as_deref(), Some("back"));
    }

    #[test]
    fn a_clip_counts_only_inside_its_base() {
        let mut art = Art::blank(8, 8);
        let mut ground = Node::layer("ground", "ground", 8, 8);
        fill(&mut ground, [0, 255, 0, 255]);
        let mut base = Node::layer("base", "base", 8, 8);
        for y in 3..5 {
            for x in 3..5 {
                paint(&mut base, 8, x, y, [255, 0, 0, 255]);
            }
        }
        let mut mask = Node::layer("mask", "mask", 8, 8);
        fill(&mut mask, [0, 0, 255, 255]);
        mask.clip = true;
        art.layers = vec![ground, base, mask];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
        assert_eq!(pick_layer(&art, 3.0, 3.0).as_deref(), Some("mask"));
        assert_eq!(pick_layer(&art, 4.0, 4.0).as_deref(), Some("mask"));
        art.layers[1].visible = false;
        assert_eq!(pick_layer(&art, 3.0, 3.0).as_deref(), Some("ground"));
    }

    #[test]
    fn hidden_muted_and_unsoloed_layers_are_not_there() {
        let mut art = Art::blank(2, 2);
        let mut back = Node::layer("back", "back", 2, 2);
        fill(&mut back, [255, 0, 0, 255]);
        let mut front = Node::layer("front", "front", 2, 2);
        fill(&mut front, [0, 0, 255, 255]);
        front.visible = false;
        art.layers = vec![back, front];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("back"));
        art.layers[1].visible = true;
        art.layers[1].mute = true;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("back"));
        art.layers[1].mute = false;
        art.layers[0].solo = true;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("back"));
    }

    #[test]
    fn a_click_inside_a_folder_is_the_layer() {
        let mut art = Art::blank(4, 4);
        let mut sky = Node::layer("sky", "sky", 4, 4);
        fill(&mut sky, [0, 255, 0, 255]);
        let mut sign = Node::layer("sign", "sign", 4, 4);
        for y in 1..3 {
            for x in 1..3 {
                paint(&mut sign, 4, x, y, [0, 0, 255, 255]);
            }
        }
        let mut folder = Node::group("booth", "booth");
        folder.children.push(sign);
        art.layers = vec![sky, folder];
        assert_eq!(pick_layer(&art, 1.0, 1.0).as_deref(), Some("sign"));
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("sky"));
    }

    #[test]
    fn an_animation_click_uses_the_cel_on_screen() {
        let mut art = Art::blank(2, 2);
        let mut red = Node::layer("red", "red", 2, 2);
        fill(&mut red, [255, 0, 0, 255]);
        let mut blue = Node::layer("blue", "blue", 2, 2);
        fill(&mut blue, [0, 0, 255, 255]);
        let mut folder = Node::group("walk", "walk");
        folder.animation = true;
        folder.children = vec![red, blue];
        art.layers = vec![folder];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("blue"));
        art.playhead = 1;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("red"));
    }

    #[test]
    fn a_hole_in_the_front_mask_falls_through_the_stack() {
        let mut art = Art::blank(8, 8);
        let ground = solid(8, 8, "ground", "ground", GREEN);
        let mut base = Node::layer("base", "base", 8, 8);
        for y in 2..6 {
            for x in 2..6 {
                paint(&mut base, 8, x, y, RED);
            }
        }
        let mut rear = solid(8, 8, "rear", "rear", BLUE);
        rear.clip = true;
        let mut front = Node::layer("front", "front", 8, 8);
        paint(&mut front, 8, 3, 3, YELLOW);
        front.clip = true;
        art.layers = vec![ground, base, rear, front];
        assert_eq!(pick_layer(&art, 3.0, 3.0).as_deref(), Some("front"));
        assert_eq!(pick_layer(&art, 4.0, 4.0).as_deref(), Some("rear"));
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
        art.layers[2].visible = false;
        assert_eq!(pick_layer(&art, 4.0, 4.0).as_deref(), Some("base"));
        art.layers[2].visible = true;
        art.layers[3].opacity = 0;
        assert_eq!(pick_layer(&art, 3.0, 3.0).as_deref(), Some("rear"));
    }

    #[test]
    fn a_clip_folder_yields_the_layer_inside_the_base() {
        let mut art = Art::blank(8, 8);
        let ground = solid(8, 8, "ground", "ground", GREEN);
        let mut base = Node::layer("base", "base", 8, 8);
        for y in 2..5 {
            for x in 2..5 {
                paint(&mut base, 8, x, y, RED);
            }
        }
        let mut sign = solid(8, 8, "sign", "sign", BLUE);
        paint(&mut sign, 8, 3, 3, CLEAR);
        let mut folder = Node::group("mask", "mask");
        folder.clip = true;
        folder.children.push(sign);
        art.layers = vec![ground, base, folder];
        assert_eq!(pick_layer(&art, 2.0, 2.0).as_deref(), Some("sign"));
        assert_eq!(pick_layer(&art, 3.0, 3.0).as_deref(), Some("base"));
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
    }

    #[test]
    fn a_solo_is_sought_through_its_folders() {
        let mut art = Art::blank(2, 2);
        let loose = solid(2, 2, "loose", "loose", RED);
        let mut star = solid(2, 2, "star", "star", BLUE);
        star.solo = true;
        let dust = solid(2, 2, "dust", "dust", GREEN);
        let mut inner = Node::group("inner", "inner");
        inner.children = vec![dust, star];
        let mut folder = Node::group("booth", "booth");
        folder.children.push(inner);
        art.layers = vec![loose, folder];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("star"));
        art.layers[1].children[0].children[1].solo = false;
        art.layers[1].solo = true;
        assert_eq!(
            pick_layer(&art, 0.0, 0.0).as_deref(),
            Some("star"),
            "a soloed folder keeps the child in front"
        );
    }

    #[test]
    fn a_hidden_folder_and_a_muted_parent_drop_their_children() {
        let mut art = Art::blank(2, 2);
        let ground = solid(2, 2, "ground", "ground", GREEN);
        let child = solid(2, 2, "child", "child", BLUE);
        let mut folder = Node::group("booth", "booth");
        folder.visible = false;
        folder.children.push(child);
        art.layers = vec![ground, folder];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
        art.layers[1].visible = true;
        art.layers[1].mute = true;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
    }

    #[test]
    fn an_animation_click_follows_the_key_and_skips_an_empty_hold() {
        let mut art = Art::blank(2, 2);
        let ground = solid(2, 2, "ground", "ground", GREEN);
        let cel = solid(2, 2, "cel", "cel", BLUE);
        let mut folder = Node::group("walk", "walk");
        folder.animation = true;
        folder.timeline = vec!["cel".into()];
        folder.frames = vec![2];
        folder.children.push(cel);
        art.layers = vec![ground.clone(), folder];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
        art.playhead = 2;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("cel"));
        art.playhead = 3;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("cel"));

        let mut red = solid(2, 2, "red", "red", RED);
        red.visible = false;
        let blue = solid(2, 2, "blue", "blue", BLUE);
        let mut held = Node::group("held", "held");
        held.animation = true;
        held.timeline = vec!["red".into(), "blue".into()];
        held.frames = vec![0, 2];
        held.children = vec![red, blue];
        art.layers = vec![ground, held];
        art.playhead = 0;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
        art.playhead = 2;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("blue"));
        art.layers[1].children[1].mute = true;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("ground"));
    }

    #[test]
    fn a_cel_that_is_a_mask_and_a_solo_off_its_key_are_absent() {
        let mut art = Art::blank(2, 2);
        let paper = solid(2, 2, "paper", "paper", WHITE);
        let mut cel = solid(2, 2, "cel", "cel", BLUE);
        cel.clip = true;
        let mut folder = Node::group("walk", "walk");
        folder.animation = true;
        folder.children.push(cel);
        art.layers = vec![paper, folder];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("paper"));

        let red = solid(2, 2, "red", "red", RED);
        let mut blue = solid(2, 2, "blue", "blue", BLUE);
        blue.solo = true;
        let mut keyed = Node::group("keyed", "keyed");
        keyed.animation = true;
        keyed.timeline = vec!["red".into(), "blue".into()];
        keyed.frames = vec![0, 2];
        keyed.children = vec![red, blue];
        art.layers = vec![keyed];
        art.playhead = 0;
        assert_eq!(pick_layer(&art, 0.0, 0.0), None);
        art.playhead = 2;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("blue"));
    }

    #[test]
    fn opacity_one_hits_and_a_folder_at_zero_still_yields_its_child() {
        let mut art = Art::blank(2, 2);
        let back = solid(2, 2, "back", "back", RED);
        let mut front = Node::layer("front", "front", 2, 2);
        paint(&mut front, 2, 1, 1, BLUE);
        front.opacity = 1;
        art.layers = vec![back, front];
        assert_eq!(pick_layer(&art, 1.0, 1.0).as_deref(), Some("front"));
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("back"));

        let child = solid(2, 2, "child", "child", GREEN);
        let mut folder = Node::group("booth", "booth");
        folder.opacity = 0;
        folder.children.push(child);
        art.layers = vec![folder];
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("child"));
        assert!(pick_layer(&art, f32::NAN, 0.0).is_none());
        assert!(pick_layer(&art, f32::INFINITY, 1.0).is_none());
        art.layers.clear();
        assert!(pick_layer(&art, 0.0, 0.0).is_none());
    }

    #[test]
    fn an_unbaked_stroke_is_ink_and_a_baked_stroke_is_only_the_pixels() {
        use crate::tool::{Tool, ToolSettings};
        use crate::vector::VectorStroke;

        let mut art = Art::blank(48, 48);
        let back = solid(48, 48, "back", "back", RED);
        let mut ink = Node::layer("ink", "ink", 48, 48);
        ink.pixels.fill(0);
        let settings = ToolSettings::default_for(Tool::Vector);
        let stroke =
            VectorStroke::from_gesture(1, Tool::Vector, &[(8.0, 8.0, 1.0)], &settings).unwrap();
        ink.strokes.push(stroke);
        art.layers = vec![back, ink];
        assert_eq!(pick_layer(&art, 8.0, 8.0).as_deref(), Some("ink"));
        assert_eq!(pick_layer(&art, 40.0, 40.0).as_deref(), Some("back"));
        art.layers[1].embedded = true;
        assert_eq!(pick_layer(&art, 8.0, 8.0).as_deref(), Some("back"));
        paint(&mut art.layers[1], 48, 2, 2, BLUE);
        assert_eq!(pick_layer(&art, 2.0, 2.0).as_deref(), Some("ink"));
    }

    #[test]
    fn onion_skin_is_not_a_hit() {
        let mut art = Art::blank(4, 4);
        let mut now = Node::layer("now", "now", 4, 4);
        paint(&mut now, 4, 0, 0, RED);
        let later = solid(4, 4, "later", "later", BLUE);
        let mut folder = Node::group("walk", "walk");
        folder.animation = true;
        folder.timeline = vec!["now".into(), "later".into()];
        folder.frames = vec![0, 2];
        folder.children = vec![now, later];
        art.layers = vec![folder];
        art.onion = true;
        art.onion_future = 3;
        art.playhead = 0;
        assert_eq!(pick_layer(&art, 0.0, 0.0).as_deref(), Some("now"));
        assert_eq!(pick_layer(&art, 3.0, 3.0), None);
    }

    #[test]
    fn slots_are_hex_and_a_button_names_its_states() {
        assert_eq!(slot_hex(0), "000");
        assert_eq!(slot_hex(10), "00A");
        assert_eq!(slot_hex(0x1000), "000");
        assert_eq!(slot_key(1, 2), "001002");
        assert_eq!(state_word(0), "idle");
        assert_eq!(state_word(2), "click");
        let mut object = DeskObject::new("o", "Door", Vec::new());
        assert_eq!(object.states(), vec![0]);
        assert_eq!(object.label_of(0, 0), "idle");
        assert_eq!(object.add_frame(0), 1);
        assert_eq!(object.kind, "sequence");
        assert_eq!(object.frames_of(0), vec![0, 1]);
        object.kind = "button".into();
        assert_eq!(object.states(), vec![0, 1, 2, 3]);
        assert_eq!(object.label_of(1, 0), "hover");
        assert_eq!(object.label_of(2, 0), "click");
        object.rename_slot(1, 0, "over");
        assert_eq!(
            object.labels.get("001000").map(String::as_str),
            Some("over")
        );
        object.rename_slot(1, 0, "");
        assert!(!object.labels.contains_key("001000"));
        assert_eq!(object.add_state(), 4);
        assert!(object.labels.contains_key("004000"));
    }

    #[test]
    fn a_library_folder_orders_symbols_and_a_state_keeps_its_sample() {
        let mut folder = DeskObject::new("f", "Faces", Vec::new());
        folder.kind = "folder".into();
        assert!(folder.is_folder());
        assert_eq!(folder.add_state(), 0);
        assert_eq!(folder.add_frame(0), 0);
        assert!(folder.labels.is_empty());

        let mut mouth = DeskObject::new("m", "Mouth", Vec::new());
        mouth.kind = "mouth".into();
        mouth.folder = "f".into();
        mouth.stack = 0;
        mouth.sound = "samples/rest.wav".into();
        mouth.set_sample(1, 0, "samples/ah.wav");
        mouth.rename_slot(1, 0, "AH");
        assert_eq!(mouth.states(), vec![0, 1]);
        assert_eq!(mouth.own_sample(0, 0), "");
        assert_eq!(mouth.sample_of(0, 0), "samples/rest.wav");
        assert_eq!(mouth.sample_of(1, 0), "samples/ah.wav");
        mouth.set_sample(1, 2, "samples/ah-hold.wav");
        assert_eq!(mouth.sample_of(1, 2), "samples/ah-hold.wav");
        assert_eq!(mouth.sample_of(1, 5), "samples/ah.wav");
        assert_eq!(mouth.add_frame(0), 1);
        assert_eq!(mouth.kind, "mouth");
        assert_eq!(mouth.take_layer("n1", 1), 0);
        assert_eq!(mouth.assign["n1"], (1, 0));
        assert_eq!(mouth.take_layer("n2", 1), 1);
        assert_eq!(mouth.assign["n2"], (1, 1));
        assert_eq!(mouth.kind, "mouth");
        mouth.set_sample(1, 0, "  ");
        assert_eq!(mouth.sample_of(1, 0), "samples/rest.wav");

        let mut hand = DeskObject::new("h", "Hand", Vec::new());
        hand.kind = "pose".into();
        hand.folder = "f".into();
        hand.stack = 1;
        let mut door = DeskObject::new("d", "Door", Vec::new());
        door.kind = "static".into();
        door.stack = 0;
        let mut warp = DeskObject::new("w", "Screen", Vec::new());
        warp.kind = "warp target".into();
        assert_eq!(warp.add_frame(0), 1);
        assert_eq!(warp.kind, "warp target");

        let mut objects = vec![
            hand.clone(),
            door.clone(),
            mouth.clone(),
            folder.clone(),
            warp,
        ];
        let rows = library_rows(&objects, &HashSet::new());
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["f", "m", "h", "d", "w"]
        );
        assert!(rows[0].folder);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[3].depth, 0);
        let mut closed = HashSet::new();
        closed.insert("f".into());
        let rows = library_rows(&objects, &closed);
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["f", "d", "w"]
        );

        assert!(move_symbol(&mut objects, "h", "", 0));
        assert_eq!(
            objects
                .iter()
                .find(|object| object.id == "h")
                .unwrap()
                .folder,
            ""
        );
        assert_eq!(
            objects
                .iter()
                .find(|object| object.id == "h")
                .unwrap()
                .stack,
            0
        );
        assert_eq!(
            objects
                .iter()
                .find(|object| object.id == "d")
                .unwrap()
                .stack,
            1
        );
        assert!(!move_symbol(&mut objects, "f", "f", 0));
        assert_eq!(
            objects
                .iter()
                .find(|object| object.id == "f")
                .unwrap()
                .folder,
            ""
        );
        let mut nested = DeskObject::new("n", "Inside", Vec::new());
        nested.kind = "folder".into();
        nested.folder = "f".into();
        objects.push(nested);
        assert!(!move_symbol(&mut objects, "f", "n", 0));
        assert!(!move_symbol(&mut objects, "m", "f", 0));
        assert_eq!(next_stack(&objects, "f", false), 1);
    }
}
