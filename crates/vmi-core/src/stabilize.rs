//! Pulled-string stabilization.
//!
//! The pen may run ahead by `length` pixels. The stroke follows behind,
//! the way Clip Studio's correction and a lazy brush do. Zero length
//! copies the pen. Release shrinks the string so the stroke ends where
//! the pen lifted.

#[derive(Clone, Debug, Default)]
pub struct Stabilizer {
    x: f32,
    y: f32,
    started: bool,
}

impl Stabilizer {
    pub fn start(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.started = true;
    }

    /// The next stroke point, or `None` while the pen is still inside the string.
    pub fn follow(&mut self, x: f32, y: f32, length: f32) -> Option<(f32, f32)> {
        if !self.started {
            self.start(x, y);
            return Some((x, y));
        }
        let length = length.max(0.0);
        if length < 0.5 {
            self.x = x;
            self.y = y;
            return Some((x, y));
        }
        let dx = x - self.x;
        let dy = y - self.y;
        let dist = dx.hypot(dy);
        if dist <= length {
            return None;
        }
        let scale = (dist - length) / dist;
        self.x += dx * scale;
        self.y += dy * scale;
        Some((self.x, self.y))
    }

    /// Walk the string in onto the pen. The last point is the lift.
    pub fn release(&mut self, x: f32, y: f32, length: f32) -> Vec<(f32, f32)> {
        if !self.started || length < 0.5 {
            self.started = false;
            return Vec::new();
        }
        let mut out = Vec::new();
        let steps = 3;
        for step in 1..=steps {
            let remain = length * (1.0 - step as f32 / steps as f32);
            if let Some(point) = self.follow(x, y, remain) {
                out.push(point);
            }
        }
        self.started = false;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_length_copies_the_pen_and_release_adds_nothing() {
        let mut stabilizer = Stabilizer::default();
        assert_eq!(stabilizer.follow(3.0, 4.0, 0.0), Some((3.0, 4.0)));
        assert_eq!(stabilizer.follow(5.0, 1.0, 0.0), Some((5.0, 1.0)));
        assert!(stabilizer.release(5.0, 1.0, 0.0).is_empty());
    }

    #[test]
    fn a_pulled_string_ignores_jitter_and_finishes_on_the_pen() {
        let mut stabilizer = Stabilizer::default();
        stabilizer.start(0.0, 0.0);
        let mut ys = Vec::new();
        for index in 1..=20 {
            let y = if index % 2 == 0 { 8.0 } else { -8.0 };
            if let Some((_, sy)) = stabilizer.follow(index as f32 * 4.0, y, 12.0) {
                ys.push(sy);
            }
        }
        assert!(!ys.is_empty());
        assert!(ys.iter().all(|y| y.abs() < 12.0), "{ys:?}");
        let end = stabilizer.release(80.0, 0.0, 12.0);
        let last = end.last().copied().unwrap();
        assert!(last.0 > 79.0, "{last:?}");
        assert!(last.1.abs() < 0.5, "{last:?}");
    }
}
