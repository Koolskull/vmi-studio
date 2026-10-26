//! Pen pressure from the operating system.
//!
//! winit hands egui a pen as a touch with force on Windows, and that path
//! lives in the window code. It hands over nothing for a tablet on macOS,
//! so this module watches AppKit's own event stream: every tablet-point
//! mouse event carries `pressure`, for a Wacom and for an Apple Pencil over
//! Sidecar alike. The latest value is parked in an atomic the window reads
//! once per frame. No pressure yet means a mouse, which presses at 1.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static PRESSURE: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0f32
static SEEN: AtomicBool = AtomicBool::new(false);

/// The latest pen pressure, or `None` while no pen has spoken.
pub fn pressure() -> Option<f32> {
    if SEEN.load(Ordering::Relaxed) {
        Some(f32::from_bits(PRESSURE.load(Ordering::Relaxed)))
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn report(value: f32) {
    let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 };
    PRESSURE.store(value.to_bits(), Ordering::Relaxed);
    SEEN.store(true, Ordering::Relaxed);
}

/// Start listening. Safe to call once at window creation.
pub fn install() {
    platform::install();
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ptr::NonNull;
    use std::sync::Once;

    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventSubtype, NSEventType};

    static ONCE: Once = Once::new();

    pub fn install() {
        ONCE.call_once(|| {
            let mask = NSEventMask::LeftMouseDown
                | NSEventMask::LeftMouseDragged
                | NSEventMask::LeftMouseUp
                | NSEventMask::TabletPoint;
            let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
                // SAFETY: AppKit hands us a live event for the duration of the call.
                let event_ref = unsafe { event.as_ref() };
                unsafe {
                    let kind = event_ref.r#type();
                    if kind == NSEventType::TabletPoint {
                        super::report(event_ref.pressure());
                    } else if kind == NSEventType::LeftMouseUp {
                        if event_ref.subtype() == NSEventSubtype::TabletPoint {
                            super::report(0.0);
                        }
                    } else if event_ref.subtype() == NSEventSubtype::TabletPoint {
                        super::report(event_ref.pressure());
                    }
                }
                event.as_ptr()
            });
            // SAFETY: called on the main thread at startup; the monitor lives for the process.
            let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) };
            std::mem::forget(monitor);
            std::mem::forget(block);
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn install() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_reported_means_a_mouse_and_a_report_clamps() {
        // Static state: run the two checks in order inside one test.
        if !SEEN.load(Ordering::Relaxed) {
            assert_eq!(pressure(), None);
        }
        report(1.7);
        assert_eq!(pressure(), Some(1.0));
        report(0.25);
        assert_eq!(pressure(), Some(0.25));
        report(f32::NAN);
        assert_eq!(pressure(), Some(1.0));
    }
}
