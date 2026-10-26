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
    let mut dst = vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
    let solo = any_solo(&art.layers);
    paint_nodes(&mut art.layers, &mut dst, width, height, 255, solo);
    dst
}

fn paint_nodes(nodes: &mut [Node], dst: &mut [u8], width: u32, height: u32, opacity: u8, solo: bool) {
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
            paint_nodes(&mut node.children, &mut plate, width, height, 255, solo);
            blit_buffer(dst, width, height, &plate, width, height, 0, 0, next, node.clip, &node.blend);
            continue;
        }
        node.pixels.resize((width as usize) * (height as usize) * 4, 0);
        blit_buffer(dst, width, height, &node.pixels, width, height, 0, 0, next, node.clip, &node.blend);
        if !node.embedded {
            let strokes = node.strokes.len();
            for index in 0..strokes {
                let raster = {
                    let stroke = &mut node.strokes[index];
                    stroke.ensure_cache(width, height).cloned()
                };
                if let Some(raster) = raster {
                    blit_buffer(dst, width, height, &raster.rgba, raster.w, raster.h, raster.x, raster.y, next, node.clip, &node.blend);
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
    if sw == 0 || sh == 0 || opacity == 0 || src.len() < (sw as usize) * (sh as usize) * 4 {
        return;
    }
    for y in 0..sh as i32 {
        for x in 0..sw as i32 {
            let dx = ox + x;
            let dy = oy + y;
            if dx < 0 || dy < 0 || dx as u32 >= dw || dy as u32 >= dh {
                continue;
            }
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
    fn objects_reorder() {
        let mut objects = vec![
            DeskObject { id: "a".into(), name: "A".into(), kind: "static".into() },
            DeskObject { id: "b".into(), name: "B".into(), kind: "static".into() },
        ];
        reorder_objects(&mut objects, 1, 0);
        assert_eq!(objects[0].id, "b");
    }
}
