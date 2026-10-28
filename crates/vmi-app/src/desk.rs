//! Desk actions. The window calls these. A raster release writes pixels and
//! drops the path. A vector release keeps the stroke and rebuilds its cache.

use std::collections::HashSet;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};

use vmi_core::anim;
use vmi_core::doc::blit_into;
use vmi_core::newpage::{blank_picture, opened_note};
use vmi_core::tabs::{default_export_parent, same_export};
use vmi_core::{
    clamp_rect, column_of, cut_folder, erase_folder, export_png, find, find_mut, hex_of,
    hit_stroke, load_manifest, load_session, load_vmib, move_handle, move_vertex, palette_action,
    parent_of, parse_hex, point_inside, present_proxy, present_region, rect_points, save_session,
    save_vmib as write_vmib, source_over, theme_or_studio, xsheet_columns, Art, Bone, ClipTime,
    DeskObject, HandleSide, History, Node, PaletteAction, Release, Rgba, Section, Session,
    StrokeHit, Tool, ToolSettings, XColumn,
};
use vmi_paint::{flood_fill, paint_samples, paint_segment, segment_bounds, Carry, StrokeSession};

use crate::gpu::{with_checker_at, Camera, Frame};

/// What the viewfinder has to upload this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Display {
    /// The whole picture over its checker.
    Full(Vec<u8>),
    /// One rectangle of it. `rgba` is `w * h * 4`.
    Part {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        rgba: Vec<u8>,
    },
    /// A playback plate. Large canvases are sampled so the long side stays near `PLAY_SIDE`.
    Play { w: u32, h: u32, rgba: Vec<u8> },
}

/// Playback composites at this long-side cap. The view stretches the plate back over the page.
const PLAY_SIDE: u32 = 1280;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
}

#[derive(Clone, Debug)]
pub struct LayerRow {
    pub id: String,
    pub name: String,
    pub depth: usize,
    pub group: bool,
    pub visible: bool,
    pub mute: bool,
    pub solo: bool,
    pub clip: bool,
    /// A group whose children are listed. A layer is always open.
    pub open: bool,
    /// A folder that is one sequence.
    pub animation: bool,
    /// A cel's first frame, from 1. None for a child no frame specifies.
    pub frame: Option<u32>,
    pub opacity: u8,
    pub blend: String,
    /// Warp role, when the row is a target, window, or mask.
    pub marker: String,
    /// A later sibling is still below this row, so the guide continues.
    pub has_next: bool,
    /// Bit `level` is set when that ancestor still has a sibling below.
    pub guides: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Mark {
    pub x: f32,
    pub y: f32,
    pub hot: bool,
    /// When set, a handle line runs from this point to `(x, y)`.
    pub line_to: Option<(f32, f32)>,
}

/// A picked range of cells in one column of the sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetPick {
    pub folder: String,
    pub start: u32,
    pub count: u32,
}

/// One open drawing, one export folder. Tabs do not share an output path.
/// An empty path is a picture that has not been saved yet.
#[derive(Debug)]
pub struct OpenTab {
    pub path: String,
    pub export_dir: PathBuf,
    pub loaded: bool,
    /// Low res fits the plate to the view. High res keeps every document pixel.
    pub low_res: bool,
    /// The drawing and its desk state while another tab is on the desk.
    stash: Option<Box<Stash>>,
}

impl OpenTab {
    fn at(path: &Path) -> Self {
        let path = absolute(path);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            export_dir: default_export_parent(&path, &cwd),
            path: path.display().to_string(),
            loaded: false,
            low_res: false,
            stash: None,
        }
    }

    fn unsaved() -> Self {
        Self {
            path: String::new(),
            export_dir: home_dir(),
            loaded: false,
            low_res: true,
            stash: None,
        }
    }
}

/// What leaves the desk when another tab comes up, and returns with the tab.
#[derive(Debug)]
struct Stash {
    art: Art,
    history: History,
    selected: Option<String>,
    camera: Camera,
    fitted: bool,
    cut_points: Vec<[f32; 2]>,
    cut_closed: bool,
    warp_draft: Vec<[f32; 2]>,
    warp_layer: Option<String>,
    sheet_pick: Option<SheetPick>,
}

enum LoadMsg {
    Note(String),
    Done(Result<Art, String>),
}

/// A file being read on its own thread, for the tab at `index`.
struct Loading {
    index: usize,
    rx: Receiver<LoadMsg>,
}

/// What the desk tells the window about reads and tab changes, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadEvent {
    /// A read began for this file name. The gate opens.
    Started(String),
    /// A loaded tab came up. The gate opens in catch-up mode until the plate is painted.
    Catchup(String),
    /// A stage note: "Reading x", "Converting Clip Studio paint"...
    Note(String),
    /// The picture is on the desk. The gate can leave once it is painted.
    Shown,
    /// The read failed. The gate leaves at once; the text goes to the status bar.
    Failed(String),
}

enum Gesture {
    Paint {
        layer: String,
        tool: Tool,
        settings: ToolSettings,
        /// The whole document at pen-down. Only a vector stroke keeps one.
        /// A raster pen stores `backup` instead, so the down does not copy every layer.
        before: Option<Art>,
        /// The target layer's pixels at pen-down. Raster undo puts these back.
        backup: Vec<u8>,
        base: Vec<u8>,
        session: StrokeSession,
        changed: bool,
    },
    Point {
        layer: String,
        stroke: u64,
        which: StrokeHit,
        before: Art,
        changed: bool,
    },
    Bone {
        layer: String,
        stroke: u64,
        before: Art,
        x0: f32,
        y0: f32,
        changed: bool,
    },
    WarpCorner {
        layer: String,
        index: usize,
        before: Art,
        changed: bool,
    },
    CutRect {
        x0: f32,
        y0: f32,
    },
    CutLasso,
}

pub struct Desk {
    pub art: Art,
    pub session: Session,
    pub selected: Option<String>,
    pub status: String,
    pub query: String,
    pub tool_window: Option<Tool>,
    pub layer_window: Option<String>,
    pub stroke_window: Option<(String, u64)>,
    pub object_window: Option<String>,
    /// Right-click on SKIN. The colours live on the session.
    pub onion_window: bool,
    pub camera: Camera,
    pub fitted: bool,
    pub last_frame: Frame,
    pub dirty: bool,
    /// Pixels a pen move touched since the last upload. `dirty` wins over this.
    pub dirty_rect: Option<[i32; 4]>,
    /// The pen's pressure for the next sample. 1 when nothing reports it.
    pub pressure: f32,
    pub cut_points: Vec<[f32; 2]>,
    pub cut_closed: bool,
    pub warp_draft: Vec<[f32; 2]>,
    /// The sheet's picked cells, for its command menu.
    pub sheet_pick: Option<SheetPick>,
    /// Playback is running.
    pub playing: bool,
    /// Play returns to the first frame at the end. Off stops there.
    pub loop_play: bool,
    /// Stepping past either end wraps. Separate from playback loop.
    pub loop_wrap: bool,
    /// Ones, twos, or threes: how long a new cel holds.
    pub hold: u32,
    /// Every open drawing, left to right after the browser tab.
    pub tabs: Vec<OpenTab>,
    /// The tab whose drawing is on the desk. None with no tab shown.
    pub shown: Option<usize>,
    /// The launcher page is up instead of the editor.
    pub on_browser: bool,
    /// Events the loading thread sent since the window last asked.
    load_events: Vec<LoadEvent>,
    loading: Option<Loading>,
    /// Seconds owed to the next frame while playing.
    play_clock: f64,
    /// Play was just pressed: the first tick's frame time is the idle gap
    /// before it, not playback time, and is dropped.
    play_armed: bool,
    history: History,
    gesture: Option<Gesture>,
    overlay: Vec<u8>,
    overlay_live: bool,
    /// The last full plate shown, over its checker. Partial uploads patch it.
    plate: Vec<u8>,
    warp_layer: Option<String>,
    previous_tool: Tool,
    persist: bool,
    session_dirty: bool,
    layer_edit_saved: bool,
    stroke_edit_saved: bool,
    object_edit_saved: bool,
    /// One ink for every tool. Switching tools does not change it.
    pub ink: [u8; 3],
    /// The other ink. X trades it with `ink`, the way Clip Studio does.
    pub ink_back: [u8; 3],
    /// C. The current brush stamps transparent pixels and stays selected.
    pub clear_ink: bool,
    /// Folders the outliner has closed. Absent means open, so a file shows its layers.
    collapsed: HashSet<String>,
    /// The library folder a new object joins. Empty is the root.
    pub library_folder: String,
    /// The animation folder the sheet is inside. None is the whole cut.
    pub clip_focus: Option<String>,
    /// The layer a picture click should scroll into view. The outliner consumes it.
    pub reveal: Option<String>,
    /// A raster stroke lives in `overlay` until the pen lifts, then composites once.
    stroke_live: bool,
    stroke_opacity: f32,
    stroke_erase: bool,
    stroke_layer: String,
    /// Where `overlay` holds stroke ink. Clearing this rect is the whole wipe.
    overlay_mark: Option<[i32; 4]>,
    /// Clean layer pixels under the rect being shown, put back after the upload.
    preview_snap: Vec<u8>,
    preview_rect: [i32; 4],
}

impl Desk {
    pub fn new(session: Session) -> Self {
        let mut session = session;
        session.presets = ToolSettings::merge(session.presets);
        let art = Art::new(1280, 720);
        let selected = first_layer(&art.layers);
        let mut desk = Self {
            art,
            session,
            selected,
            status:
                "Pick a cell in the tool grid. Color and size are on that panel. Ctrl+Z undoes."
                    .into(),
            query: String::new(),
            tool_window: None,
            layer_window: None,
            stroke_window: None,
            object_window: None,
            onion_window: false,
            camera: Camera::default(),
            fitted: false,
            last_frame: Frame {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            dirty: true,
            dirty_rect: None,
            pressure: 1.0,
            cut_points: Vec::new(),
            cut_closed: false,
            warp_draft: Vec::new(),
            sheet_pick: None,
            playing: false,
            loop_play: true,
            loop_wrap: false,
            hold: 2,
            tabs: Vec::new(),
            shown: None,
            on_browser: true,
            load_events: Vec::new(),
            loading: None,
            play_clock: 0.0,
            play_armed: false,
            history: History::default(),
            gesture: None,
            overlay: Vec::new(),
            overlay_live: false,
            plate: Vec::new(),
            warp_layer: None,
            previous_tool: Tool::Pen,
            persist: false,
            session_dirty: false,
            layer_edit_saved: false,
            stroke_edit_saved: false,
            object_edit_saved: false,
            ink: [0xFF, 0x3E, 0xB8],
            ink_back: [0, 0, 0],
            clear_ink: false,
            collapsed: HashSet::new(),
            library_folder: String::new(),
            clip_focus: None,
            reveal: None,
            stroke_live: false,
            stroke_opacity: 1.0,
            stroke_erase: false,
            stroke_layer: String::new(),
            overlay_mark: None,
            preview_snap: Vec::new(),
            preview_rect: [0, 0, 0, 0],
        };
        desk.ensure_overlay();
        desk
    }

    /// The desk comes up on the browser tab, like the Python window. The
    /// session's path seeds the launcher; nothing opens by itself.
    pub fn launch() -> Self {
        let mut desk = Self::new(load_session());
        desk.persist = true;
        desk.art = Art::blank(1, 1);
        desk.selected = None;
        desk.ensure_overlay();
        desk.status = "Open a .clip, .psd, .kra, .xcf, or .vmib file.".into();
        desk
    }

    pub fn tool(&self) -> Tool {
        self.session.tool
    }

    pub fn theme(&self) -> &'static vmi_core::Theme {
        theme_or_studio(&self.session.theme)
    }

    pub fn set_theme(&mut self, id: &str) {
        self.session.theme = theme_or_studio(id).id.to_string();
        self.session_dirty = true;
        self.status = format!("Theme: {}", self.theme().name);
    }

    /// The header's font menu. `monospace` is the house stack.
    pub fn set_font(&mut self, id: &str) {
        if self.session.font == id {
            return;
        }
        self.session.font = id.to_string();
        self.session_dirty = true;
    }

    pub fn preset(&self) -> &ToolSettings {
        self.preset_of(self.session.tool)
    }

    pub fn preset_of(&self, tool: Tool) -> &ToolSettings {
        self.session
            .presets
            .iter()
            .find(|item| item.tool == tool)
            .expect("every tool has a preset")
    }

    pub fn edit_preset<F: FnOnce(&mut ToolSettings)>(&mut self, tool: Tool, edit: F) {
        if let Some(slot) = self
            .session
            .presets
            .iter_mut()
            .find(|item| item.tool == tool)
        {
            edit(slot);
            self.session_dirty = true;
        }
    }

    pub fn reset_preset(&mut self, tool: Tool) {
        self.edit_preset(tool, |slot| *slot = ToolSettings::default_for(tool));
        self.status = format!("{} settings reset.", tool.name());
    }

    pub fn palette_click(&mut self, tool: Tool, secondary: bool) {
        match palette_action(tool, secondary) {
            PaletteAction::Select(tool) => {
                self.set_tool(tool);
                self.status = format!("{} — {}", tool.name(), tool.usage());
            }
            PaletteAction::Edit(tool) => {
                self.tool_window = Some(tool);
                self.status = format!(
                    "Settings for {}. The current tool stays {}.",
                    tool.name(),
                    self.tool().name()
                );
            }
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        if self.session.tool != tool {
            self.cancel_gesture();
            self.cut_points.clear();
            self.cut_closed = false;
            if tool != Tool::WarpTarget {
                self.warp_draft.clear();
            }
            self.previous_tool = self.session.tool;
            self.session.tool = tool;
            self.dirty = true;
        }
        self.session_dirty = true;
    }

    pub fn shortcut(&mut self, key: &str) {
        if let Some(tool) = Tool::by_shortcut(key) {
            self.set_tool(tool);
            self.status = format!("{} — {}", tool.name(), tool.usage());
        }
    }

    /// `[` and `]`. Shift steps by 10. The spin stays inside 1..=512, then inside min and max.
    pub fn is_painting(&self) -> bool {
        matches!(self.gesture, Some(Gesture::Paint { .. }))
    }

    pub fn nudge_size(&mut self, step: i32) {
        let tool = self.session.tool;
        let current = self.preset().size.round() as i32;
        let next = (current + step).clamp(1, 512);
        self.edit_preset(tool, |slot| {
            slot.size =
                (next as f32).clamp(slot.size_min.max(0.5), slot.size_max.max(slot.size_min));
        });
        let size = self.preset().size.round() as i32;
        self.status = format!("Size {size}.");
    }

    /// The color every tool paints with.
    pub fn set_ink(&mut self, color: [u8; 3]) {
        if self.ink == color {
            return;
        }
        self.ink = color;
        self.session.note_recent_color(color);
        self.session_dirty = true;
        self.status = format!("Ink {}.", hex_of(color));
    }

    /// X. The two inks trade places. The tool stays put.
    pub fn swap_inks(&mut self) {
        std::mem::swap(&mut self.ink, &mut self.ink_back);
        self.session.note_recent_color(self.ink);
        self.session_dirty = true;
        self.status = format!("Ink {}.", hex_of(self.ink));
    }

    /// C. Empty pixels, not a switch over to the eraser tool.
    pub fn toggle_clear(&mut self) {
        self.clear_ink = !self.clear_ink;
        self.status = if self.clear_ink {
            "Clear on.".into()
        } else {
            "Clear off.".into()
        };
    }

    /// Undo state for a raster stroke: this document with the target layer's old pixels.
    fn art_with_layer_pixels(&self, layer: &str, pixels: Vec<u8>) -> Art {
        let mut previous = self.art.clone();
        if let Some(node) = find_mut(&mut previous.layers, layer) {
            node.pixels = pixels;
        }
        previous
    }

    pub fn layer_rows(&self) -> Vec<LayerRow> {
        let mut rows = Vec::new();
        walk_layers(&self.art.layers, 0, &self.collapsed, 0, &mut rows);
        rows
    }

    /// Open or close a folder. A closed folder hides its children.
    pub fn toggle_folder(&mut self, id: &str) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_string());
        }
    }

    pub fn select_layer(&mut self, id: String) {
        if find(&self.art.layers, &id).is_some() {
            self.selected = Some(id);
        }
    }

    /// Select the layer painted at this document pixel and open the folders around it.
    ///
    /// Python's picture click (`choose_picture` / `select_only`). The window
    /// calls this on a release that stayed inside the drag threshold. A drag
    /// never reaches here, and a miss leaves the selection alone. The name
    /// lands on the status line. Every ancestor opens. The outliner scrolls
    /// to the row on the next pass. One click is one row.
    pub fn choose_at(&mut self, x: f32, y: f32) {
        let Some(id) = vmi_core::pick_layer(&self.art, x, y) else {
            self.status = "No layer there.".into();
            return;
        };
        let name = find(&self.art.layers, &id)
            .map(|node| node.name.clone())
            .unwrap_or_default();
        let mut cursor = Some(id.clone());
        let mut guard = 0;
        while let Some(current) = cursor {
            cursor = parent_of(&self.art.layers, &current);
            if let Some(parent) = &cursor {
                self.collapsed.remove(parent);
            }
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        self.selected = Some(id.clone());
        self.reveal = Some(id);
        self.status = name;
    }

    pub fn open_layer_window(&mut self, id: String) {
        if self.layer_window.as_deref() != Some(&id) {
            self.layer_edit_saved = false;
        }
        self.layer_window = Some(id);
    }

    pub fn open_stroke_window(&mut self, layer: String, stroke: u64) {
        let changed = self
            .stroke_window
            .as_ref()
            .map(|(id, sid)| id != &layer || *sid != stroke)
            .unwrap_or(true);
        if changed {
            self.stroke_edit_saved = false;
        }
        self.stroke_window = Some((layer, stroke));
    }

    pub fn open_object_window(&mut self, id: String) {
        if self.object_window.as_deref() != Some(&id) {
            self.object_edit_saved = false;
        }
        self.object_window = Some(id);
    }

    pub fn close_tool_window(&mut self) {
        self.tool_window = None;
    }

    pub fn close_layer_window(&mut self) {
        self.layer_window = None;
        self.layer_edit_saved = false;
    }

    pub fn close_stroke_window(&mut self) {
        self.stroke_window = None;
        self.stroke_edit_saved = false;
    }

    pub fn close_object_window(&mut self) {
        self.object_window = None;
        self.object_edit_saved = false;
    }

    pub fn open_onion_window(&mut self) {
        self.onion_window = true;
    }

    pub fn close_onion_window(&mut self) {
        self.onion_window = false;
    }

    /// Past is behind the playhead. The colour is a view, stored with the session.
    pub fn set_onion_color(&mut self, past: bool, color: [u8; 3]) {
        if past {
            self.session.onion_past_color = color;
            self.art.onion_past_color = color;
        } else {
            self.session.onion_future_color = color;
            self.art.onion_future_color = color;
        }
        self.session_dirty = true;
        self.dirty = true;
        self.status = "Onion skin color updated.".into();
    }

    /// How many neighbour cels the skin shows. `0` hides that side.
    pub fn set_onion_reach(&mut self, past: u8, future: u8) {
        self.art.onion_past = past.min(vmi_core::ONION_LIMIT);
        self.art.onion_future = future.min(vmi_core::ONION_LIMIT);
        self.dirty = true;
    }

    pub fn edit_layer<F: FnOnce(&mut Node)>(&mut self, id: &str, edit: F) {
        if find(&self.art.layers, id).is_none() {
            return;
        }
        if !self.layer_edit_saved {
            self.history.push(self.art.clone());
            self.layer_edit_saved = true;
        }
        if let Some(node) = find_mut(&mut self.art.layers, id) {
            edit(node);
            self.dirty = true;
        }
    }

    /// Dragging a layer dial. The first change keeps the drawing from before it.
    pub fn preview_opacity(&mut self, id: &str, opacity: u8, before: &mut Option<Art>) {
        let Some(current) = find(&self.art.layers, id).map(|node| node.opacity) else {
            return;
        };
        if current == opacity {
            return;
        }
        if before.is_none() {
            *before = Some(self.art.clone());
        }
        if let Some(node) = find_mut(&mut self.art.layers, id) {
            node.opacity = opacity;
            self.status = format!("{} opacity is {opacity:02X}.", node.name);
        }
        self.dirty = true;
    }

    /// The dial let go. One undo step covers the whole drag.
    pub fn finish_opacity(&mut self, before: Option<Art>) {
        if let Some(before) = before {
            self.history.push(before);
        }
    }

    pub fn update_layer<F: FnOnce(&mut Node)>(&mut self, id: &str, edit: F) {
        if find(&self.art.layers, id).is_none() {
            return;
        }
        let before = self.art.clone();
        if let Some(node) = find_mut(&mut self.art.layers, id) {
            edit(node);
        }
        self.history.push(before);
        self.dirty = true;
    }

    pub fn edit_stroke<F: FnOnce(&mut vmi_core::VectorStroke)>(&mut self, edit: F) {
        let Some((layer_id, stroke_id)) = self.stroke_window.clone() else {
            return;
        };
        if find(&self.art.layers, &layer_id).is_none() {
            return;
        }
        if !self.stroke_edit_saved {
            self.history.push(self.art.clone());
            self.stroke_edit_saved = true;
        }
        let embedded = find(&self.art.layers, &layer_id)
            .map(|node| node.embedded)
            .unwrap_or(false);
        if embedded {
            self.unbake(&layer_id);
            self.status =
                "Baked pixels were cleared so the stroke can redraw from its traits.".into();
        }
        if let Some(node) = find_mut(&mut self.art.layers, &layer_id) {
            if let Some(stroke) = node
                .strokes
                .iter_mut()
                .find(|stroke| stroke.id == stroke_id)
            {
                edit(stroke);
                stroke.touch();
            }
        }
        self.dirty = true;
    }

    pub fn edit_object<F: FnOnce(&mut DeskObject)>(&mut self, edit: F) {
        let Some(id) = self.object_window.clone() else {
            return;
        };
        if !self.art.objects.iter().any(|object| object.id == id) {
            return;
        }
        if !self.object_edit_saved {
            self.history.push(self.art.clone());
            self.object_edit_saved = true;
        }
        if let Some(object) = self.art.objects.iter_mut().find(|object| object.id == id) {
            edit(object);
        }
    }

    pub fn add_layer(&mut self) {
        let before = self.art.clone();
        let parent = self.insertion_parent();
        let id = self.art.add_layer(parent.as_deref(), "Layer");
        self.history.push(before);
        self.selected = Some(id);
        self.dirty = true;
        self.status = "Layer added.".into();
    }

    pub fn add_group(&mut self) {
        let before = self.art.clone();
        let parent = self.insertion_parent();
        let id = self.art.add_group(parent.as_deref(), "Group");
        self.history.push(before);
        self.selected = Some(id);
        self.dirty = true;
        self.status = "Group added.".into();
    }

    fn library_target(&self) -> String {
        let id = self.library_folder.as_str();
        if self
            .art
            .objects
            .iter()
            .any(|object| object.id == id && object.is_folder())
        {
            self.library_folder.clone()
        } else {
            String::new()
        }
    }

    pub fn add_folder(&mut self) {
        let before = self.art.clone();
        let id = self.art.alloc_id();
        let folder = self.library_target();
        let count = self
            .art
            .objects
            .iter()
            .filter(|object| object.is_folder())
            .count()
            + 1;
        let mut object = DeskObject::new(id.clone(), format!("Folder {count}"), Vec::new());
        object.kind = "folder".into();
        object.folder = folder.clone();
        object.stack = vmi_core::next_stack(&self.art.objects, &folder, true);
        self.art.objects.push(object);
        self.history.push(before);
        self.library_folder = id.clone();
        self.object_window = Some(id);
        self.object_edit_saved = false;
        self.status = "Folder added. New objects join it. The top row is behind.".into();
    }

    pub fn add_object(&mut self) {
        let before = self.art.clone();
        let id = self.art.alloc_id();
        let folder = self.library_target();
        let count = self
            .art
            .objects
            .iter()
            .filter(|object| !object.is_folder())
            .count()
            + 1;
        let mut object = DeskObject::new(id.clone(), format!("Object {count}"), Vec::new());
        object.kind = "static".into();
        object.folder = folder.clone();
        object.stack = vmi_core::next_stack(&self.art.objects, &folder, false);
        self.art.objects.push(object);
        self.history.push(before);
        self.object_window = Some(id);
        self.object_edit_saved = false;
        self.status = "Object added. Its settings are below the list.".into();
    }

    /// Drop `from` on `target`. A symbol dropped on a folder joins that folder, in front.
    /// A symbol dropped on a symbol takes that row. The top row is the back.
    pub fn place_on(&mut self, from: &str, target: &str) {
        let Some(source) = self
            .art
            .objects
            .iter()
            .find(|object| object.id == from)
            .cloned()
        else {
            return;
        };
        let Some(dest) = self
            .art
            .objects
            .iter()
            .find(|object| object.id == target)
            .cloned()
        else {
            return;
        };
        if source.id == dest.id || (source.is_folder() && !dest.is_folder()) {
            return;
        }
        let (to_folder, index) = if dest.is_folder() {
            let count = self
                .art
                .objects
                .iter()
                .filter(|object| {
                    object.id != source.id
                        && object.is_folder() == source.is_folder()
                        && object.folder == dest.id
                })
                .count();
            (dest.id, count)
        } else {
            let folder = dest.folder.clone();
            let mut ids: Vec<(i64, String)> = self
                .art
                .objects
                .iter()
                .filter(|object| {
                    object.is_folder() == source.is_folder() && object.folder == folder
                })
                .map(|object| (object.stack, object.id.clone()))
                .collect();
            ids.sort();
            let from_index = ids.iter().position(|(_, id)| id == &source.id);
            let mut index = ids
                .iter()
                .position(|(_, id)| id == &dest.id)
                .unwrap_or(ids.len());
            if let Some(from_index) = from_index {
                if from_index < index {
                    index -= 1;
                }
            }
            (folder, index)
        };
        self.place_symbol(&source.id, &to_folder, index);
    }

    pub fn place_object_in(&mut self, id: &str, to_folder: &str) {
        let Some(object) = self.art.objects.iter().find(|object| object.id == id) else {
            return;
        };
        if object.folder == to_folder {
            return;
        }
        let is_folder = object.is_folder();
        let count = self
            .art
            .objects
            .iter()
            .filter(|object| {
                object.id != id && object.is_folder() == is_folder && object.folder == to_folder
            })
            .count();
        self.place_symbol(id, to_folder, count);
    }

    fn place_symbol(&mut self, id: &str, to_folder: &str, index: usize) {
        let before = self.art.clone();
        if vmi_core::move_symbol(&mut self.art.objects, id, to_folder, index) {
            self.history.push(before);
            self.status = "Depth updated. The top row is behind.".into();
        }
    }

    /// The selected layer becomes this state's picture. The first one is frame 000.
    pub fn give_layer_to_state(&mut self, state: u16) {
        let Some(layer) = self.selected.clone() else {
            self.status = "Select a layer in the outliner.".into();
            return;
        };
        let Some(name) = find(&self.art.layers, &layer).map(|node| node.name.clone()) else {
            self.status = "Select a layer in the outliner.".into();
            return;
        };
        let Some(id) = self.object_window.clone() else {
            return;
        };
        if !self.art.objects.iter().any(|object| object.id == id) {
            return;
        }
        if !self.object_edit_saved {
            self.history.push(self.art.clone());
            self.object_edit_saved = true;
        }
        let mut frame = 0u16;
        let mut stored = false;
        if let Some(object) = self.art.objects.iter_mut().find(|object| object.id == id) {
            if !object.is_folder() {
                frame = object.take_layer(&layer, state);
                stored = true;
            }
        }
        if stored {
            self.status = format!("{name} is on {}.", vmi_core::slot_key(state, frame));
        }
    }

    pub fn reorder_object(&mut self, from: usize, to: usize) {
        if from == to || from >= self.art.objects.len() || to >= self.art.objects.len() {
            return;
        }
        let before = self.art.clone();
        vmi_core::reorder_objects(&mut self.art.objects, from, to);
        self.history.push(before);
    }

    /// A plain 1280×720 drawing in a new unsaved tab.
    pub fn new_document(&mut self) {
        let mut art = Art::new(1280, 720);
        art.file_name = self.fresh_title(1280, 720);
        self.begin_blank(art, false);
        self.status = "New 1280×720 drawing.".into();
    }

    /// Read a file on this thread and show it in its tab. The window uses
    /// `begin_open`, which reads on another thread behind the load gate.
    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        if !path.is_file() {
            return Err("That file is missing.".into());
        }
        let path = absolute(path);
        if let Some(index) = self.tab_at(&path) {
            self.select_tab(Some(index));
            return Ok(());
        }
        let art = read_drawing(&path, &|_| {})?;
        self.stash_shown();
        let index = self.begin_tab(&path, false);
        self.land(index, art);
        Ok(())
    }

    /// Ctrl+S. An open blueprint is overwritten. A drawing is saved beside it.
    pub fn save_blueprint(&mut self, path: &Path) -> Result<(), String> {
        let path = absolute(path);
        write_vmib(&mut self.art, &path)?;
        self.art.path = path.display().to_string();
        self.art.kind = "vmib".into();
        if let Some(name) = path.file_name() {
            self.art.file_name = name.to_string_lossy().to_string();
        }
        if let Some(tab) = self.shown.and_then(|index| self.tabs.get_mut(index)) {
            tab.path = self.art.path.clone();
        }
        self.session.path = self.art.path.clone();
        self.session_dirty = true;
        self.status = format!("Saved {}.", self.art.file_name);
        Ok(())
    }

    /// The blueprint path Ctrl+S writes without asking: an open blueprint's own.
    pub fn blueprint_path(&self) -> Option<PathBuf> {
        if self.art.kind == "vmib" && !self.art.path.is_empty() {
            Some(PathBuf::from(&self.art.path))
        } else {
            None
        }
    }

    /// Where Save blueprint as starts: beside the drawing, as `<stem>.vmib`.
    pub fn suggested_blueprint(&self) -> PathBuf {
        let start = if self.art.path.is_empty() {
            home_dir()
        } else {
            Path::new(&self.art.path)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(home_dir)
        };
        let name = if self.art.file_name.is_empty() {
            "project"
        } else {
            self.art.file_name.as_str()
        };
        let stem = match name.rfind('.') {
            Some(dot) if dot > 0 => &name[..dot],
            _ => name,
        };
        start.join(format!("{stem}.vmib"))
    }

    /// File → New picture, and the launcher button. The plate comes from the
    /// New picture dialog; Paper sits under Ink, the 2kool grid on top.
    pub fn open_blank(
        &mut self,
        width: i64,
        height: i64,
        background: &str,
        texture: Option<&Path>,
        grid: bool,
    ) -> Result<(), String> {
        if self.loading.is_some() {
            self.status = "Still reading.".into();
            return Ok(());
        }
        let (wide, high) = vmi_core::newpage::parse_plate(width, height)?;
        let title = self.fresh_title(wide, high);
        let art = blank_picture(width, height, &title, background, texture, grid)?;
        let note = opened_note(&art);
        self.begin_blank(art, true);
        let ink = self
            .art
            .layers
            .iter()
            .find(|node| node.name == "Ink")
            .or_else(|| self.art.layers.first())
            .map(|node| node.id.clone());
        if let Some(id) = ink {
            self.selected = Some(id);
        }
        self.status = note;
        Ok(())
    }

    /// "Untitled W×H", then "Untitled W×H 2"... past every open drawing's name.
    fn fresh_title(&self, width: u32, height: u32) -> String {
        let base = format!("Untitled {width}×{height}");
        let mut names: Vec<&str> = self
            .tabs
            .iter()
            .filter_map(|tab| tab.stash.as_ref().map(|stash| stash.art.file_name.as_str()))
            .collect();
        if !self.art.file_name.is_empty() && self.shown.is_some() {
            names.push(self.art.file_name.as_str());
        }
        if !names.contains(&base.as_str()) {
            return base;
        }
        let mut number = 2;
        while names.contains(&format!("{base} {number}").as_str()) {
            number += 1;
        }
        format!("{base} {number}")
    }

    /// A tab with no path. Save blueprint asks where the file goes.
    fn begin_blank(&mut self, art: Art, low_res: bool) {
        self.stash_shown();
        let mut tab = OpenTab::unsaved();
        tab.low_res = low_res;
        self.tabs.push(tab);
        let index = self.tabs.len() - 1;
        self.shown = Some(index);
        self.on_browser = false;
        self.land(index, art);
    }

    /// Add a project tab to the right of the browser. Returns the tab index.
    fn begin_tab(&mut self, path: &Path, low_res: bool) -> usize {
        let mut tab = OpenTab::at(path);
        tab.low_res = low_res;
        self.tabs.push(tab);
        let index = self.tabs.len() - 1;
        self.shown = Some(index);
        self.on_browser = false;
        index
    }

    /// The read drawing lands on the desk in its tab.
    fn land(&mut self, index: usize, mut art: Art) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if art.path.is_empty() && !tab.path.is_empty() {
            art.path = tab.path.clone();
        }
        tab.loaded = true;
        tab.stash = None;
        let low_res = tab.low_res;
        self.stop();
        self.finish_new(art);
        self.place_picture(low_res);
        self.shown = Some(index);
        self.on_browser = false;
        self.session.path = self.art.path.clone();
        self.session_dirty = true;
        self.status = self.opened_text();
    }

    /// "name  W×H  note", plus the warp counts when the drawing has any.
    fn opened_text(&self) -> String {
        let note = if self.art.note.is_empty() {
            "Opened."
        } else {
            self.art.note.as_str()
        };
        let mut text = format!(
            "{}  {}×{}  {}",
            self.art.file_name, self.art.width, self.art.height, note
        );
        let (targets, windows) = warp_counts(&self.art.layers);
        if targets > 0 || windows > 0 {
            text.push_str(&format!("  Warp targets: {targets}. Windows: {windows}."));
        }
        text
    }

    /// High res keeps every document pixel. Low res fits a smaller plate.
    fn place_picture(&mut self, low_res: bool) {
        if low_res {
            self.fitted = false;
        } else {
            self.actual();
        }
    }

    /// Open from the launcher or a path: the file is read on its own thread
    /// while the load gate holds the desk. True when a read began.
    pub fn begin_open(&mut self, path: &Path, low_res: bool) -> bool {
        if self.loading.is_some() {
            self.status = "Still reading.".into();
            return false;
        }
        let path = absolute(path);
        let name = basename(&path);
        if let Some(index) = self.tab_at(&path) {
            if low_res {
                self.tabs[index].low_res = true;
            }
            self.select_tab(Some(index));
            self.status = format!("{name} is already open.");
            return false;
        }
        self.stash_shown();
        let index = self.begin_tab(&path, low_res);
        self.status = format!("Reading {name}");
        self.load_events.push(LoadEvent::Started(name));
        let (tx, rx): (Sender<LoadMsg>, Receiver<LoadMsg>) = mpsc::channel();
        std::thread::spawn(move || {
            let notes = tx.clone();
            let result = read_drawing(&path, &|text| {
                let _ = notes.send(LoadMsg::Note(text.to_string()));
            });
            let _ = tx.send(LoadMsg::Done(result));
        });
        self.loading = Some(Loading { index, rx });
        true
    }

    /// A file is being read.
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Hear the loading thread. Call once per frame; the events come out of
    /// `take_load_events`.
    pub fn poll_load(&mut self) {
        let Some(loading) = self.loading.as_ref() else {
            return;
        };
        let index = loading.index;
        let finished = loop {
            match loading.rx.try_recv() {
                Ok(LoadMsg::Note(text)) => {
                    self.status = text.clone();
                    self.load_events.push(LoadEvent::Note(text));
                }
                Ok(LoadMsg::Done(result)) => break result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Err("The file could not be read.".into())
                }
            }
        };
        self.loading = None;
        match finished {
            Ok(art) => {
                self.land(index, art);
                self.load_events.push(LoadEvent::Shown);
            }
            Err(message) => self.open_failed(index, message),
        }
    }

    pub fn take_load_events(&mut self) -> Vec<LoadEvent> {
        std::mem::take(&mut self.load_events)
    }

    /// The read failed. Its tab leaves; the next one comes up, or the browser.
    fn open_failed(&mut self, index: usize, message: String) {
        if index < self.tabs.len() && !self.tabs[index].loaded {
            self.tabs.remove(index);
            self.shown = None;
            if self.tabs.is_empty() {
                self.clear_editor();
                self.on_browser = true;
            } else {
                let next = index.min(self.tabs.len() - 1);
                if self.tabs[next].loaded {
                    self.show_tab(next);
                } else {
                    self.shown = Some(next);
                    self.on_browser = false;
                }
            }
        }
        eprintln!("open failed: {message}");
        self.status = message.clone();
        self.load_events.push(LoadEvent::Failed(message));
    }

    /// The tab that holds `path`, when one does.
    pub fn tab_at(&self, path: &Path) -> Option<usize> {
        let path = absolute(path).display().to_string();
        self.tabs
            .iter()
            .position(|tab| tab.loaded && tab.path == path)
    }

    /// The tab bar: None is the browser, Some is a project tab.
    pub fn select_tab(&mut self, index: Option<usize>) {
        if self.loading.is_some() {
            self.status = "Still reading.".into();
            return;
        }
        match index {
            None => {
                self.stash_shown();
                self.on_browser = true;
                self.stop();
            }
            Some(index) => {
                if index >= self.tabs.len() {
                    return;
                }
                self.on_browser = false;
                if self.shown != Some(index) {
                    self.stash_shown();
                }
                if self.tabs[index].loaded
                    && (self.shown != Some(index) || self.tabs[index].stash.is_some())
                {
                    self.show_tab(index);
                } else {
                    self.shown = Some(index);
                }
            }
        }
    }

    /// File → Launcher. The leftmost tab stays, and File → Open selects it.
    pub fn show_browser(&mut self) {
        self.select_tab(None);
    }

    /// The close mark on a tab.
    pub fn close_tab(&mut self, index: usize) {
        if self.loading.is_some() {
            self.status = "Still reading.".into();
            return;
        }
        if index >= self.tabs.len() {
            return;
        }
        self.stash_shown();
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.shown = None;
            self.clear_editor();
            self.on_browser = true;
            return;
        }
        let next = index.min(self.tabs.len() - 1);
        self.shown = None;
        if self.on_browser {
            // The browser stays up; the next tab comes up when it is picked.
            return;
        }
        if self.tabs[next].loaded {
            self.show_tab(next);
        } else {
            self.shown = Some(next);
        }
    }

    /// Put the shown drawing away in its tab, so another can come up.
    fn stash_shown(&mut self) {
        let Some(index) = self.shown else {
            return;
        };
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if !tab.loaded || tab.stash.is_some() {
            return;
        }
        if !self.art.path.is_empty() && self.art.path != tab.path {
            return;
        }
        self.cancel_gesture();
        let art = std::mem::replace(&mut self.art, Art::blank(1, 1));
        let tab = &mut self.tabs[index];
        tab.stash = Some(Box::new(Stash {
            art,
            history: std::mem::take(&mut self.history),
            selected: self.selected.take(),
            camera: self.camera,
            fitted: self.fitted,
            cut_points: std::mem::take(&mut self.cut_points),
            cut_closed: std::mem::replace(&mut self.cut_closed, false),
            warp_draft: std::mem::take(&mut self.warp_draft),
            warp_layer: self.warp_layer.take(),
            sheet_pick: self.sheet_pick.take(),
        }));
        self.overlay_live = false;
        self.ensure_overlay();
        self.dirty = true;
    }

    /// Bring a tab's drawing back onto the desk.
    fn show_tab(&mut self, index: usize) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let Some(stash) = tab.stash.take() else {
            self.shown = Some(index);
            self.on_browser = false;
            return;
        };
        let name = stash.art.file_name.clone();
        let low_res = tab.low_res;
        self.stop();
        self.cancel_gesture();
        let Stash {
            art,
            history,
            selected,
            camera,
            fitted,
            cut_points,
            cut_closed,
            warp_draft,
            warp_layer,
            sheet_pick,
        } = *stash;
        self.art = art;
        self.history = history;
        self.selected = selected;
        self.camera = camera;
        self.fitted = fitted;
        self.cut_points = cut_points;
        self.cut_closed = cut_closed;
        self.warp_draft = warp_draft;
        self.warp_layer = warp_layer;
        self.sheet_pick = sheet_pick;
        self.layer_window = None;
        self.stroke_window = None;
        self.object_window = None;
        self.clip_focus = None;
        self.overlay_live = false;
        self.ensure_overlay();
        self.overlay.fill(0);
        self.overlay_mark = None;
        self.preview_snap.clear();
        self.stroke_live = false;
        self.stroke_erase = false;
        self.stroke_layer.clear();
        self.dirty = true;
        self.place_picture(low_res);
        self.shown = Some(index);
        self.on_browser = false;
        self.session.path = self.art.path.clone();
        self.session_dirty = true;
        self.status = format!(
            "{}  {}×{}",
            self.art.file_name, self.art.width, self.art.height
        );
        // Dim the studio until this plate is ready, like the Python's catch-up gate.
        self.load_events.push(LoadEvent::Catchup(name));
        self.load_events.push(LoadEvent::Shown);
    }

    fn clear_editor(&mut self) {
        self.stop();
        self.finish_new(Art::blank(1, 1));
        self.selected = None;
        self.status = "Open a .clip, .psd, .kra, .xcf, or .vmib file.".into();
    }

    /// The export parent folder of the shown tab.
    pub fn export_dir(&self) -> PathBuf {
        if let Some(tab) = self.shown.and_then(|index| self.tabs.get(index)) {
            return tab.export_dir.clone();
        }
        if !self.art.path.is_empty() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            return default_export_parent(Path::new(&self.art.path), &cwd);
        }
        home_dir()
    }

    /// Remember this tab's export parent. Refuse another tab's output folder.
    pub fn claim_export_dir(&mut self, parent: &Path) -> bool {
        let Some(index) = self.shown else {
            return true;
        };
        if index >= self.tabs.len() {
            return true;
        }
        let name = if self.art.file_name.is_empty() {
            basename(Path::new(&self.tabs[index].path))
        } else {
            self.art.file_name.clone()
        };
        for (other_index, tab) in self.tabs.iter().enumerate() {
            if other_index == index {
                continue;
            }
            let other_name = match &tab.stash {
                Some(stash) => stash.art.file_name.clone(),
                None => basename(Path::new(&tab.path)),
            };
            if same_export(parent, &name, &tab.export_dir, &other_name) {
                let who = if other_name == name {
                    tab.path.clone()
                } else {
                    other_name
                };
                self.status =
                    format!("That folder is the export for {who}. This tab needs its own path.");
                return false;
            }
        }
        self.tabs[index].export_dir = absolute(parent);
        true
    }

    /// The tab bar's labels: the file name of each drawing, left to right.
    pub fn tab_names(&self) -> Vec<String> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                if Some(index) == self.shown && tab.stash.is_none() && tab.loaded {
                    self.art.file_name.clone()
                } else if let Some(stash) = &tab.stash {
                    stash.art.file_name.clone()
                } else {
                    basename(Path::new(&tab.path))
                }
            })
            .collect()
    }

    pub fn export_png_file(&mut self, path: &Path) -> Result<(), String> {
        let percent = self.session.export_percent.clamp(1, 100);
        export_png(&mut self.art, path, percent)?;
        self.session_dirty = true;
        self.status = format!("Exported {} at {percent}%.", path.display());
        Ok(())
    }

    /// The window changed the session directly. Save it at the end of the frame.
    pub fn touch_session(&mut self) {
        self.session_dirty = true;
    }

    pub fn flush_session(&mut self) {
        if self.persist && self.session_dirty {
            if save_session(&self.session).is_ok() {
                self.session_dirty = false;
            }
        }
    }

    pub fn undo(&mut self) {
        if self.gesture.is_some() {
            return;
        }
        let current = self.art.clone();
        if let Some(previous) = self.history.undo(current) {
            self.art = previous;
            self.after_history();
            self.status = "Undo.".into();
        }
    }

    pub fn redo(&mut self) {
        if self.gesture.is_some() {
            return;
        }
        let current = self.art.clone();
        if let Some(next) = self.history.redo(current) {
            self.art = next;
            self.after_history();
            self.status = "Redo.".into();
        }
    }

    pub fn undo_depth(&self) -> usize {
        self.history.len()
    }

    pub fn down_doc(&mut self, x: f32, y: f32, button: Button) {
        if button == Button::Secondary {
            self.right_doc(x, y);
            return;
        }
        if button != Button::Primary {
            return;
        }
        match self.tool() {
            Tool::Select => self.choose_at(x, y),
            Tool::Point => self.begin_point(x, y),
            Tool::Bone => self.begin_bone(x, y),
            Tool::WarpTarget => self.add_warp_corner(x, y),
            Tool::CutRect => self.begin_cut_rect(x, y),
            Tool::CutLasso => self.begin_cut_lasso(x, y),
            Tool::CutPolygon => self.add_cut_corner(x, y),
            _ => self.begin_paint(x, y),
        }
    }

    pub fn move_doc(&mut self, x: f32, y: f32) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        self.gesture = Some(self.drag(gesture, x, y));
    }

    /// Pressure for the samples that follow. The window feeds this from the
    /// pen when the platform reports it.
    pub fn set_pressure(&mut self, pressure: f32) {
        self.pressure = if pressure.is_finite() {
            pressure.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    fn touch_rect(&mut self, rect: Option<[i32; 4]>) {
        let Some(rect) = rect else {
            self.dirty = true;
            return;
        };
        self.dirty_rect = Some(match self.dirty_rect {
            Some(have) => [
                have[0].min(rect[0]),
                have[1].min(rect[1]),
                have[2].max(rect[2]),
                have[3].max(rect[3]),
            ],
            None => rect,
        });
    }

    pub fn up_doc(&mut self, x: f32, y: f32) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        // A raster stroke is already on the plate in the rectangles it touched.
        // Uploading the whole picture here is the hitch at the start of the next line.
        let raster =
            matches!(&gesture, Gesture::Paint { tool, .. } if tool.release() != Release::Stroke);
        let gesture = if needs_last_drag(&gesture, x, y) {
            self.drag(gesture, x, y)
        } else {
            gesture
        };
        self.finish(gesture);
        if !raster {
            self.dirty = true;
        }
    }

    /// End a stroke when the release has no pointer position.
    pub fn release_doc(&mut self) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        let raster =
            matches!(&gesture, Gesture::Paint { tool, .. } if tool.release() != Release::Stroke);
        self.finish(gesture);
        if !raster {
            self.dirty = true;
        }
    }

    pub fn enter(&mut self) {
        match self.tool() {
            Tool::WarpTarget => self.commit_warp(),
            Tool::CutPolygon => {
                if self.cut_points.len() >= 3 {
                    self.cut_closed = true;
                    self.status = "Selection closed.".into();
                } else {
                    self.status = "Click at least three corners.".into();
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn escape(&mut self) {
        if self.gesture.is_some() {
            self.cancel_gesture();
            self.status = "Stroke cancelled.".into();
            return;
        }
        if !self.cut_points.is_empty() || !self.warp_draft.is_empty() {
            self.cut_points.clear();
            self.cut_closed = false;
            self.warp_draft.clear();
            self.status = "Selection cleared.".into();
            self.dirty = true;
            return;
        }
        if self.tool().section() == Section::Cut {
            self.toggle_cut();
            return;
        }
        if self.clip_focus.is_some() {
            self.leave_clip();
        }
    }

    pub fn backspace(&mut self) {
        if self.tool() == Tool::CutPolygon && !self.cut_closed && !self.cut_points.is_empty() {
            self.cut_points.pop();
            self.dirty = true;
        }
    }

    pub fn toggle_cut(&mut self) {
        if self.tool().section() == Section::Cut {
            let back = if self.previous_tool.section() == Section::Cut {
                Tool::Pen
            } else {
                self.previous_tool
            };
            self.set_tool(back);
            self.status = "Cut mode off.".into();
        } else {
            self.set_tool(Tool::CutRect);
            self.status = "Cut mode on. Drag a rectangle, then Cut to folder or Delete.".into();
        }
    }

    pub fn cut_ready(&self) -> bool {
        self.cut_points.len() >= 3 && (self.tool() != Tool::CutPolygon || self.cut_closed)
    }

    pub fn cut_to_folder(&mut self) {
        if !self.cut_ready() {
            self.status = "Draw a selection first.".into();
            return;
        }
        let Some(selected) = self.selected.clone() else {
            self.status = "Select a layer.".into();
            return;
        };
        let before = self.art.clone();
        let points = self.cut_points.clone();
        match cut_folder(&mut self.art, &selected, &points, "Cut") {
            Some(id) => {
                self.history.push(before);
                self.selected = Some(id);
                self.status = "Cut into a folder.".into();
                self.dirty = true;
            }
            None => self.status = "Nothing in that selection.".into(),
        }
    }

    pub fn delete_selection(&mut self) {
        if self.cut_points.len() < 3 {
            self.status = "Draw a selection first.".into();
            return;
        }
        let Some(selected) = self.selected.clone() else {
            self.status = "Select a layer.".into();
            return;
        };
        let before = self.art.clone();
        let count = erase_folder(&mut self.art, &selected, &self.cut_points);
        if count == 0 {
            self.status = "Nothing in that selection.".into();
            return;
        }
        self.history.push(before);
        self.status = "Cleared the selection.".into();
        self.dirty = true;
    }

    pub fn commit_warp(&mut self) {
        let Some(layer_id) = self.warp_layer.clone().or(self.selected.clone()) else {
            self.status = "Select a layer.".into();
            return;
        };
        if find(&self.art.layers, &layer_id)
            .map(|node| !node.is_layer())
            .unwrap_or(true)
        {
            self.status = "Select a layer.".into();
            return;
        }
        if self.warp_draft.len() < 3 {
            self.status = "Click at least three corners.".into();
            return;
        }
        let before = self.art.clone();
        let color = self.preset_of(Tool::WarpTarget).color;
        let points = self.warp_draft.clone();
        let (width, height) = (self.art.width, self.art.height);
        if let Some(node) = find_mut(&mut self.art.layers, &layer_id) {
            fill_polygon(&mut node.pixels, width, height, &points, color);
            node.vectors = vec![points];
            node.marker = "target".into();
            node.color = hex_of(color);
            node.embedded = false;
        }
        self.history.push(before);
        self.warp_draft.clear();
        self.status = "Warp target kept. The corners refill the layer.".into();
        self.dirty = true;
    }

    pub fn fit(&mut self, frame: Frame) {
        if frame.w < 2.0 || frame.h < 2.0 || self.art.width == 0 || self.art.height == 0 {
            return;
        }
        let zoom = (frame.w / self.art.width as f32)
            .min(frame.h / self.art.height as f32)
            .max(0.01);
        self.camera = Camera {
            zoom,
            pan_x: 0.0,
            pan_y: 0.0,
            rotation: 0.0,
        };
        self.fitted = true;
        self.last_frame = frame;
    }

    pub fn fit_once(&mut self, frame: Frame) {
        self.last_frame = frame;
        if !self.fitted {
            self.fit(frame);
        }
    }

    pub fn actual(&mut self) {
        self.camera = Camera {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            rotation: 0.0,
        };
        self.fitted = true;
    }

    pub fn zoom_at_center(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.camera.zoom = (self.camera.zoom * factor).clamp(0.05, 32.0);
        }
    }

    /// Zoom so the document point under screen `(sx, sy)` stays under it.
    pub fn zoom_at(&mut self, frame: Frame, sx: f32, sy: f32, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = self.camera.zoom;
        let after = (before * factor).clamp(0.05, 32.0);
        let applied = after / before;
        let (cx, cy) = frame.center();
        let rx = sx - cx - self.camera.pan_x;
        let ry = sy - cy - self.camera.pan_y;
        self.camera.pan_x = sx - cx - rx * applied;
        self.camera.pan_y = sy - cy - ry * applied;
        self.camera.zoom = after;
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.camera.pan_x += dx;
        self.camera.pan_y += dy;
    }

    /// Turn the picture about the view center. Pan orbits with the angle,
    /// so a panned document stays under the middle of the viewfinder.
    pub fn rotate_about_view(&mut self, delta: f32) {
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        let (sin, cos) = delta.sin_cos();
        let px = self.camera.pan_x;
        let py = self.camera.pan_y;
        self.camera.pan_x = px * cos - py * sin;
        self.camera.pan_y = px * sin + py * cos;
        self.camera.rotation += delta;
    }

    /// Drop the stroke in progress and put the layer back. Nothing else changes.
    pub fn escape_stroke(&mut self) {
        if self.gesture.is_some() {
            self.cancel_gesture();
        }
    }

    pub fn set_export_percent(&mut self, percent: u32) {
        self.session.export_percent = percent.clamp(1, 100);
        self.session_dirty = true;
    }

    pub fn note_widths(&mut self, layers: f32, brushes: f32) {
        if (self.session.layers_width - layers).abs() > 0.5
            || (self.session.brush_width - brushes).abs() > 0.5
        {
            self.session.layers_width = layers;
            self.session.brush_width = brushes;
            self.session_dirty = true;
        }
    }

    /// The right and bottom areas' sizes, and which view each area shows.
    pub fn note_areas(&mut self, right: f32, bottom: f32, views: &[String]) {
        if (self.session.right_width - right).abs() > 0.5
            || (self.session.bottom_height - bottom).abs() > 0.5
        {
            self.session.right_width = right;
            self.session.bottom_height = bottom;
            self.session_dirty = true;
        }
        if self.session.areas != views {
            self.session.areas = views.to_vec();
            self.session_dirty = true;
        }
    }

    /// The whole picture over its checker. Also refreshes the cached plate.
    pub fn display_rgba(&mut self) -> Vec<u8> {
        match self.take_display(true) {
            Some(Display::Full(rgba)) => rgba,
            _ => self.plate.clone(),
        }
    }

    /// What changed since the last call, or `None` when nothing did. A pen
    /// move hands back only the rectangle it touched.
    pub fn take_display(&mut self, force_full: bool) -> Option<Display> {
        let width = self.art.width;
        let height = self.art.height;
        let need = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        // Playback samples a smaller plate and leaves the full-res cache alone.
        // A live stroke still wants the real pixels, so it takes the full plate.
        if self.playing && !force_full && !self.stroke_live && !self.overlay_live {
            self.dirty = false;
            self.dirty_rect = None;
            self.art.onion_past_color = self.session.onion_past_color;
            self.art.onion_future_color = self.session.onion_future_color;
            let (w, h, rgba) = present_proxy(&mut self.art, PLAY_SIDE);
            let rgba = with_checker_at(&rgba, w, h, 0, 0);
            return Some(Display::Play { w, h, rgba });
        }
        let full = force_full || self.dirty || self.plate.len() != need || self.playing;
        let rect = self.dirty_rect.take();
        if full {
            self.dirty = false;
            let rgba = self.compose([0, 0, width as i32, height as i32]);
            self.plate = rgba.clone();
            return Some(Display::Full(rgba));
        }
        let rect = clamp_rect(rect?, width, height);
        let w = (rect[2] - rect[0]).max(0) as u32;
        let h = (rect[3] - rect[1]).max(0) as u32;
        if w == 0 || h == 0 {
            return None;
        }
        let rgba = self.compose(rect);
        for row in 0..h as usize {
            let src = row * w as usize * 4;
            let dst = ((rect[1] as usize + row) * width as usize + rect[0] as usize) * 4;
            self.plate[dst..dst + w as usize * 4].copy_from_slice(&rgba[src..src + w as usize * 4]);
        }
        Some(Display::Part {
            x: rect[0] as u32,
            y: rect[1] as u32,
            w,
            h,
            rgba,
        })
    }

    fn compose(&mut self, rect: [i32; 4]) -> Vec<u8> {
        let width = self.art.width;
        let height = self.art.height;
        // Undo restores a drawing and would otherwise put the default colours back.
        self.art.onion_past_color = self.session.onion_past_color;
        self.art.onion_future_color = self.session.onion_future_color;
        if self.stroke_live {
            self.blend_preview(rect);
        }
        let mut plate = present_region(&mut self.art, rect);
        let rw = (rect[2] - rect[0]).max(0) as u32;
        let rh = (rect[3] - rect[1]).max(0) as u32;
        let need = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        // A vector preview is the overlay. A raster stroke is blended for this rect, then put back.
        if self.overlay_live && !self.stroke_live && self.overlay.len() == need {
            blit_into(
                &mut plate,
                rw,
                rh,
                rect[0],
                rect[1],
                &self.overlay,
                width,
                height,
                0,
                0,
                255,
                false,
                "normal",
                1,
            );
        }
        if self.stroke_live {
            self.restore_preview();
        }
        with_checker_at(&plate, rw, rh, rect[0], rect[1])
    }

    /// Show the stroke on the layer for `rect` without keeping a copy of the whole layer.
    /// The clean pixels come back in `restore_preview`.
    fn blend_preview(&mut self, rect: [i32; 4]) {
        let width = self.art.width;
        let height = self.art.height;
        let layer = self.stroke_layer.clone();
        let opacity = self.stroke_opacity;
        let erase = self.stroke_erase;
        let snap = find(&self.art.layers, &layer)
            .map(|node| copy_tight(&node.pixels, width, height, rect))
            .unwrap_or_default();
        self.preview_rect = rect;
        self.preview_snap = snap;
        let overlay = std::mem::take(&mut self.overlay);
        if let Some(node) = find_mut(&mut self.art.layers, &layer) {
            blend_overlay(
                &mut node.pixels,
                &overlay,
                width,
                height,
                rect,
                opacity,
                erase,
            );
        }
        self.overlay = overlay;
    }

    fn restore_preview(&mut self) {
        if self.preview_snap.is_empty() {
            return;
        }
        let width = self.art.width;
        let height = self.art.height;
        let rect = self.preview_rect;
        let layer = self.stroke_layer.clone();
        let snap = std::mem::take(&mut self.preview_snap);
        if let Some(node) = find_mut(&mut self.art.layers, &layer) {
            paste_tight(&mut node.pixels, &snap, width, height, rect);
        }
    }

    /// Write the stroke onto the clean layer where the buffer has ink.
    fn commit_overlay(&mut self, layer: &str) {
        let width = self.art.width;
        let height = self.art.height;
        let opacity = self.stroke_opacity;
        let erase = self.stroke_erase;
        let rect = self
            .overlay_mark
            .unwrap_or([0, 0, width as i32, height as i32]);
        if self.overlay_mark.is_none() {
            self.overlay_mark = Some(rect);
        }
        let overlay = std::mem::take(&mut self.overlay);
        if let Some(node) = find_mut(&mut self.art.layers, layer) {
            blend_overlay(
                &mut node.pixels,
                &overlay,
                width,
                height,
                rect,
                opacity,
                erase,
            );
        }
        self.overlay = overlay;
        self.end_live_stroke();
    }

    fn end_live_stroke(&mut self) {
        self.clear_marked_overlay();
        self.stroke_live = false;
        self.stroke_erase = false;
        self.stroke_layer.clear();
        self.preview_snap.clear();
        self.overlay_live = false;
    }

    fn note_overlay(&mut self, rect: Option<[i32; 4]>) {
        let Some(rect) = rect else {
            return;
        };
        self.overlay_mark = Some(match self.overlay_mark {
            Some(have) => [
                have[0].min(rect[0]),
                have[1].min(rect[1]),
                have[2].max(rect[2]),
                have[3].max(rect[3]),
            ],
            None => rect,
        });
    }

    fn clear_marked_overlay(&mut self) {
        let width = self.art.width;
        let height = self.art.height;
        if let Some(rect) = self.overlay_mark.take() {
            zero_rect(&mut self.overlay, width, height, rect);
        }
    }

    pub fn marks(&self) -> Vec<Mark> {
        let mut marks = Vec::new();
        for point in &self.cut_points {
            marks.push(Mark {
                x: point[0],
                y: point[1],
                hot: false,
                line_to: None,
            });
        }
        for point in &self.warp_draft {
            marks.push(Mark {
                x: point[0],
                y: point[1],
                hot: true,
                line_to: None,
            });
        }
        let Some(id) = &self.selected else {
            return marks;
        };
        let Some(node) = find(&self.art.layers, id) else {
            return marks;
        };
        if self.warp_draft.is_empty() && matches!(self.tool(), Tool::WarpTarget | Tool::Point) {
            if let Some(poly) = node.vectors.first() {
                for point in poly {
                    marks.push(Mark {
                        x: point[0],
                        y: point[1],
                        hot: true,
                        line_to: None,
                    });
                }
            }
        }
        // The pen keeps its points to itself. The Point tool is the editor.
        if self.tool() == Tool::Point {
            for stroke in &node.strokes {
                for vertex in &stroke.vertices {
                    marks.push(Mark {
                        x: vertex.x,
                        y: vertex.y,
                        hot: true,
                        line_to: None,
                    });
                    if let Some(handle) = vertex.handle_in {
                        marks.push(Mark {
                            x: handle[0],
                            y: handle[1],
                            hot: false,
                            line_to: Some((vertex.x, vertex.y)),
                        });
                    }
                    if let Some(handle) = vertex.handle_out {
                        marks.push(Mark {
                            x: handle[0],
                            y: handle[1],
                            hot: false,
                            line_to: Some((vertex.x, vertex.y)),
                        });
                    }
                }
            }
        }
        marks
    }

    fn begin_paint(&mut self, x: f32, y: f32) {
        let Ok(layer) = self.paint_target() else {
            self.status = "Select a layer.".into();
            return;
        };
        let tool = self.tool();
        let mut settings = self.preset().clone();
        // One ink. The preset color is not consulted, so the tool cannot change it.
        settings.color = self.ink;
        let (width, height) = (self.art.width, self.art.height);
        if tool == Tool::Fill {
            let before = self.art.clone();
            let changed = find_mut(&mut self.art.layers, &layer)
                .map(|node| {
                    flood_fill(
                        &mut node.pixels,
                        width,
                        height,
                        x.round() as i32,
                        y.round() as i32,
                        &settings,
                    )
                })
                .unwrap_or(false);
            if changed {
                self.history.push(before);
                self.status = "Fill wrote pixels.".into();
                self.dirty = true;
            }
            return;
        }
        self.prepare_layer(&layer);
        // A vector stroke needs the whole document, because release edits the stroke list.
        // A raster pen leaves the layer alone until the pen lifts. Copying it on the
        // way down is the straight, late first segment: the pen moves while that copy runs.
        let vector = tool.release() == Release::Stroke;
        let live = !vector
            && (stroke_opacity_tool(tool) || (self.clear_ink && tool.release() == Release::Pixels));
        let before = if vector { Some(self.art.clone()) } else { None };
        let backup = if vector || live {
            Vec::new()
        } else {
            find(&self.art.layers, &layer)
                .map(|node| node.pixels.clone())
                .unwrap_or_default()
        };
        let base = Vec::new();
        let mut session = StrokeSession::default();
        let pressure = self.pressure;
        session.begin(x, y, pressure);
        let mut changed = false;
        let first = (x, y, pressure);
        if vector {
            self.ensure_overlay();
            self.clear_marked_overlay();
            let mut carry = Carry::default();
            paint_samples(
                &mut self.overlay,
                width,
                height,
                &session.samples,
                tool,
                &settings,
                &mut carry,
            );
            self.overlay_live = true;
            changed = true;
            self.note_overlay(segment_bounds(width, height, first, first, tool, &settings));
            self.dirty = true;
        } else if live {
            // Dabs build one shape in the stroke buffer. The opacity knob composites
            // that shape once, on release. The layer stays as it was until then.
            self.ensure_overlay();
            self.clear_marked_overlay();
            self.stroke_live = true;
            self.stroke_opacity = settings.opacity.clamp(0.0, 1.0);
            self.stroke_erase = tool == Tool::Eraser || (self.clear_ink && tool != Tool::Line);
            self.stroke_layer = layer.clone();
            let flow = flow_settings(&settings);
            let paint_tool = buffer_tool(tool);
            let mut carry = Carry::default();
            paint_samples(
                &mut self.overlay,
                width,
                height,
                &session.samples,
                paint_tool,
                &flow,
                &mut carry,
            );
            changed = true;
            let bounds = segment_bounds(width, height, first, first, paint_tool, &flow);
            self.note_overlay(bounds);
            self.touch_rect(bounds);
        } else if let Some(node) = find_mut(&mut self.art.layers, &layer) {
            changed = paint_samples(
                &mut node.pixels,
                width,
                height,
                &session.samples,
                tool,
                &settings,
                &mut session.carry,
            );
            self.touch_rect(segment_bounds(width, height, first, first, tool, &settings));
        }
        self.gesture = Some(Gesture::Paint {
            layer,
            tool,
            settings,
            before,
            backup,
            base,
            session,
            changed,
        });
    }

    fn begin_point(&mut self, x: f32, y: f32) {
        let Ok(layer) = self.paint_target() else {
            self.status = "Select a layer.".into();
            return;
        };
        let pick = self.preset_of(Tool::Point).pick_radius;
        if let Some((stroke, which)) = self.stroke_hit(&layer, x, y, pick) {
            if matches!(which, StrokeHit::Body) {
                self.status = "Drag a point or a handle.".into();
                return;
            }
            let before = self.art.clone();
            self.gesture = Some(Gesture::Point {
                layer,
                stroke,
                which,
                before,
                changed: false,
            });
            return;
        }
        if let Some(index) = self.nearest_corner(&layer, x, y) {
            let before = self.art.clone();
            self.gesture = Some(Gesture::WarpCorner {
                layer,
                index,
                before,
                changed: false,
            });
            return;
        }
        self.status = "No point under the pen.".into();
    }

    fn begin_bone(&mut self, x: f32, y: f32) {
        let Ok(layer) = self.paint_target() else {
            self.status = "Select a layer.".into();
            return;
        };
        let pick = self.preset_of(Tool::Bone).bone_reach.min(48.0).max(8.0);
        let Some((stroke, _)) = self.stroke_hit(&layer, x, y, pick) else {
            self.status = "Drag across a vector stroke.".into();
            return;
        };
        let before = self.art.clone();
        self.gesture = Some(Gesture::Bone {
            layer,
            stroke,
            before,
            x0: x,
            y0: y,
            changed: false,
        });
    }

    fn add_warp_corner(&mut self, x: f32, y: f32) {
        let Ok(layer) = self.paint_target() else {
            self.status = "Select a layer.".into();
            return;
        };
        self.warp_layer = Some(layer);
        self.warp_draft.push([x, y]);
        self.status = format!(
            "{} corners. Enter keeps the polygon.",
            self.warp_draft.len()
        );
        self.dirty = true;
    }

    fn begin_cut_rect(&mut self, x: f32, y: f32) {
        if self.selected.is_none() {
            self.status = "Select a layer.".into();
            return;
        }
        self.cut_closed = false;
        self.cut_points = rect_points(x, y, x, y);
        self.gesture = Some(Gesture::CutRect { x0: x, y0: y });
        self.dirty = true;
    }

    fn begin_cut_lasso(&mut self, x: f32, y: f32) {
        if self.selected.is_none() {
            self.status = "Select a layer.".into();
            return;
        }
        self.cut_closed = false;
        self.cut_points = vec![[x, y]];
        self.gesture = Some(Gesture::CutLasso);
        self.dirty = true;
    }

    fn add_cut_corner(&mut self, x: f32, y: f32) {
        if self.selected.is_none() {
            self.status = "Select a layer.".into();
            return;
        }
        if self.cut_closed {
            self.cut_points.clear();
            self.cut_closed = false;
        }
        self.cut_points.push([x, y]);
        self.status = "Polygon: click each corner. Enter closes it.".into();
        self.dirty = true;
    }

    fn drag(&mut self, gesture: Gesture, x: f32, y: f32) -> Gesture {
        let (width, height) = (self.art.width, self.art.height);
        match gesture {
            Gesture::Paint {
                mut session,
                layer,
                tool,
                settings,
                before,
                backup,
                base,
                mut changed,
            } => {
                let pressure = self.pressure;
                if tool == Tool::Line {
                    // The old line and the new one both need a repaint. The buffer holds the line at full cover.
                    let start = session.samples.first().copied().unwrap_or((x, y, pressure));
                    let old_end = session.samples.last().copied().unwrap_or(start);
                    session.line_to(x, y, pressure);
                    self.ensure_overlay();
                    let old_mark = self.overlay_mark;
                    self.clear_marked_overlay();
                    let flow = flow_settings(&settings);
                    let mut carry = Carry::default();
                    paint_samples(
                        &mut self.overlay,
                        width,
                        height,
                        &session.samples,
                        tool,
                        &flow,
                        &mut carry,
                    );
                    changed = true;
                    let old_bounds = segment_bounds(width, height, start, old_end, tool, &flow);
                    let new_bounds =
                        segment_bounds(width, height, start, (x, y, pressure), tool, &flow);
                    self.note_overlay(old_mark);
                    self.note_overlay(old_bounds);
                    self.note_overlay(new_bounds);
                    self.touch_rect(old_bounds);
                    self.touch_rect(new_bounds);
                } else if tool.release() == Release::Stroke {
                    if moved_from(session.samples.last(), x, y) {
                        // The overlay is a preview. Dabs land along the move only;
                        // the kept stroke is rebuilt from its traits on release.
                        if let Some(to) = session.push_stable(x, y, pressure, settings.stabilize) {
                            let from = session.samples[session.samples.len() - 2];
                            self.ensure_overlay();
                            let mut carry = Carry::default();
                            paint_segment(
                                &mut self.overlay,
                                width,
                                height,
                                from,
                                to,
                                tool,
                                &settings,
                                &mut carry,
                            );
                            self.overlay_live = true;
                            changed = true;
                            let bounds = segment_bounds(width, height, from, to, tool, &settings);
                            self.note_overlay(bounds);
                            self.touch_rect(bounds);
                        }
                    }
                } else if moved_from(session.samples.last(), x, y) {
                    if let Some(to) = session.push_stable(x, y, pressure, settings.stabilize) {
                        let from = session.samples[session.samples.len() - 2];
                        if self.stroke_live {
                            let flow = flow_settings(&settings);
                            let paint_tool = buffer_tool(tool);
                            self.ensure_overlay();
                            changed |= paint_segment(
                                &mut self.overlay,
                                width,
                                height,
                                from,
                                to,
                                paint_tool,
                                &flow,
                                &mut session.carry,
                            );
                            let bounds = segment_bounds(width, height, from, to, paint_tool, &flow);
                            self.note_overlay(bounds);
                            self.touch_rect(bounds);
                        } else if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                            changed |= paint_segment(
                                &mut node.pixels,
                                width,
                                height,
                                from,
                                to,
                                tool,
                                &settings,
                                &mut session.carry,
                            );
                            self.touch_rect(segment_bounds(
                                width, height, from, to, tool, &settings,
                            ));
                        }
                    }
                }
                Gesture::Paint {
                    session,
                    layer,
                    tool,
                    settings,
                    before,
                    backup,
                    base,
                    changed,
                }
            }
            Gesture::Point {
                layer,
                stroke,
                which,
                before,
                mut changed,
            } => {
                if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                    if let Some(found) = node.strokes.iter_mut().find(|item| item.id == stroke) {
                        let moved = match which {
                            StrokeHit::Vertex(index) => found
                                .vertices
                                .get(index)
                                .map(|vertex| (vertex.x - x).hypot(vertex.y - y) > 0.01)
                                .unwrap_or(false),
                            StrokeHit::HandleIn(index) => found
                                .vertices
                                .get(index)
                                .and_then(|vertex| vertex.handle_in)
                                .map(|handle| (handle[0] - x).hypot(handle[1] - y) > 0.01)
                                .unwrap_or(false),
                            StrokeHit::HandleOut(index) => found
                                .vertices
                                .get(index)
                                .and_then(|vertex| vertex.handle_out)
                                .map(|handle| (handle[0] - x).hypot(handle[1] - y) > 0.01)
                                .unwrap_or(false),
                            StrokeHit::Body => false,
                        };
                        if moved {
                            changed = true;
                            match which {
                                StrokeHit::Vertex(index) => {
                                    if let Some(vertex) = found.vertices.get_mut(index) {
                                        move_vertex(vertex, x, y);
                                    }
                                }
                                StrokeHit::HandleIn(index) => {
                                    if let Some(vertex) = found.vertices.get_mut(index) {
                                        move_handle(vertex, HandleSide::In, x, y);
                                    }
                                }
                                StrokeHit::HandleOut(index) => {
                                    if let Some(vertex) = found.vertices.get_mut(index) {
                                        move_handle(vertex, HandleSide::Out, x, y);
                                    }
                                }
                                StrokeHit::Body => {}
                            }
                            found.touch();
                        }
                    }
                }
                if changed {
                    self.unbake(&layer);
                    self.dirty = true;
                }
                Gesture::Point {
                    layer,
                    stroke,
                    which,
                    before,
                    changed,
                }
            }
            Gesture::Bone {
                layer,
                stroke,
                before,
                x0,
                y0,
                mut changed,
            } => {
                if (x - x0).hypot(y - y0) > 1.0 {
                    if !changed {
                        self.unbake(&layer);
                    }
                    let reach = self.preset_of(Tool::Bone).bone_reach;
                    if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                        if let Some(found) = node.strokes.iter_mut().find(|item| item.id == stroke)
                        {
                            found.bone = Some(Bone {
                                x0,
                                y0,
                                x1: x,
                                y1: y,
                                reach,
                            });
                            found.touch();
                            changed = true;
                            self.dirty = true;
                        }
                    }
                }
                Gesture::Bone {
                    layer,
                    stroke,
                    before,
                    x0,
                    y0,
                    changed,
                }
            }
            Gesture::WarpCorner {
                layer,
                index,
                before,
                mut changed,
            } => {
                if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                    if let Some(point) = node
                        .vectors
                        .first_mut()
                        .and_then(|poly| poly.get_mut(index))
                    {
                        if (point[0] - x).hypot(point[1] - y) > 0.01 {
                            *point = [x, y];
                            changed = true;
                            self.dirty = true;
                        }
                    }
                }
                Gesture::WarpCorner {
                    layer,
                    index,
                    before,
                    changed,
                }
            }
            Gesture::CutRect { x0, y0 } => {
                self.cut_points = rect_points(x0, y0, x, y);
                self.dirty = true;
                Gesture::CutRect { x0, y0 }
            }
            Gesture::CutLasso => {
                if self
                    .cut_points
                    .last()
                    .map(|point| (point[0] - x).hypot(point[1] - y) > 1.0)
                    .unwrap_or(true)
                {
                    self.cut_points.push([x, y]);
                    self.dirty = true;
                }
                Gesture::CutLasso
            }
        }
    }

    fn finish(&mut self, gesture: Gesture) {
        match gesture {
            Gesture::Paint {
                mut session,
                layer,
                tool,
                settings,
                before,
                backup,
                mut changed,
                ..
            } => {
                if tool != Tool::Line {
                    let count = session.release_stable(settings.stabilize).len();
                    if count > 0 && self.paint_tail(&mut session, &layer, tool, &settings, count) {
                        changed = true;
                    }
                }
                if tool.release() == Release::Stroke {
                    let id = self.art.alloc_stroke();
                    if let Some(stroke) = session.end_stroke(id, tool, &settings) {
                        self.unbake(&layer);
                        if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                            node.strokes.push(stroke);
                        }
                        if let Some(before) = before {
                            self.history.push(before);
                        }
                        self.status = "Vector stroke kept. The picture is a cache.".into();
                    }
                    self.overlay_live = false;
                    self.clear_marked_overlay();
                } else {
                    session.end_pixels();
                    let was_clear = self.clear_ink && tool != Tool::Eraser;
                    if changed {
                        if tool == Tool::WarpMask {
                            if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                                node.marker = "mask".into();
                            }
                        }
                        if self.stroke_live {
                            // The layer is still clean. The undo copy happens before the
                            // buffer lands, so pen-down never clones the plate.
                            let previous = self.art.clone();
                            self.commit_overlay(&layer);
                            self.history.push(previous);
                        } else {
                            let previous = before
                                .unwrap_or_else(|| self.art_with_layer_pixels(&layer, backup));
                            self.history.push(previous);
                        }
                        self.status = if was_clear {
                            "Clear wrote pixels.".into()
                        } else {
                            format!("{} wrote pixels.", tool.name())
                        };
                    } else if self.stroke_live {
                        self.end_live_stroke();
                    }
                }
            }
            Gesture::Point {
                changed,
                before,
                layer,
                ..
            } => {
                if changed {
                    self.unbake(&layer);
                    self.refill_warp(&layer);
                    self.history.push(before);
                    self.status = "Point moved. The stroke cache rebuilt.".into();
                }
            }
            Gesture::Bone {
                changed, before, ..
            } => {
                if changed {
                    self.history.push(before);
                    self.status = "Bone kept. The stroke cache rebuilt.".into();
                }
            }
            Gesture::WarpCorner {
                changed,
                before,
                layer,
                ..
            } => {
                if changed {
                    self.refill_warp(&layer);
                    self.history.push(before);
                    self.status = "Corner moved. The warp target refilled.".into();
                }
            }
            Gesture::CutRect { .. } | Gesture::CutLasso => {
                self.cut_closed = self.cut_points.len() >= 3;
                self.status = if self.cut_closed {
                    "Selection ready. Cut to folder, or Delete.".into()
                } else {
                    "Drag a selection.".into()
                };
            }
        }
    }

    fn right_doc(&mut self, x: f32, y: f32) {
        let Some(layer) = self.selected.clone() else {
            return;
        };
        let pick = self.preset_of(Tool::Point).pick_radius.max(6.0);
        if let Some((stroke, _)) = self.stroke_hit(&layer, x, y, pick) {
            let tool = self.tool();
            self.open_stroke_window(layer, stroke);
            self.status = format!("Stroke settings. The current tool stays {}.", tool.name());
        }
    }

    fn stroke_hit(&self, layer: &str, x: f32, y: f32, pick: f32) -> Option<(u64, StrokeHit)> {
        let node = find(&self.art.layers, layer)?;
        for stroke in node.strokes.iter().rev() {
            if let Some(hit) = hit_stroke(stroke, x, y, pick) {
                return Some((stroke.id, hit));
            }
        }
        None
    }

    fn nearest_corner(&self, layer: &str, x: f32, y: f32) -> Option<usize> {
        let pick = self.preset_of(Tool::Point).pick_radius.max(6.0);
        let node = find(&self.art.layers, layer)?;
        let poly = node.vectors.first()?;
        let mut best = None;
        let mut best_dist = pick;
        for (index, point) in poly.iter().enumerate() {
            let dist = (point[0] - x).hypot(point[1] - y);
            if dist <= best_dist {
                best_dist = dist;
                best = Some(index);
            }
        }
        best
    }

    fn refill_warp(&mut self, layer: &str) {
        let (width, height) = (self.art.width, self.art.height);
        if let Some(node) = find_mut(&mut self.art.layers, layer) {
            if node.marker != "target" {
                return;
            }
            let Some(points) = node.vectors.first().cloned() else {
                return;
            };
            let color = parse_hex(&node.color).unwrap_or([0xFF, 0x3E, 0xB8]);
            fill_polygon(&mut node.pixels, width, height, &points, color);
        }
    }

    /// Paint the samples `release_stable` just appended. A vector stroke
    /// rebuilds from those samples on release, so it does not need dabs here.
    fn paint_tail(
        &mut self,
        session: &mut StrokeSession,
        layer: &str,
        tool: Tool,
        settings: &ToolSettings,
        count: usize,
    ) -> bool {
        if count == 0 || tool.release() == Release::Stroke || session.samples.len() < count + 1 {
            return false;
        }
        let (width, height) = (self.art.width, self.art.height);
        let start = session.samples.len() - count;
        let mut changed = false;
        for index in 0..count {
            let from = session.samples[start - 1 + index];
            let to = session.samples[start + index];
            if self.stroke_live {
                let flow = flow_settings(settings);
                let paint_tool = buffer_tool(tool);
                self.ensure_overlay();
                changed |= paint_segment(
                    &mut self.overlay,
                    width,
                    height,
                    from,
                    to,
                    paint_tool,
                    &flow,
                    &mut session.carry,
                );
                let bounds = segment_bounds(width, height, from, to, paint_tool, &flow);
                self.note_overlay(bounds);
                self.touch_rect(bounds);
            } else if let Some(node) = find_mut(&mut self.art.layers, layer) {
                changed |= paint_segment(
                    &mut node.pixels,
                    width,
                    height,
                    from,
                    to,
                    tool,
                    settings,
                    &mut session.carry,
                );
                self.touch_rect(segment_bounds(width, height, from, to, tool, settings));
            }
        }
        changed
    }

    fn paint_target(&self) -> Result<String, ()> {
        let Some(id) = self.selected.clone() else {
            return Err(());
        };
        match find(&self.art.layers, &id) {
            Some(node) if node.is_layer() => Ok(id),
            _ => Err(()),
        }
    }

    fn prepare_layer(&mut self, id: &str) {
        let need = (self.art.width as usize) * (self.art.height as usize) * 4;
        if let Some(node) = find_mut(&mut self.art.layers, id) {
            if node.pixels.len() != need {
                node.pixels.resize(need, 0);
            }
        }
    }

    fn unbake(&mut self, id: &str) {
        let Some(node) = find_mut(&mut self.art.layers, id) else {
            return;
        };
        if !node.embedded {
            return;
        }
        node.pixels.fill(0);
        node.embedded = false;
        for stroke in &mut node.strokes {
            stroke.touch();
        }
        self.status = "Baked pixels were cleared so the stroke can redraw from its traits.".into();
    }

    fn cancel_gesture(&mut self) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        self.restore_preview();
        if let Gesture::Paint {
            backup,
            layer,
            before,
            ..
        } = gesture
        {
            if let Some(before) = before {
                self.art = before;
            } else if !backup.is_empty() {
                if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                    node.pixels = backup;
                }
            }
            self.end_live_stroke();
            self.dirty = true;
            return;
        }
        let before = match gesture {
            Gesture::Point { before, .. }
            | Gesture::Bone { before, .. }
            | Gesture::WarpCorner { before, .. } => Some(before),
            Gesture::CutRect { .. } | Gesture::CutLasso => None,
            Gesture::Paint { .. } => None,
        };
        if let Some(before) = before {
            self.art = before;
        }
        self.end_live_stroke();
        self.dirty = true;
    }

    fn ensure_overlay(&mut self) {
        let need = (self.art.width as usize) * (self.art.height as usize) * 4;
        if self.overlay.len() != need {
            self.overlay = vec![0; need];
        }
    }

    fn finish_new(&mut self, art: Art) {
        self.art = art;
        self.history = History::default();
        self.gesture = None;
        self.overlay_live = false;
        self.overlay.clear();
        self.overlay_mark = None;
        self.preview_snap.clear();
        self.stroke_live = false;
        self.stroke_erase = false;
        self.stroke_layer.clear();
        self.collapsed.clear();
        self.cut_points.clear();
        self.cut_closed = false;
        self.warp_draft.clear();
        self.warp_layer = None;
        self.selected = first_layer(&self.art.layers);
        self.layer_window = None;
        self.stroke_window = None;
        self.object_window = None;
        self.clip_focus = None;
        self.layer_edit_saved = false;
        self.stroke_edit_saved = false;
        self.object_edit_saved = false;
        self.fitted = false;
        self.dirty = true;
        self.ensure_overlay();
    }

    fn after_history(&mut self) {
        self.overlay.fill(0);
        self.end_live_stroke();
        self.dirty = true;
        if let Some(id) = &self.selected {
            if find(&self.art.layers, id).is_some() {
                return;
            }
        }
        self.selected = first_layer(&self.art.layers);
    }

    fn insertion_parent(&self) -> Option<String> {
        let id = self.selected.as_deref()?;
        let node = find(&self.art.layers, id)?;
        if node.is_layer() {
            parent_of(&self.art.layers, id)
        } else {
            Some(node.id.clone())
        }
    }
}

/// Where `id` sits on the cut. A nested clip adds the key of each clip above it.
fn clip_origin(nodes: &[Node], id: &str) -> Option<u32> {
    fn visit(nodes: &[Node], id: &str, base: u32) -> Option<u32> {
        for node in nodes {
            if node.id == id {
                return Some(base);
            }
            if node.is_layer() {
                continue;
            }
            if node.animation {
                for child in &node.children {
                    let next = match anim::key_of(node, &child.id) {
                        Some(start) => base.saturating_add(start),
                        None => base,
                    };
                    if child.id == id {
                        return Some(next);
                    }
                    if !child.is_layer() {
                        if let Some(found) = visit(std::slice::from_ref(child), id, next) {
                            return Some(found);
                        }
                    }
                }
            } else if let Some(found) = visit(&node.children, id, base) {
                return Some(found);
            }
        }
        None
    }
    visit(nodes, id, 0)
}

/// The animation folder that holds `id`, one level up. None at the cut.
fn enclosing_clip(nodes: &[Node], id: &str) -> Option<String> {
    fn visit(nodes: &[Node], id: &str, parent: Option<&str>) -> Option<Option<String>> {
        for node in nodes {
            if node.id == id {
                return Some(parent.map(str::to_string));
            }
            let next = if !node.is_layer() && node.animation {
                Some(node.id.as_str())
            } else {
                parent
            };
            if let Some(found) = visit(&node.children, id, next) {
                return Some(found);
            }
        }
        None
    }
    visit(nodes, id, None).flatten()
}

/// The sheet, the transport, and the onion skin. Time lives on the document:
/// `art.playhead` picks one cel per animation folder when the picture paints.
/// A focused clip shows that folder's own frames. Nested clips add their key
/// onto the clip that holds them.
impl Desk {
    /// Frames in the cut: the keys, or the stored cut end when it reaches
    /// further. Holds on the last cel live in that stored end.
    pub fn cut_end(&self) -> u32 {
        anim::cut_frames(&self.art).max(1)
    }

    pub fn playhead(&self) -> u32 {
        self.art.playhead
    }

    /// The clip the sheet is inside, when that folder is still an animation.
    pub fn clip_label(&self) -> Option<String> {
        let id = self.clip_focus.as_deref()?;
        let node = find(&self.art.layers, id)?;
        if node.is_layer() || !node.animation {
            return None;
        }
        Some(node.name.clone())
    }

    pub fn drop_lost_clip(&mut self) {
        let stale = match self.clip_focus.as_deref() {
            Some(id) => {
                find(&self.art.layers, id).is_none_or(|node| node.is_layer() || !node.animation)
            }
            None => false,
        };
        if stale {
            self.clip_focus = None;
        }
    }

    /// Frame 0 of the focused clip, on the cut. The whole cut starts at 0.
    pub fn sheet_origin(&self) -> u32 {
        let Some(id) = self.clip_focus.as_deref() else {
            return 0;
        };
        clip_origin(&self.art.layers, id).unwrap_or(0)
    }

    /// How many frames the sheet shows. A top-level clip keeps the cut.
    /// A nested clip shows its own keys.
    pub fn sheet_len(&self) -> u32 {
        let Some(id) = self.clip_focus.as_deref() else {
            return self.cut_end();
        };
        if anim::animation_folder_ids(&self.art.layers)
            .iter()
            .any(|folder| folder == id)
        {
            return self.cut_end();
        }
        let Some(folder) = find(&self.art.layers, id) else {
            return self.cut_end();
        };
        anim::frame_map(folder)
            .last()
            .map(|(frame, _)| frame.saturating_add(1))
            .unwrap_or(1)
            .max(1)
    }

    /// The needle, in the sheet's own frames.
    pub fn local_playhead(&self) -> u32 {
        let len = self.sheet_len().max(1);
        self.art
            .playhead
            .saturating_sub(self.sheet_origin())
            .min(len - 1)
    }

    /// Columns for the sheet. Inside a clip that column is the clip's own frames.
    pub fn sheet_columns(&self) -> Vec<XColumn> {
        if let Some(id) = self.clip_focus.as_deref() {
            if let Some(folder) = find(&self.art.layers, id) {
                if !folder.is_layer() && folder.animation {
                    return vec![column_of(folder, self.sheet_len())];
                }
            }
        }
        xsheet_columns(&self.art.layers, self.cut_end())
    }

    /// A sheet click. `local` is a frame on the sheet, which may sit inside a clip.
    pub fn set_sheet_frame(&mut self, local: u32) {
        let last_local = self.sheet_len().saturating_sub(1);
        let local = local.min(last_local);
        self.set_playhead(self.sheet_origin().saturating_add(local));
    }

    /// Open an animation folder on the sheet. The grid becomes that clip's frames.
    pub fn enter_clip(&mut self, id: &str) {
        let Some(node) = find(&self.art.layers, id) else {
            return;
        };
        if node.is_layer() || !node.animation {
            return;
        }
        if self.clip_focus.as_deref() == Some(node.id.as_str()) {
            return;
        }
        let name = node.name.clone();
        self.clip_focus = Some(node.id.clone());
        self.sheet_pick = None;
        let origin = self.sheet_origin();
        let last = origin.saturating_add(self.sheet_len().saturating_sub(1));
        if self.art.playhead < origin || self.art.playhead > last {
            self.set_playhead(origin);
        }
        self.status = format!("Inside {name}. OUT leaves this clip.");
    }

    /// Step out one clip. The cut is the sheet when nothing is left above.
    pub fn leave_clip(&mut self) {
        let Some(id) = self.clip_focus.clone() else {
            return;
        };
        self.clip_focus = enclosing_clip(&self.art.layers, &id);
        self.sheet_pick = None;
        self.status = if self.clip_focus.is_some() {
            "Back one clip.".into()
        } else {
            "Back on the cut.".into()
        };
    }

    /// Show a frame. Nothing is undone by this; it is where you are looking.
    pub fn set_playhead(&mut self, frame: u32) {
        let last = self.cut_end().saturating_sub(1);
        let frame = frame.min(last);
        if frame != self.art.playhead {
            if self.gesture.is_some() {
                self.cancel_gesture();
            }
            self.art.playhead = frame;
            self.dirty = true;
            self.follow_playhead();
        }
    }

    /// The pen draws on what the picture shows: when the selection is a cel
    /// of a timeline, moving the playhead selects the cel that folder holds
    /// on the new frame. Before the first key the selection stays.
    fn follow_playhead(&mut self) {
        if self.clip_focus.is_some() {
            let id = self.clip_focus.clone().unwrap_or_default();
            let local = self.local_playhead();
            if let Some(folder) = find(&self.art.layers, &id) {
                if let Some(shown) = anim::cel_at(folder, local) {
                    let inside = self.selected.as_deref().is_some_and(|sel| {
                        sel == id || folder.children.iter().any(|cel| cel.id == sel)
                    });
                    if inside && self.selected.as_deref() != Some(shown.id.as_str()) {
                        self.selected = Some(shown.id.clone());
                    }
                }
            }
            return;
        }
        let Some(selected) = self.selected.clone() else {
            return;
        };
        let Some(folder) = anim::owning_animation(&self.art.layers, &selected) else {
            return;
        };
        let Some(node) = find(&self.art.layers, &folder) else {
            return;
        };
        // A row deeper inside a group cel was chosen on purpose; leave it.
        if !node.children.iter().any(|cel| cel.id == selected) {
            return;
        }
        if let Some(shown) = anim::cel_at(node, self.art.playhead) {
            if shown.id != selected {
                self.selected = Some(shown.id.clone());
            }
        }
    }

    /// Comma and period. Past either end wraps only when loop wrap is on.
    pub fn step_frame(&mut self, delta: i64) {
        let end = i64::from(self.cut_end());
        let mut next = i64::from(self.art.playhead) + delta;
        if self.loop_wrap {
            next = next.rem_euclid(end);
        } else {
            next = next.clamp(0, end - 1);
        }
        self.set_playhead(next as u32);
    }

    pub fn first_frame(&mut self) {
        self.set_playhead(0);
    }

    pub fn last_frame(&mut self) {
        let last = self.cut_end().saturating_sub(1);
        self.set_playhead(last);
    }

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
        self.play_clock = 0.0;
        self.play_armed = self.playing;
        if self.playing && self.cut_end() <= 1 {
            self.playing = false;
            self.status = "One frame. Add a cel to the sheet first.".into();
        } else if self.playing {
            self.status = format!("Playing at {} fps.", self.art.fps.max(1));
            if !self.loop_play && self.art.playhead + 1 >= self.cut_end() {
                self.art.playhead = 0;
                self.dirty = true;
            }
        } else {
            self.dirty = true;
            self.status = format!(
                "Stopped on frame {}.",
                vmi_core::slot_hex(self.art.playhead)
            );
        }
    }

    pub fn stop(&mut self) {
        if self.playing {
            self.dirty = true;
        }
        self.playing = false;
        self.play_clock = 0.0;
    }

    /// Advance playback by `dt` seconds. True while a repaint is still owed.
    pub fn tick(&mut self, dt: f64) -> bool {
        if !self.playing {
            return false;
        }
        if self.play_armed {
            self.play_armed = false;
            return true;
        }
        let fps = f64::from(self.art.fps.max(1));
        self.play_clock += dt.clamp(0.0, 0.25);
        let period = 1.0 / fps;
        let mut moved = false;
        while self.play_clock >= period {
            self.play_clock -= period;
            let end = self.cut_end();
            let next = self.art.playhead + 1;
            if next >= end {
                if self.loop_play {
                    self.art.playhead = 0;
                } else {
                    self.art.playhead = end.saturating_sub(1);
                    self.playing = false;
                    self.play_clock = 0.0;
                    moved = true;
                    break;
                }
            } else {
                self.art.playhead = next;
            }
            moved = true;
        }
        if moved {
            self.dirty = true;
            self.follow_playhead();
        }
        self.playing
    }

    /// Seconds until the next frame is due, for the repaint timer.
    pub fn next_frame_in(&self) -> f64 {
        let fps = f64::from(self.art.fps.max(1));
        (1.0 / fps - self.play_clock).max(0.0)
    }

    pub fn set_fps(&mut self, fps: u32) {
        let fps = fps.clamp(1, 60);
        if fps != self.art.fps {
            let before = self.art.clone();
            self.art.fps = fps;
            self.history.push(before);
            self.status = format!("{fps} fps.");
        }
    }

    pub fn toggle_loop(&mut self) {
        self.loop_play = !self.loop_play;
        self.status = if self.loop_play {
            "Loop on.".into()
        } else {
            "Loop off: play stops on the last frame.".into()
        };
    }

    pub fn toggle_wrap(&mut self) {
        self.loop_wrap = !self.loop_wrap;
        self.status = if self.loop_wrap {
            "Stepping wraps around the cut.".into()
        } else {
            "Stepping stops at the ends.".into()
        };
    }

    pub fn set_hold(&mut self, hold: u32) {
        self.hold = hold.clamp(1, 3);
        self.status = match self.hold {
            1 => "On ones.".into(),
            2 => "On twos.".into(),
            _ => "On threes.".into(),
        };
    }

    pub fn toggle_onion(&mut self) {
        self.art.onion = !self.art.onion;
        self.dirty = true;
        self.status = if self.art.onion {
            "Onion skin on. Right-click SKIN for its colors.".into()
        } else {
            "Onion skin off.".into()
        };
    }

    /// The animation folder the sheet works on: the selected one, the one
    /// the selected cel is in, or the front one.
    pub fn sheet_folder(&self) -> Option<String> {
        if let Some(id) = self.selected.as_deref() {
            if let Some(node) = find(&self.art.layers, id) {
                if node.animation {
                    return Some(id.to_string());
                }
            }
            if let Some(owner) = anim::owning_animation(&self.art.layers, id) {
                return Some(owner);
            }
        }
        anim::animation_folder_ids(&self.art.layers).last().cloned()
    }

    /// Mark a folder as a timeline, or unmark it.
    pub fn set_animation(&mut self, id: &str, on: bool) {
        let before = self.art.clone();
        if anim::set_animation(&mut self.art.layers, id, on) {
            self.history.push(before);
            self.dirty = true;
            let name = find(&self.art.layers, id)
                .map(|node| node.name.clone())
                .unwrap_or_default();
            self.status = if on {
                format!("{name} is a timeline. Its layers are its cels.")
            } else {
                format!("{name} is a folder again.")
            };
        }
    }

    /// Right-click a folder: Animation.
    pub fn toggle_animation(&mut self, id: &str) {
        let on = find(&self.art.layers, id)
            .map(|node| !node.animation)
            .unwrap_or(false);
        self.set_animation(id, on);
    }

    /// Python's `mark_current_animation`: the selected folder becomes a
    /// timeline, or stops being one.
    pub fn animate_selected(&mut self) {
        let Some(id) = self.selected.clone() else {
            self.status = "Pick a folder to mark as an animation.".into();
            return;
        };
        let is_group = find(&self.art.layers, &id)
            .map(|node| !node.is_layer())
            .unwrap_or(false);
        if is_group {
            self.toggle_animation(&id);
        } else {
            self.status = "Pick a folder to mark as an animation.".into();
        }
    }

    /// Where a new frame goes without a frame named: on ones, twos or threes
    /// the drawing under the playhead keeps its hold and the new cel starts
    /// after it, when it is the last drawing. Inside the cut it is Python's
    /// rule: the new cel goes right after the playhead and later keys move up.
    /// A lone layer about to become the first frame counts as keyed at 0.
    fn new_cel_slot(&self, folder: &str) -> u32 {
        let playhead = self.art.playhead;
        let Some(node) = find(&self.art.layers, folder) else {
            return playhead;
        };
        if node.is_layer() {
            return playhead.max(self.hold);
        }
        let pairs = anim::frame_map(node);
        let Some(current) = anim::cel_at(node, playhead) else {
            return playhead;
        };
        let start = pairs
            .iter()
            .find(|(_, cel)| cel.id == current.id)
            .map(|(start, _)| *start)
            .unwrap_or(playhead);
        let is_last = pairs
            .last()
            .map(|(_, cel)| cel.id == current.id)
            .unwrap_or(true);
        if is_last {
            playhead.max(start.saturating_add(self.hold))
        } else {
            playhead
        }
    }

    /// The stored cut end grows so a cel keyed at `start` holds `hold`
    /// frames when nothing comes after it. It never shrinks.
    fn hold_last_cel(&mut self, folder: &str, start: u32) {
        let is_last = find(&self.art.layers, folder)
            .map(|node| {
                anim::frame_map(node)
                    .iter()
                    .all(|(frame, _)| *frame <= start)
            })
            .unwrap_or(false);
        if !is_last {
            return;
        }
        let end = start.saturating_add(self.hold.max(1)).saturating_sub(1);
        let clip = self.art.clip_time.get_or_insert_with(ClipTime::default);
        clip.end = clip.end.max(end);
    }

    /// Shift+F, Python's `new_animation_frame`: a new cel at the playhead, or
    /// at `frame`, in `folder` or in the timeline the selection belongs to. A
    /// selected plain layer is wrapped in an Animation folder as the first
    /// frame, and a selected plain folder is marked. The playhead follows.
    pub fn new_cel(&mut self, folder: Option<&str>, frame: Option<u32>) {
        let target = folder
            .map(str::to_string)
            .or_else(|| self.sheet_folder())
            .or_else(|| self.selected.clone());
        let Some(target) = target else {
            self.status = "Select a layer.".into();
            return;
        };
        if self.gesture.is_some() {
            self.cancel_gesture();
        }
        let before = self.art.clone();
        let at = frame.unwrap_or_else(|| self.new_cel_slot(&target));
        match anim::add_cel(&mut self.art, &target, at) {
            Some(id) => {
                self.history.push(before);
                let owner = anim::owning_animation(&self.art.layers, &id).unwrap_or(target);
                let start = find(&self.art.layers, &owner)
                    .and_then(|node| {
                        anim::frame_map(node)
                            .into_iter()
                            .find(|(_, cel)| cel.id == id)
                            .map(|(start, _)| start)
                    })
                    .unwrap_or(at);
                self.hold_last_cel(&owner, start);
                self.selected = Some(id);
                self.art.playhead = start;
                // Like Shift+F in the Python desk: the skin comes on so the
                // drawing you just left shows through.
                self.art.onion = true;
                self.dirty = true;
                self.status = format!("Frame {}. Onion skin on.", vmi_core::slot_hex(start));
            }
            None => self.status = "Select a layer.".into(),
        }
    }

    /// Drag a block: the cel starts on another frame.
    pub fn move_cel(&mut self, folder: &str, cel: &str, start: u32) {
        let before = self.art.clone();
        let moved = find_mut(&mut self.art.layers, folder)
            .map(|node| anim::move_frame(node, cel, start))
            .unwrap_or(false);
        if moved {
            self.history.push(before);
            self.dirty = true;
            self.status = format!("Cel moved to frame {}.", vmi_core::slot_hex(start));
        }
    }

    /// Remove a cel and its layer.
    pub fn delete_cel(&mut self, folder: &str, cel: &str) {
        let before = self.art.clone();
        let removed = find_mut(&mut self.art.layers, folder)
            .map(|node| {
                let len = node.children.len();
                node.children.retain(|child| child.id != cel);
                if let Some(index) = node.timeline.iter().position(|id| id == cel) {
                    node.timeline.remove(index);
                    if index < node.frames.len() {
                        node.frames.remove(index);
                    }
                }
                node.children.len() != len
            })
            .unwrap_or(false);
        if removed {
            self.history.push(before);
            self.sheet_pick = None;
            self.after_history();
            self.status = "Cel deleted.".into();
        }
    }

    /// Select the cel a level shows at a frame, so the pen draws on it.
    pub fn pick_cel(&mut self, folder: &str, frame: u32) {
        let id = find(&self.art.layers, folder)
            .and_then(|node| anim::cel_at(node, frame).map(|cel| cel.id.clone()));
        match id {
            Some(id) => self.selected = Some(id),
            None => self.selected = Some(folder.to_string()),
        }
    }

    pub fn pick_range(&mut self, folder: &str, start: u32, count: u32) {
        self.sheet_pick = Some(SheetPick {
            folder: folder.to_string(),
            start,
            count: count.max(1),
        });
    }

    /// The first command on a column stores a sheet made from its keys
    /// (Python's `_xsheet_command`), here held to the cut end so the stored
    /// column reads exactly as the grid did before the command.
    fn ensure_sheet(&mut self, folder: &str) {
        let end = self.cut_end() as usize;
        let slots = match find(&self.art.layers, folder) {
            Some(node) if node.animation && node.exposure.is_none() => {
                let mut slots = anim::project_slots(&anim::frame_map(node));
                if let Some(last) = slots.last().cloned() {
                    while slots.len() < end {
                        slots.push(last.clone());
                    }
                }
                slots
            }
            _ => return,
        };
        if let Some(node) = find_mut(&mut self.art.layers, folder) {
            node.exposure = anim::sheet_from_slots(&slots, 0);
        }
    }

    /// One exposure command on a picked range of one column. The sheet is
    /// what the X-sheet shows from then on; the keys still paint the picture.
    pub fn sheet_command(&mut self, folder: &str, name: &str, start: u32, count: u32, arg: i64) {
        let before = self.art.clone();
        self.ensure_sheet(folder);
        let changed = anim::folder_command(
            &mut self.art,
            folder,
            name,
            start as usize,
            count as usize,
            arg,
        );
        if changed {
            self.history.push(before);
            self.dirty = true;
            let end = start + count.saturating_sub(1);
            self.status = format!(
                "{name}: frames {}–{}.",
                vmi_core::slot_hex(start),
                vmi_core::slot_hex(end)
            );
        } else {
            self.status = format!("{name} left the sheet as it was.");
        }
    }
}

fn first_layer(nodes: &[Node]) -> Option<String> {
    for node in nodes {
        if node.is_layer() {
            return Some(node.id.clone());
        }
        if let Some(found) = first_layer(&node.children) {
            return Some(found);
        }
    }
    None
}

fn walk_layers(
    nodes: &[Node],
    depth: usize,
    collapsed: &HashSet<String>,
    guides: u32,
    out: &mut Vec<LayerRow>,
) {
    let count = nodes.len();
    for (index, node) in nodes.iter().rev().enumerate() {
        let group = !node.is_layer();
        let open = group && !collapsed.contains(&node.id);
        let has_next = index + 1 < count;
        out.push(LayerRow {
            id: node.id.clone(),
            name: node.name.clone(),
            depth,
            group,
            visible: node.visible,
            mute: node.mute,
            solo: node.solo,
            clip: node.clip,
            open,
            animation: node.animation,
            frame: None,
            opacity: node.opacity,
            blend: node.blend.clone(),
            marker: node.marker.clone(),
            has_next,
            guides,
        });
        if group && !open {
            continue;
        }
        let child_guides = if has_next {
            guides | (1u32 << depth.min(31))
        } else {
            guides
        };
        if node.animation {
            // Cels carry the frame they start on, from 1.
            let starts: Vec<(String, u32)> = anim::frame_map(node)
                .into_iter()
                .map(|(start, cel)| (cel.id.clone(), start))
                .collect();
            let before = out.len();
            walk_layers(&node.children, depth + 1, collapsed, child_guides, out);
            for row in &mut out[before..] {
                if row.depth == depth + 1 {
                    row.frame = starts
                        .iter()
                        .find(|(id, _)| *id == row.id)
                        .map(|(_, start)| start + 1);
                }
            }
        } else {
            walk_layers(&node.children, depth + 1, collapsed, child_guides, out);
        }
    }
}

/// Pens, the eraser, and the line. Smudge and blend read the pixels they stand on,
/// so they stay per dab. Opacity here is the whole stroke, the way Photoshop and
/// Clip Studio composite one pass.
fn stroke_opacity_tool(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Pen
            | Tool::Pencil
            | Tool::Charcoal
            | Tool::Paint
            | Tool::Air
            | Tool::Dither
            | Tool::Eraser
            | Tool::Line
            | Tool::WarpMask
    )
}

/// Eraser stamps would erase the empty stroke buffer. A pen builds the mask instead.
fn buffer_tool(tool: Tool) -> Tool {
    match tool {
        Tool::Eraser | Tool::Smudge | Tool::Blend => Tool::Pen,
        other => other,
    }
}

/// Dab cover is full, or pressure when that box is on. The opacity knob is applied once, later.
fn flow_settings(settings: &ToolSettings) -> ToolSettings {
    let mut flow = settings.clone();
    flow.opacity = 1.0;
    if !settings.opacity_pressure {
        flow.opacity_pressure = false;
    }
    flow
}

/// Composite stroke-buffer ink onto the layer. Clear paper in the buffer stays put,
/// so the destination can be the live layer instead of a full pen-down copy.
fn blend_overlay(
    layer: &mut [u8],
    overlay: &[u8],
    width: u32,
    height: u32,
    rect: [i32; 4],
    opacity: f32,
    erase: bool,
) {
    if overlay.len() != layer.len() || width == 0 || height == 0 {
        return;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let (x0, y0, x1, y1) = rect_span(width, height, rect);
    for y in y0..y1 {
        for x in x0..x1 {
            let index = ((y * width + x) * 4) as usize;
            let src_a = overlay[index + 3];
            if src_a == 0 {
                continue;
            }
            if erase {
                let cover = (f32::from(src_a) / 255.0) * opacity;
                let alpha = f32::from(layer[index + 3]) * (1.0 - cover);
                layer[index + 3] = alpha.round().clamp(0.0, 255.0) as u8;
                continue;
            }
            let dst = Rgba::new(
                layer[index],
                layer[index + 1],
                layer[index + 2],
                layer[index + 3],
            );
            let src = Rgba::new(
                overlay[index],
                overlay[index + 1],
                overlay[index + 2],
                overlay[index + 3],
            );
            let channels = source_over(dst, src, opacity).channels();
            layer[index..index + 4].copy_from_slice(&channels);
        }
    }
}

fn rect_span(width: u32, height: u32, rect: [i32; 4]) -> (u32, u32, u32, u32) {
    let x0 = rect[0].max(0) as u32;
    let y0 = rect[1].max(0) as u32;
    let x1 = (rect[2].max(0) as u32).min(width);
    let y1 = (rect[3].max(0) as u32).min(height);
    (x0, y0, x1.max(x0), y1.max(y0))
}

fn copy_tight(src: &[u8], width: u32, height: u32, rect: [i32; 4]) -> Vec<u8> {
    let (x0, y0, x1, y1) = rect_span(width, height, rect);
    let span = ((x1 - x0) * 4) as usize;
    let rows = (y1 - y0) as usize;
    if span == 0 || rows == 0 || src.len() < (width as usize) * (height as usize) * 4 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(rows * span);
    for y in y0..y1 {
        let start = ((y * width + x0) * 4) as usize;
        out.extend_from_slice(&src[start..start + span]);
    }
    out
}

fn paste_tight(dst: &mut [u8], snap: &[u8], width: u32, height: u32, rect: [i32; 4]) {
    let (x0, y0, x1, y1) = rect_span(width, height, rect);
    let span = ((x1 - x0) * 4) as usize;
    let rows = (y1 - y0) as usize;
    if span == 0
        || snap.len() != rows * span
        || dst.len() < (width as usize) * (height as usize) * 4
    {
        return;
    }
    for (row, y) in (y0..y1).enumerate() {
        let start = ((y * width + x0) * 4) as usize;
        let from = row * span;
        dst[start..start + span].copy_from_slice(&snap[from..from + span]);
    }
}

fn zero_rect(buf: &mut [u8], width: u32, height: u32, rect: [i32; 4]) {
    let (x0, y0, x1, y1) = rect_span(width, height, rect);
    let span = ((x1 - x0) * 4) as usize;
    if span == 0 || buf.len() < (width as usize) * (height as usize) * 4 {
        return;
    }
    for y in y0..y1 {
        let start = ((y * width + x0) * 4) as usize;
        buf[start..start + span].fill(0);
    }
}

fn moved_from(last: Option<&(f32, f32, f32)>, x: f32, y: f32) -> bool {
    last.map(|point| (point.0 - x).hypot(point.1 - y) > 0.4)
        .unwrap_or(true)
}

fn needs_last_drag(gesture: &Gesture, x: f32, y: f32) -> bool {
    match gesture {
        Gesture::Paint { tool, session, .. } => {
            if *tool == Tool::Line {
                return true;
            }
            moved_from(session.samples.last(), x, y)
        }
        Gesture::CutLasso
        | Gesture::CutRect { .. }
        | Gesture::Point { .. }
        | Gesture::Bone { .. }
        | Gesture::WarpCorner { .. } => true,
    }
}

fn fill_polygon(pixels: &mut [u8], width: u32, height: u32, points: &[[f32; 2]], color: [u8; 3]) {
    let need = (width as usize) * (height as usize) * 4;
    if pixels.len() != need {
        return;
    }
    pixels.fill(0);
    for y in 0..height {
        for x in 0..width {
            if !point_inside(x as f32 + 0.5, y as f32 + 0.5, points) {
                continue;
            }
            let index = ((y * width + x) * 4) as usize;
            pixels[index] = color[0];
            pixels[index + 1] = color[1];
            pixels[index + 2] = color[2];
            pixels[index + 3] = 255;
        }
    }
}

fn extension_is(path: &Path, expect: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(expect))
        .unwrap_or(false)
}

/// `os.path.abspath`: against the working folder, without touching the disk.
fn absolute(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Warp targets and windows in the drawing, for the opened line.
fn warp_counts(nodes: &[Node]) -> (usize, usize) {
    let mut targets = 0;
    let mut windows = 0;
    for node in nodes {
        match node.marker.as_str() {
            "target" => targets += 1,
            "window" => windows += 1,
            _ => {}
        }
        let (below_targets, below_windows) = warp_counts(&node.children);
        targets += below_targets;
        windows += below_windows;
    }
    (targets, windows)
}

/// Read one drawing, telling `progress` each stage the way the Python
/// `open_drawing` does. Blueprints read here; the other kinds read through
/// the Python studio next to this program.
fn read_drawing(path: &Path, progress: &dyn Fn(&str)) -> Result<Art, String> {
    if !path.is_file() {
        return Err("That file is missing.".into());
    }
    progress(&format!("Reading {}", basename(path)));
    if extension_is(path, "vmib") {
        load_vmib(path)
    } else {
        open_with_python(path, progress)
    }
}

fn studio_root() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_dir() {
        candidates.push(current);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.to_path_buf());
        }
    }
    for start in candidates {
        let mut cursor = start;
        for _ in 0..8 {
            if cursor.join("vmi_studio").join("bridge.py").is_file() {
                return Some(cursor);
            }
            if !cursor.pop() {
                break;
            }
        }
    }
    None
}

/// `.venv` beside this tree, or beside the main checkout when this tree is a
/// git worktree. System `python3` has no PySide6, so a bare name cannot open
/// a clip.
fn python_program(root: &Path) -> PathBuf {
    if let Some(from_env) = std::env::var_os("VMI_PYTHON") {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return path;
        }
    }
    if let Some(found) = venv_python(root) {
        return found;
    }
    if let Some(main) = main_worktree(root) {
        if let Some(found) = venv_python(&main) {
            return found;
        }
    }
    PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
}

fn venv_python(root: &Path) -> Option<PathBuf> {
    let candidate = if cfg!(windows) {
        root.join(".venv").join("Scripts").join("python.exe")
    } else {
        root.join(".venv").join("bin").join("python")
    };
    candidate.is_file().then_some(candidate)
}

/// A linked worktree's `.git` file points at `<main>/.git/worktrees/<name>`.
fn main_worktree(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join(".git")).ok()?;
    let line = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?;
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let gitdir = PathBuf::from(line);
    let gitdir = if gitdir.is_absolute() {
        gitdir
    } else {
        root.join(gitdir)
    };
    let worktrees = gitdir.parent()?;
    if worktrees.file_name().and_then(|name| name.to_str()) != Some("worktrees") {
        return None;
    }
    let git_dir = worktrees.parent()?;
    if git_dir.file_name().and_then(|name| name.to_str()) != Some(".git") {
        return None;
    }
    git_dir.parent().map(Path::to_path_buf)
}

/// The bridge prints `note <text>` on stdout at each stage; those lines feed
/// the load gate while the file converts.
fn open_with_python(path: &Path, progress: &dyn Fn(&str)) -> Result<Art, String> {
    let root = studio_root()
        .ok_or_else(|| "The Python studio is not next to this program.".to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("vmi-open-{stamp}"));
    std::fs::create_dir_all(dir.join("rasters")).map_err(|err| err.to_string())?;
    let manifest = dir.join("manifest.json");
    let python = python_program(&root);
    let mut child = Command::new(&python)
        .arg("-m")
        .arg("vmi_studio.bridge")
        .arg("open")
        .arg(path)
        .arg("--manifest")
        .arg(&manifest)
        .arg("--rasters")
        .arg(dir.join("rasters"))
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Python could not start ({}: {err}).", python.display()))?;
    if let Some(stdout) = child.stdout.take() {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(note) = line.strip_prefix("note ") {
                progress(note.trim());
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("Python did not finish ({err})."))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let line = err
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Python could not open that file.");
        let _ = std::fs::remove_dir_all(&dir);
        return Err(line.to_string());
    }
    let art = load_manifest(&manifest);
    let _ = std::fs::remove_dir_all(&dir);
    art
}

#[cfg(test)]
mod tests {
    use super::*;
    use vmi_core::{present, Edge};

    fn alpha_at(art: &Art, x: i32, y: i32) -> u8 {
        let node = art.layers.iter().find(|node| node.is_layer());
        let Some(node) = node else {
            return 0;
        };
        if x < 0 || y < 0 || x as u32 >= art.width || y as u32 >= art.height {
            return 0;
        }
        let index = ((y as u32 * art.width + x as u32) * 4 + 3) as usize;
        node.pixels.get(index).copied().unwrap_or(0)
    }

    fn tiny() -> Desk {
        let mut desk = Desk::new(Session::default());
        desk.finish_new(Art::new(48, 32));
        desk
    }

    #[test]
    fn a_folder_holds_the_next_object_and_a_drop_sets_its_depth() {
        let mut desk = tiny();
        desk.add_folder();
        let folder = desk.art.objects[0].id.clone();
        assert_eq!(desk.art.objects[0].kind, "folder");
        assert_eq!(desk.library_folder, folder);
        desk.add_object();
        desk.add_object();
        assert_eq!(desk.art.objects.len(), 3);
        assert!(desk.art.objects[1..]
            .iter()
            .all(|object| object.folder == folder));
        assert_eq!(desk.art.objects[1].stack, 0);
        assert_eq!(desk.art.objects[2].stack, 1);
        let back = desk.art.objects[1].id.clone();
        let front = desk.art.objects[2].id.clone();
        desk.place_on(&front, &back);
        let rows = vmi_core::library_rows(&desk.art.objects, &std::collections::HashSet::new());
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec![folder.as_str(), front.as_str(), back.as_str()]
        );
        desk.undo();
        let rows = vmi_core::library_rows(&desk.art.objects, &std::collections::HashSet::new());
        assert_eq!(rows[1].id, back);
        assert_eq!(rows[2].id, front);
        desk.object_window = Some(front.clone());
        desk.object_edit_saved = false;
        desk.selected = Some(desk.art.layers[0].id.clone());
        desk.give_layer_to_state(0);
        let symbol = desk
            .art
            .objects
            .iter()
            .find(|object| object.id == front)
            .unwrap();
        assert_eq!(symbol.assign[&desk.art.layers[0].id], (0, 0));
        assert_eq!(desk.art.objects.len(), 3);
    }

    #[test]
    fn onion_colors_stay_on_the_session() {
        let mut desk = tiny();
        assert!(!desk.onion_window);
        desk.open_onion_window();
        assert!(desk.onion_window);
        desk.set_onion_color(true, [9, 8, 7]);
        desk.set_onion_color(false, [1, 2, 3]);
        assert_eq!(desk.session.onion_past_color, [9, 8, 7]);
        assert_eq!(desk.session.onion_future_color, [1, 2, 3]);
        assert_eq!(desk.art.onion_past_color, [9, 8, 7]);
        desk.set_onion_reach(1, 2);
        assert_eq!((desk.art.onion_past, desk.art.onion_future), (1, 2));
        desk.close_onion_window();
        assert!(!desk.onion_window);
    }

    fn paint_red(desk: &mut Desk, x: u32, y: u32) {
        let index = ((y * desk.art.width + x) * 4) as usize;
        desk.art.layers[0].pixels[index] = 255;
        desk.art.layers[0].pixels[index + 3] = 255;
    }

    #[test]
    fn right_click_opens_that_tools_settings_and_leaves_the_selection() {
        let mut desk = tiny();
        assert_eq!(desk.tool(), Tool::Pen);
        desk.palette_click(Tool::Smudge, true);
        assert_eq!(desk.tool(), Tool::Pen);
        assert_eq!(desk.tool_window, Some(Tool::Smudge));
        desk.palette_click(Tool::Eraser, false);
        assert_eq!(desk.tool(), Tool::Eraser);
        assert_eq!(desk.tool_window, Some(Tool::Smudge));
        desk.reset_preset(Tool::Paint);
        assert!((desk.preset_of(Tool::Paint).softness - 0.55).abs() < 1e-5);
        desk.edit_preset(Tool::Paint, |slot| slot.softness = 0.1);
        desk.reset_preset(Tool::Paint);
        assert!((desk.preset_of(Tool::Paint).softness - 0.55).abs() < 1e-5);
    }

    #[test]
    fn raster_release_drops_the_path_and_vector_release_keeps_the_stroke() {
        let mut desk = tiny();
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 8.0;
            slot.fringe = 0.0;
            slot.edge = Edge::Hard;
            slot.size_pressure = false;
            slot.opacity_pressure = false;
        });
        desk.down_doc(8.0, 8.0, Button::Primary);
        desk.move_doc(20.0, 8.0);
        desk.up_doc(20.0, 8.0);
        assert!(desk.gesture.is_none());
        assert!(alpha_at(&desk.art, 8, 8) > 0);
        assert!(desk.art.layers[0].strokes.is_empty());
        assert_eq!(desk.undo_depth(), 1);
        desk.undo();
        assert_eq!(alpha_at(&desk.art, 8, 8), 0);

        desk.set_tool(Tool::Vector);
        desk.down_doc(6.0, 16.0, Button::Primary);
        desk.move_doc(24.0, 18.0);
        desk.up_doc(24.0, 18.0);
        assert_eq!(desk.art.layers[0].strokes.len(), 1);
        assert!(desk.art.layers[0].strokes[0].vertices.len() >= 2);
        assert!(desk.art.layers[0]
            .pixels
            .iter()
            .skip(3)
            .step_by(4)
            .all(|alpha| *alpha == 0));
        let plate = present(&mut desk.art);
        assert!(plate.iter().skip(3).step_by(4).any(|alpha| *alpha > 0));

        desk.set_tool(Tool::Line);
        desk.down_doc(4.0, 4.0, Button::Primary);
        desk.move_doc(16.0, 4.0);
        desk.up_doc(16.0, 4.0);
        assert!(desk.art.layers[0].strokes.len() == 1);
        assert!(alpha_at(&desk.art, 10, 4) > 0 || alpha_at(&desk.art, 8, 4) > 0);
    }

    #[test]
    fn brackets_step_the_brush_and_x_trades_the_two_inks() {
        let mut desk = tiny();
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 24.0;
            slot.size_min = 1.0;
            slot.size_max = 128.0;
            slot.color = [255, 0, 0];
        });
        desk.nudge_size(-1);
        assert_eq!(desk.preset().size.round() as i32, 23);
        desk.nudge_size(-10);
        assert_eq!(desk.preset().size.round() as i32, 13);
        desk.nudge_size(-1000);
        assert_eq!(desk.preset().size.round() as i32, 1);
        desk.ink = [255, 0, 0];
        desk.ink_back = [0, 0, 255];
        desk.edit_preset(Tool::Paint, |slot| slot.color = [0, 255, 0]);
        desk.swap_inks();
        assert_eq!(desk.ink, [0, 0, 255]);
        assert_eq!(desk.ink_back, [255, 0, 0]);
        assert_eq!(
            desk.preset_of(Tool::Paint).color,
            [0, 255, 0],
            "a tool preset does not own the ink"
        );
        desk.toggle_clear();
        assert!(desk.clear_ink);
    }

    #[test]
    fn rotation_stays_on_the_view_center() {
        let mut desk = tiny();
        desk.camera.zoom = 2.0;
        desk.camera.pan_x = 30.0;
        desk.camera.pan_y = -12.0;
        desk.camera.rotation = 0.2;
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 120.0,
        };
        let (cx, cy) = frame.center();
        let (dx, dy) = desk.camera.to_doc(frame, 48.0, 32.0, cx, cy);
        desk.rotate_about_view(0.35);
        let (sx, sy) = desk.camera.to_screen(frame, 48.0, 32.0, dx, dy);
        assert!((sx - cx).abs() < 1e-2, "{sx} vs {cx}");
        assert!((sy - cy).abs() < 1e-2, "{sy} vs {cy}");
    }

    #[test]
    fn a_crossed_stroke_keeps_one_opacity() {
        let mut desk = tiny();
        desk.set_ink([255, 0, 0]);
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 8.0;
            slot.fringe = 0.0;
            slot.softness = 0.0;
            slot.edge = Edge::Hard;
            slot.opacity = 0.5;
            slot.size_pressure = false;
            slot.opacity_pressure = false;
            slot.color = [0, 0, 255];
        });
        desk.down_doc(4.0, 16.0, Button::Primary);
        desk.move_doc(40.0, 16.0);
        desk.up_doc(40.0, 16.0);
        let once = alpha_at(&desk.art, 20, 16);
        assert!((100..160).contains(&once), "one pass {once}");
        let index = ((16 * desk.art.width + 20) * 4) as usize;
        assert_eq!(
            desk.art.layers[0].pixels[index], 255,
            "shared ink, not the preset"
        );
        desk.undo();
        desk.down_doc(4.0, 16.0, Button::Primary);
        desk.move_doc(40.0, 16.0);
        desk.move_doc(4.0, 16.0);
        desk.up_doc(4.0, 16.0);
        let crossed = alpha_at(&desk.art, 20, 16);
        assert!(
            (crossed as i32 - once as i32).abs() < 8,
            "a retraced stroke stacked to {crossed} from {once}"
        );
    }

    #[test]
    fn clear_punches_with_the_current_brush_and_a_folder_closes() {
        let mut desk = tiny();
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 8.0;
            slot.fringe = 0.0;
            slot.softness = 0.0;
            slot.edge = Edge::Hard;
            slot.opacity = 1.0;
            slot.size_pressure = false;
            slot.opacity_pressure = false;
        });
        desk.down_doc(16.0, 16.0, Button::Primary);
        desk.up_doc(16.0, 16.0);
        assert!(alpha_at(&desk.art, 16, 16) > 200);
        desk.toggle_clear();
        desk.down_doc(16.0, 16.0, Button::Primary);
        desk.up_doc(16.0, 16.0);
        assert_eq!(alpha_at(&desk.art, 16, 16), 0);
        assert_eq!(desk.tool(), Tool::Pen);

        desk.add_group();
        desk.add_layer();
        let group = desk
            .layer_rows()
            .into_iter()
            .find(|row| row.group)
            .expect("group")
            .id;
        assert!(desk.layer_rows().iter().any(|row| row.depth > 0));
        desk.toggle_folder(&group);
        assert!(desk.layer_rows().iter().all(|row| row.depth == 0));
        assert!(desk.layer_rows().iter().any(|row| row.group && !row.open));
    }

    #[test]
    fn a_cancelled_pen_keeps_the_layer() {
        let mut desk = tiny();
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 4.0;
            slot.fringe = 0.0;
            slot.softness = 0.0;
            slot.edge = Edge::Hard;
            slot.opacity = 1.0;
            slot.size_pressure = false;
            slot.opacity_pressure = false;
        });
        desk.down_doc(8.0, 8.0, Button::Primary);
        desk.up_doc(8.0, 8.0);
        assert!(alpha_at(&desk.art, 8, 8) > 200);
        desk.down_doc(24.0, 8.0, Button::Primary);
        desk.move_doc(36.0, 8.0);
        desk.escape();
        assert!(
            alpha_at(&desk.art, 8, 8) > 200,
            "cancel leaves the earlier ink"
        );
        assert_eq!(
            alpha_at(&desk.art, 30, 8),
            0,
            "the open stroke is not committed"
        );
        assert!(desk.gesture.is_none());
    }

    #[test]
    fn a_pen_move_uploads_only_the_rectangle_it_touched() {
        let mut desk = tiny();
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 6.0;
            slot.size_pressure = false;
        });
        let first = desk.take_display(false);
        assert!(matches!(first, Some(Display::Full(_))));
        assert_eq!(
            desk.take_display(false),
            None,
            "nothing changed, nothing to upload"
        );
        desk.down_doc(10.0, 10.0, Button::Primary);
        desk.move_doc(20.0, 12.0);
        let Some(Display::Part { x, y, w, h, rgba }) = desk.take_display(false) else {
            panic!("a pen move is a partial upload");
        };
        assert!(x <= 6 && y <= 6, "{x},{y}");
        assert!(x + w >= 24 && y + h >= 16, "{w},{h}");
        assert!(w < desk.art.width && h < desk.art.height);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        // The patched plate is the same picture a full compose gives.
        let patched = desk.plate.clone();
        let fresh = desk.display_rgba();
        assert_eq!(patched, fresh);
        desk.up_doc(20.0, 12.0);
        // Pen-up leaves the patched plate. It does not upload the whole picture.
        assert_eq!(desk.take_display(false), None);
    }

    #[test]
    fn pressure_is_read_per_sample_and_a_light_pen_draws_thin() {
        let mut heavy = tiny();
        heavy.edit_preset(Tool::Pen, |slot| slot.size = 12.0);
        heavy.down_doc(10.0, 16.0, Button::Primary);
        heavy.move_doc(36.0, 16.0);
        heavy.up_doc(36.0, 16.0);
        let mut light = tiny();
        light.edit_preset(Tool::Pen, |slot| slot.size = 12.0);
        light.set_pressure(0.15);
        light.down_doc(10.0, 16.0, Button::Primary);
        light.move_doc(36.0, 16.0);
        light.up_doc(36.0, 16.0);
        let count = |desk: &Desk| {
            desk.art.layers[0]
                .pixels
                .iter()
                .skip(3)
                .step_by(4)
                .filter(|a| **a > 0)
                .count()
        };
        assert!(count(&light) > 0);
        assert!(
            count(&light) < count(&heavy),
            "{} vs {}",
            count(&light),
            count(&heavy)
        );
        assert!(ToolSettings::default_for(Tool::Pencil).size_pressure);
    }

    #[test]
    fn zoom_at_keeps_the_document_point_under_the_pointer() {
        let mut desk = tiny();
        let frame = Frame {
            x: 100.0,
            y: 50.0,
            w: 400.0,
            h: 300.0,
        };
        desk.fit(frame);
        let (sx, sy) = (180.0, 120.0);
        let (w, h) = (desk.art.width as f32, desk.art.height as f32);
        let before = desk.camera.to_doc(frame, w, h, sx, sy);
        desk.zoom_at(frame, sx, sy, 2.5);
        let after = desk.camera.to_doc(frame, w, h, sx, sy);
        assert!((before.0 - after.0).abs() < 1e-3, "{before:?} {after:?}");
        assert!((before.1 - after.1).abs() < 1e-3, "{before:?} {after:?}");
        assert!(desk.camera.zoom > 1.0);
    }

    #[test]
    fn smudge_keeps_its_carry_until_release_and_a_group_cannot_be_painted() {
        let mut desk = tiny();
        paint_red(&mut desk, 8, 16);
        desk.set_tool(Tool::Smudge);
        desk.edit_preset(Tool::Smudge, |slot| {
            slot.size = 8.0;
            slot.edge = Edge::Hard;
            slot.fringe = 0.0;
            slot.softness = 0.0;
            slot.size_pressure = false;
            slot.opacity_pressure = false;
            slot.opacity = 1.0;
        });
        desk.down_doc(8.0, 16.0, Button::Primary);
        desk.move_doc(18.0, 16.0);
        desk.move_doc(30.0, 16.0);
        desk.up_doc(30.0, 16.0);
        assert!(desk.gesture.is_none());
        let index = ((16 * desk.art.width + 24) * 4) as usize;
        assert!(
            desk.art.layers[0].pixels[index] > 0,
            "carry should still be on the layer"
        );

        let group = desk.art.add_group(None, "Folder");
        desk.select_layer(group);
        let before = desk.art.layers[0].pixels.clone();
        desk.set_tool(Tool::Pen);
        desk.down_doc(4.0, 4.0, Button::Primary);
        assert!(desk.status.contains("Select a layer"));
        assert!(desk.gesture.is_none());
        assert_eq!(desk.art.layers[0].pixels, before);
    }

    #[test]
    fn warp_fill_point_bone_and_cut_follow_release() {
        let mut desk = tiny();
        desk.set_tool(Tool::WarpTarget);
        desk.down_doc(2.0, 2.0, Button::Primary);
        desk.down_doc(20.0, 2.0, Button::Primary);
        desk.down_doc(20.0, 20.0, Button::Primary);
        desk.down_doc(2.0, 20.0, Button::Primary);
        desk.enter();
        assert_eq!(desk.art.layers[0].vectors[0].len(), 4);
        assert_eq!(desk.art.layers[0].marker, "target");
        assert_eq!(alpha_at(&desk.art, 10, 10), 255);
        assert_eq!(alpha_at(&desk.art, 0, 0), 0);
        assert!(desk.warp_draft.is_empty());

        desk.set_tool(Tool::Point);
        desk.down_doc(20.0, 20.0, Button::Primary);
        desk.move_doc(30.0, 24.0);
        desk.up_doc(30.0, 24.0);
        assert!((desk.art.layers[0].vectors[0][2][0] - 30.0).abs() < 0.1);
        assert_eq!(alpha_at(&desk.art, 10, 10), 255);

        desk.set_tool(Tool::Vector);
        desk.down_doc(8.0, 28.0, Button::Primary);
        desk.move_doc(20.0, 28.0);
        desk.up_doc(20.0, 28.0);
        let stroke = desk.art.layers[0].strokes[0].id;
        desk.down_doc(8.0, 28.0, Button::Secondary);
        assert_eq!(
            desk.stroke_window,
            Some((desk.art.layers[0].id.clone(), stroke))
        );
        assert_eq!(desk.tool(), Tool::Vector);
        desk.set_tool(Tool::Bone);
        desk.down_doc(12.0, 28.0, Button::Primary);
        desk.move_doc(12.0, 8.0);
        desk.up_doc(12.0, 8.0);
        assert!(desk.art.layers[0].strokes[0].bone.is_some());

        desk.set_tool(Tool::WarpMask);
        desk.edit_preset(Tool::WarpMask, |slot| {
            slot.size = 6.0;
            slot.edge = Edge::Hard;
            slot.fringe = 0.0;
            slot.size_pressure = false;
        });
        desk.down_doc(40.0, 8.0, Button::Primary);
        desk.up_doc(40.0, 8.0);
        assert_eq!(desk.art.layers[0].marker, "mask");
        assert!(alpha_at(&desk.art, 40, 8) > 0);
        assert!(desk.art.layers[0].strokes.len() == 1);

        paint_red(&mut desk, 4, 4);
        desk.set_tool(Tool::CutRect);
        desk.down_doc(0.0, 0.0, Button::Primary);
        desk.move_doc(12.0, 12.0);
        desk.up_doc(12.0, 12.0);
        assert!(desk.cut_ready());
        desk.cut_to_folder();
        assert_eq!(alpha_at(&desk.art, 4, 4), 0);
        assert!(desk
            .art
            .layers
            .iter()
            .any(|node| node.kind == vmi_core::NodeKind::Group));
    }

    #[test]
    fn undo_stops_at_twenty_and_fill_is_pixels() {
        let mut desk = tiny();
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 4.0;
            slot.edge = Edge::Hard;
            slot.fringe = 0.0;
            slot.size_pressure = false;
            slot.opacity_pressure = false;
        });
        for index in 0..25 {
            let x = 3.0 + (index % 8) as f32 * 5.0;
            let y = 3.0 + (index / 8) as f32 * 6.0;
            desk.down_doc(x, y, Button::Primary);
            desk.up_doc(x, y);
        }
        assert_eq!(desk.undo_depth(), 20);
        desk.set_tool(Tool::Fill);
        desk.set_ink([0, 255, 0]);
        desk.art.layers[0].pixels.fill(0);
        desk.down_doc(1.0, 1.0, Button::Primary);
        assert_eq!(desk.art.layers[0].pixels[1], 255);
        desk.undo();
        assert_eq!(desk.art.layers[0].pixels[1], 0);
    }

    #[test]
    fn new_frames_land_after_the_hold_and_the_cut_grows_with_them() {
        let mut desk = tiny();
        let ink = desk.selected.clone().expect("the lone layer is selected");
        assert_eq!(desk.cut_end(), 1);
        assert!(desk.sheet_folder().is_none());

        // Shift+F on a plain layer wraps it, like Python: the layer is frame 1.
        // On twos it keeps frames 1–2 and the new cel takes 3–4.
        desk.set_hold(2);
        desk.new_cel(None, None);
        let folder = desk.sheet_folder().expect("the wrap is the sheet's folder");
        assert_eq!(
            anim::owning_animation(&desk.art.layers, &ink).as_deref(),
            Some(folder.as_str())
        );
        assert_eq!(desk.playhead(), 2);
        assert_eq!(desk.cut_end(), 4);
        assert!(desk.art.onion, "a new frame turns the skin on");
        let node = find(&desk.art.layers, &folder).unwrap();
        assert_eq!(node.name, "Animation");
        assert_eq!(node.frames, vec![0, 2]);
        assert_eq!(node.timeline[0], ink);

        // On threes from frame 3: the next starts at 6, the cut reaches 9.
        desk.set_hold(3);
        desk.new_cel(None, None);
        assert_eq!(desk.playhead(), 5);
        assert_eq!(desk.cut_end(), 8);
        let node = find(&desk.art.layers, &folder).unwrap();
        assert_eq!(node.frames, vec![0, 2, 5]);

        // Inside the cut it is Python's rule: right after the playhead, and
        // a later key moves only when it sat on that slot.
        desk.set_playhead(0);
        let first = find(&desk.art.layers, &folder).unwrap().timeline[0].clone();
        assert_eq!(
            desk.selected.as_deref(),
            Some(first.as_str()),
            "the pen follows the playhead to the cel it shows"
        );
        desk.set_hold(1);
        desk.new_cel(None, None);
        let node = find(&desk.art.layers, &folder).unwrap();
        assert_eq!(node.frames, vec![0, 1, 2, 5]);
        assert_eq!(desk.playhead(), 1);
        assert_eq!(
            desk.cut_end(),
            8,
            "a frame inside the cut leaves the stored end alone"
        );

        // Stepping stops at the ends unless wrap is on.
        desk.last_frame();
        assert_eq!(desk.playhead(), 7);
        desk.step_frame(1);
        assert_eq!(desk.playhead(), 7);
        desk.toggle_wrap();
        desk.step_frame(1);
        assert_eq!(desk.playhead(), 0);
        desk.step_frame(-1);
        assert_eq!(desk.playhead(), 7);

        // Undo takes the whole last frame back, cut end and all.
        desk.undo();
        let node = find(&desk.art.layers, &folder).unwrap();
        assert_eq!(node.frames, vec![0, 2, 5]);
        assert_eq!(desk.cut_end(), 8);

        // Playback at the cut's rate wraps when loop is on.
        desk.set_playhead(7);
        desk.toggle_play();
        assert!(desk.playing);
        assert!(
            desk.tick(5.0),
            "the idle gap before Play is not playback time"
        );
        assert_eq!(desk.playhead(), 7);
        assert!(desk.tick(1.0 / f64::from(desk.art.fps) + 0.001));
        assert_eq!(desk.playhead(), 0);
        desk.stop();

        // An exposure command stores a sheet on the folder without moving a
        // key. The sheet is the grid as it read, held to the cut end.
        desk.sheet_command(&folder, "reverse", 0, 3, 0);
        let node = find(&desk.art.layers, &folder).unwrap();
        let sheet = node
            .exposure
            .as_ref()
            .expect("the first command stores the sheet");
        assert_eq!(sheet.slots.len(), 8);
        assert_eq!(&sheet.slots[..3], ["001", "000", "000"]);
        assert_eq!(&sheet.slots[3..], ["001", "001", "002", "002", "002"]);
        assert_eq!(node.frames, vec![0, 2, 5]);
    }

    fn scratch(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("vmi-desk-{name}-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn tabs_keep_their_own_drawing_history_and_export_folder() {
        let mut desk = Desk::new(Session::default());
        desk.art = Art::blank(1, 1);
        desk.selected = None;
        assert!(desk.on_browser);
        assert!(desk.tabs.is_empty());

        // New picture: an unsaved tab, Paper under Ink, Ink selected. The pen
        // keeps the shared ink. A new page does not give this tool its own color.
        desk.open_blank(1920, 1080, "#FFFFFF", None, false).unwrap();
        assert_eq!(desk.tabs.len(), 1);
        assert_eq!(desk.shown, Some(0));
        assert!(!desk.on_browser);
        assert_eq!(desk.art.file_name, "Untitled 1920×1080");
        assert_eq!(desk.tab_names(), ["Untitled 1920×1080"]);
        let ink = desk
            .art
            .layers
            .iter()
            .find(|node| node.name == "Ink")
            .unwrap()
            .id
            .clone();
        assert_eq!(desk.selected.as_deref(), Some(ink.as_str()));
        assert_eq!(desk.ink, [0xFF, 0x3E, 0xB8]);
        assert_eq!(desk.preset().color, [0xFF, 0x3E, 0xB8]);
        assert_eq!(
            desk.status,
            "New picture 1920×1080. Draw on Ink. Save blueprint writes the file."
        );
        assert_eq!(desk.export_dir(), home_dir());
        let ink_alpha = |desk: &Desk| {
            let ink = desk
                .art
                .layers
                .iter()
                .find(|node| node.name == "Ink")
                .unwrap();
            ink.pixels[((3 * desk.art.width + 3) * 4 + 3) as usize]
        };
        desk.history.push(desk.art.clone());
        let width = desk.art.width;
        let ink_node = desk
            .art
            .layers
            .iter_mut()
            .find(|node| node.name == "Ink")
            .unwrap();
        ink_node.pixels[((3 * width + 3) * 4) as usize] = 255;
        ink_node.pixels[((3 * width + 3) * 4 + 3) as usize] = 255;

        // A second new picture of the same size takes the next number.
        desk.open_blank(1920, 1080, "#808080", None, false).unwrap();
        assert_eq!(
            desk.tab_names(),
            ["Untitled 1920×1080", "Untitled 1920×1080 2"]
        );
        assert_eq!(desk.shown, Some(1));
        assert_eq!(desk.undo_depth(), 0, "the new tab starts its own history");
        assert_eq!(ink_alpha(&desk), 0);

        // Back to the first: its pixels and history return.
        desk.select_tab(Some(0));
        assert_eq!(desk.shown, Some(0));
        assert_eq!(desk.art.file_name, "Untitled 1920×1080");
        assert_eq!(ink_alpha(&desk), 255);
        assert_eq!(desk.undo_depth(), 1);
        let events = desk.take_load_events();
        assert_eq!(
            events,
            [
                LoadEvent::Catchup("Untitled 1920×1080".into()),
                LoadEvent::Shown
            ]
        );

        // The browser tab stashes the drawing; the editor's state stays put.
        desk.select_tab(None);
        assert!(desk.on_browser);
        assert_eq!(desk.shown, Some(0));
        assert_eq!(
            desk.tab_names(),
            ["Untitled 1920×1080", "Untitled 1920×1080 2"]
        );

        // Save as: the tab takes the path, the export folder stays its own.
        desk.select_tab(Some(0));
        let dir = scratch("tabs");
        let path = dir.join("first.vmib");
        desk.save_blueprint(&path).unwrap();
        assert_eq!(desk.art.kind, "vmib");
        assert_eq!(desk.tabs[0].path, absolute(&path).display().to_string());
        assert_eq!(desk.blueprint_path(), Some(absolute(&path)));
        assert_eq!(desk.tab_names()[0], "first.vmib");
        assert_eq!(desk.status, "Saved first.vmib.");

        // Another tab cannot claim this tab's export folder.
        let mine = dir.join("out");
        assert!(desk.claim_export_dir(&mine));
        assert_eq!(desk.export_dir(), absolute(&mine));
        desk.select_tab(Some(1));
        desk.save_blueprint(&dir.join("first.vmib"))
            .unwrap_or_default();
        assert!(
            !desk.claim_export_dir(&mine),
            "two tabs with one scene folder"
        );
        assert!(desk.status.contains("This tab needs its own path."));

        // Closing the shown tab brings up the neighbor; closing the last one
        // goes back to the browser.
        desk.close_tab(1);
        assert_eq!(desk.tabs.len(), 1);
        assert_eq!(desk.shown, Some(0));
        assert_eq!(desk.art.file_name, "first.vmib");
        desk.close_tab(0);
        assert!(desk.tabs.is_empty());
        assert!(desk.on_browser);
        assert_eq!(desk.shown, None);
        assert_eq!(
            desk.status,
            "Open a .clip, .psd, .kra, .xcf, or .vmib file."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_blueprint_reads_on_its_own_thread_behind_the_gate() {
        let dir = scratch("load");
        let path = dir.join("shot.vmib");
        let mut art = Art::new(64, 48);
        write_vmib(&mut art, &path).unwrap();

        let mut desk = Desk::new(Session::default());
        desk.art = Art::blank(1, 1);
        assert!(desk.begin_open(&path, false));
        assert!(desk.is_loading());
        assert_eq!(desk.status, "Reading shot.vmib");
        assert_eq!(desk.tabs.len(), 1);
        assert!(!desk.tabs[0].loaded);
        assert_eq!(desk.tab_names(), ["shot.vmib"]);
        assert_eq!(
            desk.take_load_events(),
            [LoadEvent::Started("shot.vmib".into())]
        );
        // A second open while reading is refused.
        assert!(!desk.begin_open(&path, false));
        assert_eq!(desk.status, "Still reading.");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while desk.is_loading() && std::time::Instant::now() < deadline {
            desk.poll_load();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!desk.is_loading(), "the read finished");
        let events = desk.take_load_events();
        assert_eq!(
            events.first(),
            Some(&LoadEvent::Note("Reading shot.vmib".into()))
        );
        assert_eq!(events.last(), Some(&LoadEvent::Shown));
        assert!(desk.tabs[0].loaded);
        assert_eq!(desk.art.width, 64);
        assert_eq!(desk.art.path, absolute(&path).display().to_string());
        assert_eq!(desk.status, "shot.vmib  64×48  Opened.");
        assert!(desk.fitted, "a high-res open keeps every document pixel");
        assert_eq!(desk.camera.zoom, 1.0);
        assert_eq!(desk.tabs[0].export_dir, absolute(&dir.join("shot-vmi")));

        // The same file again selects its tab instead of reading twice.
        assert!(!desk.begin_open(&path, true));
        assert_eq!(desk.status, "shot.vmib is already open.");
        assert!(desk.tabs[0].low_res);

        // A missing file: the tab leaves and the text lands in the status bar.
        let missing = dir.join("gone.vmib");
        assert!(desk.begin_open(&missing, false));
        assert_eq!(desk.tabs.len(), 2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while desk.is_loading() && std::time::Instant::now() < deadline {
            desk.poll_load();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(desk.tabs.len(), 1);
        assert_eq!(desk.shown, Some(0));
        assert_eq!(desk.art.file_name, "shot.vmib");
        assert_eq!(desk.status, "That file is missing.");
        assert!(desk
            .take_load_events()
            .contains(&LoadEvent::Failed("That file is missing.".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_worktree_uses_the_main_checkout_python() {
        let saved = std::env::var_os("VMI_PYTHON");
        std::env::remove_var("VMI_PYTHON");
        let root = scratch("venv");
        let main = root.join("vmi-studio");
        let branch = root.join("vmi-studio-latest");
        let python = main.join(".venv").join("bin").join("python");
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, b"").unwrap();
        std::fs::create_dir_all(&branch).unwrap();
        let gitdir = main
            .join(".git")
            .join("worktrees")
            .join("vmi-studio-latest");
        std::fs::write(
            branch.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();

        assert_eq!(main_worktree(&branch), Some(main.clone()));
        assert_eq!(python_program(&branch), python);

        let own = branch.join(".venv").join("bin").join("python");
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::fs::write(&own, b"").unwrap();
        assert_eq!(python_program(&branch), own);
        let _ = std::fs::remove_dir_all(&root);
        if let Some(saved) = saved {
            std::env::set_var("VMI_PYTHON", saved);
        }
    }

    #[test]
    fn select_opens_the_folder_and_names_the_layer() {
        let mut desk = tiny();
        let mut sky = Node::layer("sky", "sky", desk.art.width, desk.art.height);
        for pixel in sky.pixels.chunks_mut(4) {
            pixel[1] = 255;
            pixel[3] = 255;
        }
        let mut sign = Node::layer("sign", "sign", desk.art.width, desk.art.height);
        let width = desk.art.width;
        let at = ((4 * width + 4) * 4) as usize;
        sign.pixels[at] = 255;
        sign.pixels[at + 3] = 255;
        let mut folder = Node::group("booth", "booth");
        folder.children.push(sign);
        desk.art.layers = vec![sky, folder];
        desk.collapsed.insert("booth".into());
        desk.set_tool(Tool::Select);
        desk.down_doc(4.2, 4.2, Button::Primary);
        assert_eq!(desk.selected.as_deref(), Some("sign"));
        assert_eq!(desk.status, "sign");
        assert!(!desk.collapsed.contains("booth"));
        assert_eq!(desk.reveal.as_deref(), Some("sign"));
        desk.down_doc(0.0, 0.0, Button::Primary);
        assert_eq!(desk.selected.as_deref(), Some("sky"));
        desk.down_doc(-1.0, 0.0, Button::Primary);
        assert_eq!(desk.status, "No layer there.");
        assert_eq!(desk.selected.as_deref(), Some("sky"));
    }

    #[test]
    fn select_opens_every_folder_on_the_way_in() {
        let mut desk = tiny();
        let width = desk.art.width;
        let height = desk.art.height;
        let mut sky = Node::layer("sky", "sky", width, height);
        for pixel in sky.pixels.chunks_mut(4) {
            pixel[1] = 255;
            pixel[3] = 255;
        }
        let mut sign = Node::layer("sign", "sign", width, height);
        let at = ((4 * width + 4) * 4) as usize;
        sign.pixels[at] = 255;
        sign.pixels[at + 3] = 255;
        let mut room = Node::group("room", "room");
        room.children.push(sign);
        let mut booth = Node::group("booth", "booth");
        booth.children.push(room);
        desk.art.layers = vec![sky, booth];
        desk.collapsed.insert("booth".into());
        desk.collapsed.insert("room".into());
        desk.selected = Some("sky".into());
        desk.set_tool(Tool::Select);
        desk.down_doc(4.2, 4.2, Button::Primary);
        assert_eq!(desk.selected.as_deref(), Some("sign"));
        assert_eq!(desk.status, "sign");
        assert!(!desk.collapsed.contains("booth"));
        assert!(!desk.collapsed.contains("room"));
        assert_eq!(desk.reveal.as_deref(), Some("sign"));
    }
}
