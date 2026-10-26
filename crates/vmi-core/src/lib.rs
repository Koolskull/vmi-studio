//! Shared studio types. The desk paints with these. The window does not live here.

pub mod blend;
pub mod color;
pub mod palette;
pub mod coverage;
pub mod cut;
pub mod doc;
pub mod export;
pub mod session;
pub mod theme;
pub mod tip;
pub mod tool;
pub mod vector;
pub mod vmib;

pub mod bridge;

pub use blend::{blend_label, BLENDS};
pub use bridge::load_manifest;
pub use color::{hex_of, parse_hex, quiet_color, Rgba};
pub use coverage::{coverage, dither_keeps, mix_half, pixel, shade, stamp, stroke_steps, Edge, Stamp, StampMode};
pub use tip::{fbm, tip_sample, value_noise, Grain, Tip, TipSample};
pub use cut::{cut_folder, erase_folder, point_inside, rect_points, Marquee};
pub use doc::{clamp_rect, find, find_mut, parent_of, present, present_region, reorder_objects, Art, DeskObject, History, Node, NodeKind};
pub use export::{export_png, scale_nearest};
pub use session::{load as load_session, save as save_session, session_path, Session};
pub use theme::{theme_or_studio, Theme, THEMES};
pub use tool::{
    ink_spacing, palette_action, palette_matching, CurveKind, Field, GrainKind, PaletteAction, Release, Section, Tool,
    ToolSettings,
};
pub use vector::{bake, hit_stroke, Bone, StrokeHit, VectorStroke, Vertex};
pub use vmib::{load as load_vmib, save as save_vmib, strokes_from_value};
