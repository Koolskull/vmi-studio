//! Raster ink. A stroke session keeps the smudge color until the pen lifts.
//! Release either drops the path or returns an editable vector stroke.

pub use vmi_core::stroke_steps;

use vmi_core::coverage::dither_keeps;
use vmi_core::{
    coverage, ink_spacing, pixel, stamp, Edge, Grain, Rgba, Stamp, StampMode, Tool, ToolSettings, VectorStroke,
};

/// How many hairs a smudge drags across its width. Each one carries its own color.
pub const SMUDGE_HAIRS: usize = 8;

/// What a smudge holds between dabs. `color` is the average the status line
/// and tests read. `hairs` is one color per band across the stroke, so a drag
/// across a red and blue edge leaves red and blue streaks, not purple.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Carry {
    pub color: Option<Rgba>,
    pub hairs: Vec<Rgba>,
}

impl Carry {
    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.hairs.is_empty()
    }

    pub fn clear(&mut self) {
        self.color = None;
        self.hairs.clear();
    }
}

/// One gesture. `carry` survives every move and dies on release.
#[derive(Clone, Debug, Default)]
pub struct StrokeSession {
    pub samples: Vec<(f32, f32, f32)>,
    pub carry: Carry,
}

impl StrokeSession {
    pub fn begin(&mut self, x: f32, y: f32, pressure: f32) {
        self.samples.clear();
        self.carry.clear();
        self.samples.push((x, y, pressure.clamp(0.0, 1.0)));
    }

    pub fn push(&mut self, x: f32, y: f32, pressure: f32) {
        self.samples.push((x, y, pressure.clamp(0.0, 1.0)));
    }

    /// A straight line keeps the first sample and the latest point.
    pub fn line_to(&mut self, x: f32, y: f32, pressure: f32) {
        let start = self.samples.first().copied().unwrap_or((x, y, pressure));
        self.samples = vec![start, (x, y, pressure.clamp(0.0, 1.0))];
    }

    pub fn end_pixels(&mut self) {
        self.samples.clear();
        self.carry.clear();
    }

    pub fn end_stroke(&mut self, id: u64, tool: Tool, settings: &ToolSettings) -> Option<VectorStroke> {
        let points = std::mem::take(&mut self.samples);
        self.carry.clear();
        VectorStroke::from_gesture(id, tool, &points, settings)
    }
}

pub fn replay(
    buf: &mut [u8],
    width: u32,
    height: u32,
    base: &[u8],
    samples: &[(f32, f32, f32)],
    tool: Tool,
    settings: &ToolSettings,
) {
    let count = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    if base.len() == count && buf.len() == count {
        buf.copy_from_slice(base);
    }
    let mut carry = Carry::default();
    paint_samples(buf, width, height, samples, tool, settings, &mut carry);
}

pub fn paint_samples(
    buf: &mut [u8],
    width: u32,
    height: u32,
    samples: &[(f32, f32, f32)],
    tool: Tool,
    settings: &ToolSettings,
    carry: &mut Carry,
) -> bool {
    if samples.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut previous = samples[0];
    let first_angle = samples.get(1).map(|next| heading(previous, *next)).unwrap_or(0.0);
    changed |= dab_at(buf, width, height, previous, first_angle, tool, settings, carry);
    for sample in samples.iter().skip(1) {
        let spacing = ink_spacing(settings.diameter_for(previous.2.max(sample.2)), tool);
        let angle = heading(previous, *sample);
        let mut steps = stroke_steps(previous.0, previous.1, previous.2, sample.0, sample.1, sample.2, spacing);
        if steps.last().map(|step| (step.0 - sample.0).abs() > 0.01 || (step.1 - sample.1).abs() > 0.01).unwrap_or(true) {
            steps.push(*sample);
        }
        for step in steps {
            changed |= dab_at(buf, width, height, step, angle, tool, settings, carry);
        }
        previous = *sample;
    }
    changed
}

fn heading(from: (f32, f32, f32), to: (f32, f32, f32)) -> f32 {
    let dx = to.0 - from.0;
    let dy = to.1 - from.1;
    if dx.abs() < 1e-4 && dy.abs() < 1e-4 {
        0.0
    } else {
        dy.atan2(dx)
    }
}

/// The pixels a move can touch: `x0, y0, x1, y1`, end exclusive, clamped to the buffer.
pub fn segment_bounds(
    width: u32,
    height: u32,
    from: (f32, f32, f32),
    to: (f32, f32, f32),
    tool: Tool,
    settings: &ToolSettings,
) -> Option<[i32; 4]> {
    if tool == Tool::Fill {
        return Some([0, 0, width as i32, height as i32]);
    }
    let radius = settings.radius_for(from.2.max(to.2)).max(settings.radius_for(1.0).min(settings.size_max * 0.5));
    let pad = radius + settings.fringe.max(1.0) + 2.0;
    let x0 = ((from.0.min(to.0) - pad).floor() as i32).max(0);
    let y0 = ((from.1.min(to.1) - pad).floor() as i32).max(0);
    let x1 = ((from.0.max(to.0) + pad).ceil() as i32 + 1).min(width as i32);
    let y1 = ((from.1.max(to.1) + pad).ceil() as i32 + 1).min(height as i32);
    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some([x0, y0, x1, y1])
    }
}

/// Dabs along a move, not the point the previous move already stamped.
pub fn paint_segment(
    buf: &mut [u8],
    width: u32,
    height: u32,
    from: (f32, f32, f32),
    to: (f32, f32, f32),
    tool: Tool,
    settings: &ToolSettings,
    carry: &mut Carry,
) -> bool {
    let spacing = ink_spacing(settings.diameter_for(from.2.max(to.2)), tool);
    let angle = heading(from, to);
    let mut steps = stroke_steps(from.0, from.1, from.2, to.0, to.1, to.2, spacing);
    let end_far = steps
        .last()
        .map(|step| (step.0 - to.0).abs() > 0.01 || (step.1 - to.1).abs() > 0.01)
        .unwrap_or(true);
    if end_far {
        steps.push(to);
    }
    let mut changed = false;
    for step in steps {
        changed |= dab_at(buf, width, height, step, angle, tool, settings, carry);
    }
    changed
}

fn grain_for(settings: &ToolSettings, angle: f32, cx: f32, cy: f32) -> Grain {
    Grain { tip: settings.tip, amount: settings.texture, seed: settings.seed, angle, index: 0 }.at(cx, cy)
}

fn dab_at(
    buf: &mut [u8],
    width: u32,
    height: u32,
    sample: (f32, f32, f32),
    angle: f32,
    tool: Tool,
    settings: &ToolSettings,
    carry: &mut Carry,
) -> bool {
    let (x, y, pressure) = sample;
    if tool == Tool::Fill {
        return flood_fill(buf, width, height, x.round() as i32, y.round() as i32, settings);
    }
    let radius = settings.radius_for(pressure);
    let opacity = settings.opacity_for(pressure);
    let grain = grain_for(settings, angle, x, y);
    if tool == Tool::Smudge {
        return smudge_dab(buf, width, height, SmudgeDab { cx: x, cy: y, radius, opacity, settings, grain }, carry);
    }
    let mode = match tool {
        Tool::Eraser => StampMode::Erase,
        Tool::Blend => StampMode::Blend,
        _ => StampMode::Over,
    };
    let mut color = Rgba::rgb(settings.color[0], settings.color[1], settings.color[2]);
    if tool == Tool::Blend {
        let under = disc_average(buf, width, height, x, y, radius);
        if under.a == 0 {
            return false;
        }
        color = under;
    }
    stamp(buf, width, height, Stamp {
        cx: x,
        cy: y,
        radius,
        softness: settings.softness_for(tool),
        fringe: settings.fringe,
        opacity,
        color,
        edge: settings.edge,
        chunk: settings.chunk,
        tones: settings.tones || tool == Tool::Dither,
        mode,
        grain,
    })
}

struct SmudgeDab<'a> {
    cx: f32,
    cy: f32,
    radius: f32,
    opacity: f32,
    settings: &'a ToolSettings,
    grain: Grain,
}

/// The smudge is the one tool where the engine shapes what is picked up, not
/// only what is laid down. The disc is split into hairs across the heading.
/// Each hair lifts the color under it through the tip mask, holds most of
/// what it already carried, and lays that down. Streaks come out of the
/// mask; two colors under one disc stay two colors.
fn smudge_dab(buf: &mut [u8], width: u32, height: u32, dab: SmudgeDab<'_>, carry: &mut Carry) -> bool {
    let settings = dab.settings;
    let radius = dab.radius.max(0.5);
    if width == 0 || height == 0 || dab.opacity <= 0.0 {
        return false;
    }
    let softness = settings.softness.clamp(0.0, 1.0);
    let fringe = if settings.edge == Edge::Hard { 0.0 } else { settings.fringe.max(0.0) };
    let pad = if fringe > 0.0 || softness > 0.0 { fringe.max(1.0) } else { 0.0 };
    let reach = radius + pad;
    let x0 = (dab.cx - reach).floor().max(0.0) as i32;
    let y0 = (dab.cy - reach).floor().max(0.0) as i32;
    let x1 = ((dab.cx + reach).ceil() as i32 + 1).min(width as i32);
    let y1 = ((dab.cy + reach).ceil() as i32 + 1).min(height as i32);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let (sin, cos) = dab.grain.angle.sin_cos();
    let hair_of = |px: f32, py: f32| -> usize {
        let dx = px - dab.cx;
        let dy = py - dab.cy;
        let across = -dx * sin + dy * cos;
        let t = (across / radius + 1.0) * 0.5;
        ((t * SMUDGE_HAIRS as f32).floor() as i32).clamp(0, SMUDGE_HAIRS as i32 - 1) as usize
    };
    let mask_of = |x: i32, y: i32, dist: f32| -> f32 {
        if dab.grain.is_plain() {
            1.0
        } else {
            vmi_core::tip_sample(dab.grain, x, y, dab.cx, dab.cy, radius, dist).mask
        }
    };

    // Pass one: what is under each hair, weighted by the mask.
    let mut sum = [[0.0f32; 5]; SMUDGE_HAIRS];
    for y in y0..y1 {
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dist = (px - dab.cx).hypot(py - dab.cy);
            let cover = coverage(dist, radius, softness, fringe);
            if cover <= 0.0 {
                continue;
            }
            let weight = cover * mask_of(x, y, dist);
            if weight <= 0.0 {
                continue;
            }
            let p = pixel(buf, width, height, x, y);
            let a = f32::from(p.a) * weight;
            let slot = &mut sum[hair_of(px, py)];
            slot[0] += f32::from(p.r) * a;
            slot[1] += f32::from(p.g) * a;
            slot[2] += f32::from(p.b) * a;
            slot[3] += a;
            slot[4] += weight;
        }
    }
    let under: Vec<Rgba> = sum
        .iter()
        .map(|slot| {
            if slot[4] <= 0.0 || slot[3] <= 0.0 {
                Rgba::CLEAR
            } else {
                Rgba::new(
                    (slot[0] / slot[3]).round() as u8,
                    (slot[1] / slot[3]).round() as u8,
                    (slot[2] / slot[3]).round() as u8,
                    (slot[3] / slot[4]).round() as u8,
                )
            }
        })
        .collect();

    // The hairs keep most of what they held. A thin texture keeps more,
    // because a hard tip scrapes while a soft one drags.
    let pickup = 0.22 + 0.16 * (1.0 - settings.texture.clamp(0.0, 1.0));
    if carry.hairs.len() != SMUDGE_HAIRS {
        carry.hairs = under.clone();
    } else {
        for (held, fresh) in carry.hairs.iter_mut().zip(under.iter()) {
            *held = mix_toward(*held, *fresh, pickup);
        }
    }
    carry.color = Some(average_of(&carry.hairs)).filter(|color| color.a > 0);
    if carry.hairs.iter().all(|hair| hair.a == 0) {
        return false;
    }

    // Pass two: each hair lays down its own color.
    let opacity = dab.opacity.clamp(0.0, 1.0);
    let mut changed = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dist = (px - dab.cx).hypot(py - dab.cy);
            let mut cover = coverage(dist, radius, softness, fringe) * opacity;
            if cover <= 0.0 {
                continue;
            }
            cover *= mask_of(x, y, dist);
            if cover <= 0.001 {
                continue;
            }
            if settings.edge == Edge::Dither {
                if dither_keeps(cover, x, y, settings.chunk) {
                    cover = 1.0;
                } else {
                    continue;
                }
            }
            let hair = carry.hairs[hair_of(px, py)];
            if hair.a == 0 {
                continue;
            }
            let index = ((y as u32 * width + x as u32) * 4) as usize;
            let dst = Rgba::new(buf[index], buf[index + 1], buf[index + 2], buf[index + 3]);
            // Moving paint, not adding it: the pixel heads toward the hair,
            // alpha included, so a smudge dragged into clear paper thins out.
            let out = lerp_rgba(dst, hair, cover * 0.85);
            let bytes = out.channels();
            if bytes != [dst.r, dst.g, dst.b, dst.a] {
                buf[index..index + 4].copy_from_slice(&bytes);
                changed = true;
            }
        }
    }
    changed
}

fn lerp_rgba(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let ch = |p: u8, q: u8| (f32::from(p) + (f32::from(q) - f32::from(p)) * t).round() as u8;
    Rgba { r: ch(a.r, b.r), g: ch(a.g, b.g), b: ch(a.b, b.b), a: ch(a.a, b.a) }
}

fn average_of(colors: &[Rgba]) -> Rgba {
    let mut sum = [0.0f32; 4];
    for color in colors {
        let a = f32::from(color.a);
        sum[0] += f32::from(color.r) * a;
        sum[1] += f32::from(color.g) * a;
        sum[2] += f32::from(color.b) * a;
        sum[3] += a;
    }
    if colors.is_empty() || sum[3] <= 0.0 {
        return Rgba::CLEAR;
    }
    Rgba::new(
        (sum[0] / sum[3]).round() as u8,
        (sum[1] / sum[3]).round() as u8,
        (sum[2] / sum[3]).round() as u8,
        (sum[3] / colors.len() as f32).round() as u8,
    )
}

/// Alpha-weighted average of the pixels inside a disc. Clear pixels thin the alpha
/// but do not pull the color toward black.
pub fn disc_average(buf: &[u8], width: u32, height: u32, cx: f32, cy: f32, radius: f32) -> Rgba {
    let reach = radius.max(0.5);
    let x0 = (cx - reach).floor().max(0.0) as i32;
    let y0 = (cy - reach).floor().max(0.0) as i32;
    let x1 = ((cx + reach).ceil() as i32 + 1).min(width as i32);
    let y1 = ((cy + reach).ceil() as i32 + 1).min(height as i32);
    let mut sum = [0.0f32; 4];
    let mut count = 0.0f32;
    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((x as f32 + 0.5) - cx).hypot((y as f32 + 0.5) - cy);
            if dist > reach {
                continue;
            }
            let p = pixel(buf, width, height, x, y);
            let a = f32::from(p.a);
            sum[0] += f32::from(p.r) * a;
            sum[1] += f32::from(p.g) * a;
            sum[2] += f32::from(p.b) * a;
            sum[3] += a;
            count += 1.0;
        }
    }
    if count <= 0.0 || sum[3] <= 0.0 {
        return Rgba::CLEAR;
    }
    Rgba::new(
        (sum[0] / sum[3]).round() as u8,
        (sum[1] / sum[3]).round() as u8,
        (sum[2] / sum[3]).round() as u8,
        (sum[3] / count).round() as u8,
    )
}

/// Move `held` toward `under` by `amount`. Color follows alpha weight so a clear
/// patch thins the carry without darkening it.
fn mix_toward(held: Rgba, under: Rgba, amount: f32) -> Rgba {
    let t = amount.clamp(0.0, 1.0);
    let ha = f32::from(held.a) * (1.0 - t);
    let ua = f32::from(under.a) * t;
    let total = ha + ua;
    if total <= 0.0 {
        return Rgba::CLEAR;
    }
    let ch = |h: u8, u: u8| ((f32::from(h) * ha + f32::from(u) * ua) / total).round() as u8;
    Rgba::new(ch(held.r, under.r), ch(held.g, under.g), ch(held.b, under.b), total.round() as u8)
}

pub fn flood_fill(buf: &mut [u8], width: u32, height: u32, x: i32, y: i32, settings: &ToolSettings) -> bool {
    let target = pixel(buf, width, height, x, y);
    let ink = Rgba::rgb(settings.color[0], settings.color[1], settings.color[2]);
    if same(target, ink, settings.fill_tolerance) || x < 0 || y < 0 {
        return false;
    }
    let mut stack = vec![(x, y)];
    let mut seen = vec![false; (width as usize) * (height as usize)];
    let mut changed = false;
    while let Some((cx, cy)) = stack.pop() {
        if cx < 0 || cy < 0 || cx as u32 >= width || cy as u32 >= height {
            continue;
        }
        let index = (cy as u32 * width + cx as u32) as usize;
        if seen[index] {
            continue;
        }
        seen[index] = true;
        let current = pixel(buf, width, height, cx, cy);
        if !same(current, target, settings.fill_tolerance) {
            continue;
        }
        let byte = index * 4;
        buf[byte] = ink.r;
        buf[byte + 1] = ink.g;
        buf[byte + 2] = ink.b;
        buf[byte + 3] = 255;
        changed = true;
        stack.push((cx + 1, cy));
        stack.push((cx - 1, cy));
        stack.push((cx, cy + 1));
        stack.push((cx, cy - 1));
    }
    changed
}

fn same(a: Rgba, b: Rgba, tolerance: u8) -> bool {
    a.r.abs_diff(b.r) <= tolerance
        && a.g.abs_diff(b.g) <= tolerance
        && a.b.abs_diff(b.b) <= tolerance
        && a.a.abs_diff(b.a) <= tolerance
}

/// Spacing law used by the warp mask, kept next to the paint dabs.
pub fn mask_spacing(diameter: f32) -> f32 {
    ink_spacing(diameter, Tool::WarpMask)
}

pub fn mask_cover(dist: f32, radius: f32, softness: f32, fringe: f32) -> f32 {
    coverage(dist, radius, softness, fringe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vmi_core::{Edge, Tip};

    #[test]
    fn release_drops_a_raster_path_and_keeps_a_vector_stroke() {
        let mut session = StrokeSession::default();
        session.begin(1.0, 2.0, 1.0);
        session.push(8.0, 2.0, 0.4);
        assert_eq!(session.samples.len(), 2);
        session.end_pixels();
        assert!(session.samples.is_empty());
        assert!(session.carry.is_empty());

        session.begin(1.0, 2.0, 1.0);
        session.push(9.0, 4.0, 1.0);
        let settings = ToolSettings::default_for(Tool::Vector);
        let stroke = session.end_stroke(4, Tool::Vector, &settings).unwrap();
        assert!(session.samples.is_empty());
        assert_eq!(stroke.vertices.len(), 2);
        assert!((stroke.vertices[0].x - 1.0).abs() < 1e-4);
    }

    #[test]
    fn smudge_carry_survives_the_next_segment() {
        let mut buf = vec![0u8; 16 * 8 * 4];
        buf[(4 * 16 + 4) * 4] = 255;
        buf[(4 * 16 + 4) * 4 + 3] = 255;
        let mut settings = ToolSettings::default_for(Tool::Smudge);
        settings.size = 6.0;
        settings.size_min = 1.0;
        settings.softness = 0.0;
        settings.fringe = 0.0;
        settings.edge = Edge::Hard;
        settings.size_pressure = false;
        settings.opacity = 1.0;
        settings.opacity_pressure = false;
        settings.texture = 0.0;
        let mut carry = Carry::default();
        paint_samples(&mut buf, 16, 8, &[(4.0, 4.0, 1.0)], Tool::Smudge, &settings, &mut carry);
        let picked = carry.color.unwrap();
        assert!(picked.r > 0, "{picked:?}");
        paint_samples(&mut buf, 16, 8, &[(4.0, 4.0, 1.0), (12.0, 4.0, 1.0)], Tool::Smudge, &settings, &mut carry);
        assert!(carry.color.unwrap().r > 0, "{carry:?}");
        let carried = pixel(&buf, 16, 8, 10, 4);
        assert!(carried.r > 0, "{carried:?}");
    }

    #[test]
    fn a_smudge_across_two_colors_keeps_both_in_its_hairs() {
        // Top half red, bottom half blue. Drag right along the seam.
        let (w, h) = (48u32, 24u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if y < h / 2 {
                    buf[i] = 255;
                } else {
                    buf[i + 2] = 255;
                }
                buf[i + 3] = 255;
            }
        }
        let mut settings = ToolSettings::default_for(Tool::Smudge);
        settings.size = 12.0;
        settings.size_pressure = false;
        settings.opacity_pressure = false;
        let mut carry = Carry::default();
        paint_samples(&mut buf, w, h, &[(8.0, 12.0, 1.0), (40.0, 12.0, 1.0)], Tool::Smudge, &settings, &mut carry);
        assert_eq!(carry.hairs.len(), SMUDGE_HAIRS);
        let top = carry.hairs[0];
        let bottom = carry.hairs[SMUDGE_HAIRS - 1];
        assert!(top.r > top.b, "top hair {top:?}");
        assert!(bottom.b > bottom.r, "bottom hair {bottom:?}");
        let average = carry.color.unwrap();
        assert!(average.r > 60 && average.b > 60, "{average:?}");
    }

    #[test]
    fn a_textured_paint_dab_is_not_a_flat_disc() {
        let mut flat = vec![0u8; 64 * 64 * 4];
        let mut hairy = flat.clone();
        let mut settings = ToolSettings::default_for(Tool::Paint);
        settings.size = 40.0;
        settings.size_pressure = false;
        settings.opacity_pressure = false;
        settings.texture = 0.0;
        let mut carry = Carry::default();
        paint_samples(&mut flat, 64, 64, &[(32.0, 32.0, 1.0)], Tool::Paint, &settings, &mut carry);
        settings.texture = 1.0;
        settings.tip = Tip::Bristle;
        paint_samples(&mut hairy, 64, 64, &[(32.0, 32.0, 1.0)], Tool::Paint, &settings, &mut carry);
        let alphas = |buf: &[u8]| -> Vec<u8> { buf.chunks(4).map(|p| p[3]).filter(|a| *a > 0).collect() };
        let flat_a = alphas(&flat);
        let hairy_a = alphas(&hairy);
        assert!(!hairy_a.is_empty());
        let weight = |v: &[u8]| v.iter().map(|a| u32::from(*a)).sum::<u32>();
        assert!(weight(&hairy_a) < weight(&flat_a));
        let distinct: std::collections::HashSet<u8> = hairy_a.iter().copied().collect();
        assert!(distinct.len() > 8, "a bristle dab has many partial pixels: {}", distinct.len());
    }

    #[test]
    fn segment_bounds_cover_the_whole_dab() {
        let mut settings = ToolSettings::default_for(Tool::Pen);
        settings.size = 10.0;
        let bounds = segment_bounds(100, 100, (20.0, 20.0, 1.0), (30.0, 25.0, 1.0), Tool::Pen, &settings).unwrap();
        assert!(bounds[0] <= 12 && bounds[1] <= 12, "{bounds:?}");
        assert!(bounds[2] >= 38 && bounds[3] >= 33, "{bounds:?}");
        assert_eq!(segment_bounds(100, 100, (0.0, 0.0, 1.0), (1.0, 1.0, 1.0), Tool::Fill, &settings), Some([0, 0, 100, 100]));
    }

    #[test]
    fn eraser_opens_a_hole_and_fill_stops_at_a_different_color() {
        let mut buf = vec![255u8; 8 * 8 * 4];
        let mut eraser = ToolSettings::default_for(Tool::Eraser);
        eraser.size = 2.0;
        eraser.size_min = 1.0;
        eraser.fringe = 0.0;
        eraser.edge = Edge::Hard;
        eraser.size_pressure = false;
        eraser.opacity_pressure = false;
        let mut carry = Carry::default();
        paint_samples(&mut buf, 8, 8, &[(1.0, 1.0, 1.0)], Tool::Eraser, &eraser, &mut carry);
        assert_eq!(pixel(&buf, 8, 8, 1, 1).a, 0);
        assert_eq!(pixel(&buf, 8, 8, 7, 7).a, 255);

        let mut plate = vec![0u8; 4 * 4 * 4];
        let mut fill = ToolSettings::default_for(Tool::Fill);
        fill.color = [0, 255, 0];
        fill.fill_tolerance = 0;
        assert!(flood_fill(&mut plate, 4, 4, 0, 0, &fill));
        assert_eq!(pixel(&plate, 4, 4, 3, 3).g, 255);
        plate[(0) * 4] = 1;
        plate[3] = 255;
        let before = pixel(&plate, 4, 4, 0, 0);
        flood_fill(&mut plate, 4, 4, 1, 0, &fill);
        assert_eq!(pixel(&plate, 4, 4, 0, 0), before);
    }

    #[test]
    fn a_line_session_keeps_only_the_ends() {
        let mut session = StrokeSession::default();
        session.begin(4.0, 20.0, 1.0);
        session.line_to(40.0, 28.0, 1.0);
        session.line_to(58.0, 44.0, 1.0);
        assert_eq!(session.samples.len(), 2);
        assert_eq!(session.samples[0].0, 4.0);
        assert_eq!(session.samples[1].0, 58.0);
    }
}
