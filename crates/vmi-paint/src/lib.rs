//! Raster ink. A stroke session keeps the smudge color until the pen lifts.
//! Release either drops the path or returns an editable vector stroke.

pub use vmi_core::stroke_steps;

use vmi_core::{coverage, ink_spacing, pixel, stamp, Rgba, Stamp, StampMode, Tool, ToolSettings, VectorStroke};

/// One gesture. `carry` survives every move and dies on release.
#[derive(Clone, Debug, Default)]
pub struct StrokeSession {
    pub samples: Vec<(f32, f32, f32)>,
    pub carry: Option<Rgba>,
}

impl StrokeSession {
    pub fn begin(&mut self, x: f32, y: f32, pressure: f32) {
        self.samples.clear();
        self.carry = None;
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
        self.carry = None;
    }

    pub fn end_stroke(&mut self, id: u64, tool: Tool, settings: &ToolSettings) -> Option<VectorStroke> {
        let points = std::mem::take(&mut self.samples);
        self.carry = None;
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
    let mut carry = None;
    paint_samples(buf, width, height, samples, tool, settings, &mut carry);
}

pub fn paint_samples(
    buf: &mut [u8],
    width: u32,
    height: u32,
    samples: &[(f32, f32, f32)],
    tool: Tool,
    settings: &ToolSettings,
    carry: &mut Option<Rgba>,
) -> bool {
    if samples.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut previous = samples[0];
    changed |= dab_at(buf, width, height, previous, tool, settings, carry);
    for sample in samples.iter().skip(1) {
        let spacing = ink_spacing(settings.diameter_for(previous.2.max(sample.2)), tool);
        let mut steps = stroke_steps(previous.0, previous.1, previous.2, sample.0, sample.1, sample.2, spacing);
        if steps.last().map(|step| (step.0 - sample.0).abs() > 0.01 || (step.1 - sample.1).abs() > 0.01).unwrap_or(true) {
            steps.push(*sample);
        }
        for step in steps {
            changed |= dab_at(buf, width, height, step, tool, settings, carry);
        }
        previous = *sample;
    }
    changed
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
    carry: &mut Option<Rgba>,
) -> bool {
    let spacing = ink_spacing(settings.diameter_for(from.2.max(to.2)), tool);
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
        changed |= dab_at(buf, width, height, step, tool, settings, carry);
    }
    changed
}

fn dab_at(
    buf: &mut [u8],
    width: u32,
    height: u32,
    sample: (f32, f32, f32),
    tool: Tool,
    settings: &ToolSettings,
    carry: &mut Option<Rgba>,
) -> bool {
    let (x, y, pressure) = sample;
    if tool == Tool::Fill {
        return flood_fill(buf, width, height, x.round() as i32, y.round() as i32, settings);
    }
    let radius = settings.radius_for(pressure);
    let opacity = settings.opacity_for(pressure);
    let mode = match tool {
        Tool::Eraser => StampMode::Erase,
        Tool::Blend => StampMode::Blend,
        _ => StampMode::Over,
    };
    let mut color = Rgba::rgb(settings.color[0], settings.color[1], settings.color[2]);
    if tool == Tool::Smudge {
        // Pick up what is under the whole disc, keep most of what the pen already
        // carries, and lay that down. The trail fades as the carry thins out.
        let under = disc_average(buf, width, height, x, y, radius);
        let mixed = match *carry {
            Some(held) => mix_toward(held, under, 0.3),
            None => under,
        };
        *carry = Some(mixed);
        if mixed.a == 0 {
            return false;
        }
        color = mixed;
    } else if tool == Tool::Blend {
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
    })
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
    use vmi_core::Edge;

    #[test]
    fn release_drops_a_raster_path_and_keeps_a_vector_stroke() {
        let mut session = StrokeSession::default();
        session.begin(1.0, 2.0, 1.0);
        session.push(8.0, 2.0, 0.4);
        assert_eq!(session.samples.len(), 2);
        session.end_pixels();
        assert!(session.samples.is_empty());
        assert!(session.carry.is_none());

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
        let mut carry = None;
        paint_samples(&mut buf, 16, 8, &[(4.0, 4.0, 1.0)], Tool::Smudge, &settings, &mut carry);
        let picked = carry.unwrap();
        assert!(picked.r > 0, "{picked:?}");
        paint_samples(&mut buf, 16, 8, &[(4.0, 4.0, 1.0), (12.0, 4.0, 1.0)], Tool::Smudge, &settings, &mut carry);
        assert!(carry.unwrap().r > 0, "{carry:?}");
        let carried = pixel(&buf, 16, 8, 10, 4);
        assert!(carried.r > 0, "{carried:?}");
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
        let mut carry = None;
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
