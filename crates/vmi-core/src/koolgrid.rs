//! The 2kool grid, measured from the Clip Studio template. A port of
//! `vmi_studio/koolgrid.py`.
//!
//! The template is 3840×3840. Paper is a separate layer. The grid art is a
//! 3589×3589 block of 5px lines placed at (125, 125), which centers it.
//! Solid lines are #2CB0FF. The lighter guides are the same blue at alpha 158.
//! Coordinates below are in that 3589 block, before the placement offset.

pub const PLATE: u32 = 3840;
pub const OFFSET: u32 = 125;
pub const ART: u32 = 3589;
pub const THICK: u32 = 5;
pub const BLUE: [u8; 3] = [44, 176, 255];
pub const SOLID: u8 = 255;
pub const FAINT: u8 = 158;

/// (x, y, w, h, alpha) inside the 3589 block. Faint guides first, then solid.
const BLOCK: &[(u32, u32, u32, u32, u8)] = &[
    (0, 1280, ART, THICK, FAINT),
    (0, 2304, ART, THICK, FAINT),
    (1280, 0, THICK, ART, FAINT),
    (2304, 0, THICK, ART, FAINT),
    (1280, 0, 1029, THICK, FAINT),
    (1280, 3584, 1029, THICK, FAINT),
    (0, 1280, THICK, 1029, FAINT),
    (3584, 1280, THICK, 1029, FAINT),
    (1152, 256, 1285, THICK, SOLID),
    (1024, 640, 1541, THICK, SOLID),
    (640, 1024, 2309, THICK, SOLID),
    (256, 1152, 3077, THICK, SOLID),
    (640, 1792, 2309, THICK, SOLID),
    (256, 2432, 3077, THICK, SOLID),
    (640, 2560, 2309, THICK, SOLID),
    (1024, 2944, 1541, THICK, SOLID),
    (1152, 3328, 1285, THICK, SOLID),
    (256, 1152, THICK, 1285, SOLID),
    (640, 1024, THICK, 1541, SOLID),
    (1024, 640, THICK, 2309, SOLID),
    (1152, 256, THICK, 3077, SOLID),
    (1792, 640, THICK, 2309, SOLID),
    (2432, 256, THICK, 3077, SOLID),
    (2560, 640, THICK, 2309, SOLID),
    (2944, 1024, THICK, 1541, SOLID),
    (3328, 1152, THICK, 1285, SOLID),
];

/// Line rectangles on the 3840 plate. Solid rows win over the guides.
pub fn canvas_rects() -> Vec<(u32, u32, u32, u32, u8)> {
    BLOCK.iter().map(|(x, y, w, h, alpha)| (x + OFFSET, y + OFFSET, *w, *h, *alpha)).collect()
}

/// RGBA at one plate pixel. Outside a line this is empty.
pub fn pixel(x: i64, y: i64) -> [u8; 4] {
    let ix = x - i64::from(OFFSET);
    let iy = y - i64::from(OFFSET);
    if ix < 0 || iy < 0 || ix >= i64::from(ART) || iy >= i64::from(ART) {
        return [0, 0, 0, 0];
    }
    let mut found = 0u8;
    for (left, top, wide, high, alpha) in BLOCK {
        let inside = i64::from(*left) <= ix
            && ix < i64::from(left + wide)
            && i64::from(*top) <= iy
            && iy < i64::from(top + high);
        if inside {
            found = *alpha;
            if *alpha == SOLID {
                break;
            }
        }
    }
    if found == 0 {
        return [0, 0, 0, 0];
    }
    [BLUE[0], BLUE[1], BLUE[2], found]
}

/// A small RGBA crop of the plate. Tests use this instead of the full plate.
pub fn render_crop(x: u32, y: u32, width: u32, height: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
    for (left, top, wide, high, alpha) in canvas_rects() {
        let col0 = left.max(x);
        let row0 = top.max(y);
        let col1 = (left + wide).min(x + width);
        let row1 = (top + high).min(y + height);
        if col1 <= col0 || row1 <= row0 {
            continue;
        }
        for row in row0..row1 {
            for col in col0..col1 {
                let at = (((row - y) * width + (col - x)) * 4) as usize;
                out[at..at + 4].copy_from_slice(&[BLUE[0], BLUE[1], BLUE[2], alpha]);
            }
        }
    }
    out
}

/// The full 3840 plate, straight RGBA. Only the 2kool preset asks for this.
pub fn grid_raster() -> Vec<u8> {
    render_crop(0, 0, PLATE, PLATE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_sits_at_the_template_offsets() {
        // Off the art block.
        assert_eq!(pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(124, 2000), [0, 0, 0, 0]);
        // The faint guide row at block y 1280 → plate y 1405.
        assert_eq!(pixel(200, 1405), [44, 176, 255, FAINT]);
        assert_eq!(pixel(200, 1409), [44, 176, 255, FAINT]);
        assert_eq!(pixel(200, 1410), [0, 0, 0, 0]);
        // A solid row at block y 256 → plate y 381, from x 1152+125.
        assert_eq!(pixel(1277, 381), [44, 176, 255, SOLID]);
        assert_eq!(pixel(1276, 381), [0, 0, 0, 0]);
        // The faint vertical guide at block x 1280, above every solid row.
        assert_eq!(pixel(1405, 250), [44, 176, 255, FAINT]);
        // Where a solid line crosses a faint one the solid wins.
        assert_eq!(pixel(381, 1405), [44, 176, 255, SOLID]);
        // The crop and the pixel agree.
        let crop = render_crop(1270, 376, 16, 16);
        for y in 0..16u32 {
            for x in 0..16u32 {
                let at = ((y * 16 + x) * 4) as usize;
                assert_eq!(&crop[at..at + 4], &pixel(1270 + i64::from(x), 376 + i64::from(y)), "{x},{y}");
            }
        }
    }

    #[test]
    fn the_full_plate_is_3840_square_and_mostly_clear() {
        let plate = grid_raster();
        assert_eq!(plate.len(), (PLATE as usize) * (PLATE as usize) * 4);
        let inked = plate.chunks_exact(4).filter(|px| px[3] != 0).count();
        assert!(inked > 0);
        assert!(inked < plate.len() / 4 / 10);
        let at = ((381 * PLATE + 1277) * 4) as usize;
        assert_eq!(&plate[at..at + 4], &[44, 176, 255, SOLID]);
    }
}
