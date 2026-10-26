//! Animation folders, cels, the exposure sheet, and onion skin.
//!
//! A port of the timeline half of `vmi_studio/document.py`, of `onion.py`
//! (the neighbour rules; the tint itself is in `doc.rs`), of `exposure.py`,
//! and of `xsheet.run_command`. Frames are 0-based, the way Python keeps them.
//!
//! # Timeline, frames, and the exposure sheet
//!
//! An animation folder is a group with `animation` on that is not already a
//! cel inside another one. Its direct children are the cels. `timeline` holds
//! cel ids in timeline order and `frames` holds the start frame of each, in
//! the same order. Those two lists are the keys and they alone decide the
//! picture: `cel_at(folder, playhead)` is the cel shown, it holds until the
//! next key, the last key holds through the end of the cut, and before the
//! first key the folder shows nothing. An empty `frames` means the keys are
//! 0, 1, 2… in timeline order, and an empty `timeline` means the panel order,
//! front child first. When `frames` is set, a child on neither list is an
//! unspecified cel: it stays in the folder and never reaches the picture.
//!
//! The exposure sheet (`Node::exposure`) is Python's OpenToonz style column:
//! slots of three hex digit drawing ids, one per frame, pointing into a pool
//! of ids that are never reissued. It is what the X-sheet shows and edits. It
//! does not drive the picture and is never written back into `timeline` or
//! `frames`. A folder without a sheet shows a projection of its keys
//! (`project_slots`, 000 for the first cel, 001 for the next…); the first
//! X-sheet command turns that projection into a stored sheet
//! (`sheet_from_slots`) and edits it from then on. The filmstrip reads hold
//! and reuse marks from the sheet by the cel's place in frame order
//! (`cel_mark`). This is the relationship the Python window keeps, and the
//! blueprint stores both halves as they are.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::doc::{find, find_mut, fresh_name, Art, Node, NodeKind};

/// Default playback rate, the same as `csptime.py`.
pub const DEFAULT_FPS: u32 = 24;
/// Onion skin tone for past cels. KoolDraw's purple.
pub const PAST: [u8; 3] = [180, 80, 255];
/// Onion skin tone for future cels. KoolDraw's green.
pub const FUTURE: [u8; 3] = [60, 220, 120];
/// Onion skin opacity by distance from the playhead: KoolDraw's 0.7, 0.4, 0.18.
pub const STRENGTH: [f32; 3] = [0.7, 0.4, 0.18];
/// How far a neighbour's own colour is pulled toward its tone. KoolDraw's
/// 0.88 leaves a little of the drawing's colour in the skin.
pub const ONION_TINT: f32 = 0.88;
/// Neighbour cels each way: three back, three ahead.
pub const ONION_LIMIT: u8 = 3;
/// The last drawing id an exposure sheet can issue, `exposure.FRAME_MAX`.
pub const FRAME_MAX: u32 = 0xFFF;

/// One cel on a folder's timeline, with the frames it holds.
#[derive(Clone, Copy, Debug)]
pub struct Cel<'a> {
    pub id: &'a str,
    pub start: u32,
    /// Exclusive. The last cel holds to the cut end.
    pub end: u32,
    pub node: &'a Node,
    /// Place in frame order, from 0. The filmstrip prints `index + 1`.
    pub index: usize,
}

/// One X-sheet column. The first column is the back of the picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XColumn {
    pub folder_id: String,
    pub name: String,
    /// The cel holding each frame, `cut_end` long. None is an empty row.
    pub cells: Vec<Option<String>>,
    /// True where a key puts a cel on the frame, `cut_end` long.
    pub keys: Vec<bool>,
    /// What Python's X-sheet prints in each row: the stored sheet's slots when
    /// the folder has one, else `project_slots` of its keys. Not padded to the
    /// cut, and a stored sheet may run past it.
    pub labels: Vec<String>,
    /// True when `labels` come from a stored `ExposureSheet`.
    pub stored: bool,
}

fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for node in nodes {
        out.push(node);
        walk(&node.children, out);
    }
}

/// Marked folders that are not already a frame inside another animation.
pub fn animation_folders(nodes: &[Node]) -> Vec<&Node> {
    fn visit<'a>(nodes: &'a [Node], under: bool, out: &mut Vec<&'a Node>) {
        for node in nodes {
            let marked = node.kind == NodeKind::Group && node.animation && !under;
            if marked {
                out.push(node);
            }
            visit(&node.children, under || marked, out);
        }
    }
    let mut out = Vec::new();
    visit(nodes, false, &mut out);
    out
}

pub fn animation_folder_ids(nodes: &[Node]) -> Vec<String> {
    animation_folders(nodes).into_iter().map(|folder| folder.id.clone()).collect()
}

/// Direct children in timeline order. The top row is frame 0 until that
/// order is moved. Cels that were never specified stay in the folder and are
/// not frames once the folder has frame numbers.
pub fn timeline_cels(folder: &Node) -> Vec<&Node> {
    let kids: Vec<&Node> = folder.children.iter().rev().collect();
    if folder.timeline.is_empty() {
        return kids;
    }
    let mut ordered: Vec<&Node> = Vec::new();
    for id in &folder.timeline {
        if let Some(child) = kids.iter().rev().find(|child| child.id == *id) {
            ordered.push(child);
        }
    }
    if !folder.frames.is_empty() {
        return ordered;
    }
    for kid in kids {
        if !ordered.iter().any(|child| child.id == kid.id) {
            ordered.push(kid);
        }
    }
    ordered
}

/// Children of an animation folder that no frame specifies.
pub fn unspecified_cels(folder: &Node) -> Vec<&Node> {
    if folder.frames.is_empty() {
        return Vec::new();
    }
    let shown: HashSet<&str> = timeline_cels(folder).iter().map(|cel| cel.id.as_str()).collect();
    folder.children.iter().rev().filter(|child| !shown.contains(child.id.as_str())).collect()
}

/// (start frame, cel) pairs in frame order. A hand-marked folder uses 0, 1, 2…
pub fn frame_map(folder: &Node) -> Vec<(u32, &Node)> {
    let cels = timeline_cels(folder);
    let frames: Vec<u32> = if folder.frames.len() == cels.len() {
        folder.frames.clone()
    } else {
        (0..cels.len() as u32).collect()
    };
    let mut pairs: Vec<(u32, &Node)> = frames.into_iter().zip(cels).collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    pairs
}

/// The cel specified at this frame. It holds until the next specification.
/// Before the first key there is none.
pub fn cel_at(folder: &Node, frame: u32) -> Option<&Node> {
    let pairs = frame_map(folder);
    for (index, (at, cel)) in pairs.iter().enumerate() {
        if frame < *at {
            break;
        }
        let next = pairs.get(index + 1).map(|pair| pair.0);
        if next.is_none_or(|next| frame < next) {
            return Some(cel);
        }
    }
    None
}

/// The frame where this cel is keyed, or None when it is not on the timeline.
pub fn key_of(folder: &Node, cel_id: &str) -> Option<u32> {
    frame_map(folder).into_iter().find(|(_, cel)| cel.id == cel_id).map(|(frame, _)| frame)
}

/// The cels of one folder with the frames each holds. `cut_end` is the frame
/// count of the cut; the last cel holds through it (or through its own key
/// when that lies past the cut).
pub fn cels(folder: &Node, cut_end: u32) -> Vec<Cel<'_>> {
    let pairs = frame_map(folder);
    let mut out = Vec::with_capacity(pairs.len());
    for (index, (start, node)) in pairs.iter().enumerate() {
        let after = start.saturating_add(1);
        let end = match pairs.get(index + 1) {
            Some((next, _)) => (*next).max(after),
            None => cut_end.max(after),
        };
        out.push(Cel { id: &node.id, start: *start, end, node, index });
    }
    out
}

/// Frames in the cut, from the keys alone: the last key plus one, at least 1.
/// This is Python's `filmstrip.cut_range` when the file has no `clip_time`.
pub fn cut_end(nodes: &[Node]) -> u32 {
    let mut last = 0u32;
    for folder in animation_folders(nodes) {
        for (frame, _) in frame_map(folder) {
            last = last.max(frame);
        }
    }
    last.saturating_add(1)
}

/// Frames in the cut for this document: the keys, or the stored Clip Studio
/// cut when that reaches further. `set_playhead` and `step_frame` stay inside it.
pub fn cut_frames(art: &Art) -> u32 {
    let keys = cut_end(&art.layers);
    match &art.clip_time {
        Some(clip) => keys.max(clip.end.saturating_add(1)),
        None => keys,
    }
}

fn holds(folder: &Node, id: &str) -> bool {
    folder.children.iter().any(|cel| {
        cel.id == id || (cel.kind == NodeKind::Group && find(&cel.children, id).is_some())
    })
}

/// The animation folder this row is, or is a cel of (at any depth inside a
/// group cel).
pub fn owning_animation(nodes: &[Node], id: &str) -> Option<String> {
    fn visit(nodes: &[Node], id: &str) -> Option<String> {
        for node in nodes {
            if node.kind == NodeKind::Group && node.animation && (node.id == id || holds(node, id)) {
                return Some(node.id.clone());
            }
            if let Some(found) = visit(&node.children, id) {
                return Some(found);
            }
        }
        None
    }
    visit(nodes, id)
}

/// The cel of `folder` that is, or contains, this row.
pub fn cel_of<'a>(folder: &'a Node, id: &str) -> Option<&'a Node> {
    if id == folder.id {
        return None;
    }
    timeline_cels(folder).into_iter().find(|cel| {
        cel.id == id || (cel.kind == NodeKind::Group && find(&cel.children, id).is_some())
    })
}

fn clear_marks(nodes: &mut [Node]) {
    for node in nodes {
        node.animation = false;
        node.timeline.clear();
        node.frames.clear();
        clear_marks(&mut node.children);
    }
}

/// Mark a folder as a timeline, or unmark it. A folder already inside an
/// animation stays a frame: that returns false, and `owning_animation` names
/// the owner. Marking puts every child on the timeline in panel order with no
/// frame numbers and unmarks everything inside.
pub fn set_animation(nodes: &mut [Node], id: &str, on: bool) -> bool {
    match find(nodes, id) {
        Some(node) if node.kind == NodeKind::Group => {}
        _ => return false,
    }
    if on {
        if let Some(owner) = owning_animation(nodes, id) {
            if owner != id {
                return false;
            }
        }
    }
    let Some(node) = find_mut(nodes, id) else {
        return false;
    };
    node.animation = on;
    if on {
        clear_marks(&mut node.children);
        node.timeline = node.children.iter().rev().map(|cel| cel.id.clone()).collect();
        node.frames.clear();
    } else {
        node.timeline.clear();
        node.frames.clear();
    }
    true
}

/// Put one cel on a frame (Python `place_cel`). A cel already on that frame
/// swaps with it. A hand-marked folder has no frame numbers; there the move
/// is along the timeline order. The layer stack stays where it is.
pub fn move_frame(folder: &mut Node, cel_id: &str, start: u32) -> bool {
    let ids: Vec<String> = timeline_cels(folder).iter().map(|cel| cel.id.clone()).collect();
    let Some(index) = ids.iter().position(|id| id == cel_id) else {
        return false;
    };
    if folder.frames.len() != ids.len() {
        let slot = start as usize;
        if slot >= ids.len() || slot == index {
            return false;
        }
        let mut order = ids;
        let moved = order.remove(index);
        order.insert(slot, moved);
        folder.timeline = order;
        return true;
    }
    let mut frames = folder.frames.clone();
    if start == frames[index] {
        return false;
    }
    let other = frames.iter().enumerate().find(|(pos, value)| *pos != index && **value == start).map(|(pos, _)| pos);
    match other {
        None => frames[index] = start,
        Some(other) => frames.swap(index, other),
    }
    folder.frames = frames;
    true
}

/// Move one cel `delta` places along the timeline (Python `move_cel`). The
/// frame numbers travel with the places, so two cels swap frames.
pub fn move_cel(folder: &mut Node, cel_id: &str, delta: i64) -> bool {
    let mut order: Vec<String> = timeline_cels(folder).iter().map(|cel| cel.id.clone()).collect();
    let Some(index) = order.iter().position(|id| id == cel_id) else {
        return false;
    };
    let next = index as i64 + delta;
    if next < 0 || next >= order.len() as i64 {
        return false;
    }
    let next = next as usize;
    order.swap(index, next);
    if folder.frames.len() == order.len() {
        folder.frames.swap(index, next);
    }
    folder.timeline = order;
    true
}

/// Where a new cel goes so the drawing under the playhead stays in the past.
/// A playhead that already has a key steps forward. A later key moves only
/// when it occupies that new slot. Returns the target and the shifted keys.
pub fn next_frame_slot(frames: &[u32], playhead: u32) -> (u32, Vec<u32>) {
    let target = if frames.contains(&playhead) { playhead.saturating_add(1) } else { playhead };
    let frames = if frames.contains(&target) {
        frames.iter().map(|&frame| if frame >= target { frame.saturating_add(1) } else { frame }).collect()
    } else {
        frames.to_vec()
    };
    (target, frames)
}

/// Store keys on a folder, sorted by (frame, id) like Python `_write_frames`.
pub fn write_frames(folder: &mut Node, frames: Vec<u32>, ids: Vec<String>) {
    let mut order: Vec<(u32, String)> = frames.into_iter().zip(ids).collect();
    order.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    folder.frames = order.iter().map(|(frame, _)| *frame).collect();
    folder.timeline = order.into_iter().map(|(_, id)| id).collect();
}

fn containing_mut<'a>(nodes: &'a mut Vec<Node>, id: &str) -> Option<&'a mut Vec<Node>> {
    if nodes.iter().any(|node| node.id == id) {
        return Some(nodes);
    }
    for node in nodes.iter_mut() {
        if let Some(found) = containing_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

/// The folder a new frame goes into: the one this row already belongs to, a
/// plain folder marked now, or a new animation folder wrapped around a lone
/// layer where it stood (Python `_ensure_animation`).
fn ensure_animation(art: &mut Art, id: &str) -> Option<String> {
    if let Some(owner) = owning_animation(&art.layers, id) {
        return Some(owner);
    }
    let node = find(&art.layers, id)?;
    if node.kind == NodeKind::Group {
        return set_animation(&mut art.layers, id, true).then(|| id.to_string());
    }
    if node.marker == "target" || node.marker == "mask" {
        return None;
    }
    let folder_id = art.alloc_id();
    let name = fresh_name(&art.layers, "Animation");
    let list = containing_mut(&mut art.layers, id)?;
    let at = list.iter().position(|node| node.id == id)?;
    let layer = list.remove(at);
    let mut folder = Node::group(folder_id.clone(), name);
    folder.children.push(layer);
    list.insert(at, folder);
    set_animation(&mut art.layers, &folder_id, true).then_some(folder_id)
}

/// A new empty cel after the drawing under the playhead (Python
/// `new_animation_frame`, Shift+F). The cel is a full canvas layer named the
/// way Python names it (`fresh_name` of "1": "1", then "1 2", "1 3"…), keyed at
/// `next_frame_slot`. The playhead moves there and onion skin turns on, as in
/// Python. `folder_id` may be the folder, any cel of it, a plain folder, or a
/// lone layer; a warp target or mask refuses. Returns the new cel's id.
pub fn add_cel(art: &mut Art, folder_id: &str, playhead: u32) -> Option<String> {
    let owner = ensure_animation(art, folder_id)?;
    let (frames, mut ids): (Vec<u32>, Vec<String>) = {
        let folder = find(&art.layers, &owner)?;
        frame_map(folder).into_iter().map(|(frame, cel)| (frame, cel.id.clone())).unzip()
    };
    let (target, mut frames) = next_frame_slot(&frames, playhead);
    let id = art.alloc_id();
    let name = fresh_name(&art.layers, "1");
    let cel = Node::layer(id.clone(), name, art.width, art.height);
    let folder = find_mut(&mut art.layers, &owner)?;
    folder.children.push(cel);
    frames.push(target);
    ids.push(id.clone());
    write_frames(folder, frames, ids);
    art.playhead = target;
    art.onion = true;
    Some(id)
}

/// Move the playhead inside the cut.
pub fn set_playhead(art: &mut Art, frame: u32) {
    art.playhead = frame.min(cut_frames(art).saturating_sub(1));
}

/// Comma and period step by one; playback steps with `wrap` for loop. Without
/// `wrap` the playhead stops at the ends of the cut; with it the step past the
/// last frame lands on the first and the step before the first on the last.
pub fn step_frame(art: &mut Art, delta: i64, wrap: bool) {
    let count = i64::from(cut_frames(art).max(1));
    let next = i64::from(art.playhead) + delta;
    let frame = if wrap {
        if next < 0 {
            count - 1
        } else if next >= count {
            0
        } else {
            next
        }
    } else {
        next.clamp(0, count - 1)
    };
    art.playhead = frame as u32;
}

/// W: back to the first frame of the cut.
pub fn first_frame(art: &mut Art) {
    let start = art.clip_time.as_ref().map_or(0, |clip| clip.start);
    set_playhead(art, start);
}

/// Past and future cels, nearest first, and the cel the playhead is showing,
/// each side cut to its own limit. The current cel is in neither list.
pub fn onion_neighbors(folder: &Node, frame: u32, past_limit: usize, future_limit: usize) -> (Vec<&Node>, Vec<&Node>, Option<&Node>) {
    let mut past = Vec::new();
    let mut future = Vec::new();
    let mut keyed = None;
    for (at, cel) in frame_map(folder) {
        if at < frame {
            past.push(cel);
        } else if at > frame {
            future.push(cel);
        } else {
            keyed = Some(cel);
        }
    }
    let current = keyed.or_else(|| cel_at(folder, frame));
    if let Some(current) = current {
        past.retain(|cel| cel.id != current.id);
        future.retain(|cel| cel.id != current.id);
    }
    past.reverse();
    past.truncate(past_limit);
    future.truncate(future_limit);
    (past, future, current)
}

/// Python `onion.neighbors`: both sides cut to `ONION_LIMIT`.
pub fn neighbors(folder: &Node, frame: u32) -> (Vec<&Node>, Vec<&Node>, Option<&Node>) {
    onion_neighbors(folder, frame, usize::from(ONION_LIMIT), usize::from(ONION_LIMIT))
}

/// Onion opacity for a cel this many keys away from the playhead.
pub fn onion_strength(distance: usize) -> f32 {
    STRENGTH[distance.min(STRENGTH.len() - 1)]
}

// ---------------------------------------------------------------------------
// Exposure sheet
// ---------------------------------------------------------------------------

/// Three hex digits, upper case. None when the token is not a drawing id.
pub fn frame_id(token: &str) -> Option<String> {
    let token = token.trim().to_ascii_uppercase();
    if token.len() != 3 || !token.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(token)
}

fn drawing_number(cell: &str) -> i64 {
    if cell.is_empty() {
        return -1;
    }
    i64::from_str_radix(cell, 16).unwrap_or(-1)
}

/// Previous and next pool ids around one slot. A blank neighbour is None.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetOnion {
    pub prev: Option<String>,
    pub next: Option<String>,
}

/// An exposure sheet: ordered slots pointing at a pool of drawing ids.
///
/// A hold is several slots and one pool id. An empty cell (`""`) is not a
/// hold. Ids are permanent three digit hex; an issued id is never reused for
/// a later drawing. The field names are the keys of Python's `to_row`, so the
/// serde form of this struct is the blueprint row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExposureSheet {
    /// One cell per frame. `""` is a blank.
    pub slots: Vec<String>,
    /// Every id issued, in issue order, whether or not a slot still shows it.
    pub pool: Vec<String>,
    /// Ids handed out so far.
    pub issued: u32,
    pub base_seed: u32,
    /// The slot the last edit left current.
    pub active: usize,
}

impl Default for ExposureSheet {
    fn default() -> Self {
        Self::new(0)
    }
}

impl ExposureSheet {
    pub fn new(base_seed: u32) -> Self {
        Self { slots: Vec::new(), pool: Vec::new(), issued: 0, base_seed, active: 0 }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The drawing id in this slot. None for a blank or a frame past the sheet.
    pub fn cell(&self, frame: usize) -> Option<&str> {
        self.slots.get(frame).map(String::as_str).filter(|cell| !cell.is_empty())
    }

    /// The slot as stored: `Some("")` is a blank inside the sheet.
    pub fn slot(&self, frame: usize) -> Option<&str> {
        self.slots.get(frame).map(String::as_str)
    }

    /// Python's `to_row`: slots, pool, issued, base_seed, and active.
    pub fn to_row(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// Rebuild a sheet from a row. The ids are copied, not allocated. A bad
    /// id, a slot that is not in the pool, an issued count behind the pool, or
    /// an active slot off the sheet is None (Python raises).
    pub fn from_row(value: &Value) -> Option<ExposureSheet> {
        let row = value.as_object()?;
        let slots = row
            .get("slots")?
            .as_array()?
            .iter()
            .map(|item| match item {
                Value::Null => Some(String::new()),
                Value::String(text) if text.is_empty() => Some(String::new()),
                Value::String(text) => frame_id(text),
                _ => None,
            })
            .collect::<Option<Vec<String>>>()?;
        let pool = row
            .get("pool")?
            .as_array()?
            .iter()
            .map(|item| item.as_str().and_then(frame_id))
            .collect::<Option<Vec<String>>>()?;
        let issued = row.get("issued")?.as_u64()?;
        if issued < pool.len() as u64 {
            return None;
        }
        let known: HashSet<&str> = pool.iter().map(String::as_str).collect();
        if slots.iter().any(|slot| !slot.is_empty() && !known.contains(slot.as_str())) {
            return None;
        }
        let base_seed = row.get("base_seed")?.as_u64()?;
        let active = row.get("active")?.as_u64()? as usize;
        if slots.is_empty() {
            if active != 0 {
                return None;
            }
        } else if active >= slots.len() {
            return None;
        }
        Some(Self {
            slots,
            pool,
            issued: u32::try_from(issued).ok()?,
            base_seed: (base_seed & 0xFFFF_FFFF) as u32,
            active,
        })
    }

    fn slot_index(&self, index: Option<usize>) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        let index = index.unwrap_or(self.active);
        (index < self.slots.len()).then_some(index)
    }

    fn alloc(&mut self) -> Option<String> {
        if self.issued > FRAME_MAX {
            return None;
        }
        let id = format!("{:03X}", self.issued);
        self.issued += 1;
        self.pool.push(id.clone());
        Some(id)
    }

    fn parse(&self, frame: &str) -> Option<String> {
        let token = frame_id(frame)?;
        self.pool.contains(&token).then_some(token)
    }

    /// A new blank pool frame on the end of the sheet. None when no ids are left.
    pub fn append_blank(&mut self) -> Option<String> {
        let id = self.alloc()?;
        self.slots.push(id.clone());
        self.active = self.slots.len() - 1;
        Some(id)
    }

    /// `count` more slots on the same pool frame. No new drawing. None on a blank.
    pub fn hold(&mut self, index: Option<usize>, count: usize) -> Option<String> {
        if count < 1 {
            return None;
        }
        let index = self.slot_index(index)?;
        let id = self.slots[index].clone();
        if id.is_empty() {
            return None;
        }
        for step in 1..=count {
            self.slots.insert(index + step, id.clone());
        }
        self.active = index + count;
        Some(id)
    }

    /// A new pool frame inserted after this slot. Not a second pointer.
    pub fn duplicate(&mut self, index: Option<usize>) -> Option<String> {
        let index = self.slot_index(index)?;
        if self.slots[index].is_empty() {
            return None;
        }
        let id = self.alloc()?;
        self.slots.insert(index + 1, id.clone());
        self.active = index + 1;
        Some(id)
    }

    /// Point a slot at a pool frame that already exists.
    pub fn reuse(&mut self, frame: &str, index: Option<usize>) -> Option<String> {
        let id = self.parse(frame)?;
        let index = self.slot_index(index)?;
        self.slots[index] = id.clone();
        self.active = index;
        Some(id)
    }

    /// Drop a slot. The pool keeps the id. The last slot on the sheet stays.
    pub fn delete(&mut self, index: Option<usize>) -> Option<String> {
        if self.slots.len() <= 1 {
            return None;
        }
        let index = self.slot_index(index)?;
        let id = self.slots.remove(index);
        self.active = index.saturating_sub(1);
        Some(id)
    }

    /// Move one slot. The pool order does not follow.
    pub fn move_slot(&mut self, src: usize, dst: usize) -> Option<String> {
        let src = self.slot_index(Some(src))?;
        if dst >= self.slots.len() {
            return None;
        }
        if src == dst {
            self.active = dst;
            return Some(self.slots[dst].clone());
        }
        let id = self.slots.remove(src);
        self.slots.insert(dst, id.clone());
        self.active = dst;
        Some(id)
    }

    /// Seed for this exposure. Same pool frame, same seed. None on a blank.
    pub fn boil_seed(&self, index: usize) -> Option<u32> {
        let id = self.slots.get(index)?;
        if id.is_empty() {
            return None;
        }
        let number = u32::from_str_radix(id, 16).ok()?;
        Some(self.base_seed.wrapping_add(number))
    }

    fn shown(&self, index: i64) -> Option<String> {
        if index < 0 || index >= self.slots.len() as i64 {
            return None;
        }
        let cell = &self.slots[index as usize];
        (!cell.is_empty()).then(|| cell.clone())
    }

    /// Previous and next pool ids around this slot. A blank neighbour is None.
    pub fn onion(&self, index: Option<usize>) -> Option<SheetOnion> {
        let index = self.slot_index(index)? as i64;
        Some(SheetOnion { prev: self.shown(index - 1), next: self.shown(index + 1) })
    }

    /// "hold" continues the previous id. "reuse" points back at an earlier one.
    /// A blank is neither. The same id with a blank between them is a reuse.
    pub fn slot_state(&self, index: usize) -> &'static str {
        let Some(id) = self.slots.get(index) else {
            return "";
        };
        if id.is_empty() {
            return "";
        }
        if index > 0 && self.slots[index - 1] == *id {
            return "hold";
        }
        if self.slots[..index].iter().any(|earlier| earlier == id) {
            return "reuse";
        }
        ""
    }

    fn span(&self, start: usize, count: usize) -> Option<(usize, usize)> {
        (count >= 1 && start.checked_add(count)? <= self.slots.len()).then_some((start, count))
    }

    /// OpenToonz insert: empty cells at this row, and the rest shift down.
    pub fn insert_empty(&mut self, index: usize, count: usize) -> bool {
        if count < 1 || index > self.slots.len() {
            return false;
        }
        self.slots.splice(index..index, std::iter::repeat_n(String::new(), count));
        self.active = index;
        true
    }

    /// Empty the cells and leave the following rows where they are.
    pub fn clear_cells(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        for cell in &mut self.slots[start..start + count] {
            cell.clear();
        }
        self.active = start;
        true
    }

    /// Cut with shift. Following cells move up. The column may end empty.
    pub fn remove_cells(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        self.slots.drain(start..start + count);
        self.active = if self.slots.is_empty() { 0 } else { start.min(self.slots.len() - 1) };
        true
    }

    /// Swap the selection end for end, blanks included.
    pub fn reverse(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        if count >= 2 {
            self.slots[start..start + count].reverse();
            self.active = start;
        }
        true
    }

    /// Append the selection reversed, without repeating its last cell.
    pub fn swing(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        if count < 2 {
            return true;
        }
        let tail: Vec<String> = self.slots[start..start + count - 1].iter().rev().cloned().collect();
        self.slots.splice(start + count..start + count, tail);
        self.active = start;
        true
    }

    /// Repeat every cell n times, blanks included. Cells after shift down.
    pub fn step(&mut self, start: usize, count: usize, n: i64) -> bool {
        if n < 1 {
            return false;
        }
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        if n == 1 {
            return true;
        }
        let expanded: Vec<String> = self.slots[start..start + count]
            .iter()
            .flat_map(|cell| std::iter::repeat_n(cell.clone(), n as usize))
            .collect();
        self.slots.splice(start..start + count, expanded);
        self.active = start;
        true
    }

    /// Keep cell 0, n, 2n, and delete the others. Shorter than n is a no-op.
    pub fn each(&mut self, start: usize, count: usize, n: i64) -> bool {
        if n < 1 {
            return false;
        }
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        let n = n as usize;
        if count < n {
            return false;
        }
        let kept: Vec<String> = self.slots[start..start + count].iter().step_by(n).cloned().collect();
        self.slots.splice(start..start + count, kept);
        self.active = start;
        true
    }

    /// Collapse each run of the same cell, blanks included, to one cell.
    pub fn reset_step(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        let cells = &self.slots[start..start + count];
        let mut kept: Vec<String> = Vec::new();
        let mut index = 0;
        while index < count {
            kept.push(cells[index].clone());
            index += 1;
            while index < count && cells[index] == *kept.last().unwrap() {
                index += 1;
            }
        }
        self.slots.splice(start..start + count, kept);
        self.active = start;
        true
    }

    /// Each non-blank run grows by one. A blank stays.
    pub fn increase_step(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        let mut cells: Vec<String> = self.slots[start..start + count].to_vec();
        let mut index = 0usize;
        let mut end = count;
        while index < end {
            let cell = cells[index].clone();
            if !cell.is_empty() {
                cells.insert(index, cell.clone());
                end += 1;
                index += 1;
                while index < end && cells[index] == cell {
                    index += 1;
                }
            } else {
                index += 1;
            }
        }
        self.slots.splice(start..start + count, cells);
        self.active = start;
        true
    }

    /// Each non-blank run shrinks by one. A run of one stays.
    pub fn decrease_step(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        let mut cells: Vec<String> = self.slots[start..start + count].to_vec();
        let mut index = 0usize;
        let mut end = count;
        while index < end {
            let cell = cells[index].clone();
            if !cell.is_empty() {
                index += 1;
                let mut removed = false;
                while index < end && cells[index] == cell {
                    if !removed {
                        removed = true;
                        cells.remove(index);
                        end -= 1;
                    } else {
                        index += 1;
                    }
                }
            } else {
                index += 1;
            }
        }
        self.slots.splice(start..start + count, cells);
        self.active = start;
        true
    }

    /// Expose each distinct run for `step` frames. A trailing hold of the last
    /// selected cell is part of the run. Blank runs are kept.
    pub fn reframe(&mut self, start: usize, count: usize, step: i64) -> bool {
        self.reframe_with_blank(start, count, step, -1)
    }

    /// `reframe` with OpenToonz's blank option: below 0 keeps a blank run, zero
    /// and above drop blanks, and a positive value inserts that many blank
    /// steps after each drawing.
    pub fn reframe_with_blank(&mut self, start: usize, count: usize, step: i64, with_blank: i64) -> bool {
        if step < 1 {
            return false;
        }
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        let step = step as usize;
        let mut end = start + count - 1;
        let last = self.slots[end].clone();
        while end + 1 < self.slots.len() && self.slots[end + 1] == last {
            end += 1;
        }
        let count = end - start + 1;
        let mut unique: Vec<String> = Vec::new();
        for cell in &self.slots[start..start + count] {
            if unique.last() != Some(cell) {
                unique.push(cell.clone());
            }
        }
        if with_blank >= 0 {
            unique.retain(|cell| !cell.is_empty());
        }
        if unique.is_empty() {
            return false;
        }
        let gap = if with_blank > 0 { with_blank as usize * step } else { 0 };
        let mut out: Vec<String> = Vec::new();
        for cell in unique {
            out.extend(std::iter::repeat_n(cell, step));
            out.extend(std::iter::repeat_n(String::new(), gap));
        }
        let made = !out.is_empty();
        self.slots.splice(start..start + count, out);
        self.active = start;
        made
    }

    /// Fill or trim so each drawing's exposure matches the number gap.
    /// Drawing 2 then 5 becomes 2, 2, 2, 5. A decreasing number does nothing
    /// and returns false. A blank stops the fill. The cell just after a
    /// trailing hold counts.
    pub fn autoexpose(&mut self, start: usize, count: usize) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        let len = |slots: &Vec<String>| slots.len() as i64;
        let blank = |slots: &Vec<String>, at: i64| slots[at as usize].is_empty();
        let number = |slots: &Vec<String>, at: i64| drawing_number(&slots[at as usize]);
        let start = start as i64;
        let mut r1 = start + count as i64 - 1;
        let mut index = start;
        let limit = r1 - 1;
        while index <= limit && blank(&self.slots, index) {
            index += 1;
        }
        while index <= limit {
            if index + 1 >= len(&self.slots) || blank(&self.slots, index + 1) {
                break;
            }
            if number(&self.slots, index + 1) < number(&self.slots, index) {
                return false;
            }
            index += 1;
        }
        index = start;
        while index < r1 && blank(&self.slots, index) {
            index += 1;
        }
        while index < r1 {
            let mut run = 1i64;
            if index + 1 >= len(&self.slots) || blank(&self.slots, index + 1) {
                index += 1;
                continue;
            }
            let frame1 = number(&self.slots, index);
            if frame1 < 0 {
                break;
            }
            while index + 1 < len(&self.slots)
                && !blank(&self.slots, index + 1)
                && number(&self.slots, index + 1) == number(&self.slots, index)
            {
                index += 1;
                run += 1;
            }
            if index + 1 >= len(&self.slots) || blank(&self.slots, index + 1) {
                break;
            }
            let frame2 = number(&self.slots, index + 1);
            if frame2 < 0 {
                break;
            }
            if frame1 + run == frame2 {
                index += 1;
                continue;
            }
            if frame1 + run < frame2 {
                let extra = frame2 - frame1 - run;
                let cell = self.slots[index as usize].clone();
                let at = (index + 1) as usize;
                self.slots.splice(at..at, std::iter::repeat_n(cell, extra as usize));
                index += extra;
                r1 += extra;
                index += 1;
            } else {
                let extra = run - frame2 + frame1;
                index -= extra;
                let at = (index + 1) as usize;
                self.slots.drain(at..at + extra as usize);
                r1 -= extra;
                index += 1;
            }
        }
        self.active = if self.slots.is_empty() { 0 } else { start as usize };
        true
    }

    /// Append `times` more cycles of the selection. One cycle is the first repeat.
    pub fn repeat(&mut self, start: usize, count: usize, times: i64) -> bool {
        let Some((start, count)) = self.span(start, count) else {
            return false;
        };
        if times < 1 {
            return false;
        }
        let chunk: Vec<String> = self.slots[start..start + count].to_vec();
        let cycles: Vec<String> = (0..times).flat_map(|_| chunk.iter().cloned()).collect();
        self.slots.splice(start + count..start + count, cycles);
        self.active = start;
        true
    }
}

/// Hold, reuse, or nothing for one filmstrip cel. No sheet is nothing.
pub fn cel_mark(sheet: Option<&ExposureSheet>, index: usize) -> &'static str {
    sheet.map(|sheet| sheet.slot_state(index)).unwrap_or_default()
}

/// Timeline keys as an X-sheet column. Nothing is saved. Rows before the
/// first key are empty. A cel holds until the next key. Distinct cels become
/// 000, 001, and so on, in the order they first appear.
pub fn project_slots(pairs: &[(u32, &Node)]) -> Vec<String> {
    let mut by_frame: HashMap<u32, &str> = HashMap::new();
    for (frame, cel) in pairs {
        by_frame.insert(*frame, cel.id.as_str());
    }
    let Some(last) = by_frame.keys().max().copied() else {
        return Vec::new();
    };
    let mut ids: HashMap<&str, String> = HashMap::new();
    let mut slots = Vec::new();
    let mut current = "";
    let mut started = false;
    for frame in 0..=last {
        if let Some(id) = by_frame.get(&frame) {
            current = id;
            started = true;
        }
        if !started || current.is_empty() {
            slots.push(String::new());
            continue;
        }
        let next = format!("{:03X}", ids.len());
        slots.push(ids.entry(current).or_insert(next).clone());
    }
    slots
}

/// A sheet whose slots are these cells. The pool is the ids already named,
/// in order of first appearance. None when a cell is not a drawing id.
pub fn sheet_from_slots(slots: &[String], base_seed: u32) -> Option<ExposureSheet> {
    let mut sheet = ExposureSheet::new(base_seed);
    for slot in slots {
        if slot.is_empty() {
            sheet.slots.push(String::new());
            continue;
        }
        let id = frame_id(slot)?;
        if !sheet.pool.contains(&id) {
            sheet.pool.push(id.clone());
        }
        sheet.slots.push(id);
    }
    sheet.issued = sheet.pool.len() as u32;
    sheet.active = 0;
    Some(sheet)
}

/// One cell command by its X-sheet menu name (`xsheet.run_command`). False
/// means the sheet was left as it was. `arg` is the step, each, reframe, or
/// repeat count; the other commands ignore it.
pub fn run_command(sheet: &mut ExposureSheet, name: &str, start: usize, count: usize, arg: i64) -> bool {
    match name {
        "reverse" => sheet.reverse(start, count),
        "swing" => sheet.swing(start, count),
        "step" => sheet.step(start, count, arg),
        "each" => sheet.each(start, count, arg),
        "reset" => sheet.reset_step(start, count),
        "increase" => sheet.increase_step(start, count),
        "decrease" => sheet.decrease_step(start, count),
        "autoexpose" => sheet.autoexpose(start, count),
        "reframe" => sheet.reframe(start, count, arg),
        "repeat" => sheet.repeat(start, count, arg),
        "insert" => sheet.insert_empty(start, count),
        "clear" => sheet.clear_cells(start, count),
        "cut" => sheet.remove_cells(start, count),
        _ => false,
    }
}

/// The X-sheet: one column per animation folder, back to front, `cut_end`
/// rows of cels and keys, plus the labels Python prints.
pub fn xsheet_columns(nodes: &[Node], cut_end: u32) -> Vec<XColumn> {
    animation_folders(nodes)
        .into_iter()
        .map(|folder| {
            let pairs = frame_map(folder);
            let starts: HashSet<u32> = pairs.iter().map(|(frame, _)| *frame).collect();
            let cells = (0..cut_end).map(|frame| cel_at(folder, frame).map(|cel| cel.id.clone())).collect();
            let keys = (0..cut_end).map(|frame| starts.contains(&frame)).collect();
            let (labels, stored) = match &folder.exposure {
                Some(sheet) => (sheet.slots.clone(), true),
                None => (project_slots(&pairs), false),
            };
            XColumn { folder_id: folder.id.clone(), name: folder.name.clone(), cells, keys, labels, stored }
        })
        .collect()
}

/// Run one cell command on the `column`th animation folder (Python
/// `_xsheet_command`). A folder without a sheet gets one made from its keys
/// first, and keeps it even when the command refuses.
pub fn xsheet_command(nodes: &mut [Node], column: usize, name: &str, start: usize, count: usize, arg: i64) -> bool {
    let folders = animation_folder_ids(nodes);
    let Some(folder_id) = folders.get(column) else {
        return false;
    };
    sheet_command(nodes, folder_id, name, start, count, arg)
}

/// `xsheet_command` for the animation folder with `folder_id`. The desk
/// addresses a column by the folder it stands for.
pub fn folder_command(art: &mut Art, folder_id: &str, name: &str, start: usize, count: usize, arg: i64) -> bool {
    sheet_command(&mut art.layers, folder_id, name, start, count, arg)
}

fn sheet_command(nodes: &mut [Node], folder_id: &str, name: &str, start: usize, count: usize, arg: i64) -> bool {
    let projected = match find(nodes, folder_id) {
        Some(folder) if folder.animation && folder.exposure.is_none() => {
            sheet_from_slots(&project_slots(&frame_map(folder)), 0)
        }
        Some(folder) if folder.animation => None,
        _ => return false,
    };
    let Some(folder) = find_mut(nodes, folder_id) else {
        return false;
    };
    if let Some(sheet) = projected {
        folder.exposure = Some(sheet);
    }
    match folder.exposure.as_mut() {
        Some(sheet) => run_command(sheet, name, start, count, arg),
        None => false,
    }
}

/// Every node, depth first, the way Python's `walk` yields them.
pub fn all_nodes(nodes: &[Node]) -> Vec<&Node> {
    let mut out = Vec::new();
    walk(nodes, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::ClipTime;

    fn layer(id: &str, name: &str) -> Node {
        Node::layer(id, name, 4, 4)
    }

    fn keyed_folder() -> Node {
        let mut folder = Node::group("shot", "Shot");
        folder.animation = true;
        folder.children = vec![layer("a", "1"), layer("b", "2"), layer("c", "3")];
        folder.timeline = vec!["a".into(), "b".into(), "c".into()];
        folder.frames = vec![0, 2, 5];
        folder
    }

    fn names(cels: &[&Node]) -> Vec<String> {
        cels.iter().map(|cel| cel.name.clone()).collect()
    }

    fn ids(numbers: &[i64]) -> Vec<String> {
        numbers.iter().map(|number| if *number < 0 { String::new() } else { format!("{number:03X}") }).collect()
    }

    fn sheet(numbers: &[i64]) -> ExposureSheet {
        sheet_from_slots(&ids(numbers), 0).unwrap()
    }

    #[test]
    fn frame_map_and_cel_at_hold_each_key_until_the_next_and_the_last_to_the_cut() {
        let folder = keyed_folder();
        let map: Vec<(u32, &str)> = frame_map(&folder).into_iter().map(|(frame, cel)| (frame, cel.id.as_str())).collect();
        assert_eq!(map, vec![(0, "a"), (2, "b"), (5, "c")]);
        assert_eq!(cel_at(&folder, 0).unwrap().id, "a");
        assert_eq!(cel_at(&folder, 1).unwrap().id, "a");
        assert_eq!(cel_at(&folder, 2).unwrap().id, "b");
        assert_eq!(cel_at(&folder, 4).unwrap().id, "b");
        assert_eq!(cel_at(&folder, 5).unwrap().id, "c");
        assert_eq!(cel_at(&folder, 100).unwrap().id, "c");
        assert_eq!(key_of(&folder, "b"), Some(2));
        assert_eq!(key_of(&folder, "zz"), None);
        let nodes = vec![folder];
        assert_eq!(cut_end(&nodes), 6);
        let blocks: Vec<(u32, u32, usize)> = cels(&nodes[0], 10).iter().map(|cel| (cel.start, cel.end, cel.index)).collect();
        assert_eq!(blocks, vec![(0, 2, 0), (2, 5, 1), (5, 10, 2)]);
        let short: Vec<u32> = cels(&nodes[0], 3).iter().map(|cel| cel.end).collect();
        assert_eq!(short, vec![2, 5, 6]);
        assert_eq!(cut_end(&[]), 1);
    }

    #[test]
    fn before_the_first_key_the_folder_shows_nothing_and_spare_cels_stay_off_the_frames() {
        let mut folder = Node::group("f2", "Folder 2");
        folder.animation = true;
        for name in 1..=9 {
            folder.children.push(layer(&format!("c{name}"), &name.to_string()));
        }
        folder.timeline = (1..=8).map(|name| format!("c{name}")).collect();
        folder.frames = (1..=8).collect();
        assert_eq!(names(&timeline_cels(&folder)), ["1", "2", "3", "4", "5", "6", "7", "8"]);
        assert_eq!(names(&unspecified_cels(&folder)), ["9"]);
        assert_eq!(cel_at(&folder, 5).unwrap().name, "5");
        assert_eq!(cel_at(&folder, 100).unwrap().name, "8");
        assert!(cel_at(&folder, 0).is_none());
        let shown: Vec<Option<String>> = (0..3).map(|frame| cel_at(&folder, frame).map(|cel| cel.name.clone())).collect();
        assert_eq!(shown, vec![None, Some("1".into()), Some("2".into())]);
    }

    #[test]
    fn a_hand_marked_folder_reads_the_panel_order_front_first() {
        let mut folder = Node::group("beetle", "beetle");
        folder.animation = true;
        folder.children = vec![layer("line", "line"), layer("color", "color")];
        assert_eq!(names(&timeline_cels(&folder)), ["color", "line"]);
        assert!(unspecified_cels(&folder).is_empty());
        let map: Vec<(u32, &str)> = frame_map(&folder).into_iter().map(|(frame, cel)| (frame, cel.name.as_str())).collect();
        assert_eq!(map, vec![(0, "color"), (1, "line")]);
        assert_eq!(cel_at(&folder, 0).unwrap().name, "color");
        assert_eq!(cel_at(&folder, 7).unwrap().name, "line");
        folder.timeline = vec!["line".into()];
        assert_eq!(names(&timeline_cels(&folder)), ["line", "color"]);
        folder.timeline = vec!["line".into(), "ghost".into()];
        folder.frames = vec![3, 4];
        assert_eq!(names(&timeline_cels(&folder)), ["line"]);
        let map: Vec<u32> = frame_map(&folder).into_iter().map(|(frame, _)| frame).collect();
        assert_eq!(map, vec![0], "a frame list that does not fit falls back to 0, 1, 2…");
    }

    #[test]
    fn set_animation_marks_the_outer_folder_and_a_nested_folder_stays_a_frame() {
        let mut line = Node::group("line", "line");
        line.children = vec![layer("ink", "ink")];
        let mut color = Node::group("color", "color");
        color.children = vec![layer("fill", "fill")];
        let mut beetle = Node::group("beetle", "beetle");
        beetle.children = vec![line, color];
        let mut nodes = vec![layer("sky", "sky"), beetle];
        assert!(set_animation(&mut nodes, "line", true));
        assert!(set_animation(&mut nodes, "beetle", true));
        let beetle = find(&nodes, "beetle").unwrap();
        assert!(beetle.animation);
        assert_eq!(beetle.timeline, vec!["color".to_string(), "line".to_string()]);
        assert!(beetle.frames.is_empty());
        assert!(!find(&nodes, "line").unwrap().animation, "marking the outer folder unmarks the frame inside");
        assert!(!set_animation(&mut nodes, "line", true));
        assert!(!find(&nodes, "line").unwrap().animation);
        assert_eq!(owning_animation(&nodes, "line").as_deref(), Some("beetle"));
        assert_eq!(owning_animation(&nodes, "fill").as_deref(), Some("beetle"));
        assert_eq!(owning_animation(&nodes, "beetle").as_deref(), Some("beetle"));
        assert_eq!(owning_animation(&nodes, "sky"), None);
        assert!(!set_animation(&mut nodes, "sky", true), "a layer is not a timeline");
        assert_eq!(animation_folder_ids(&nodes), vec!["beetle".to_string()]);
        assert_eq!(cel_of(find(&nodes, "beetle").unwrap(), "fill").unwrap().id, "color");
        assert!(set_animation(&mut nodes, "beetle", false));
        let beetle = find(&nodes, "beetle").unwrap();
        assert!(!beetle.animation && beetle.timeline.is_empty());
        assert!(animation_folders(&nodes).is_empty());
    }

    #[test]
    fn move_frame_swaps_an_occupied_frame_and_reorders_a_hand_marked_folder() {
        let mut folder = keyed_folder();
        assert!(move_frame(&mut folder, "c", 2));
        assert_eq!(folder.frames, vec![0, 5, 2]);
        assert_eq!(folder.timeline, vec!["a", "b", "c"]);
        assert_eq!(cel_at(&folder, 2).unwrap().id, "c");
        assert!(!move_frame(&mut folder, "a", 0), "same frame is no move");
        assert!(!move_frame(&mut folder, "zz", 1));
        assert!(move_frame(&mut folder, "a", 9));
        assert_eq!(folder.frames, vec![9, 5, 2]);
        assert!(cel_at(&folder, 0).is_none());
        let stack: Vec<&str> = folder.children.iter().map(|cel| cel.id.as_str()).collect();
        assert_eq!(stack, ["a", "b", "c"], "the layer stack stays put");

        let mut hand = keyed_folder();
        hand.frames.clear();
        assert!(move_frame(&mut hand, "c", 0));
        assert_eq!(hand.timeline, vec!["c", "a", "b"]);
        assert!(!move_frame(&mut hand, "c", 0));
        assert!(!move_frame(&mut hand, "c", 3));
        assert!(hand.frames.is_empty());
    }

    #[test]
    fn move_cel_swaps_places_and_takes_the_frames_along() {
        let mut folder = keyed_folder();
        assert!(move_cel(&mut folder, "a", 1));
        assert_eq!(folder.timeline, vec!["b", "a", "c"]);
        assert_eq!(folder.frames, vec![2, 0, 5]);
        assert_eq!(cel_at(&folder, 0).unwrap().id, "a");
        assert!(!move_cel(&mut folder, "b", -1));
        assert!(!move_cel(&mut folder, "c", 1));
        let mut hand = Node::group("beetle", "beetle");
        hand.animation = true;
        hand.children = vec![layer("line", "line"), layer("color", "color")];
        hand.timeline = vec!["color".into(), "line".into()];
        assert!(move_cel(&mut hand, "color", 1));
        assert_eq!(names(&timeline_cels(&hand)), ["line", "color"]);
        assert_eq!(names(&hand.children.iter().collect::<Vec<_>>()), ["line", "color"]);
    }

    #[test]
    fn a_new_frame_keeps_the_current_drawing_in_the_past() {
        assert_eq!(next_frame_slot(&[0], 0), (1, vec![0]));
        assert_eq!(next_frame_slot(&[0, 1], 0), (1, vec![0, 2]));
        assert_eq!(next_frame_slot(&[0, 8], 0), (1, vec![0, 8]));
        assert_eq!(next_frame_slot(&[0], 3), (3, vec![0]));
        assert_eq!(next_frame_slot(&[], 4), (4, vec![]));
    }

    #[test]
    fn a_hold_is_the_current_cel_for_onion() {
        let mut folder = Node::group("folder", "Shot");
        folder.animation = true;
        folder.children = vec![layer("a", "1"), layer("b", "2")];
        folder.timeline = vec!["a".into(), "b".into()];
        folder.frames = vec![0, 4];
        let (past, future, current) = neighbors(&folder, 2);
        assert_eq!(current.unwrap().id, "a");
        assert!(past.is_empty());
        assert_eq!(names(&future), ["2"]);
        let (past, future, current) = neighbors(&folder, 4);
        assert_eq!(current.unwrap().id, "b");
        assert_eq!(names(&past), ["1"]);
        assert!(future.is_empty());

        let mut long = Node::group("long", "Long");
        long.animation = true;
        for index in 0..9 {
            long.children.push(layer(&format!("c{index}"), &index.to_string()));
        }
        long.timeline = (0..9).map(|index| format!("c{index}")).collect();
        long.frames = (0..9).collect();
        let (past, future, current) = neighbors(&long, 4);
        assert_eq!(current.unwrap().name, "4");
        assert_eq!(names(&past), ["3", "2", "1"], "nearest first, three at most");
        assert_eq!(names(&future), ["5", "6", "7"]);
        let (past, future, _) = onion_neighbors(&long, 4, 1, 0);
        assert_eq!(names(&past), ["3"]);
        assert!(future.is_empty());
        assert_eq!(onion_strength(0), 0.7);
        assert_eq!(onion_strength(2), 0.18);
        assert_eq!(onion_strength(9), 0.18);
    }

    #[test]
    fn add_cel_wraps_a_lone_layer_keys_the_new_cel_after_the_playhead_and_turns_onion_on() {
        let mut art = Art::new(8, 8);
        let ink = art.layers[0].id.clone();
        assert!(!art.onion);
        let first = add_cel(&mut art, &ink, 0).unwrap();
        assert_eq!(art.layers.len(), 1);
        let folder = &art.layers[0];
        assert_eq!(folder.name, "Animation");
        assert!(folder.animation);
        assert_eq!(folder.children.len(), 2);
        assert_eq!(folder.timeline, vec![ink.clone(), first.clone()]);
        assert_eq!(folder.frames, vec![0, 1]);
        let cel = find(&art.layers, &first).unwrap();
        assert_eq!(cel.name, "1");
        assert!(cel.is_layer());
        assert_eq!(cel.pixels.len(), 8 * 8 * 4);
        assert_eq!(art.playhead, 1);
        assert!(art.onion);

        art.playhead = 0;
        let between = add_cel(&mut art, &first, 0).unwrap();
        let folder = &art.layers[0];
        assert_eq!(find(&art.layers, &between).unwrap().name, "1 2", "Python's fresh_name of \"1\"");
        assert_eq!(folder.timeline, vec![ink.clone(), between.clone(), first.clone()]);
        assert_eq!(folder.frames, vec![0, 1, 2], "the later key moved out of the new slot");
        assert_eq!(art.playhead, 1);

        let plain = art.add_group(None, "Props");
        let prop = add_cel(&mut art, &plain, 3).unwrap();
        let props = find(&art.layers, &plain).unwrap();
        assert!(props.animation);
        assert_eq!(props.timeline, vec![prop]);
        assert_eq!(props.frames, vec![3]);
        assert_eq!(art.playhead, 3);
        assert_eq!(animation_folder_ids(&art.layers).len(), 2);

        let mut mask = Art::new(4, 4);
        mask.layers[0].marker = "mask".into();
        let mask_id = mask.layers[0].id.clone();
        assert!(add_cel(&mut mask, &mask_id, 0).is_none());
    }

    #[test]
    fn the_playhead_stays_inside_the_cut_and_wraps_only_when_asked() {
        let mut art = Art::blank(4, 4);
        art.layers.push(keyed_folder());
        assert_eq!(cut_frames(&art), 6);
        set_playhead(&mut art, 10);
        assert_eq!(art.playhead, 5);
        step_frame(&mut art, 1, false);
        assert_eq!(art.playhead, 5);
        step_frame(&mut art, 1, true);
        assert_eq!(art.playhead, 0);
        step_frame(&mut art, -1, false);
        assert_eq!(art.playhead, 0);
        step_frame(&mut art, -1, true);
        assert_eq!(art.playhead, 5);
        step_frame(&mut art, -2, false);
        assert_eq!(art.playhead, 3);
        first_frame(&mut art);
        assert_eq!(art.playhead, 0);
        art.clip_time = Some(ClipTime { name: "Timeline 1".into(), start: 0, end: 48 });
        assert_eq!(cut_frames(&art), 49);
        set_playhead(&mut art, 100);
        assert_eq!(art.playhead, 48);
        step_frame(&mut art, 1, true);
        assert_eq!(art.playhead, 0);
        let mut empty = Art::blank(4, 4);
        set_playhead(&mut empty, 3);
        assert_eq!(empty.playhead, 0);
    }

    #[test]
    fn a_blank_is_not_a_hold_and_a_row_round_trips() {
        let mut sheet = sheet(&[0, 0, -1, 0]);
        assert_eq!(sheet.slots, ["000", "000", "", "000"]);
        let states: Vec<&str> = (0..4).map(|index| sheet.slot_state(index)).collect();
        assert_eq!(states, ["", "hold", "", "reuse"]);
        assert_eq!(cel_mark(Some(&sheet), 2), "");
        assert_eq!(cel_mark(Some(&sheet), 1), "hold");
        assert_eq!(cel_mark(None, 1), "");
        assert_eq!(sheet.onion(Some(2)), Some(SheetOnion { prev: Some("000".into()), next: Some("000".into()) }));
        assert_eq!(sheet.onion(Some(0)).unwrap().prev, None);
        assert_eq!(sheet.boil_seed(2), None);
        assert_eq!(sheet.hold(Some(2), 1), None);
        assert_eq!(sheet.duplicate(Some(2)), None);
        assert_eq!(sheet.reuse("000", Some(2)).as_deref(), Some("000"));
        assert_eq!(sheet.slots[2], "000");
        assert_eq!(sheet.cell(2), Some("000"));
        assert_eq!(sheet.cell(9), None);
        let loaded = ExposureSheet::from_row(&super::sheet_from_slots(&ids(&[1, -1, 2]), 0).unwrap().to_row()).unwrap();
        assert_eq!(loaded.slots, ["001", "", "002"]);
        assert_eq!(loaded.pool, ["001", "002"]);
        assert_eq!(loaded.cell(1), None);
        assert_eq!(loaded.slot(1), Some(""));
    }

    #[test]
    fn holds_duplicates_reuse_delete_and_move_follow_the_pool_rules() {
        let mut sheet = ExposureSheet::new(0);
        assert_eq!(sheet.append_blank().as_deref(), Some("000"));
        assert_eq!(sheet.hold(None, 2).as_deref(), Some("000"));
        assert_eq!(sheet.slots, ["000", "000", "000"]);
        assert_eq!(sheet.pool, ["000"]);
        let seeds: HashSet<u32> = (0..3).map(|index| sheet.boil_seed(index).unwrap()).collect();
        assert_eq!(seeds.len(), 1);

        let mut sheet = ExposureSheet::new(0);
        let first = sheet.append_blank().unwrap();
        let copied = sheet.duplicate(Some(0)).unwrap();
        assert_eq!(sheet.slots, vec![first.clone(), copied.clone()]);
        assert_eq!(copied, "001");
        assert_ne!(sheet.boil_seed(0), sheet.boil_seed(1));
        assert_eq!(sheet.append_blank().as_deref(), Some("002"));
        assert_eq!(sheet.reuse("000", Some(2)).as_deref(), Some("000"));
        assert_eq!(sheet.slots, ["000", "001", "000"]);
        assert_eq!(sheet.pool, ["000", "001", "002"]);
        assert_eq!(sheet.slot_state(2), "reuse");
        sheet.reuse(&first, Some(1));
        assert_eq!(sheet.slots, ["000", "000", "000"]);
        assert_eq!(sheet.slot_state(1), "hold");
        assert_eq!(sheet.reuse("12", Some(0)), None);
        assert_eq!(sheet.reuse("FFF", Some(0)), None);
        assert_eq!(sheet.reuse("00a", Some(0)), None, "001 is in the pool, 00A is not");

        let mut held = ExposureSheet::new(0);
        held.append_blank();
        held.hold(None, 2);
        let other = held.append_blank().unwrap();
        assert_eq!(held.delete(Some(1)).as_deref(), Some("000"));
        assert_eq!(held.slots, vec!["000".to_string(), "000".to_string(), other.clone()]);
        assert_eq!(held.delete(Some(2)), Some(other.clone()));
        assert!(!held.slots.contains(&other));
        assert!(held.pool.contains(&other));
        assert_eq!(held.append_blank().as_deref(), Some("002"));
        assert_eq!(held.reuse(&other, Some(0)), Some(other.clone()));
        let mut only = ExposureSheet::new(0);
        only.append_blank();
        assert_eq!(only.delete(Some(0)), None);
        assert_eq!(only.slots, ["000"]);
        let mut full = ExposureSheet::new(0);
        full.issued = FRAME_MAX;
        assert_eq!(full.append_blank().as_deref(), Some("FFF"));
        assert_eq!(full.append_blank(), None);

        let mut moved = ExposureSheet::new(0);
        let all: Vec<String> = (0..3).map(|_| moved.append_blank().unwrap()).collect();
        let pool = moved.pool.clone();
        let seed = moved.boil_seed(0);
        assert_eq!(moved.move_slot(0, 2), Some(all[0].clone()));
        assert_eq!(moved.slots, vec![all[1].clone(), all[2].clone(), all[0].clone()]);
        assert_eq!(moved.pool, pool);
        assert_eq!(moved.boil_seed(2), seed);
        assert_eq!(moved.move_slot(2, 0), Some(all[0].clone()));
        assert_eq!(moved.slots, all);
        assert_eq!(moved.move_slot(1, 1), Some(all[1].clone()));
        assert_eq!(moved.move_slot(0, 3), None);

        let mut seeded = ExposureSheet::new(4);
        seeded.append_blank();
        seeded.hold(None, 2);
        seeded.append_blank();
        seeded.append_blank();
        let first = seeded.slots[0].clone();
        seeded.reuse(&first, Some(4));
        assert_eq!(seeded.slots, ["000", "000", "000", "001", "000"]);
        let seeds: Vec<u32> = (0..seeded.len()).map(|index| seeded.boil_seed(index).unwrap()).collect();
        assert_eq!(seeds, [4, 4, 4, 5, 4]);
        assert_eq!(seeded.onion(Some(3)), Some(SheetOnion { prev: Some("000".into()), next: Some("000".into()) }));
        assert_eq!(seeded.onion(Some(4)).unwrap().next, None);
        seeded.active = 4;
        assert_eq!(seeded.onion(None), seeded.onion(Some(4)));
    }

    #[test]
    fn reverse_swing_step_and_each_match_opentoonz() {
        let mut sheet = sheet(&[1, 2, 3, 4]);
        let tail = "00A".to_string();
        sheet.slots.push(tail.clone());
        assert!(sheet.reverse(0, 4));
        assert_eq!(sheet.slots, ["004", "003", "002", "001", "00A"]);
        sheet.slots = ids(&[1, 2, 3, 4]);
        sheet.slots.push(tail.clone());
        assert!(sheet.swing(0, 4));
        assert_eq!(sheet.slots, ["001", "002", "003", "004", "003", "002", "001", "00A"]);
        sheet.slots = vec!["001".into(), "".into(), "002".into(), tail.clone()];
        assert!(sheet.step(0, 3, 2));
        assert_eq!(sheet.slots, ["001", "001", "", "", "002", "002", "00A"]);
        sheet.slots = ids(&[1, 2, 3, 4, 5]);
        sheet.slots.push(tail);
        assert!(sheet.each(0, 5, 2));
        assert_eq!(sheet.slots, ["001", "003", "005", "00A"]);
        let mut short = self::sheet(&[1, 2]);
        assert!(!short.each(0, 2, 4));
        assert_eq!(short.slots, ["001", "002"]);
        assert!(!short.step(0, 2, 0));
        assert!(!short.reverse(1, 5), "a span off the sheet is refused");
        assert!(short.reverse(0, 1), "one cell reverses to itself");
        assert!(short.step(0, 2, 1));
        assert_eq!(short.slots, ["001", "002"]);
    }

    #[test]
    fn step_commands_and_reframe_keep_the_pool() {
        let mut sheet = sheet(&[1, 1, 1, 2, -1, 3, 3]);
        let pool = sheet.pool.clone();
        assert!(sheet.reset_step(0, sheet.len()));
        assert_eq!(sheet.slots, ["001", "002", "", "003"]);
        assert_eq!(sheet.pool, pool);
        let mut sheet = self::sheet(&[1, 1, 2, -1, 3]);
        assert!(sheet.increase_step(0, sheet.len()));
        assert_eq!(sheet.slots, ["001", "001", "001", "002", "002", "", "003", "003"]);
        let mut sheet = self::sheet(&[1, 1, 1, 2, -1, 3, 3]);
        assert!(sheet.decrease_step(0, sheet.len()));
        assert_eq!(sheet.slots, ["001", "001", "002", "", "003"]);
        let mut single = self::sheet(&[4]);
        assert!(single.decrease_step(0, 1));
        assert_eq!(single.slots, ["004"]);
        let mut held = self::sheet(&[1, 1, 1, 2]);
        assert!(held.reframe(0, 2, 2));
        assert_eq!(held.slots, ["001", "001", "002"]);
        let mut spread = self::sheet(&[1, 2, 3]);
        assert!(spread.reframe(0, 3, 2));
        assert_eq!(spread.slots, ["001", "001", "002", "002", "003", "003"]);
        assert!(!spread.reframe(0, 3, 0));
        let mut blanks = self::sheet(&[1, -1, -1, 2]);
        assert!(blanks.reframe(0, 4, 1));
        assert_eq!(blanks.slots, ["001", "", "002"], "a blank run is one run");
        let mut dropped = self::sheet(&[1, -1, 2]);
        assert!(dropped.reframe_with_blank(0, 3, 1, 1));
        assert_eq!(dropped.slots, ["001", "", "002", ""]);
        let mut only_blank = self::sheet(&[-1, -1]);
        assert!(!only_blank.reframe_with_blank(0, 2, 1, 0));
    }

    #[test]
    fn autoexpose_fills_number_gaps_and_refuses_a_decrease() {
        let mut gap = sheet(&[2, 5]);
        assert!(gap.autoexpose(0, 2));
        assert_eq!(gap.slots, ["002", "002", "002", "005"]);
        let mut ladder = sheet(&[1, 3, 5, 7]);
        assert!(ladder.autoexpose(0, 4));
        assert_eq!(ladder.slots, ["001", "001", "003", "003", "005", "005", "007"]);
        let mut packed = sheet(&[1, 2, 3]);
        assert!(packed.autoexpose(0, 3));
        assert_eq!(packed.slots, ["001", "002", "003"]);
        let mut fat = sheet(&[1, 1, 1, 2]);
        assert!(fat.autoexpose(0, 4));
        assert_eq!(fat.slots, ["001", "002"]);
        let mut down = sheet(&[5, 2, 9]);
        assert!(!down.autoexpose(0, 2));
        assert_eq!(down.slots, ["005", "002", "009"]);
        let mut blank = sheet(&[1, -1, 3]);
        assert!(blank.autoexpose(0, 3));
        assert_eq!(blank.slots, ["001", "", "003"]);
        let mut outside = sheet(&[1, 1, 5]);
        assert!(outside.autoexpose(0, 2));
        assert_eq!(outside.slots, ["001", "001", "001", "001", "005"]);
        let mut one = sheet(&[7]);
        assert!(one.autoexpose(0, 1));
        assert_eq!(one.slots, ["007"]);
    }

    #[test]
    fn repeat_insert_clear_and_cut_shift_the_tail() {
        let mut sheet = sheet(&[1, 2, 3]);
        assert!(sheet.repeat(0, 2, 1));
        assert_eq!(sheet.slots, ["001", "002", "001", "002", "003"]);
        assert!(!sheet.repeat(0, 1, 0));
        assert!(sheet.insert_empty(1, 2));
        assert_eq!(sheet.slots, ["001", "", "", "002", "001", "002", "003"]);
        assert!(sheet.clear_cells(0, 1));
        assert_eq!(sheet.slots[0], "");
        assert_eq!(&sheet.slots[1..3], ["", ""]);
        assert!(sheet.remove_cells(1, 2));
        assert_eq!(sheet.slots, ["", "002", "001", "002", "003"]);
        assert_eq!(sheet.active, 1);
        assert!(!sheet.insert_empty(9, 1));
        assert!(sheet.insert_empty(sheet.len(), 1), "insert at the end is allowed");
        assert!(!sheet.clear_cells(0, 0));
        assert!(sheet.remove_cells(0, sheet.len()));
        assert!(sheet.is_empty());
        assert_eq!(sheet.active, 0);
        assert!(!sheet.remove_cells(0, 1));
    }

    #[test]
    fn run_command_names_the_menu_and_refuses_the_rest() {
        let mut sheet = sheet(&[1, 2]);
        assert!(run_command(&mut sheet, "step", 0, 2, 2));
        assert_eq!(sheet.slots, ["001", "001", "002", "002"]);
        assert!(run_command(&mut sheet, "reset", 0, 4, 0));
        assert_eq!(sheet.slots, ["001", "002"]);
        assert!(run_command(&mut sheet, "increase", 0, 2, 0));
        assert!(run_command(&mut sheet, "decrease", 0, 4, 0));
        assert_eq!(sheet.slots, ["001", "002"]);
        assert!(run_command(&mut sheet, "swing", 0, 2, 0));
        assert_eq!(sheet.slots, ["001", "002", "001"]);
        assert!(run_command(&mut sheet, "reverse", 0, 2, 0));
        assert_eq!(sheet.slots, ["002", "001", "001"]);
        assert!(run_command(&mut sheet, "each", 0, 3, 2));
        assert_eq!(sheet.slots, ["002", "001"]);
        assert!(run_command(&mut sheet, "repeat", 0, 2, 1));
        assert!(run_command(&mut sheet, "reframe", 0, 4, 2));
        assert_eq!(sheet.slots, ["002", "002", "001", "001", "002", "002", "001", "001"]);
        assert!(run_command(&mut sheet, "insert", 0, 1, 0));
        assert_eq!(sheet.slots[0], "");
        assert!(run_command(&mut sheet, "clear", 1, 1, 0));
        assert_eq!(sheet.slots[1], "");
        assert!(run_command(&mut sheet, "cut", 0, 2, 0));
        assert_eq!(sheet.slots, ["002", "001", "001", "002", "002", "001", "001"]);
        assert!(!run_command(&mut sheet, "autoexpose", 0, 2, 0));
        assert!(!run_command(&mut sheet, "random", 0, 2, 3));
        assert!(!run_command(&mut sheet, "", 0, 2, 0));
    }

    #[test]
    fn rows_keep_slots_pool_issued_base_seed_and_active_and_refuse_bad_ones() {
        let mut held = ExposureSheet::new(11);
        let first = held.append_blank().unwrap();
        held.hold(None, 2);
        held.append_blank();
        held.append_blank();
        held.reuse(&first, Some(3));
        held.delete(Some(1));
        held.active = 2;
        let row = held.to_row();
        let keys: HashSet<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, HashSet::from(["slots", "pool", "issued", "base_seed", "active"]));
        assert_eq!(row["slots"], serde_json::json!(["000", "000", "000", "002"]));
        assert_eq!(row["pool"], serde_json::json!(["000", "001", "002"]));
        assert_eq!(row["issued"], 3);
        assert_eq!(row["base_seed"], 11);
        assert_eq!(row["active"], 2);
        let back = ExposureSheet::from_row(&row).unwrap();
        assert_eq!(back, held);
        assert_eq!(back.to_row(), row);
        let seeds: Vec<u32> = (0..back.len()).map(|index| back.boil_seed(index).unwrap()).collect();
        assert_eq!(seeds, [11, 11, 11, 13]);

        let mut good = ExposureSheet::new(0);
        good.append_blank();
        good.append_blank();
        let row = good.to_row();
        let mut bad_id = row.clone();
        bad_id["pool"] = serde_json::json!(["000", "001", "GG0"]);
        bad_id["issued"] = serde_json::json!(3);
        assert!(ExposureSheet::from_row(&bad_id).is_none());
        let mut short_token = row.clone();
        short_token["slots"] = serde_json::json!(["12"]);
        short_token["pool"] = serde_json::json!(["12"]);
        short_token["issued"] = serde_json::json!(1);
        assert!(ExposureSheet::from_row(&short_token).is_none());
        let mut not_a_list = row.clone();
        not_a_list["slots"] = serde_json::json!("000");
        assert!(ExposureSheet::from_row(&not_a_list).is_none());
        let mut missing = row.clone();
        missing["slots"] = serde_json::json!(["000", "003"]);
        assert!(ExposureSheet::from_row(&missing).is_none());
        let mut short = row.clone();
        short["issued"] = serde_json::json!(1);
        assert!(ExposureSheet::from_row(&short).is_none());
        let mut off = row.clone();
        off["active"] = serde_json::json!(2);
        assert!(ExposureSheet::from_row(&off).is_none());
        let mut lower = row.clone();
        lower["slots"] = serde_json::json!(["00a", null]);
        lower["pool"] = serde_json::json!(["000", "001", "00a"]);
        lower["issued"] = serde_json::json!(3);
        let loaded = ExposureSheet::from_row(&lower).unwrap();
        assert_eq!(loaded.slots, ["00A", ""], "ids are upper case, null is a blank");
        assert_eq!(loaded.pool, ["000", "001", "00A"]);
        let mut unknown = row.clone();
        unknown["slots"] = serde_json::json!(["00a"]);
        assert!(ExposureSheet::from_row(&unknown).is_none(), "a slot outside the pool is refused");

        let gapped = serde_json::json!({
            "slots": ["000", "002", "000"],
            "pool": ["000", "002"],
            "issued": 5,
            "base_seed": 4,
            "active": 1,
        });
        let loaded = ExposureSheet::from_row(&gapped).unwrap();
        assert_eq!(loaded.to_row(), gapped);
        assert!(!loaded.pool.contains(&"001".to_string()));
        let seeds: Vec<u32> = (0..3).map(|index| loaded.boil_seed(index).unwrap()).collect();
        assert_eq!(seeds, [4, 6, 4]);
        let empty = serde_json::json!({"slots": [], "pool": [], "issued": 0, "base_seed": 0, "active": 0});
        let blank = ExposureSheet::from_row(&empty).unwrap();
        assert!(blank.slots.is_empty() && blank.pool.is_empty());
        assert_eq!(blank.to_row(), empty);
        assert!(ExposureSheet::from_row(&serde_json::json!({"issued": 0, "base_seed": 0, "active": 0})).is_none());
        assert!(ExposureSheet::from_row(&serde_json::json!({"note": "no sheet"})).is_none());
        assert!(ExposureSheet::from_row(&serde_json::json!(null)).is_none());
        assert_eq!(ExposureSheet::default().to_row(), empty);
    }

    #[test]
    fn a_timeline_projects_holds_without_saving_a_sheet() {
        let first = layer("a", "A");
        let second = layer("b", "B");
        let slots = project_slots(&[(0, &first), (3, &second), (5, &first)]);
        assert_eq!(slots, ["000", "000", "000", "001", "001", "000"]);
        assert_eq!(project_slots(&[(2, &first)]), ["", "", "000"]);
        assert!(project_slots(&[]).is_empty());
        let sheet = sheet_from_slots(&slots, 0).unwrap();
        assert_eq!(sheet.pool, ["000", "001"]);
        assert_eq!(sheet.issued, 2);
        assert_eq!(sheet.active, 0);
        assert!(sheet_from_slots(&["0G0".to_string()], 0).is_none());
        let mixed = sheet_from_slots(&["00A".to_string(), "001".to_string(), "00a".to_string()], 0).unwrap();
        assert_eq!(mixed.pool, ["00A", "001"]);
    }

    #[test]
    fn xsheet_columns_run_back_to_front_with_one_row_per_frame() {
        let mut back = Node::group("b", "back");
        back.animation = true;
        back.children = vec![layer("b1", "1"), layer("b2", "2")];
        back.timeline = vec!["b1".into(), "b2".into()];
        back.frames = vec![0, 3];
        let mut front = Node::group("f", "front");
        front.animation = true;
        front.children = vec![layer("f1", "1")];
        front.timeline = vec!["f1".into()];
        front.frames = vec![2];
        front.exposure = Some(sheet(&[2, 2, 2, 2, 2, 2, 2]));
        let mut nodes = vec![back, layer("paper", "Paper"), front];
        let end = cut_end(&nodes);
        assert_eq!(end, 4);
        let columns = xsheet_columns(&nodes, end);
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].name, "back");
        assert_eq!(columns[0].folder_id, "b");
        assert_eq!(columns[1].name, "front");
        for column in &columns {
            assert_eq!(column.cells.len(), 4);
            assert_eq!(column.keys.len(), 4);
        }
        let back_cells: Vec<Option<&str>> = columns[0].cells.iter().map(Option::as_deref).collect();
        assert_eq!(back_cells, [Some("b1"), Some("b1"), Some("b1"), Some("b2")]);
        assert_eq!(columns[0].keys, [true, false, false, true]);
        assert_eq!(columns[0].labels, ["000", "000", "000", "001"]);
        assert!(!columns[0].stored);
        let front_cells: Vec<Option<&str>> = columns[1].cells.iter().map(Option::as_deref).collect();
        assert_eq!(front_cells, [None, None, Some("f1"), Some("f1")]);
        assert_eq!(columns[1].keys, [false, false, true, false]);
        assert_eq!(columns[1].labels.len(), 7, "a stored sheet keeps its own length");
        assert!(columns[1].stored);

        // The first command stores a sheet made from the keys, then edits it.
        assert!(find(&nodes, "b").unwrap().exposure.is_none());
        assert!(xsheet_command(&mut nodes, 0, "step", 0, 4, 2));
        let stored = find(&nodes, "b").unwrap().exposure.as_ref().unwrap();
        assert_eq!(stored.slots, ["000", "000", "000", "000", "000", "000", "001", "001"]);
        let folder = find(&nodes, "b").unwrap();
        assert_eq!(folder.frames, vec![0, 3], "the sheet does not touch the keys");
        assert_eq!(cel_at(folder, 3).unwrap().id, "b2");
        assert!(!xsheet_command(&mut nodes, 5, "step", 0, 1, 2), "a column off the sheet is refused");
        let columns = xsheet_columns(&nodes, cut_end(&nodes));
        assert!(columns[0].stored);
        assert_eq!(columns[0].labels.len(), 8);
        assert_eq!(cel_mark(find(&nodes, "b").unwrap().exposure.as_ref(), 1), "hold");

        let mut desk_back = Node::group("db", "back");
        desk_back.animation = true;
        desk_back.exposure = Some(sheet(&[1, 3]));
        let mut nodes = vec![desk_back];
        assert!(xsheet_command(&mut nodes, 0, "autoexpose", 0, 2, 0));
        assert_eq!(nodes[0].exposure.as_ref().unwrap().slots, ["001", "001", "003"]);

        // The desk names the column by its folder.
        let mut art = Art::blank(1, 1);
        art.layers = vec![Node::group("folder", "Folder"), keyed_folder()];
        assert!(!folder_command(&mut art, "folder", "step", 0, 1, 2), "a plain folder has no column");
        assert!(!folder_command(&mut art, "missing", "step", 0, 1, 2));
        assert!(folder_command(&mut art, "shot", "step", 0, 6, 2));
        let stored = find(&art.layers, "shot").unwrap().exposure.as_ref().unwrap();
        assert_eq!(stored.len(), 12, "the keys [0, 2, 5] project six cells, stepped twice");
        assert_eq!(&stored.slots[..4], ["000", "000", "000", "000"]);
        assert!(find(&art.layers, "folder").unwrap().exposure.is_none());
    }
}
