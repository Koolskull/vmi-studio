//! Clip Studio's border edge. The paint stays. The outline sits under it,
//! out to the width, including the inner edge of a hole.
//!
//! Coverage at a pixel is `width + 1 - distance`, where distance is the
//! Euclidean distance to the nearest pixel that has any alpha. The same
//! rule is `vmi_studio/outline.py`.

use crate::color::Rgba;
use crate::coverage::source_over;

pub const MAX_PX: f32 = 250.0;
const FAR: f64 = 1.0e20;

/// The width to draw, when the edge is on and the width is positive.
pub fn active(on: bool, px: f32) -> Option<f32> {
    if !on || !px.is_finite() || px <= 0.0 {
        None
    } else {
        Some(px.min(MAX_PX))
    }
}

pub fn clamp_px(px: f64) -> f32 {
    if !px.is_finite() || px <= 0.0 {
        0.0
    } else {
        (px as f32).min(MAX_PX)
    }
}

pub fn rgb_from(value: Option<&serde_json::Value>) -> [u8; 3] {
    let Some(row) = value.and_then(|item| item.as_array()) else {
        return [0, 0, 0];
    };
    let mut out = [0u8; 3];
    for (index, channel) in out.iter_mut().enumerate() {
        *channel = row
            .get(index)
            .and_then(|item| item.as_f64())
            .map(|number| number.round().clamp(0.0, 255.0) as u8)
            .unwrap_or(0);
    }
    out
}

/// Draw the outline of `src` (a full-canvas RGBA buffer) under `dst`.
/// A destination sample at `(dx, dy)` is the document pixel
/// `(dox + dx * step, doy + dy * step)`.
pub fn under_layer(
    dst: &mut [u8],
    dw: u32,
    dh: u32,
    dox: i32,
    doy: i32,
    src: &[u8],
    sw: u32,
    sh: u32,
    px: f32,
    rgb: [u8; 3],
    step: u32,
) {
    let Some(px) = active(true, px) else {
        return;
    };
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return;
    }
    let step_i = step.max(1) as i32;
    let pad = px.ceil() as i32;
    let last_x = dox + (dw as i32 - 1) * step_i;
    let last_y = doy + (dh as i32 - 1) * step_i;
    let Some(cover) = cover_of(
        src,
        sw,
        sh,
        dox - pad,
        doy - pad,
        last_x + pad + 1,
        last_y + pad + 1,
        px,
        1.0,
    ) else {
        return;
    };
    for dy in 0..dh {
        let doc_y = doy + dy as i32 * step_i;
        let iy = doc_y - cover.y0;
        if iy < 0 || iy >= cover.h as i32 {
            continue;
        }
        for dx in 0..dw {
            let doc_x = dox + dx as i32 * step_i;
            let ix = doc_x - cover.x0;
            if ix < 0 || ix >= cover.w as i32 {
                continue;
            }
            let alpha = cover.alpha[iy as usize * cover.w + ix as usize];
            if alpha == 0 {
                continue;
            }
            let di = ((dy * dw + dx) * 4) as usize;
            if di + 3 >= dst.len() {
                continue;
            }
            let base = Rgba::new(dst[di], dst[di + 1], dst[di + 2], dst[di + 3]);
            let over = source_over(base, Rgba::new(rgb[0], rgb[1], rgb[2], alpha), 1.0);
            dst[di..di + 4].copy_from_slice(&over.channels());
        }
    }
}

/// The outline under the paint already in `buf`. `step` is the document
/// distance between neighbouring samples. A playback plate passes its stride.
pub fn underlay(buf: &mut [u8], w: u32, h: u32, px: f32, rgb: [u8; 3], step: u32) {
    let Some(px) = active(true, px) else {
        return;
    };
    let Some(cover) = cover_of(buf, w, h, 0, 0, w as i32, h as i32, px, f64::from(step.max(1))) else {
        return;
    };
    for y in 0..cover.h {
        for x in 0..cover.w {
            let alpha = cover.alpha[y * cover.w + x];
            if alpha == 0 {
                continue;
            }
            let dx = cover.x0 + x as i32;
            let dy = cover.y0 + y as i32;
            if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                continue;
            }
            let di = ((dy as u32 * w + dx as u32) * 4) as usize;
            if di + 3 >= buf.len() {
                continue;
            }
            let paint = Rgba::new(buf[di], buf[di + 1], buf[di + 2], buf[di + 3]);
            if paint.a == 255 {
                continue;
            }
            let under = Rgba::new(rgb[0], rgb[1], rgb[2], alpha);
            let over = source_over(under, paint, 1.0);
            buf[di..di + 4].copy_from_slice(&over.channels());
        }
    }
}

/// The outline of `src` onto `dst`. Both buffers are the same sample grid.
/// The paint in `src` is not copied. The caller paints it afterwards.
pub fn disc_onto(dst: &mut [u8], src: &[u8], w: u32, h: u32, px: f32, rgb: [u8; 3], step: u32) {
    let Some(px) = active(true, px) else {
        return;
    };
    let Some(cover) = cover_of(src, w, h, 0, 0, w as i32, h as i32, px, f64::from(step.max(1))) else {
        return;
    };
    for y in 0..cover.h {
        for x in 0..cover.w {
            let alpha = cover.alpha[y * cover.w + x];
            if alpha == 0 {
                continue;
            }
            let dx = cover.x0 + x as i32;
            let dy = cover.y0 + y as i32;
            if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                continue;
            }
            let di = ((dy as u32 * w + dx as u32) * 4) as usize;
            if di + 3 >= dst.len() {
                continue;
            }
            let base = Rgba::new(dst[di], dst[di + 1], dst[di + 2], dst[di + 3]);
            let over = source_over(base, Rgba::new(rgb[0], rgb[1], rgb[2], alpha), 1.0);
            dst[di..di + 4].copy_from_slice(&over.channels());
        }
    }
}

struct Cover {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
    alpha: Vec<u8>,
}

/// Coverage for the document window `[x0, x1) × [y0, y1)`, clipped to `src`.
/// `step` scales a sample-grid distance into document pixels. A full-canvas
/// buffer uses 1.
fn cover_of(
    src: &[u8],
    sw: u32,
    sh: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    px: f32,
    step: f64,
) -> Option<Cover> {
    if sw == 0 || sh == 0 || src.len() < (sw as usize) * (sh as usize) * 4 {
        return None;
    }
    let (bx0, by0, bx1, by1) = bounds(src, sw, sh, x0, y0, x1, y1)?;
    let pad = px.ceil() as i32;
    let gx0 = (bx0 - pad).max(x0).max(0);
    let gy0 = (by0 - pad).max(y0).max(0);
    let gx1 = (bx1 + pad).min(x1).min(sw as i32);
    let gy1 = (by1 + pad).min(y1).min(sh as i32);
    if gx1 <= gx0 || gy1 <= gy0 {
        return None;
    }
    let gw = (gx1 - gx0) as usize;
    let gh = (gy1 - gy0) as usize;
    let mut sites = vec![FAR; gw * gh];
    for y in by0..by1 {
        let row = (y as usize) * (sw as usize) * 4;
        let gy = (y - gy0) as usize;
        for x in bx0..bx1 {
            if src[row + (x as usize) * 4 + 3] > 0 {
                sites[gy * gw + (x - gx0) as usize] = 0.0;
            }
        }
    }
    let dist = edt_sq(&sites, gw, gh);
    let mut alpha = vec![0u8; gw * gh];
    let step = if step.is_finite() && step > 0.0 { step } else { 1.0 };
    for (slot, sq) in alpha.iter_mut().zip(dist) {
        let distance = sq.max(0.0).sqrt() * step;
        let cover = (f64::from(px) + 1.0 - distance).clamp(0.0, 1.0);
        if cover <= 0.0 {
            continue;
        }
        *slot = (cover * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    Some(Cover {
        x0: gx0,
        y0: gy0,
        w: gw,
        h: gh,
        alpha,
    })
}

/// Inclusive-exclusive bounds of pixels with alpha, inside the window.
fn bounds(
    src: &[u8],
    w: u32,
    h: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> Option<(i32, i32, i32, i32)> {
    let x0 = x0.clamp(0, w as i32);
    let y0 = y0.clamp(0, h as i32);
    let x1 = x1.clamp(0, w as i32);
    let y1 = y1.clamp(0, h as i32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut min_x = x1;
    let mut min_y = y1;
    let mut max_x = x0;
    let mut max_y = y0;
    let mut any = false;
    let stride = w as usize;
    for y in y0..y1 {
        let row = (y as usize) * stride * 4;
        for x in x0..x1 {
            if src[row + (x as usize) * 4 + 3] == 0 {
                continue;
            }
            any = true;
            if x < min_x {
                min_x = x;
            }
            if x + 1 > max_x {
                max_x = x + 1;
            }
            if y < min_y {
                min_y = y;
            }
            if y + 1 > max_y {
                max_y = y + 1;
            }
        }
    }
    any.then_some((min_x, min_y, max_x, max_y))
}

fn edt_sq(field: &[f64], width: usize, height: usize) -> Vec<f64> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let long = width.max(height);
    let mut work = vec![0.0; width * height];
    let mut line = vec![0.0; long];
    let mut done = vec![0.0; long];
    let mut parabola = vec![0i32; long];
    let mut border = vec![0.0; long + 1];
    for x in 0..width {
        for y in 0..height {
            line[y] = field[y * width + x];
        }
        dt1(&line[..height], &mut done[..height], &mut parabola, &mut border);
        for y in 0..height {
            work[y * width + x] = done[y];
        }
    }
    let mut out = vec![0.0; width * height];
    for y in 0..height {
        let start = y * width;
        line[..width].copy_from_slice(&work[start..start + width]);
        dt1(&line[..width], &mut done[..width], &mut parabola, &mut border);
        out[start..start + width].copy_from_slice(&done[..width]);
    }
    out
}

/// Squared distance to the nearest 0. Other cells are far. The lower hull
/// starts at negative infinity, so the cursor stays on the parabola.
fn dt1(values: &[f64], out: &mut [f64], parabola: &mut [i32], border: &mut [f64]) {
    let count = values.len();
    if count == 0 || out.len() < count || parabola.len() < count || border.len() < count + 1 {
        return;
    }
    parabola[0] = 0;
    border[0] = f64::NEG_INFINITY;
    border[1] = f64::INFINITY;
    let mut k: i32 = 0;
    for q in 1..count {
        let mut s = intersect(values, q, parabola[k as usize] as usize);
        while k > 0 && s <= border[k as usize] {
            k -= 1;
            s = intersect(values, q, parabola[k as usize] as usize);
        }
        k += 1;
        parabola[k as usize] = q as i32;
        border[k as usize] = s;
        border[(k + 1) as usize] = f64::INFINITY;
    }
    k = 0;
    for q in 0..count {
        while border[(k + 1) as usize] < q as f64 {
            k += 1;
        }
        let p = parabola[k as usize] as usize;
        let span = q as f64 - p as f64;
        out[q] = span * span + values[p];
    }
}

fn intersect(values: &[f64], q: usize, p: usize) -> f64 {
    let qf = q as f64;
    let pf = p as f64;
    ((values[q] + qf * qf) - (values[p] + pf * pf)) / (2.0 * qf - 2.0 * pf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pixel_is_solid_on_the_cross_and_partial_on_the_diagonal() {
        let mut src = vec![0u8; 5 * 5 * 4];
        let center = (2 * 5 + 2) * 4;
        src[center] = 255;
        src[center + 3] = 255;
        let mut dst = vec![0u8; src.len()];
        under_layer(&mut dst, 5, 5, 0, 0, &src, 5, 5, 1.0, [0, 0, 0], 1);
        let at = |x: usize, y: usize| {
            let i = (y * 5 + x) * 4;
            [dst[i], dst[i + 1], dst[i + 2], dst[i + 3]]
        };
        assert_eq!(at(2, 1), [0, 0, 0, 255]);
        assert_eq!(at(2, 3), [0, 0, 0, 255]);
        assert_eq!(at(1, 2), [0, 0, 0, 255]);
        assert_eq!(at(3, 2), [0, 0, 0, 255]);
        let diagonal = at(1, 1);
        assert_eq!(&diagonal[..3], &[0, 0, 0]);
        assert!(diagonal[3] > 0 && diagonal[3] < 255);
        assert_eq!(at(2, 0), [0, 0, 0, 0]);
        assert!(active(false, 4.0).is_none());
        assert_eq!(clamp_px(f64::NAN), 0.0);
        assert_eq!(clamp_px(400.0), 250.0);
    }

    #[test]
    fn a_hole_takes_the_inner_edge() {
        let mut buf = vec![0u8; 3 * 3 * 4];
        for y in 0..3 {
            for x in 0..3 {
                if x == 1 && y == 1 {
                    continue;
                }
                let i = (y * 3 + x) * 4;
                buf[i] = 255;
                buf[i + 3] = 255;
            }
        }
        underlay(&mut buf, 3, 3, 1.0, [0, 0, 0], 1);
        assert_eq!(&buf[4 * 4..4 * 4 + 4], &[0, 0, 0, 255]);
        assert_eq!(buf[3], 255);
    }
}
