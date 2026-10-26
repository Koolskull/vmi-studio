//! Desk actions. The window calls these. A raster release writes pixels and
//! drops the path. A vector release keeps the stroke and rebuilds its cache.

use std::path::{Path, PathBuf};
use std::process::Command;

use vmi_core::doc::blit_into;
use vmi_core::{
    clamp_rect, cut_folder, erase_folder, export_png, find, find_mut, hex_of, hit_stroke, load_manifest, load_session,
    load_vmib, palette_action, parent_of, parse_hex, point_inside, present_region, rect_points, save_session,
    save_vmib as write_vmib, theme_or_studio, Art, Bone, DeskObject, History, Node, PaletteAction, Release, Section,
    Session, StrokeHit, Tool, ToolSettings,
};
use vmi_paint::{flood_fill, paint_samples, paint_segment, segment_bounds, Carry, StrokeSession};

use crate::gpu::{with_checker_at, Camera, Frame};

/// What the viewfinder has to upload this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Display {
    /// The whole picture over its checker.
    Full(Vec<u8>),
    /// One rectangle of it. `rgba` is `w * h * 4`.
    Part { x: u32, y: u32, w: u32, h: u32, rgba: Vec<u8> },
}

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
}

#[derive(Clone, Copy, Debug)]
pub struct Mark {
    pub x: f32,
    pub y: f32,
    pub hot: bool,
}

enum Gesture {
    Paint {
        layer: String,
        tool: Tool,
        settings: ToolSettings,
        before: Art,
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
            status: "Pick a cell in the tool grid. Color and size are on that panel. Ctrl+Z undoes.".into(),
            query: String::new(),
            tool_window: None,
            layer_window: None,
            stroke_window: None,
            object_window: None,
            camera: Camera::default(),
            fitted: false,
            last_frame: Frame { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
            dirty: true,
            dirty_rect: None,
            pressure: 1.0,
            cut_points: Vec::new(),
            cut_closed: false,
            warp_draft: Vec::new(),
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
        };
        desk.ensure_overlay();
        desk
    }

    pub fn launch() -> Self {
        let mut desk = Self::new(load_session());
        desk.persist = true;
        if !desk.session.path.is_empty() {
            let path = PathBuf::from(&desk.session.path);
            if path.is_file() {
                if let Err(err) = desk.open_path(&path) {
                    desk.status = err;
                }
            }
        }
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
        self.status = format!("{} theme.", self.theme().name);
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
        if let Some(slot) = self.session.presets.iter_mut().find(|item| item.tool == tool) {
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
                self.status = format!("Settings for {}. The current tool stays {}.", tool.name(), self.tool().name());
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

    pub fn layer_rows(&self) -> Vec<LayerRow> {
        let mut rows = Vec::new();
        walk_layers(&self.art.layers, 0, &mut rows);
        rows
    }

    pub fn select_layer(&mut self, id: String) {
        if find(&self.art.layers, &id).is_some() {
            self.selected = Some(id);
        }
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
        let embedded = find(&self.art.layers, &layer_id).map(|node| node.embedded).unwrap_or(false);
        if embedded {
            self.unbake(&layer_id);
            self.status = "Baked pixels were cleared so the stroke can redraw from its traits.".into();
        }
        if let Some(node) = find_mut(&mut self.art.layers, &layer_id) {
            if let Some(stroke) = node.strokes.iter_mut().find(|stroke| stroke.id == stroke_id) {
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

    pub fn add_object(&mut self) {
        let before = self.art.clone();
        let id = self.art.alloc_id();
        let name = format!("Object {}", self.art.objects.len() + 1);
        self.art.objects.push(DeskObject { id: id.clone(), name, kind: "static".into() });
        self.history.push(before);
        self.object_window = Some(id);
        self.object_edit_saved = false;
        self.status = "Object added. Right-click it for its name and kind.".into();
    }

    pub fn reorder_object(&mut self, from: usize, to: usize) {
        if from == to || from >= self.art.objects.len() || to >= self.art.objects.len() {
            return;
        }
        let before = self.art.clone();
        vmi_core::reorder_objects(&mut self.art.objects, from, to);
        self.history.push(before);
    }

    pub fn new_document(&mut self) {
        self.finish_new(Art::new(1280, 720));
        self.session.path.clear();
        self.session_dirty = true;
        self.status = "New 1280×720 drawing.".into();
    }

    pub fn open_path(&mut self, path: &Path) -> Result<(), String> {
        if !path.is_file() {
            return Err("That file is missing.".into());
        }
        let art = if extension_is(path, "vmib") {
            load_vmib(path)?
        } else {
            open_with_python(path)?
        };
        self.finish_new(art);
        self.session.path = path.display().to_string();
        self.session_dirty = true;
        self.status = format!("Opened {}.", path.display());
        Ok(())
    }

    pub fn save_blueprint(&mut self, path: &Path) -> Result<(), String> {
        write_vmib(&mut self.art, path)?;
        self.art.path = path.display().to_string();
        if let Some(name) = path.file_name() {
            self.art.file_name = name.to_string_lossy().to_string();
        }
        self.session.path = self.art.path.clone();
        self.session_dirty = true;
        self.status = format!("Saved {}.", path.display());
        Ok(())
    }

    pub fn export_png_file(&mut self, path: &Path) -> Result<(), String> {
        let percent = self.session.export_percent.clamp(1, 100);
        export_png(&mut self.art, path, percent)?;
        self.session_dirty = true;
        self.status = format!("Exported {} at {percent}%.", path.display());
        Ok(())
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
        self.pressure = if pressure.is_finite() { pressure.clamp(0.0, 1.0) } else { 1.0 };
    }

    fn touch_rect(&mut self, rect: Option<[i32; 4]>) {
        let Some(rect) = rect else {
            self.dirty = true;
            return;
        };
        self.dirty_rect = Some(match self.dirty_rect {
            Some(have) => [have[0].min(rect[0]), have[1].min(rect[1]), have[2].max(rect[2]), have[3].max(rect[3])],
            None => rect,
        });
    }

    pub fn up_doc(&mut self, x: f32, y: f32) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        let gesture = if needs_last_drag(&gesture, x, y) {
            self.drag(gesture, x, y)
        } else {
            gesture
        };
        self.finish(gesture);
        self.dirty = true;
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
        if find(&self.art.layers, &layer_id).map(|node| !node.is_layer()).unwrap_or(true) {
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
        let zoom = (frame.w / self.art.width as f32).min(frame.h / self.art.height as f32).max(0.01);
        self.camera = Camera { zoom, pan_x: 0.0, pan_y: 0.0, rotation: 0.0 };
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
        self.camera = Camera { zoom: 1.0, pan_x: 0.0, pan_y: 0.0, rotation: 0.0 };
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

    pub fn rotate(&mut self, delta: f32) {
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
        if (self.session.layers_width - layers).abs() > 0.5 || (self.session.brush_width - brushes).abs() > 0.5 {
            self.session.layers_width = layers;
            self.session.brush_width = brushes;
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
        let need = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        let full = force_full || self.dirty || self.plate.len() != need;
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
        Some(Display::Part { x: rect[0] as u32, y: rect[1] as u32, w, h, rgba })
    }

    fn compose(&mut self, rect: [i32; 4]) -> Vec<u8> {
        let width = self.art.width;
        let height = self.art.height;
        let mut plate = present_region(&mut self.art, rect);
        let rw = (rect[2] - rect[0]).max(0) as u32;
        let rh = (rect[3] - rect[1]).max(0) as u32;
        let need = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        if self.overlay_live && self.overlay.len() == need {
            blit_into(&mut plate, rw, rh, rect[0], rect[1], &self.overlay, width, height, 0, 0, 255, false, "normal");
        }
        with_checker_at(&plate, rw, rh, rect[0], rect[1])
    }

    pub fn marks(&self) -> Vec<Mark> {
        let mut marks = Vec::new();
        for point in &self.cut_points {
            marks.push(Mark { x: point[0], y: point[1], hot: false });
        }
        for point in &self.warp_draft {
            marks.push(Mark { x: point[0], y: point[1], hot: true });
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
                    marks.push(Mark { x: point[0], y: point[1], hot: true });
                }
            }
        }
        if matches!(self.tool(), Tool::Point | Tool::Bone | Tool::Curve | Tool::Vector | Tool::Grain) {
            for stroke in &node.strokes {
                for vertex in &stroke.vertices {
                    marks.push(Mark { x: vertex.x, y: vertex.y, hot: true });
                    if let Some(handle) = vertex.handle_in {
                        marks.push(Mark { x: handle[0], y: handle[1], hot: false });
                    }
                    if let Some(handle) = vertex.handle_out {
                        marks.push(Mark { x: handle[0], y: handle[1], hot: false });
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
        let settings = self.preset().clone();
        let (width, height) = (self.art.width, self.art.height);
        if tool == Tool::Fill {
            let before = self.art.clone();
            let changed = find_mut(&mut self.art.layers, &layer)
                .map(|node| flood_fill(&mut node.pixels, width, height, x.round() as i32, y.round() as i32, &settings))
                .unwrap_or(false);
            if changed {
                self.history.push(before);
                self.status = "Fill wrote pixels.".into();
                self.dirty = true;
            }
            return;
        }
        self.prepare_layer(&layer);
        let before = self.art.clone();
        let base = find(&self.art.layers, &layer).map(|node| node.pixels.clone()).unwrap_or_default();
        let mut session = StrokeSession::default();
        let pressure = self.pressure;
        session.begin(x, y, pressure);
        let mut changed = false;
        if tool.release() == Release::Stroke {
            self.ensure_overlay();
            self.overlay.fill(0);
            let mut carry = Carry::default();
            paint_samples(&mut self.overlay, width, height, &session.samples, tool, &settings, &mut carry);
            self.overlay_live = true;
            changed = true;
            self.dirty = true;
        } else if let Some(node) = find_mut(&mut self.art.layers, &layer) {
            changed = paint_samples(&mut node.pixels, width, height, &session.samples, tool, &settings, &mut session.carry);
            let first = (x, y, pressure);
            self.touch_rect(segment_bounds(width, height, first, first, tool, &settings));
        }
        self.gesture = Some(Gesture::Paint { layer, tool, settings, before, base, session, changed });
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
            self.gesture = Some(Gesture::Point { layer, stroke, which, before, changed: false });
            return;
        }
        if let Some(index) = self.nearest_corner(&layer, x, y) {
            let before = self.art.clone();
            self.gesture = Some(Gesture::WarpCorner { layer, index, before, changed: false });
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
        self.gesture = Some(Gesture::Bone { layer, stroke, before, x0: x, y0: y, changed: false });
    }

    fn add_warp_corner(&mut self, x: f32, y: f32) {
        let Ok(layer) = self.paint_target() else {
            self.status = "Select a layer.".into();
            return;
        };
        self.warp_layer = Some(layer);
        self.warp_draft.push([x, y]);
        self.status = format!("{} corners. Enter keeps the polygon.", self.warp_draft.len());
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
            Gesture::Paint { mut session, layer, tool, settings, before, base, mut changed } => {
                let pressure = self.pressure;
                if tool == Tool::Line {
                    // The old line and the new one both need a repaint.
                    let start = session.samples.first().copied().unwrap_or((x, y, pressure));
                    let old_end = session.samples.last().copied().unwrap_or(start);
                    session.line_to(x, y, pressure);
                    if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                        if node.pixels.len() == base.len() {
                            node.pixels.copy_from_slice(&base);
                        }
                        let mut carry = Carry::default();
                        changed |= paint_samples(&mut node.pixels, width, height, &session.samples, tool, &settings, &mut carry);
                    }
                    self.touch_rect(segment_bounds(width, height, start, old_end, tool, &settings));
                    self.touch_rect(segment_bounds(width, height, start, (x, y, pressure), tool, &settings));
                } else if tool.release() == Release::Stroke {
                    if moved_from(session.samples.last(), x, y) {
                        // The overlay is a preview. Dabs land along the move only;
                        // the kept stroke is rebuilt from its traits on release.
                        let from = session.samples.last().copied().unwrap_or((x, y, pressure));
                        session.push(x, y, pressure);
                        self.ensure_overlay();
                        let mut carry = Carry::default();
                        paint_segment(&mut self.overlay, width, height, from, (x, y, pressure), tool, &settings, &mut carry);
                        self.overlay_live = true;
                        changed = true;
                        self.touch_rect(segment_bounds(width, height, from, (x, y, pressure), tool, &settings));
                    }
                } else if moved_from(session.samples.last(), x, y) {
                    let from = session.samples.last().copied().unwrap_or((x, y, pressure));
                    session.push(x, y, pressure);
                    if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                        changed |= paint_segment(
                            &mut node.pixels,
                            width,
                            height,
                            from,
                            (x, y, pressure),
                            tool,
                            &settings,
                            &mut session.carry,
                        );
                    }
                    self.touch_rect(segment_bounds(width, height, from, (x, y, pressure), tool, &settings));
                }
                Gesture::Paint { session, layer, tool, settings, before, base, changed }
            }
            Gesture::Point { layer, stroke, which, before, mut changed } => {
                if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                    if let Some(found) = node.strokes.iter_mut().find(|item| item.id == stroke) {
                        let moved = match which {
                            StrokeHit::Vertex(index) => found.vertices.get(index).map(|vertex| (vertex.x - x).hypot(vertex.y - y) > 0.01).unwrap_or(false),
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
                                        vertex.x = x;
                                        vertex.y = y;
                                    }
                                }
                                StrokeHit::HandleIn(index) => {
                                    if let Some(vertex) = found.vertices.get_mut(index) {
                                        vertex.handle_in = Some([x, y]);
                                    }
                                }
                                StrokeHit::HandleOut(index) => {
                                    if let Some(vertex) = found.vertices.get_mut(index) {
                                        vertex.handle_out = Some([x, y]);
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
                Gesture::Point { layer, stroke, which, before, changed }
            }
            Gesture::Bone { layer, stroke, before, x0, y0, mut changed } => {
                if (x - x0).hypot(y - y0) > 1.0 {
                    if !changed {
                        self.unbake(&layer);
                    }
                    let reach = self.preset_of(Tool::Bone).bone_reach;
                    if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                        if let Some(found) = node.strokes.iter_mut().find(|item| item.id == stroke) {
                            found.bone = Some(Bone { x0, y0, x1: x, y1: y, reach });
                            found.touch();
                            changed = true;
                            self.dirty = true;
                        }
                    }
                }
                Gesture::Bone { layer, stroke, before, x0, y0, changed }
            }
            Gesture::WarpCorner { layer, index, before, mut changed } => {
                if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                    if let Some(point) = node.vectors.first_mut().and_then(|poly| poly.get_mut(index)) {
                        if (point[0] - x).hypot(point[1] - y) > 0.01 {
                            *point = [x, y];
                            changed = true;
                            self.dirty = true;
                        }
                    }
                }
                Gesture::WarpCorner { layer, index, before, changed }
            }
            Gesture::CutRect { x0, y0 } => {
                self.cut_points = rect_points(x0, y0, x, y);
                self.dirty = true;
                Gesture::CutRect { x0, y0 }
            }
            Gesture::CutLasso => {
                if self.cut_points.last().map(|point| (point[0] - x).hypot(point[1] - y) > 1.0).unwrap_or(true) {
                    self.cut_points.push([x, y]);
                    self.dirty = true;
                }
                Gesture::CutLasso
            }
        }
    }

    fn finish(&mut self, gesture: Gesture) {
        match gesture {
            Gesture::Paint { mut session, layer, tool, settings, before, changed, .. } => {
                if tool.release() == Release::Stroke {
                    let id = self.art.alloc_stroke();
                    if let Some(stroke) = session.end_stroke(id, tool, &settings) {
                        self.unbake(&layer);
                        if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                            node.strokes.push(stroke);
                        }
                        self.history.push(before);
                        self.status = "Vector stroke kept. The picture is a cache.".into();
                    }
                    self.overlay_live = false;
                    self.overlay.fill(0);
                } else {
                    session.end_pixels();
                    if changed {
                        if tool == Tool::WarpMask {
                            if let Some(node) = find_mut(&mut self.art.layers, &layer) {
                                node.marker = "mask".into();
                            }
                        }
                        self.history.push(before);
                        self.status = format!("{} wrote pixels.", tool.name());
                    }
                }
            }
            Gesture::Point { changed, before, layer, .. } => {
                if changed {
                    self.unbake(&layer);
                    self.refill_warp(&layer);
                    self.history.push(before);
                    self.status = "Point moved. The stroke cache rebuilt.".into();
                }
            }
            Gesture::Bone { changed, before, .. } => {
                if changed {
                    self.history.push(before);
                    self.status = "Bone kept. The stroke cache rebuilt.".into();
                }
            }
            Gesture::WarpCorner { changed, before, layer, .. } => {
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
        let before = match gesture {
            Gesture::Paint { before, .. }
            | Gesture::Point { before, .. }
            | Gesture::Bone { before, .. }
            | Gesture::WarpCorner { before, .. } => Some(before),
            Gesture::CutRect { .. } | Gesture::CutLasso => None,
        };
        if let Some(before) = before {
            self.art = before;
        }
        self.overlay_live = false;
        self.overlay.fill(0);
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
        self.cut_points.clear();
        self.cut_closed = false;
        self.warp_draft.clear();
        self.warp_layer = None;
        self.selected = first_layer(&self.art.layers);
        self.layer_window = None;
        self.stroke_window = None;
        self.object_window = None;
        self.layer_edit_saved = false;
        self.stroke_edit_saved = false;
        self.object_edit_saved = false;
        self.fitted = false;
        self.dirty = true;
        self.ensure_overlay();
    }

    fn after_history(&mut self) {
        self.overlay_live = false;
        self.overlay.fill(0);
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

fn walk_layers(nodes: &[Node], depth: usize, out: &mut Vec<LayerRow>) {
    for node in nodes.iter().rev() {
        out.push(LayerRow {
            id: node.id.clone(),
            name: node.name.clone(),
            depth,
            group: !node.is_layer(),
            visible: node.visible,
        });
        walk_layers(&node.children, depth + 1, out);
    }
}

fn moved_from(last: Option<&(f32, f32, f32)>, x: f32, y: f32) -> bool {
    last.map(|point| (point.0 - x).hypot(point.1 - y) > 0.4).unwrap_or(true)
}

fn needs_last_drag(gesture: &Gesture, x: f32, y: f32) -> bool {
    match gesture {
        Gesture::Paint { tool, session, .. } => {
            if *tool == Tool::Line {
                return true;
            }
            moved_from(session.samples.last(), x, y)
        }
        Gesture::CutLasso | Gesture::CutRect { .. } | Gesture::Point { .. } | Gesture::Bone { .. } | Gesture::WarpCorner { .. } => {
            true
        }
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
    path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case(expect)).unwrap_or(false)
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

fn open_with_python(path: &Path) -> Result<Art, String> {
    let root = studio_root().ok_or_else(|| "The Python studio is not next to this program.".to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("vmi-open-{stamp}"));
    std::fs::create_dir_all(dir.join("rasters")).map_err(|err| err.to_string())?;
    let manifest = dir.join("manifest.json");
    let output = Command::new("python3")
        .arg("-m")
        .arg("vmi_studio.bridge")
        .arg("open")
        .arg(path)
        .arg("--manifest")
        .arg(&manifest)
        .arg("--rasters")
        .arg(dir.join("rasters"))
        .current_dir(&root)
        .output()
        .map_err(|err| format!("Python could not start ({err})."))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let line = err.lines().find(|line| !line.trim().is_empty()).unwrap_or("Python could not open that file.");
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
        assert!(desk.art.layers[0].pixels.iter().skip(3).step_by(4).all(|alpha| *alpha == 0));
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
    fn a_pen_move_uploads_only_the_rectangle_it_touched() {
        let mut desk = tiny();
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 6.0;
            slot.size_pressure = false;
        });
        let first = desk.take_display(false);
        assert!(matches!(first, Some(Display::Full(_))));
        assert_eq!(desk.take_display(false), None, "nothing changed, nothing to upload");
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
        assert!(matches!(desk.take_display(false), Some(Display::Full(_))));
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
        let count = |desk: &Desk| desk.art.layers[0].pixels.iter().skip(3).step_by(4).filter(|a| **a > 0).count();
        assert!(count(&light) > 0);
        assert!(count(&light) < count(&heavy), "{} vs {}", count(&light), count(&heavy));
        assert!(ToolSettings::default_for(Tool::Pencil).size_pressure);
    }

    #[test]
    fn zoom_at_keeps_the_document_point_under_the_pointer() {
        let mut desk = tiny();
        let frame = Frame { x: 100.0, y: 50.0, w: 400.0, h: 300.0 };
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
        assert!(desk.art.layers[0].pixels[index] > 0, "carry should still be on the layer");

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
        assert_eq!(desk.stroke_window, Some((desk.art.layers[0].id.clone(), stroke)));
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
        assert!(desk.art.layers.iter().any(|node| node.kind == vmi_core::NodeKind::Group));
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
        desk.edit_preset(Tool::Fill, |slot| slot.color = [0, 255, 0]);
        desk.art.layers[0].pixels.fill(0);
        desk.down_doc(1.0, 1.0, Button::Primary);
        assert_eq!(desk.art.layers[0].pixels[1], 255);
        desk.undo();
        assert_eq!(desk.art.layers[0].pixels[1], 0);
    }
}
