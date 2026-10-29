//! Finger taps on a touch screen. Two fingers tapped together undo. Three
//! redo. A pen is never a finger. A tap is short and does not travel.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug)]
struct Finger {
    x: f32,
    y: f32,
    moved: bool,
}

/// Feed every touch event in. Ask `take` once per frame.
#[derive(Debug, Default)]
pub struct Fingers {
    down: HashMap<u64, Finger>,
    /// Most fingers down at once since the last full release.
    pub max_count: usize,
    started_ms: Option<u64>,
    travelled: bool,
    ready: Option<Tap>,
}

/// A tap must finish inside this window, or it is a hold.
pub const TAP_MS: u64 = 400;
/// A finger may drift this far and still be a tap.
pub const DRIFT: f32 = 14.0;

impl Fingers {
    pub fn start(&mut self, id: u64, x: f32, y: f32, now_ms: u64) {
        if self.down.is_empty() {
            self.max_count = 0;
            self.travelled = false;
            self.started_ms = Some(now_ms);
        }
        self.down.insert(id, Finger { x, y, moved: false });
        self.max_count = self.max_count.max(self.down.len());
    }

    pub fn moved(&mut self, id: u64, x: f32, y: f32) {
        if let Some(finger) = self.down.get_mut(&id) {
            if (finger.x - x).hypot(finger.y - y) > DRIFT {
                finger.moved = true;
                self.travelled = true;
            }
        }
    }

    pub fn end(&mut self, id: u64, now_ms: u64) {
        self.down.remove(&id);
        if !self.down.is_empty() {
            return;
        }
        let quick = self.started_ms.map(|start| now_ms.saturating_sub(start) <= TAP_MS).unwrap_or(false);
        if quick && !self.travelled {
            self.ready = match self.max_count {
                2 => Some(Tap::Undo),
                3 => Some(Tap::Redo),
                _ => None,
            };
        }
        self.started_ms = None;
    }

    pub fn cancel(&mut self) {
        self.down.clear();
        self.started_ms = None;
        self.ready = None;
        self.max_count = 0;
    }

    /// More than one finger is on the glass right now. Painting must stop.
    pub fn many(&self) -> bool {
        self.down.len() >= 2
    }

    pub fn take(&mut self) -> Option<Tap> {
        self.ready.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_fingers_undo_three_redo_and_one_does_nothing() {
        let mut fingers = Fingers::default();
        fingers.start(1, 10.0, 10.0, 0);
        fingers.start(2, 60.0, 10.0, 20);
        assert!(fingers.many());
        fingers.end(1, 120);
        fingers.end(2, 140);
        assert_eq!(fingers.take(), Some(Tap::Undo));
        assert_eq!(fingers.take(), None);

        fingers.start(1, 10.0, 10.0, 500);
        fingers.start(2, 60.0, 10.0, 510);
        fingers.start(3, 110.0, 10.0, 520);
        fingers.end(3, 600);
        fingers.end(2, 610);
        fingers.end(1, 620);
        assert_eq!(fingers.take(), Some(Tap::Redo));

        fingers.start(1, 10.0, 10.0, 1000);
        fingers.end(1, 1100);
        assert_eq!(fingers.take(), None);
    }

    #[test]
    fn a_hold_or_a_drag_is_not_a_tap() {
        let mut fingers = Fingers::default();
        fingers.start(1, 10.0, 10.0, 0);
        fingers.start(2, 60.0, 10.0, 0);
        fingers.end(1, 900);
        fingers.end(2, 900);
        assert_eq!(fingers.take(), None);

        fingers.start(1, 10.0, 10.0, 2000);
        fingers.start(2, 60.0, 10.0, 2000);
        fingers.moved(2, 120.0, 40.0);
        fingers.end(1, 2100);
        fingers.end(2, 2100);
        assert_eq!(fingers.take(), None);
    }
}
