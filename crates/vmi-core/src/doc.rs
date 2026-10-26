//! The open drawing. Index 0 is the back of a list. The front is the last child.

use crate::blend::{mix, mix_hsl};
use crate::color::Rgba;
use crate::coverage::source_over;
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
        }
    }

    pub fn is_layer(&self) -> bool {
        self.kind == NodeKind::Layer
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskObject {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct Art {
    pub width: u32,
    pub height: u32,
    pub file_name: String,
    pub path: String,
    pub layers: Vec<Node>,
    pub next_id: u64,
    pub next_stroke: u64,
    pub objects: Vec<DeskObject>,
}

impl Art {
    pub fn new(width: u32, height: u32) -> Self {
        let mut art = Self {
            width,
            height,
            file_name: "untitled".into(),
            path: String::new(),
            layers: Vec::new(),
            next_id: 1,
            next_stroke: 1,
            objects: Vec::new(),
        };
        art.add_layer(None, "Ink");
        art
    }

    pub fn blank(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            file_name: "untitled".into(),
            path: String::new(),
            layers: Vec::new(),
            next_id: 1,
            next_stroke: 1,
            objects: Vec::new(),
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

/// Paper is clear. The viewfinder draws the checker behind this.
pub fn present(art: &mut Art) -> Vec<u8> {
    let width = art.width;
    let height = art.height;
    present_region(art, [0, 0, width as i32, height as i32])
}

/// Composite only `x0, y0, x1, y1` (end exclusive). The buffer is region sized;
/// its first pixel is the document pixel at `(x0, y0)`. A pen move only needs
/// the pixels it touched.
pub fn present_region(art: &mut Art, rect: [i32; 4]) -> Vec<u8> {
    let [x0, y0, x1, y1] = clamp_rect(rect, art.width, art.height);
    let rw = (x1 - x0).max(0) as u32;
    let rh = (y1 - y0).max(0) as u32;
    let mut dst = vec![0u8; (rw as usize).saturating_mul(rh as usize).saturating_mul(4)];
    if rw == 0 || rh == 0 {
        return dst;
    }
    let solo = any_solo(&art.layers);
    let (width, height) = (art.width, art.height);
    let target = Target { buf: &mut dst, w: rw, h: rh, ox: x0, oy: y0 };
    paint_nodes(&mut art.layers, target, width, height, 255, solo);
    dst
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

fn paint_nodes(nodes: &mut [Node], target: Target<'_>, width: u32, height: u32, opacity: u8, solo: bool) {
    let Target { buf: dst, w: rw, h: rh, ox, oy } = target;
    for node in nodes {
        if !node.visible || node.mute {
            continue;
        }
        if solo && !branch_solo(node) {
            continue;
        }
        let next = mul_opacity(opacity, node.opacity);
        if node.kind == NodeKind::Group {
            let mut plate = vec![0u8; dst.len()];
            paint_nodes(&mut node.children, Target { buf: &mut plate, w: rw, h: rh, ox, oy }, width, height, 255, solo);
            blit_into(dst, rw, rh, ox, oy, &plate, rw, rh, ox, oy, next, node.clip, &node.blend);
            continue;
        }
        node.pixels.resize((width as usize) * (height as usize) * 4, 0);
        blit_into(dst, rw, rh, ox, oy, &node.pixels, width, height, 0, 0, next, node.clip, &node.blend);
        if !node.embedded {
            let strokes = node.strokes.len();
            for index in 0..strokes {
                let raster = {
                    let stroke = &mut node.strokes[index];
                    stroke.ensure_cache(width, height).cloned()
                };
                if let Some(raster) = raster {
                    blit_into(dst, rw, rh, ox, oy, &raster.rgba, raster.w, raster.h, raster.x, raster.y, next, node.clip, &node.blend);
                }
            }
        }
    }
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

pub fn tight_pixels(pixels: &[u8], width: u32, height: u32) -> Option<Raster> {
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
        let mut objects = vec![
            DeskObject { id: "a".into(), name: "A".into(), kind: "static".into() },
            DeskObject { id: "b".into(), name: "B".into(), kind: "static".into() },
        ];
        reorder_objects(&mut objects, 1, 0);
        assert_eq!(objects[0].id, "b");
    }
}
