//! Editable vector strokes.
//!
//! The stroke is the record. `bake` rebuilds a tight RGBA cache from the
//! points, handles, bone, and grain. It does not change those traits.

use serde::{Deserialize, Serialize};

use crate::color::Rgba;
use crate::coverage::{liquid_level, stamp, Stamp, StampMode, TONE};
use crate::tool::{CurveKind, GrainKind, Tool, ToolSettings};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub width: f32,
    #[serde(default)]
    pub handle_in: Option<[f32; 2]>,
    #[serde(default)]
    pub handle_out: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub reach: f32,
}

/// A tight RGBA image. `x` and `y` are the document origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VectorStroke {
    pub id: u64,
    pub vertices: Vec<Vertex>,
    pub closed: bool,
    pub curve: CurveKind,
    pub color: [u8; 3],
    pub softness: f32,
    pub fringe: f32,
    pub edge: crate::coverage::Edge,
    pub chunk: u32,
    pub tones: bool,
    pub grain: GrainKind,
    pub seed: u64,
    pub bone: Option<Bone>,
    pub size: f32,
    #[serde(skip)]
    pub cache: Option<Raster>,
    #[serde(skip, default = "dirty_new")]
    pub dirty: bool,
}

fn dirty_new() -> bool {
    true
}

impl VectorStroke {
    pub fn from_gesture(id: u64, tool: Tool, points: &[(f32, f32, f32)], settings: &ToolSettings) -> Option<Self> {
        if points.is_empty() {
            return None;
        }
        let curve = if tool == Tool::Curve { CurveKind::Cubic } else { settings.curve };
        let mut vertices = Vec::with_capacity(points.len());
        for (x, y, pressure) in points {
            let radius = settings.radius_for((*pressure).clamp(0.0, 1.0)).max(0.5);
            vertices.push(Vertex {
                x: *x,
                y: *y,
                pressure: (*pressure).clamp(0.0, 1.0),
                width: radius,
                handle_in: None,
                handle_out: None,
            });
        }
        if curve != CurveKind::Poly {
            assign_handles(&mut vertices, curve);
        }
        let grain = if tool == Tool::Grain && settings.grain == GrainKind::None {
            GrainKind::Liquid
        } else {
            settings.grain
        };
        Some(Self {
            id,
            vertices,
            closed: settings.closed,
            curve,
            color: settings.color,
            softness: settings.softness_for(tool),
            fringe: settings.fringe,
            edge: settings.edge,
            chunk: settings.chunk.max(1),
            tones: settings.tones || grain == GrainKind::Liquid,
            grain,
            seed: settings.seed,
            bone: None,
            size: settings.size,
            cache: None,
            dirty: true,
        })
    }

    pub fn touch(&mut self) {
        self.dirty = true;
        self.cache = None;
    }

    /// Change the curve and rebuild handles when the new kind needs them.
    pub fn set_curve(&mut self, curve: CurveKind) {
        self.curve = curve;
        if curve != CurveKind::Poly {
            assign_handles(&mut self.vertices, curve);
        }
        self.touch();
    }

    pub fn ensure_cache(&mut self, width: u32, height: u32) -> Option<&Raster> {
        if self.cache.is_none() || self.dirty {
            self.cache = bake(self, width, height);
            self.dirty = false;
        }
        self.cache.as_ref()
    }
}

fn assign_handles(vertices: &mut [Vertex], curve: CurveKind) {
    let count = vertices.len();
    if count < 2 {
        return;
    }
    for index in 0..count {
        let prev = if index == 0 { 0 } else { index - 1 };
        let next = if index + 1 == count { index } else { index + 1 };
        let point = [vertices[index].x, vertices[index].y];
        let before = [vertices[prev].x, vertices[prev].y];
        let after = [vertices[next].x, vertices[next].y];
        let delta = [(after[0] - before[0]) / 6.0, (after[1] - before[1]) / 6.0];
        if curve == CurveKind::Cubic || curve == CurveKind::Spline {
            vertices[index].handle_in = Some([point[0] - delta[0], point[1] - delta[1]]);
            vertices[index].handle_out = Some([point[0] + delta[0], point[1] + delta[1]]);
        } else if curve == CurveKind::Quadratic {
            vertices[index].handle_out = Some([
                point[0] + (after[0] - point[0]) / 2.0,
                point[1] + (after[1] - point[1]) / 2.0,
            ]);
        }
    }
}

/// Rotate a point around the bone root. Influence falls off with distance.
pub fn apply_bone(x: f32, y: f32, bone: Bone) -> (f32, f32) {
    let angle = (bone.y1 - bone.y0).atan2(bone.x1 - bone.x0);
    let dx = x - bone.x0;
    let dy = y - bone.y0;
    let reach = bone.reach.max(1.0);
    let weight = (1.0 - dx.hypot(dy) / reach).clamp(0.0, 1.0);
    let (sin, cos) = (angle * weight).sin_cos();
    (bone.x0 + dx * cos - dy * sin, bone.y0 + dx * sin + dy * cos)
}

struct Sample {
    x: f32,
    y: f32,
    radius: f32,
}

fn placed(vertex: &Vertex, bone: Option<Bone>) -> (f32, f32) {
    match bone {
        Some(bone) => apply_bone(vertex.x, vertex.y, bone),
        None => (vertex.x, vertex.y),
    }
}

fn samples_of(stroke: &VectorStroke) -> Vec<Sample> {
    let count = stroke.vertices.len();
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        let (x, y) = placed(&stroke.vertices[0], stroke.bone);
        return vec![Sample { x, y, radius: stroke.vertices[0].width.max(0.5) }];
    }
    let mut out = Vec::new();
    let last = if stroke.closed { count } else { count - 1 };
    for index in 0..last {
        let a = &stroke.vertices[index % count];
        let b = &stroke.vertices[(index + 1) % count];
        let (ax, ay) = placed(a, stroke.bone);
        let (bx, by) = placed(b, stroke.bone);
        let span = (bx - ax).hypot(by - ay).max(0.5);
        let step = (a.width.max(b.width) * 0.45).clamp(0.75, 8.0);
        let steps = ((span / step).ceil() as i32).clamp(1, 256);
        for step_index in 0..=steps {
            let t = step_index as f32 / steps as f32;
            let (x, y) = point_at(stroke, index % count, (index + 1) % count, t, (ax, ay), (bx, by));
            let radius = a.width + (b.width - a.width) * t;
            out.push(Sample { x, y, radius: radius.max(0.5) });
            if out.len() >= 8000 {
                return out;
            }
        }
    }
    out
}

fn point_at(
    stroke: &VectorStroke,
    ia: usize,
    ib: usize,
    t: f32,
    a: (f32, f32),
    b: (f32, f32),
) -> (f32, f32) {
    let va = &stroke.vertices[ia];
    let vb = &stroke.vertices[ib];
    match stroke.curve {
        CurveKind::Poly => (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t),
        CurveKind::Quadratic => {
            let handle = va.handle_out.map(|h| bone_point(h, stroke.bone)).unwrap_or(((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5));
            quad(a, handle, b, t)
        }
        CurveKind::Cubic => {
            let c1 = va.handle_out.map(|h| bone_point(h, stroke.bone)).unwrap_or(a);
            let c2 = vb.handle_in.map(|h| bone_point(h, stroke.bone)).unwrap_or(b);
            cubic(a, c1, c2, b, t)
        }
        CurveKind::Spline => {
            let count = stroke.vertices.len();
            let p0 = placed(&stroke.vertices[(ia + count - 1) % count], stroke.bone);
            let p3 = placed(&stroke.vertices[(ib + 1) % count], stroke.bone);
            catmull(p0, a, b, p3, t)
        }
    }
}

fn bone_point(point: [f32; 2], bone: Option<Bone>) -> (f32, f32) {
    match bone {
        Some(bone) => apply_bone(point[0], point[1], bone),
        None => (point[0], point[1]),
    }
}

fn quad(a: (f32, f32), b: (f32, f32), c: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    (u * u * a.0 + 2.0 * u * t * b.0 + t * t * c.0, u * u * a.1 + 2.0 * u * t * b.1 + t * t * c.1)
}

fn cubic(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let x = u * u * u * a.0 + 3.0 * u * u * t * b.0 + 3.0 * u * t * t * c.0 + t * t * t * d.0;
    let y = u * u * u * a.1 + 3.0 * u * u * t * b.1 + 3.0 * u * t * t * c.1 + t * t * t * d.1;
    (x, y)
}

fn catmull(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    let axis = |a: f32, b: f32, c: f32, d: f32| {
        0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (axis(p0.0, p1.0, p2.0, p3.0), axis(p0.1, p1.1, p2.1, p3.1))
}

pub fn bake(stroke: &VectorStroke, width: u32, height: u32) -> Option<Raster> {
    if width == 0 || height == 0 {
        return None;
    }
    let samples = samples_of(stroke);
    if samples.is_empty() {
        return None;
    }
    let mut canvas = vec![0u8; (width as usize) * (height as usize) * 4];
    let color = Rgba::rgb(stroke.color[0], stroke.color[1], stroke.color[2]);
    for sample in &samples {
        stamp(&mut canvas, width, height, Stamp {
            cx: sample.x,
            cy: sample.y,
            radius: sample.radius,
            softness: stroke.softness,
            fringe: stroke.fringe,
            opacity: 1.0,
            color,
            edge: stroke.edge,
            chunk: stroke.chunk,
            tones: stroke.tones && stroke.grain != GrainKind::Liquid,
            mode: StampMode::Over,
        });
    }
    apply_grain(&mut canvas, width, height, stroke);
    tight(canvas, width, height)
}

fn apply_grain(canvas: &mut [u8], width: u32, height: u32, stroke: &VectorStroke) {
    match stroke.grain {
        GrainKind::None => {}
        GrainKind::Liquid => {
            for y in 0..height as i32 {
                for x in 0..width as i32 {
                    let index = ((y as u32 * width + x as u32) * 4) as usize;
                    if canvas[index + 3] == 0 {
                        continue;
                    }
                    let step = stroke.chunk.max(1) as i32;
                    let level = liquid_level(stroke.seed as u32, x / step, y / step);
                    let tone = TONE[level as usize];
                    canvas[index] = tone[0];
                    canvas[index + 1] = tone[1];
                    canvas[index + 2] = tone[2];
                    canvas[index + 3] = 255;
                }
            }
        }
        GrainKind::Hatch => {
            let pitch = (stroke.chunk.max(1) as i32) * 3;
            for y in 0..height as i32 {
                for x in 0..width as i32 {
                    let index = ((y as u32 * width + x as u32) * 4) as usize;
                    if canvas[index + 3] == 0 {
                        continue;
                    }
                    if (x + y).rem_euclid(pitch) > 0 {
                        canvas[index + 3] = 0;
                    }
                }
            }
        }
        GrainKind::Stipple => {
            let mut mask = vec![0u8; canvas.len()];
            mask.copy_from_slice(canvas);
            canvas.fill(0);
            let bounds = alpha_bounds(&mask, width, height);
            let Some((min_x, min_y, max_x, max_y)) = bounds else {
                return;
            };
            let dots = generate(stroke.seed, min_x, min_y, max_x, max_y, 24);
            let color = Rgba::rgb(stroke.color[0], stroke.color[1], stroke.color[2]);
            for (x, y) in dots {
                stamp(canvas, width, height, Stamp {
                    cx: x,
                    cy: y,
                    radius: (stroke.size * 0.08).max(1.0),
                    softness: 0.0,
                    fringe: 0.0,
                    opacity: 1.0,
                    color,
                    edge: crate::coverage::Edge::Hard,
                    chunk: 1,
                    tones: false,
                    mode: StampMode::Over,
                });
            }
            for index in (0..canvas.len()).step_by(4) {
                if mask[index + 3] == 0 {
                    canvas[index + 3] = 0;
                }
            }
        }
    }
}

fn alpha_bounds(buf: &[u8], width: u32, height: u32) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = width as i32;
    let mut min_y = height as i32;
    let mut max_x = 0i32;
    let mut max_y = 0i32;
    let mut any = false;
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let index = ((y as u32 * width + x as u32) * 4 + 3) as usize;
            if buf[index] > 0 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    any.then_some((min_x as f32, min_y as f32, max_x as f32, max_y as f32))
}

pub fn generate(seed: u64, min_x: f32, min_y: f32, max_x: f32, max_y: f32, count: u32) -> Vec<(f32, f32)> {
    let count = count.min(64);
    let mut rng = seed | 1;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let ux = (rng >> 33) as f32 / (1u32 << 31) as f32;
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let uy = (rng >> 33) as f32 / (1u32 << 31) as f32;
        out.push((min_x + (max_x - min_x) * ux.clamp(0.0, 1.0), min_y + (max_y - min_y) * uy.clamp(0.0, 1.0)));
    }
    out
}

pub fn tight(buf: Vec<u8>, width: u32, height: u32) -> Option<Raster> {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4 + 3) as usize;
            if buf[index] > 0 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !any {
        return None;
    }
    let w = max_x - min_x + 1;
    let h = max_y - min_y + 1;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let src = (((min_y + y) * width + min_x) * 4) as usize;
        let dst = (y * w * 4) as usize;
        rgba[dst..dst + (w * 4) as usize].copy_from_slice(&buf[src..src + (w * 4) as usize]);
    }
    Some(Raster { x: min_x as i32, y: min_y as i32, w, h, rgba })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeHit {
    Vertex(usize),
    HandleIn(usize),
    HandleOut(usize),
    Body,
}

pub fn hit_stroke(stroke: &VectorStroke, x: f32, y: f32, pick: f32) -> Option<StrokeHit> {
    let pick = pick.max(4.0);
    for (index, vertex) in stroke.vertices.iter().enumerate() {
        if let Some(handle) = vertex.handle_in {
            if (handle[0] - x).hypot(handle[1] - y) <= pick {
                return Some(StrokeHit::HandleIn(index));
            }
        }
        if let Some(handle) = vertex.handle_out {
            if (handle[0] - x).hypot(handle[1] - y) <= pick {
                return Some(StrokeHit::HandleOut(index));
            }
        }
    }
    for (index, vertex) in stroke.vertices.iter().enumerate() {
        if (vertex.x - x).hypot(vertex.y - y) <= pick {
            return Some(StrokeHit::Vertex(index));
        }
    }
    let radius = stroke.vertices.iter().map(|vertex| vertex.width).fold(0.0, f32::max) + 6.0;
    let count = stroke.vertices.len();
    if count == 1 {
        return Some(StrokeHit::Body);
    }
    let last = if stroke.closed { count } else { count.saturating_sub(1) };
    for index in 0..last {
        let a = &stroke.vertices[index % count];
        let b = &stroke.vertices[(index + 1) % count];
        if distance_to_segment(x, y, a.x, a.y, b.x, b.y) <= radius {
            return Some(StrokeHit::Body);
        }
    }
    None
}

fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 <= 1e-6 {
        return (px - ax).hypot(py - ay);
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    (px - (ax + dx * t)).hypot(py - (ay + dy * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::Edge;

    #[test]
    fn a_vector_pen_keeps_its_points_and_bakes_pixels() {
        let settings = ToolSettings::default_for(Tool::Vector);
        let points = [(4.0, 8.0, 1.0), (20.0, 12.0, 0.5), (36.0, 8.0, 1.0)];
        let stroke = VectorStroke::from_gesture(1, Tool::Vector, &points, &settings).unwrap();
        assert_eq!(stroke.vertices.len(), 3);
        assert!((stroke.vertices[1].pressure - 0.5).abs() < 1e-5);
        let baked = bake(&stroke, 64, 32).unwrap();
        assert!(baked.rgba.chunks(4).any(|pixel| pixel[3] > 0));
        assert_eq!(stroke.vertices.len(), 3);
    }

    #[test]
    fn a_bone_moves_a_near_point_and_leaves_a_far_one() {
        let bone = Bone { x0: 0.0, y0: 0.0, x1: 0.0, y1: 10.0, reach: 20.0 };
        let (x, y) = apply_bone(10.0, 0.0, bone);
        assert!((x - 10.0).abs() > 0.5 || (y - 0.0).abs() > 0.5, "{x},{y}");
        let (far_x, far_y) = apply_bone(400.0, 0.0, bone);
        assert!((far_x - 400.0).abs() < 0.01, "{far_x}");
        assert!(far_y.abs() < 0.01, "{far_y}");
    }

    #[test]
    fn curve_release_stores_handles() {
        let settings = ToolSettings::default_for(Tool::Curve);
        let stroke = VectorStroke::from_gesture(2, Tool::Curve, &[(0.0, 0.0, 1.0), (10.0, 20.0, 1.0), (30.0, 0.0, 1.0)], &settings).unwrap();
        assert_eq!(stroke.curve, CurveKind::Cubic);
        assert!(stroke.vertices[1].handle_in.is_some());
        assert!(stroke.vertices[1].handle_out.is_some());
    }

    #[test]
    fn the_same_seed_rebuilds_the_same_stipple() {
        let once = generate(9, 0.0, 0.0, 10.0, 10.0, 5);
        let twice = generate(9, 0.0, 0.0, 10.0, 10.0, 5);
        assert_eq!(once, twice);
        assert_eq!(once.len(), 5);
    }

    #[test]
    fn grain_liquid_uses_the_three_tones() {
        let mut settings = ToolSettings::default_for(Tool::Grain);
        settings.edge = Edge::Hard;
        settings.fringe = 0.0;
        settings.softness = 0.0;
        let stroke = VectorStroke::from_gesture(3, Tool::Grain, &[(16.0, 16.0, 1.0)], &settings).unwrap();
        let baked = bake(&stroke, 48, 48).unwrap();
        assert!(baked.rgba.chunks(4).any(|pixel| pixel[3] == 255));
        for pixel in baked.rgba.chunks(4) {
            if pixel[3] == 0 {
                continue;
            }
            assert!(
                pixel[..3] == TONE[0] || pixel[..3] == TONE[1] || pixel[..3] == TONE[2],
                "{pixel:?}"
            );
        }
    }
}
