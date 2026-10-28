//! Shared studio types. The desk paints with these. The window does not live here.

pub mod anim;
pub mod blend;
pub mod catalog;
pub mod cloud;
pub mod color;
pub mod coverage;
pub mod cut;
pub mod doc;
pub mod export;
pub mod fonts;
pub mod icloud;
pub mod koolgrid;
pub mod kosv;
pub mod naming;
pub mod newpage;
pub mod outline;
pub mod palette;
pub mod scene;
pub mod session;
pub mod stabilize;
pub mod tabs;
pub mod tasks;
pub mod theme;
pub mod tip;
pub mod tool;
pub mod vector;
pub mod vmib;

pub mod bridge;

pub use anim::{
    add_cel, animation_folder_ids, animation_folders, cel_at, cel_mark, cel_of, cels, column_of,
    cut_end, cut_frames, first_frame, folder_command, frame_map, key_of, move_cel, move_frame,
    neighbors, next_frame_slot, onion_neighbors, owning_animation, project_slots, run_command,
    set_animation, set_playhead, sheet_from_slots, step_frame, timeline_cels, unspecified_cels,
    write_frames, xsheet_columns, xsheet_command, Cel, ExposureSheet, SheetOnion, XColumn,
    DEFAULT_FPS, FUTURE, ONION_LIMIT, ONION_TINT, PAST, STRENGTH,
};
pub use blend::{blend_label, BLENDS};
pub use bridge::load_manifest;
pub use color::{hex_of, hex_rgba, hex_rgba_wide, parse_channel_hex, parse_hex, parse_hex_rgba, quiet_color, Rgba};
pub use coverage::{
    coverage, dither_keeps, mix_half, pixel, shade, source_over, stamp, stroke_steps,
    DitherPattern, Edge, Stamp, StampMode,
};
pub use cut::{cut_folder, erase_folder, point_inside, rect_points, Marquee};
pub use doc::{
    clamp_rect, find, find_mut, folder_contains, library_rows, move_symbol, next_stack, parent_of,
    pick_layer, present, present_proxy, present_region, reorder_objects, slot_hex, slot_key,
    state_word, Art, DeskObject, History, LibraryRow, Node, NodeKind, OBJECT_KINDS,
};
pub use doc::{present_plain, present_region_plain, ClipTime};
pub use export::{export_png, scale_nearest};
pub use kosv::{CelMark, Effect, Kosv, Param, Preset};
pub use newpage::blank_picture;
pub use session::{load as load_session, save as save_session, session_path, Session};
pub use stabilize::Stabilizer;
pub use tasks::{Task, TaskColors};
pub use theme::{theme_or_studio, Theme, THEMES};
pub use tip::{fbm, tip_sample, value_noise, Grain, Tip, TipSample};
pub use tool::{
    ink_spacing, palette_action, palette_matching, CurveKind, Field, GrainKind, PaletteAction,
    Release, Section, Tool, ToolSettings,
};
pub use vector::{
    bake, hit_stroke, move_handle, move_vertex, Bone, HandleSide, StrokeHit, VectorStroke, Vertex,
};
pub use vmib::{load as load_vmib, save as save_vmib, strokes_from_value};
