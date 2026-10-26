//! Cut a marquee out of a folder. Pixels inside the polygon move to a new folder.

use crate::doc::{find_mut, fresh_name, parent_of, Art, Node, NodeKind};
use crate::vector::tight;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marquee {
    Rect,
    Lasso,
    Polygon,
}

pub fn rect_points(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 2]> {
    let (left, right) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let (top, bottom) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    vec![[left, top], [right, top], [right, bottom], [left, bottom]]
}

pub fn point_inside(x: f32, y: f32, points: &[[f32; 2]]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = points.len() - 1;
    for index in 0..points.len() {
        let (xi, yi) = (points[index][0], points[index][1]);
        let (xj, yj) = (points[previous][0], points[previous][1]);
        let hit = ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi + f32::EPSILON) + xi);
        if hit {
            inside = !inside;
        }
        previous = index;
    }
    inside
}

/// Move painted pixels inside `points` into a new folder in front of the source.
/// A selected layer uses the folder that holds it. Returns the new folder id.
pub fn cut_folder(art: &mut Art, selected: &str, points: &[[f32; 2]], name: &str) -> Option<String> {
    if points.len() < 3 {
        return None;
    }
    let folder_id = folder_target(&art.layers, selected)?;
    let label = {
        let trimmed = name.trim();
        if trimmed.is_empty() { fresh_name(&art.layers, "Cut") } else { fresh_name(&art.layers, trimmed) }
    };
    let mut made_children = Vec::new();
    let changed = {
        let folder = find_mut(&mut art.layers, &folder_id)?;
        split_folder(folder, art.width, art.height, points, &mut made_children, &mut art.next_id)
    };
    if !changed {
        return None;
    }
    let id = art.alloc_id();
    let mut group = Node::group(id.clone(), label);
    group.children = made_children;
    if !insert_after(&mut art.layers, &folder_id, group) {
        return None;
    }
    Some(id)
}

fn split_folder(folder: &mut Node, width: u32, height: u32, points: &[[f32; 2]], made: &mut Vec<Node>, next_id: &mut u64) -> bool {
    let mut changed = false;
    if folder.kind == NodeKind::Layer {
        if let Some(cut) = split_layer(folder, width, height, points, next_id) {
            made.push(cut);
            changed = true;
        }
        return changed;
    }
    for child in &mut folder.children {
        if child.kind == NodeKind::Group {
            let mut nested = Vec::new();
            if split_folder(child, width, height, points, &mut nested, next_id) {
                let id = format!("n{next_id}");
                *next_id += 1;
                let mut group = Node::group(id, child.name.clone());
                group.children = nested;
                made.push(group);
                changed = true;
            }
        } else if let Some(cut) = split_layer(child, width, height, points, next_id) {
            made.push(cut);
            changed = true;
        }
    }
    changed
}

fn split_layer(layer: &mut Node, width: u32, height: u32, points: &[[f32; 2]], next_id: &mut u64) -> Option<Node> {
    let need = (width as usize) * (height as usize) * 4;
    if layer.pixels.len() != need {
        return None;
    }
    let mut cut_pixels = vec![0u8; need];
    let mut hit = false;
    for y in 0..height {
        for x in 0..width {
            if !point_inside(x as f32 + 0.5, y as f32 + 0.5, points) {
                continue;
            }
            let index = ((y * width + x) * 4) as usize;
            if layer.pixels[index + 3] == 0 {
                continue;
            }
            cut_pixels[index..index + 4].copy_from_slice(&layer.pixels[index..index + 4]);
            layer.pixels[index + 3] = 0;
            hit = true;
        }
    }
    if !hit {
        return None;
    }
    let id = format!("n{next_id}");
    *next_id += 1;
    let mut node = Node::layer(id, layer.name.clone(), width, height);
    node.pixels = cut_pixels;
    Some(node)
}

pub fn erase_folder(art: &mut Art, selected: &str, points: &[[f32; 2]]) -> usize {
    if points.len() < 3 {
        return 0;
    }
    let Some(folder_id) = folder_target(&art.layers, selected) else {
        return 0;
    };
    let width = art.width;
    let height = art.height;
    let Some(folder) = find_mut(&mut art.layers, &folder_id) else {
        return 0;
    };
    erase_node(folder, width, height, points)
}

fn erase_node(node: &mut Node, width: u32, height: u32, points: &[[f32; 2]]) -> usize {
    if node.kind == NodeKind::Group {
        return node.children.iter_mut().map(|child| erase_node(child, width, height, points)).sum();
    }
    let need = (width as usize) * (height as usize) * 4;
    if node.pixels.len() != need {
        return 0;
    }
    let mut hit = false;
    for y in 0..height {
        for x in 0..width {
            if !point_inside(x as f32 + 0.5, y as f32 + 0.5, points) {
                continue;
            }
            let index = ((y * width + x) * 4 + 3) as usize;
            if node.pixels[index] == 0 {
                continue;
            }
            node.pixels[index] = 0;
            hit = true;
        }
    }
    usize::from(hit)
}

fn folder_target(nodes: &[Node], selected: &str) -> Option<String> {
    let node = crate::doc::find(nodes, selected)?;
    if node.kind == NodeKind::Group {
        Some(node.id.clone())
    } else {
        parent_of(nodes, selected).or_else(|| Some(selected.to_string()))
    }
}

fn contains(nodes: &[Node], id: &str) -> bool {
    nodes.iter().any(|node| node.id == id || contains(&node.children, id))
}

fn insert_after(nodes: &mut Vec<Node>, id: &str, made: Node) -> bool {
    if let Some(index) = nodes.iter().position(|node| node.id == id) {
        nodes.insert(index + 1, made);
        return true;
    }
    for child in nodes.iter_mut() {
        if contains(&child.children, id) {
            return insert_after(&mut child.children, id, made);
        }
    }
    false
}

pub fn cropped_alpha(pixels: &[u8], width: u32, height: u32) -> bool {
    tight(pixels.to_vec(), width, height).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_cut_moves_the_pixel_into_a_new_folder() {
        let mut art = Art::blank(4, 4);
        let id = art.add_layer(None, "Ink");
        art.layers[0].pixels[(1 * 4 + 1) * 4 + 3] = 255;
        let points = rect_points(0.0, 0.0, 2.0, 2.0);
        let made = cut_folder(&mut art, &id, &points, "Piece").unwrap();
        assert_eq!(art.layers[0].pixels[(1 * 4 + 1) * 4 + 3], 0);
        let folder = crate::doc::find(&art.layers, &made).unwrap();
        assert_eq!(folder.kind, NodeKind::Group);
        assert_eq!(folder.children[0].pixels[(1 * 4 + 1) * 4 + 3], 255);
        assert!(art.layers.iter().any(|node| node.id == id));
    }
}
