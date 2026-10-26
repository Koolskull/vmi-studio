//! The pen generating engine. A dab is not a disc with a picture on it.
//! Every pixel of every dab asks this module how much of the brush reaches
//! it, and the answer is computed from the seed, the pixel, and the stroke
//! heading. Graphite catches the paper tooth. Bristles streak along the
//! stroke. Spray speckles and piles up. Charcoal breaks at the edge.
//! The same seed gives the same stroke, so a tip is a trait, not a bitmap.

use serde::{Deserialize, Serialize};

use crate::coverage::hash_cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tip {
    #[default]
    Round,
    Pencil,
    Bristle,
    Spray,
    Charcoal,
}

impl Tip {
    pub fn label(self) -> &'static str {
        match self {
            Self::Round => "Round",
            Self::Pencil => "Pencil",
            Self::Bristle => "Bristle",
            Self::Spray => "Spray",
            Self::Charcoal => "Charcoal",
        }
    }

    pub fn usage(self) -> &'static str {
        match self {
            Self::Round => "A plain disc. Texture does nothing.",
            Self::Pencil => "Graphite on paper tooth with fine strands along the stroke.",
            Self::Bristle => "Painted streaks that follow the stroke and shade the color.",
            Self::Spray => "Speckle that piles up the longer the can stays.",
            Self::Charcoal => "Coarse grain that breaks the edge of the stick.",
        }
    }

    pub fn all() -> [Self; 5] {
        [Self::Round, Self::Pencil, Self::Bristle, Self::Spray, Self::Charcoal]
    }
}

/// Everything one dab needs from the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    pub tip: Tip,
    /// 0 is a plain disc, 1 is the full texture.
    pub amount: f32,
    pub seed: u64,
    /// Stroke heading in radians. Bristles and strands run along it.
    pub angle: f32,
    /// Changes per dab so spray lands somewhere new each time.
    pub index: u32,
}

impl Default for Grain {
    fn default() -> Self {
        Self { tip: Tip::Round, amount: 0.0, seed: 1, angle: 0.0, index: 0 }
    }
}

impl Grain {
    pub fn plain() -> Self {
        Self::default()
    }

    pub fn is_plain(self) -> bool {
        self.tip == Tip::Round || self.amount <= 0.0
    }

    /// A dab index from where the dab lands, so two moves never share spray.
    pub fn at(mut self, cx: f32, cy: f32) -> Self {
        let ix = (cx * 16.0).round() as i32;
        let iy = (cy * 16.0).round() as i32;
        self.index = (ix.wrapping_mul(73_856_093) ^ iy.wrapping_mul(19_349_663)) as u32;
        self
    }
}

/// What the engine says about one pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TipSample {
    /// Multiply coverage by this.
    pub mask: f32,
    /// Add this to each color channel, in -1..=1 of full range. Streaks shade.
    pub shade: f32,
}

const FLAT: TipSample = TipSample { mask: 1.0, shade: 0.0 };

/// The engine answer for the pixel at `(x, y)` of a dab centered at `(cx, cy)`.
pub fn tip_sample(grain: Grain, x: i32, y: i32, cx: f32, cy: f32, radius: f32, dist: f32) -> TipSample {
    if grain.is_plain() {
        return FLAT;
    }
    let amount = grain.amount.clamp(0.0, 1.0);
    let seed = seed_part(grain.seed);
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let reach = radius.max(0.5);
    let edge = (dist / reach).clamp(0.0, 1.0);
    let (sin, cos) = grain.angle.sin_cos();
    let dx = px - cx;
    let dy = py - cy;
    let along = dx * cos + dy * sin;
    let across = -dx * sin + dy * cos;
    let raw = match grain.tip {
        Tip::Round => FLAT,
        Tip::Pencil => {
            // Paper tooth lives in the picture, so a second pass darkens the
            // same high spots. Strands run along the heading.
            let tooth = fbm(px * 0.55, py * 0.55, seed, 3);
            let strand = value_noise(across * 0.9, along * 0.08, seed + 11.0);
            let deposit = smoothstep(0.28, 0.72, tooth) * (0.55 + 0.45 * strand);
            TipSample { mask: 0.12 + 0.88 * deposit, shade: 0.0 }
        }
        Tip::Bristle => {
            // Streaks are a function of the across coordinate, so the dabs
            // of one move line up into hairs. A few hairs carry more paint.
            let hair = value_noise(across * 0.42, along * 0.035, seed + 23.0);
            let fine = value_noise(across * 1.7, along * 0.05, seed + 41.0);
            let streak = smoothstep(0.26, 0.62, hair * 0.75 + fine * 0.25);
            let body = 0.18 + 0.82 * streak;
            // Ragged tip: the outer rim thins where the hairs are weak.
            let rim = smoothstep(1.0, 0.72, edge * (1.1 - 0.5 * streak));
            TipSample { mask: body * rim.max(0.0), shade: (hair - 0.5) * 0.36 }
        }
        Tip::Spray => {
            // Speckle. Each dab lands its own set of grains, and the middle
            // of the cone is denser than the rim.
            let cell = if reach > 18.0 { 2 } else { 1 };
            let gx = x.div_euclid(cell);
            let gy = y.div_euclid(cell);
            let roll = hash_cell(gx, gy, (grain.index as i32).wrapping_add(grain.seed as i32));
            let keep = 0.05 + 0.34 * (1.0 - edge) * (1.0 - edge);
            if roll < keep {
                TipSample { mask: 1.0, shade: (hash_cell(gy, gx, grain.seed as i32) - 0.5) * 0.12 }
            } else {
                TipSample { mask: 0.0, shade: 0.0 }
            }
        }
        Tip::Charcoal => {
            // Coarse grain in picture space. The edge needs a higher spot to
            // take any black, so the stick breaks up instead of fading out.
            let coarse = fbm(px * 0.22, py * 0.22, seed + 5.0, 3);
            let tooth = value_noise(px * 1.3, py * 1.3, seed + 17.0);
            let threshold = 0.3 + 0.42 * edge * edge;
            let hit = smoothstep(threshold - 0.16, threshold + 0.16, coarse);
            let mask = hit * (0.55 + 0.45 * tooth);
            TipSample { mask, shade: -(1.0 - tooth) * 0.2 }
        }
    };
    TipSample {
        mask: 1.0 - amount * (1.0 - raw.mask.clamp(0.0, 1.0)),
        shade: raw.shade * amount,
    }
}

fn seed_part(seed: u64) -> f32 {
    (seed % 4096) as f32 * 7.31
}

fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    if (high - low).abs() < 1e-6 {
        return if value >= high { 1.0 } else { 0.0 };
    }
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fade(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

/// Value noise in 0..=1 at the given frequency. `z` is the seed plane.
pub fn value_noise(x: f32, y: f32, z: f32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let iz = z.floor() as i32;
    let fx = fade(x - ix as f32);
    let fy = fade(y - iy as f32);
    let fz = fade(z - iz as f32);
    let at = |dx: i32, dy: i32, dz: i32| hash_cell(ix + dx, iy + dy, iz + dz);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let front = lerp(lerp(at(0, 0, 0), at(1, 0, 0), fx), lerp(at(0, 1, 0), at(1, 1, 0), fx), fy);
    let back = lerp(lerp(at(0, 0, 1), at(1, 0, 1), fx), lerp(at(0, 1, 1), at(1, 1, 1), fx), fy);
    lerp(front, back, fz)
}

/// Stacked octaves, normalized back to 0..=1.
pub fn fbm(x: f32, y: f32, z: f32, octaves: u32) -> f32 {
    let mut sum = 0.0;
    let mut weight = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    for octave in 0..octaves.max(1) {
        sum += value_noise(x * freq, y * freq, z + octave as f32 * 3.7) * amp;
        weight += amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    sum / weight
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mean_mask(tip: Tip, seed: u64) -> f32 {
        let grain = Grain { tip, amount: 1.0, seed, angle: 0.0, index: 3 };
        let mut total = 0.0;
        let mut count = 0.0;
        for y in 0..40 {
            for x in 0..40 {
                let dist = ((x as f32 + 0.5) - 20.0).hypot((y as f32 + 0.5) - 20.0);
                if dist > 18.0 {
                    continue;
                }
                total += tip_sample(grain, x, y, 20.0, 20.0, 18.0, dist).mask;
                count += 1.0;
            }
        }
        total / count
    }

    #[test]
    fn a_round_tip_or_zero_texture_leaves_the_disc_alone() {
        let plain = Grain::plain();
        assert_eq!(tip_sample(plain, 3, 4, 3.5, 4.5, 6.0, 0.0), FLAT);
        let off = Grain { tip: Tip::Charcoal, amount: 0.0, ..Grain::plain() };
        assert_eq!(tip_sample(off, 3, 4, 3.5, 4.5, 6.0, 0.0), FLAT);
    }

    #[test]
    fn every_textured_tip_takes_some_paint_away_and_keeps_some() {
        for tip in [Tip::Pencil, Tip::Bristle, Tip::Spray, Tip::Charcoal] {
            let mean = mean_mask(tip, 7);
            assert!(mean > 0.05 && mean < 0.95, "{tip:?} {mean}");
        }
    }

    #[test]
    fn the_same_seed_repeats_and_a_new_seed_differs() {
        let a = Grain { tip: Tip::Bristle, amount: 1.0, seed: 4, angle: 0.3, index: 1 };
        let b = Grain { seed: 9, ..a };
        let mut same = true;
        let mut differs = false;
        for y in 0..24 {
            for x in 0..24 {
                let dist = ((x as f32 + 0.5) - 12.0).hypot((y as f32 + 0.5) - 12.0);
                let first = tip_sample(a, x, y, 12.0, 12.0, 11.0, dist);
                let again = tip_sample(a, x, y, 12.0, 12.0, 11.0, dist);
                same &= first == again;
                differs |= first != tip_sample(b, x, y, 12.0, 12.0, 11.0, dist);
            }
        }
        assert!(same);
        assert!(differs);
    }

    #[test]
    fn spray_lands_on_different_grains_each_dab() {
        let first = Grain { tip: Tip::Spray, amount: 1.0, seed: 2, angle: 0.0, index: 0 }.at(10.0, 10.0);
        let second = first.at(10.5, 10.0);
        assert_ne!(first.index, second.index);
        let mut moved = false;
        for y in 0..20 {
            for x in 0..20 {
                let dist = ((x as f32 + 0.5) - 10.0).hypot((y as f32 + 0.5) - 10.0);
                moved |= tip_sample(first, x, y, 10.0, 10.0, 9.0, dist).mask
                    != tip_sample(second, x, y, 10.0, 10.0, 9.0, dist).mask;
            }
        }
        assert!(moved);
    }

    #[test]
    fn bristle_hairs_line_up_along_the_heading() {
        // Two dabs on one horizontal move share the across coordinate, so the
        // mask on the shared row agrees far more than on a random row.
        let grain = Grain { tip: Tip::Bristle, amount: 1.0, seed: 3, angle: 0.0, index: 0 };
        let mut agree = 0.0;
        let mut total = 0.0;
        for y in 0..30 {
            let dist_a = ((15.5) - 15.0_f32).hypot((y as f32 + 0.5) - 15.0);
            let dist_b = ((16.5) - 16.0_f32).hypot((y as f32 + 0.5) - 15.0);
            if dist_a > 14.0 {
                continue;
            }
            let a = tip_sample(grain, 15, y, 15.0, 15.0, 14.0, dist_a).mask;
            let b = tip_sample(grain, 16, y, 16.0, 15.0, 14.0, dist_b).mask;
            agree += 1.0 - (a - b).abs();
            total += 1.0;
        }
        assert!(agree / total > 0.9, "{}", agree / total);
    }

    #[test]
    fn charcoal_thins_toward_the_edge() {
        let grain = Grain { tip: Tip::Charcoal, amount: 1.0, seed: 5, angle: 0.0, index: 0 };
        let mut inner = 0.0;
        let mut outer = 0.0;
        let mut inner_n = 0.0;
        let mut outer_n = 0.0;
        for y in 0..60 {
            for x in 0..60 {
                let dist = ((x as f32 + 0.5) - 30.0).hypot((y as f32 + 0.5) - 30.0);
                if dist > 28.0 {
                    continue;
                }
                let mask = tip_sample(grain, x, y, 30.0, 30.0, 28.0, dist).mask;
                if dist < 10.0 {
                    inner += mask;
                    inner_n += 1.0;
                } else if dist > 22.0 {
                    outer += mask;
                    outer_n += 1.0;
                }
            }
        }
        assert!(inner / inner_n > outer / outer_n);
    }
}
