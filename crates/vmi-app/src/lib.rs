//! KOOL SKULL STUDIO desk. The palette names every tool. A right-click opens
//! that tool's settings and leaves the current tool selected.

pub mod desk;
pub mod fingers;
pub mod gpu;
pub mod icons;
pub mod tablet;

#[cfg(feature = "window")]
mod cloudui;
#[cfg(feature = "window")]
mod effects;
#[cfg(feature = "window")]
mod exportui;
#[cfg(feature = "window")]
mod gate;
#[cfg(feature = "window")]
mod launch;
#[cfg(feature = "window")]
mod newpage;
#[cfg(feature = "window")]
mod settings;
#[cfg(feature = "window")]
mod sheet;
#[cfg(feature = "window")]
mod taskboard;
#[cfg(feature = "window")]
mod ui;

#[cfg(feature = "window")]
pub fn run() -> eframe::Result<()> {
    ui::run()
}
