//! The viewfinder. One rectangle is the scissor and the pen hit test.
//! The picture texture stays nearest so a dither chunk stays a square.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Frame {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn center(self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// Screen transform of the document. Zoom stays about the viewfinder center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub rotation: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self { zoom: 1.0, pan_x: 0.0, pan_y: 0.0, rotation: 0.0 }
    }
}

impl Camera {
    pub fn to_screen(self, frame: Frame, doc_w: f32, doc_h: f32, x: f32, y: f32) -> (f32, f32) {
        let dx = x - doc_w * 0.5;
        let dy = y - doc_h * 0.5;
        let (sin, cos) = self.rotation.sin_cos();
        let rx = dx * cos - dy * sin;
        let ry = dx * sin + dy * cos;
        let (cx, cy) = frame.center();
        (cx + self.pan_x + rx * self.zoom, cy + self.pan_y + ry * self.zoom)
    }

    pub fn to_doc(self, frame: Frame, doc_w: f32, doc_h: f32, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy) = frame.center();
        let px = (x - cx - self.pan_x) / self.zoom.max(0.01);
        let py = (y - cy - self.pan_y) / self.zoom.max(0.01);
        let (sin, cos) = self.rotation.sin_cos();
        let dx = px * cos + py * sin;
        let dy = -px * sin + py * cos;
        (dx + doc_w * 0.5, dy + doc_h * 0.5)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureFilter {
    Nearest,
}

/// Dither and chunk brushes upload with nearest filtering.
pub const PICTURE_FILTER: PictureFilter = PictureFilter::Nearest;

/// Composite the clear plate over a checker. The document itself stays clear.
pub fn with_checker(plate: &[u8], width: u32, height: u32) -> Vec<u8> {
    with_checker_at(plate, width, height, 0, 0)
}

/// The same checker for a region whose first pixel is document `(ox, oy)`,
/// so a patched rectangle lines up with the squares around it.
pub fn with_checker_at(plate: &[u8], width: u32, height: u32, ox: i32, oy: i32) -> Vec<u8> {
    let need = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    if width == 0 || height == 0 || plate.len() < need {
        return plate.to_vec();
    }
    let mut out = vec![0u8; need];
    for y in 0..height {
        for x in 0..width {
            let index = ((y * width + x) * 4) as usize;
            let doc_x = (x as i32 + ox).max(0) as u32;
            let doc_y = (y as i32 + oy).max(0) as u32;
            let dark = ((doc_x / 8) ^ (doc_y / 8)) & 1 == 0;
            let base = if dark { 32u32 } else { 48 };
            let alpha = u32::from(plate[index + 3]);
            let mix = |src: u8| ((u32::from(src) * alpha + base * (255 - alpha)) / 255) as u8;
            out[index] = mix(plate[index]);
            out[index + 1] = mix(plate[index + 1]);
            out[index + 2] = mix(plate[index + 2]);
            out[index + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pen_round_trips_and_zoom_stays_on_the_center() {
        let frame = Frame { x: 100.0, y: 40.0, w: 300.0, h: 200.0 };
        let camera = Camera { zoom: 2.0, pan_x: 10.0, pan_y: -4.0, rotation: 0.4 };
        let (sx, sy) = camera.to_screen(frame, 80.0, 40.0, 12.0, 18.0);
        let (x, y) = camera.to_doc(frame, 80.0, 40.0, sx, sy);
        assert!((x - 12.0).abs() < 1e-3, "{x}");
        assert!((y - 18.0).abs() < 1e-3, "{y}");

        let mut centered = Camera::default();
        let before = centered.to_screen(frame, 80.0, 40.0, 40.0, 20.0);
        centered.zoom = 4.0;
        let after = centered.to_screen(frame, 80.0, 40.0, 40.0, 20.0);
        assert!((before.0 - after.0).abs() < 1e-4);
        assert!((before.1 - after.1).abs() < 1e-4);
        assert!(Frame { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }.contains(0.0, 0.0));
        assert!(!Frame { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }.contains(10.0, 0.0));
        assert_eq!(PICTURE_FILTER, PictureFilter::Nearest);
    }

    #[test]
    fn the_checker_keeps_opaque_ink_and_fills_clear_pixels() {
        let mut plate = vec![0u8; 16 * 4];
        plate[0] = 200;
        plate[3] = 255;
        let shown = with_checker(&plate, 4, 1);
        assert_eq!(shown[0], 200);
        assert_eq!(shown[3], 255);
        assert_eq!(shown[7], 255);
        assert!(shown[4] == 32 || shown[4] == 48);
    }
}
