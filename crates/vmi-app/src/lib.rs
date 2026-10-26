//! KOOL SKULL STUDIO desk. The palette names every tool. A right-click opens
//! that tool's settings and leaves the current tool selected.

pub mod desk;
pub mod gpu;

#[cfg(feature = "window")]
mod ui;

#[cfg(feature = "window")]
pub fn run() -> eframe::Result<()> {
    ui::run()
}
