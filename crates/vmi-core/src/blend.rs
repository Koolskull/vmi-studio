//! Layer blend. Normal is source-over. The other names follow the studio list.

use crate::color::{rint_u8, Rgba};
use crate::coverage::source_over;

pub const BLENDS: &[&str] = &[
    "normal",
    "pass through",
    "darken",
    "multiply",
    "color burn",
    "linear burn",
    "subtract",
    "darker color",
    "lighten",
    "screen",
    "color dodge",
    "glow dodge",
    "add",
    "add (glow)",
    "lighter color",
    "overlay",
    "soft light",
    "hard light",
    "vivid light",
    "linear light",
    "pin light",
    "hard mix",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
    "divide",
];

pub fn blend_label(name: &str) -> &str {
    match name {
        "pass through" => "Pass through",
        "color burn" => "Color burn",
        "linear burn" => "Linear burn",
        "darker color" => "Darker color",
        "color dodge" => "Color dodge",
        "glow dodge" => "Glow dodge",
        "add (glow)" => "Add (glow)",
        "lighter color" => "Lighter color",
        "soft light" => "Soft light",
        "hard light" => "Hard light",
        "vivid light" => "Vivid light",
        "linear light" => "Linear light",
        "pin light" => "Pin light",
        "hard mix" => "Hard mix",
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(_first) => {
                    let rest: String = chars.collect();
                    // Leak-free label for the known list. Unknown stays lower.
                    let _ = rest;
                    match other {
                        "normal" => "Normal",
                        "darken" => "Darken",
                        "multiply" => "Multiply",
                        "subtract" => "Subtract",
                        "lighten" => "Lighten",
                        "screen" => "Screen",
                        "add" => "Add",
                        "overlay" => "Overlay",
                        "difference" => "Difference",
                        "exclusion" => "Exclusion",
                        "hue" => "Hue",
                        "saturation" => "Saturation",
                        "color" => "Color",
                        "luminosity" => "Luminosity",
                        "divide" => "Divide",
                        _ => "Normal",
                    }
                }
                None => "Normal",
            }
        }
    }
}

pub fn mix(dst: Rgba, src: Rgba, mode: &str, opacity: u8) -> Rgba {
    if src.a == 0 || opacity == 0 {
        return dst;
    }
    let src = scale_alpha(src, opacity);
    if mode == "normal" || mode == "pass through" || !BLENDS.contains(&mode) {
        return source_over(dst, src, 1.0);
    }
    let glow = mode == "add (glow)" || mode == "glow dodge";
    let src_a = f32::from(src.a) / 255.0;
    let dst_a = f32::from(dst.a) / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return dst;
    }
    let channel = |dc: u8, sc: u8| -> u8 {
        let d = f32::from(dc) / 255.0;
        let s = f32::from(sc) / 255.0;
        let mixed = blend_channel(d, s, mode).clamp(0.0, 1.0);
        let boosted = if glow { (mixed + (1.0 - src_a) * 0.25).clamp(0.0, 1.0) } else { mixed };
        let out = (boosted * src_a + d * dst_a * (1.0 - src_a)) / out_a;
        rint_u8(out * 255.0)
    };
    Rgba {
        r: channel(dst.r, src.r),
        g: channel(dst.g, src.g),
        b: channel(dst.b, src.b),
        a: rint_u8(out_a * 255.0),
    }
}

fn scale_alpha(pixel: Rgba, opacity: u8) -> Rgba {
    if opacity == 255 {
        return pixel;
    }
    let a = (u16::from(pixel.a) * u16::from(opacity) / 255) as u8;
    Rgba { a, ..pixel }
}

fn blend_channel(d: f32, s: f32, mode: &str) -> f32 {
    match mode {
        "darken" => d.min(s),
        "multiply" => d * s,
        "color burn" => {
            if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - d) / s).min(1.0) }
        }
        "linear burn" => (d + s - 1.0).max(0.0),
        "subtract" => (d - s).max(0.0),
        "darker color" => d.min(s),
        "lighten" => d.max(s),
        "screen" => 1.0 - (1.0 - d) * (1.0 - s),
        "color dodge" | "glow dodge" => {
            if s >= 1.0 { 1.0 } else { (d / (1.0 - s)).min(1.0) }
        }
        "add" | "add (glow)" => (d + s).min(1.0),
        "lighter color" => d.max(s),
        "overlay" => overlay(d, s),
        "soft light" => {
            if s <= 0.5 {
                d - (1.0 - 2.0 * s) * d * (1.0 - d)
            } else {
                let g = if d <= 0.25 { ((16.0 * d - 12.0) * d + 4.0) * d } else { d.sqrt() };
                d + (2.0 * s - 1.0) * (g - d)
            }
        }
        "hard light" => overlay(s, d),
        "vivid light" => {
            if s <= 0.5 {
                blend_channel(d, (s * 2.0).clamp(0.0, 1.0), "color burn")
            } else {
                blend_channel(d, (2.0 * s - 1.0).clamp(0.0, 1.0), "color dodge")
            }
        }
        "linear light" => (d + 2.0 * s - 1.0).clamp(0.0, 1.0),
        "pin light" => {
            if s <= 0.5 { d.min(2.0 * s) } else { d.max(2.0 * s - 1.0) }
        }
        "hard mix" => {
            if d + s >= 1.0 { 1.0 } else { 0.0 }
        }
        "difference" => (d - s).abs(),
        "exclusion" => d + s - 2.0 * d * s,
        "divide" => {
            if s <= 0.0 { 1.0 } else { (d / s).min(1.0) }
        }
        "hue" | "saturation" | "color" | "luminosity" => d,
        _ => s,
    }
}

fn overlay(d: f32, s: f32) -> f32 {
    if d <= 0.5 { 2.0 * d * s } else { 1.0 - 2.0 * (1.0 - d) * (1.0 - s) }
}

/// Hue, saturation, color, and luminosity use the destination channel above
/// only as a fallback inside `blend_channel`. The pixel mix replaces RGB.
pub fn mix_hsl(dst: Rgba, src: Rgba, mode: &str, opacity: u8) -> Rgba {
    if !matches!(mode, "hue" | "saturation" | "color" | "luminosity") {
        return mix(dst, src, mode, opacity);
    }
    let src = scale_alpha(src, opacity);
    let mut base = rgb_to_hsl(dst.r, dst.g, dst.b);
    let ink = rgb_to_hsl(src.r, src.g, src.b);
    match mode {
        "hue" => base.0 = ink.0,
        "saturation" => base.1 = ink.1,
        "color" => {
            base.0 = ink.0;
            base.1 = ink.1;
        }
        "luminosity" => base.2 = ink.2,
        _ => {}
    }
    let [r, g, b] = hsl_to_rgb(base.0, base.1, base.2);
    let colored = Rgba::new(r, g, b, src.a);
    source_over(dst, colored, 1.0)
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = f32::from(r) / 255.0;
    let g = f32::from(g) / 255.0;
    let b = f32::from(b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if (max - r).abs() < 1e-6 {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    if s <= 0.0 {
        let v = rint_u8(l * 255.0);
        return [v, v, v];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let ch = |t: f32| {
        let mut t = t;
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        rint_u8(v * 255.0)
    };
    [ch(h + 1.0 / 3.0), ch(h), ch(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_grey_over_white_is_grey_and_there_are_twenty_nine_modes() {
        assert_eq!(BLENDS.len(), 29);
        let white = Rgba::rgb(255, 255, 255);
        let grey = Rgba::rgb(128, 128, 128);
        let out = mix(white, grey, "multiply", 255);
        assert_eq!(out.r, 128);
        assert_eq!(out.a, 255);
        assert_eq!(blend_label("multiply"), "Multiply");
    }
}
