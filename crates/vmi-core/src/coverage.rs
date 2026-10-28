//! Disc coverage from `vmi_studio/paint.py`, plus the KOOLDRAW Bayer gate.
//!
//! A pixel is sampled at its center. `fringe` is the anti-alias width.
//! `0` is a hard edge. `0.5` is the old checked AA button.

use serde::{Deserialize, Serialize};

use crate::color::{rint_u8, Rgba};
use crate::tip::{tip_sample, Grain};

pub const TONE: [[u8; 3]; 3] = [[0, 0, 0], [0x44, 0x44, 0x44], [0xCC, 0xCC, 0xCC]];

const BAYER: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

const BAYER8: [[u32; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// How a dither edge breaks cover into ink. Bayer is the original 4×4 gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DitherPattern {
    #[default]
    Bayer,
    Bayer8,
    Checker,
    Lines,
    Dots,
}

impl DitherPattern {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bayer => "Bayer",
            Self::Bayer8 => "Bayer 8",
            Self::Checker => "Checker",
            Self::Lines => "Lines",
            Self::Dots => "Dots",
        }
    }

    pub fn letter(self) -> &'static str {
        match self {
            Self::Bayer => "B",
            Self::Bayer8 => "8",
            Self::Checker => "C",
            Self::Lines => "L",
            Self::Dots => "D",
        }
    }

    pub fn all() -> [Self; 5] {
        [
            Self::Bayer,
            Self::Bayer8,
            Self::Checker,
            Self::Lines,
            Self::Dots,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    Smooth,
    Hard,
    Dither,
}

impl Edge {
    pub fn label(self) -> &'static str {
        match self {
            Self::Smooth => "Smooth",
            Self::Hard => "Hard",
            Self::Dither => "Dither",
        }
    }

    pub fn all() -> [Self; 3] {
        [Self::Smooth, Self::Hard, Self::Dither]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampMode {
    Over,
    Erase,
    Blend,
}

/// Piecewise linear. `t` and the stored values are clamped to 0..=1.
pub fn curve_value(curve: &[(f32, f32)], t: f32) -> f32 {
    let amount = t.clamp(0.0, 1.0);
    let mut points = if curve.is_empty() {
        vec![(0.0, 0.0), (1.0, 1.0)]
    } else {
        curve.to_vec()
    };
    points.sort_by(|left, right| left.0.total_cmp(&right.0));
    let clamp01 = |value: f32| value.clamp(0.0, 1.0);
    if amount <= points[0].0 {
        return clamp01(points[0].1);
    }
    for pair in points.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if amount <= right.0 {
            let span = right.0 - left.0;
            let mix = if span <= 0.0 {
                0.0
            } else {
                (amount - left.0) / span
            };
            return clamp01(left.1 + (right.1 - left.1) * mix);
        }
    }
    clamp01(points[points.len() - 1].1)
}

/// Coverage at `dist` from the dab center. `radius` is the brush radius.
pub fn coverage(dist: f32, radius: f32, softness: f32, fringe: f32) -> f32 {
    let edge = radius.max(0.5);
    let softness = softness.clamp(0.0, 1.0);
    let fringe = if fringe.is_finite() && fringe > 0.0 {
        fringe
    } else {
        0.0
    };
    if softness <= 0.0 && fringe <= 0.0 {
        return if dist <= edge { 1.0 } else { 0.0 };
    }
    if softness <= 0.0 {
        return (edge + fringe - dist).clamp(0.0, 1.0);
    }
    let inner = edge * (1.0 - softness);
    let span = (edge - inner).max(0.001);
    let cover = ((edge - dist) / span).clamp(0.0, 1.0);
    if fringe <= 0.0 {
        if cover >= 0.5 {
            1.0
        } else {
            0.0
        }
    } else {
        cover
    }
}

pub fn bayer4(x: i32, y: i32) -> u32 {
    let step = chunk_index(x, y, 1);
    BAYER[step.1 as usize][step.0 as usize]
}

fn chunk_index(x: i32, y: i32, chunk: u32) -> (u32, u32) {
    let step = chunk.max(1) as i32;
    let cx = x.div_euclid(step);
    let cy = y.div_euclid(step);
    (cx.rem_euclid(4) as u32, cy.rem_euclid(4) as u32)
}

/// The ordered threshold at this pixel, 0..=1. `bias` slides it: negative keeps
/// more ink, positive keeps less. Zero bias on Bayer is the original gate.
pub fn dither_gate(x: i32, y: i32, chunk: u32, pattern: DitherPattern, bias: f32) -> f32 {
    let step = chunk.max(1) as i32;
    let limit = match pattern {
        DitherPattern::Bayer => {
            let (bx, by) = chunk_index(x, y, chunk);
            (BAYER[by as usize][bx as usize] as f32 + 0.5) / 16.0
        }
        DitherPattern::Bayer8 => {
            let bx = x.div_euclid(step).rem_euclid(8) as usize;
            let by = y.div_euclid(step).rem_euclid(8) as usize;
            (BAYER8[by][bx] as f32 + 0.5) / 64.0
        }
        DitherPattern::Checker => {
            if (x.div_euclid(step) + y.div_euclid(step)) & 1 == 0 {
                0.25
            } else {
                0.75
            }
        }
        DitherPattern::Lines => (y.div_euclid(step).rem_euclid(4) as f32 + 0.5) / 4.0,
        DitherPattern::Dots => {
            let cx = x.div_euclid(step);
            let cy = y.div_euclid(step);
            let fx = (x - cx * step) as f32 + 0.5;
            let fy = (y - cy * step) as f32 + 0.5;
            let mid = step as f32 * 0.5;
            (fx - mid).hypot(fy - mid) / mid.max(0.5)
        }
    };
    (limit + bias.clamp(-1.0, 1.0) * 0.45).clamp(0.02, 0.98)
}

/// 0, 1, or 2. The load-gate tones: black, `#444444`, `#CCCCCC`.
pub fn tone_level(cover: f32, x: i32, y: i32, chunk: u32, pattern: DitherPattern, bias: f32) -> u8 {
    let limit = dither_gate(x, y, chunk, pattern, bias);
    let scaled = cover.clamp(0.0, 1.0) * 2.0;
    let mut level = scaled.floor();
    if scaled - level > limit {
        level += 1.0;
    }
    level.clamp(0.0, 2.0) as u8
}

pub fn dither_keeps(
    cover: f32,
    x: i32,
    y: i32,
    chunk: u32,
    pattern: DitherPattern,
    bias: f32,
) -> bool {
    cover > dither_gate(x, y, chunk, pattern, bias)
}

pub fn stroke_steps(
    x0: f32,
    y0: f32,
    p0: f32,
    x1: f32,
    y1: f32,
    p1: f32,
    spacing: f32,
) -> Vec<(f32, f32, f32)> {
    let distance = (x1 - x0).hypot(y1 - y0);
    // Spacing is already inside the brush. The floor only stops a zero step.
    let step = spacing.max(0.15);
    if distance < step {
        return Vec::new();
    }
    let count = (distance / step) as i32;
    if count <= 0 {
        return Vec::new();
    }
    let mut points = Vec::with_capacity(count as usize);
    for index in 1..=count {
        let mix = ((index as f32) * step / distance).min(1.0);
        points.push((
            x0 + (x1 - x0) * mix,
            y0 + (y1 - y0) * mix,
            p0 + (p1 - p0) * mix,
        ));
    }
    points
}

#[derive(Clone, Copy, Debug)]
pub struct Stamp {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    pub softness: f32,
    pub fringe: f32,
    pub opacity: f32,
    pub color: Rgba,
    pub edge: Edge,
    pub chunk: u32,
    pub tones: bool,
    pub pattern: DitherPattern,
    pub bias: f32,
    pub mode: StampMode,
    /// The generating engine. `Grain::plain()` is a bare disc.
    pub grain: Grain,
}

impl Stamp {
    pub fn fringe_used(self) -> f32 {
        if self.edge == Edge::Hard {
            0.0
        } else {
            self.fringe.max(0.0)
        }
    }
}

/// Stamp one disc. Returns whether a pixel changed.
pub fn stamp(buf: &mut [u8], width: u32, height: u32, dab: Stamp) -> bool {
    if width == 0 || height == 0 || dab.radius < 0.5 || dab.opacity <= 0.0 {
        return false;
    }
    let fringe = dab.fringe_used();
    let softness = dab.softness.clamp(0.0, 1.0);
    let pad = if fringe > 0.0 || softness > 0.0 {
        fringe.max(1.0)
    } else {
        0.0
    };
    let reach = dab.radius + pad;
    let x0 = (dab.cx - reach).floor().max(0.0) as i32;
    let y0 = (dab.cy - reach).floor().max(0.0) as i32;
    let x1 = ((dab.cx + reach).ceil() as i32 + 1).min(width as i32);
    let y1 = ((dab.cy + reach).ceil() as i32 + 1).min(height as i32);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    let mut changed = false;
    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((x as f32 + 0.5) - dab.cx).hypot((y as f32 + 0.5) - dab.cy);
            let mut cover =
                coverage(dist, dab.radius, softness, fringe) * dab.opacity.clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let mut color = dab.color;
            if !dab.grain.is_plain() {
                let sample = tip_sample(dab.grain, x, y, dab.cx, dab.cy, dab.radius, dist);
                cover *= sample.mask;
                if cover <= 0.001 {
                    continue;
                }
                if sample.shade != 0.0 && dab.mode == StampMode::Over {
                    color = shade(color, sample.shade);
                }
            }
            if dab.edge == Edge::Dither {
                if dab.tones {
                    let level = tone_level(cover, x, y, dab.chunk, dab.pattern, dab.bias);
                    let tone = TONE[level as usize];
                    color = Rgba::rgb(tone[0], tone[1], tone[2]);
                    cover = 1.0;
                } else if dither_keeps(cover, x, y, dab.chunk, dab.pattern, dab.bias) {
                    cover = 1.0;
                } else {
                    continue;
                }
            }
            let index = ((y as u32 * width + x as u32) * 4) as usize;
            if write_pixel(&mut buf[index..index + 4], color, cover, dab.mode) {
                changed = true;
            }
        }
    }
    changed
}

/// Push every channel by `amount` of full range. Streaks shade the paint.
pub fn shade(color: Rgba, amount: f32) -> Rgba {
    let shift = amount.clamp(-1.0, 1.0) * 255.0;
    let ch = |value: u8| rint_u8((f32::from(value) + shift).clamp(0.0, 255.0));
    Rgba {
        r: ch(color.r),
        g: ch(color.g),
        b: ch(color.b),
        a: color.a,
    }
}

fn write_pixel(pixel: &mut [u8], color: Rgba, cover: f32, mode: StampMode) -> bool {
    let before = [pixel[0], pixel[1], pixel[2], pixel[3]];
    match mode {
        StampMode::Erase => {
            let alpha = rint_u8(before[3] as f32 * (1.0 - cover));
            pixel[3] = alpha;
        }
        StampMode::Blend => {
            // `color` is the average already under the disc. Every pixel moves toward it.
            let dst = Rgba::new(before[0], before[1], before[2], before[3]);
            if dst.a == 0 && color.a == 0 {
                return false;
            }
            let out = lerp_rgba(dst, color, cover * 0.5);
            pixel.copy_from_slice(&out.channels());
        }
        StampMode::Over => {
            let out = source_over(
                Rgba::new(before[0], before[1], before[2], before[3]),
                color,
                cover,
            );
            pixel.copy_from_slice(&out.channels());
        }
    }
    pixel != before
}

pub fn source_over(dst: Rgba, src: Rgba, cover: f32) -> Rgba {
    let src_a = cover.clamp(0.0, 1.0) * (f32::from(src.a) / 255.0);
    if src_a <= 0.0 {
        return dst;
    }
    let dst_a = f32::from(dst.a) / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    let channel = |value: u8, dest: u8| -> u8 {
        if out_a <= 0.0 {
            dest
        } else {
            rint_u8((f32::from(value) * src_a + f32::from(dest) * dst_a * (1.0 - src_a)) / out_a)
        }
    };
    Rgba {
        r: channel(src.r, dst.r),
        g: channel(src.g, dst.g),
        b: channel(src.b, dst.b),
        a: rint_u8(out_a * 255.0),
    }
}

pub fn mix_half(a: Rgba, b: Rgba) -> Rgba {
    let ch = |p: u8, q: u8| ((u16::from(p) + u16::from(q)) / 2) as u8;
    Rgba {
        r: ch(a.r, b.r),
        g: ch(a.g, b.g),
        b: ch(a.b, b.b),
        a: ch(a.a, b.a),
    }
}

fn lerp_rgba(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let ch = |p: u8, q: u8| rint_u8(f32::from(p) + (f32::from(q) - f32::from(p)) * t);
    Rgba {
        r: ch(a.r, b.r),
        g: ch(a.g, b.g),
        b: ch(a.b, b.b),
        a: ch(a.a, b.a),
    }
}

pub fn pixel(buf: &[u8], width: u32, height: u32, x: i32, y: i32) -> Rgba {
    if x < 0 || y < 0 || x as u32 >= width || y as u32 >= height {
        return Rgba::CLEAR;
    }
    let index = ((y as u32 * width + x as u32) * 4) as usize;
    Rgba::new(buf[index], buf[index + 1], buf[index + 2], buf[index + 3])
}

fn fade(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

pub(crate) fn hash_cell(ix: i32, iy: i32, iz: i32) -> f32 {
    let mut n = (i64::from(ix).wrapping_mul(374761393)
        + i64::from(iy).wrapping_mul(668265263)
        + i64::from(iz).wrapping_mul(1440673973)) as u32;
    n ^= n >> 13;
    n = n.wrapping_mul(1274126177);
    (n & 255) as f32 / 255.0
}

fn noise(x: f32, y: f32, z: f32) -> f32 {
    let x = x * 0.13;
    let y = y * 0.13;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let iz = z.floor() as i32;
    let fx = fade(x - ix as f32);
    let fy = fade(y - iy as f32);
    let fz = fade(z - iz as f32);
    let at = |dx: i32, dy: i32, dz: i32| hash_cell(ix + dx, iy + dy, iz + dz);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let front = lerp(
        lerp(at(0, 0, 0), at(1, 0, 0), fx),
        lerp(at(0, 1, 0), at(1, 1, 0), fx),
        fy,
    );
    let back = lerp(
        lerp(at(0, 0, 1), at(1, 0, 1), fx),
        lerp(at(0, 1, 1), at(1, 1, 1), fx),
        fy,
    );
    lerp(front, back, fz)
}

/// One load-gate cell. 0 is black, 1 is `#444444`, 2 is `#CCCCCC`.
pub fn liquid_level(tick: u32, x: i32, y: i32) -> u8 {
    let t = tick as f32 * 0.17;
    let xf = x as f32;
    let yf = y as f32;
    let wave_y = 10.0 * (yf * 0.11 + t).sin();
    let u = xf + wave_y;
    let v = yf + 8.0 * (xf * 0.09 - t * 1.2).sin();
    let sample = noise(u, v, t * 2.0);
    let limit = (bayer4(x, y) as f32 + 0.5) / 16.0;
    let scaled = sample * 2.0;
    let mut level = scaled.floor();
    if scaled - level > limit {
        level += 1.0;
    }
    level.clamp(0.0, 2.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(buf: &[u8], width: u32, y: u32) -> Vec<u8> {
        (0..width)
            .map(|x| buf[((y * width + x) * 4 + 3) as usize])
            .collect()
    }

    #[test]
    fn hard_and_smooth_discs_match_the_python_dab() {
        let mut hard = vec![0u8; 24 * 24 * 4];
        let mut soft = vec![0u8; 24 * 24 * 4];
        let color = Rgba::rgb(255, 255, 255);
        stamp(
            &mut hard,
            24,
            24,
            Stamp {
                cx: 12.0,
                cy: 12.0,
                radius: 4.0,
                softness: 0.0,
                fringe: 0.0,
                opacity: 1.0,
                color,
                edge: Edge::Hard,
                chunk: 1,
                tones: false,
                pattern: DitherPattern::Bayer,
                bias: 0.0,
                mode: StampMode::Over,
                grain: Grain::plain(),
            },
        );
        stamp(
            &mut soft,
            24,
            24,
            Stamp {
                cx: 12.0,
                cy: 12.0,
                radius: 4.0,
                softness: 0.0,
                fringe: 0.5,
                opacity: 1.0,
                color,
                edge: Edge::Smooth,
                chunk: 1,
                tones: false,
                pattern: DitherPattern::Bayer,
                bias: 0.0,
                mode: StampMode::Over,
                grain: Grain::plain(),
            },
        );
        assert_eq!(
            row(&hard, 24, 12),
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
                0, 0
            ]
        );
        assert_eq!(
            row(&soft, 24, 12),
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 246, 255, 255, 255, 255, 255, 255, 246, 0, 0, 0, 0, 0, 0,
                0, 0
            ]
        );
    }

    #[test]
    fn bayer_at_zero_bias_is_the_original_gate_and_a_pattern_can_disagree() {
        assert!((dither_gate(0, 0, 1, DitherPattern::Bayer, 0.0) - 0.5 / 16.0).abs() < 1e-5);
        assert!(dither_keeps(0.5, 0, 0, 1, DitherPattern::Bayer, 0.0));
        assert!(!dither_keeps(0.5, 1, 0, 1, DitherPattern::Bayer, 0.0));
        let bayer = dither_keeps(0.4, 1, 0, 1, DitherPattern::Bayer, 0.0);
        let dots = dither_keeps(0.4, 1, 0, 1, DitherPattern::Dots, 0.0);
        assert_ne!(bayer, dots);
        assert!(
            dither_gate(0, 0, 1, DitherPattern::Bayer, -1.0)
                < dither_gate(0, 0, 1, DitherPattern::Bayer, 1.0)
        );
    }

    #[test]
    fn dither_is_ink_or_clear() {
        let mut buf = vec![0u8; 32 * 32 * 4];
        stamp(
            &mut buf,
            32,
            32,
            Stamp {
                cx: 16.0,
                cy: 16.0,
                radius: 10.0,
                softness: 0.8,
                fringe: 1.0,
                opacity: 1.0,
                color: Rgba::rgb(255, 0, 0),
                edge: Edge::Dither,
                chunk: 2,
                tones: false,
                pattern: DitherPattern::Bayer,
                bias: 0.0,
                mode: StampMode::Over,
                grain: Grain::plain(),
            },
        );
        for pixel in buf.chunks(4) {
            assert!(pixel[3] == 0 || pixel[3] == 255, "{pixel:?}");
            if pixel[3] == 255 {
                assert_eq!(&pixel[..3], &[255, 0, 0]);
            }
        }
    }

    #[test]
    fn the_curve_bends_in_the_middle() {
        assert!((curve_value(&[(0.0, 0.0), (0.5, 0.0), (1.0, 1.0)], 0.75) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn liquid_levels_stay_in_the_three_tones() {
        for y in 0..8 {
            for x in 0..8 {
                assert!(liquid_level(3, x, y) <= 2);
            }
        }
    }
}
