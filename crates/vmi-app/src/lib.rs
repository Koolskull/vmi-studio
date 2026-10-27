//! KOOL SKULL STUDIO desk. The palette names every tool. A right-click opens
//! that tool's settings and leaves the current tool selected.

pub mod desk;
pub mod fingers;
pub mod gpu;
pub mod icons;
pub mod tablet;

#[cfg(feature = "window")]
pub mod cloudui;
#[cfg(feature = "window")]
pub mod effects;
#[cfg(feature = "window")]
pub mod exportui;
#[cfg(feature = "window")]
pub mod gate;
#[cfg(feature = "window")]
pub mod launch;
#[cfg(feature = "window")]
pub mod newpage;
#[cfg(feature = "window")]
pub mod settings;
#[cfg(feature = "window")]
pub mod sheet;
#[cfg(feature = "window")]
pub mod taskboard;
#[cfg(feature = "window")]
pub mod ui;

#[cfg(feature = "window")]
pub fn run() -> eframe::Result<()> {
    ui::run()
}
