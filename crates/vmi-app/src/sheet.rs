//! The sheet: KoolDraw's timeline, copied. One grid drawn two ways.
//!
//! Vertically it is a tracker: time runs down, every level is a column, the
//! front level is the left column and the back level is the last. On its
//! side it is the film strip: levels are lanes, frames run right. Time is
//! stable, so lanes and columns never jump when the playhead moves. A cel
//! that holds is one cell: a dotted line down the sheet, dashes in the film
//! cell. A rest is `---`. A red needle marks the exact frame.

use eframe::egui;
use vmi_core::anim::{frame_map, timeline_cels, XColumn};
use vmi_core::{find, Theme};

use crate::desk::Desk;

/// X-sheet geometry, the same numbers as the Python sheet.
pub const CELL_W: f32 = 44.0;
pub const CELL_H: f32 = 16.0;
pub const HEAD: f32 = 18.0;
pub const FRAME_W: f32 = 32.0;
/// Film geometry: a frame is wide enough for a three-digit hex slot.
pub const FILM_W: f32 = 32.0;
pub const LANE_NAME_W: f32 = 56.0;
/// The needle.
pub const RED: egui::Color32 = egui::Color32::from_rgb(0xff, 0x2a, 0x2a);

/// How the sheet is turned. Auto picks by the window: wide shows the sheet,
/// tall shows the film.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Film,
    XSheet,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::Film => "FILM",
            Self::XSheet => "X-SHEET",
        }
    }

    /// The orientation actually drawn.
    pub fn resolve(self, window: egui::Vec2) -> Self {
        match self {
            Self::Auto if window.x >= window.y => Self::XSheet,
            Self::Auto => Self::Film,
            other => other,
        }
    }
}

/// A drag in progress on the sheet.
#[derive(Clone, Debug)]
pub enum Drag {
    /// The pointer is scrubbing the ruler.
    Scrub,
    /// A key cell is being moved along time.
    Key {
        folder: String,
        cel: String,
        from: u32,
        to: u32,
    },
    /// A range is being picked in one column.
    Pick { folder: String, anchor: u32 },
}

#[derive(Clone, Debug)]
pub struct SheetState {
    pub drag: Option<Drag>,
    /// The frame the view last scrolled to keep in sight.
    pub kept: Option<u32>,
    /// 1 is the normal cell. Smaller shows a longer cut on the same grid.
    pub zoom: f32,
}

impl Default for SheetState {
    fn default() -> Self {
        Self {
            drag: None,
            kept: None,
            zoom: 1.0,
        }
    }
}

/// What a click or drag on the sheet asks the desk to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    SetFrame(u32),
    /// Show and select the cel a level holds at a frame.
    Pick {
        folder: String,
        frame: u32,
    },
    /// Pick a range of cells in one column, for the command menu.
    Range {
        folder: String,
        start: u32,
        count: u32,
    },
    MoveCel {
        folder: String,
        cel: String,
        start: u32,
    },
    NewCel {
        folder: String,
        frame: u32,
    },
    DeleteCel {
        folder: String,
        cel: String,
    },
    /// An exposure command on the picked range.
    Command {
        folder: String,
        name: &'static str,
        start: u32,
        count: u32,
        arg: i64,
    },
    /// Shift+F with nothing picked: a frame after the selected drawing.
    NewFrame,
    /// Layer > Animation folder on the selected folder.
    AnimateSelected,
    /// Open a clip. The sheet becomes that folder's frames.
    Enter(String),
    /// Step out of the clip. The cut is the sheet when nothing is left above.
    Leave,
}

/// One row of one column, in KoolDraw's grammar: a key, a held cell, or a rest.
/// The painted label is the frame slot in hex. `number` is the drawing's place
/// in the pool, kept so the sheet grammar test can still read that order.
#[derive(Clone, Debug, Default)]
struct Row {
    /// The cel the level shows here, when the row stands for one.
    cel: Option<String>,
    /// A drawing is on this frame (key or hold). False is a rest.
    filled: bool,
    /// This frame starts the drawing.
    key: bool,
    /// Drawing order in the pool, from 1. The cell paints the slot, not this.
    #[allow(dead_code)]
    number: Option<usize>,
}

/// A cell of the sheet, resolved for drawing.
struct CellInfo<'a> {
    column: &'a XColumn,
    frame: u32,
    cel: Option<&'a str>,
    filled: bool,
    key: bool,
}

/// The rows of one column. A folder without a stored exposure sheet reads
/// its keys: the cel holds until the next key. A folder with one reads the
/// sheet, the way Python's X-sheet did: a slot that repeats the slot above
/// is a hold, a changed slot is a key, a blank is a rest. Slot `n` is the
/// `n`th drawing in frame order, which is also its printed number.
fn rows_of(column: &XColumn, folder: Option<&vmi_core::Node>, rows: u32) -> Vec<Row> {
    let mut out = Vec::with_capacity(rows as usize);
    if column.stored {
        let mut order: Vec<String> = Vec::new();
        if let Some(folder) = folder {
            for (_, cel) in frame_map(folder) {
                if !order.contains(&cel.id) {
                    order.push(cel.id.clone());
                }
            }
        }
        for frame in 0..rows as usize {
            let slot = column.labels.get(frame).map(String::as_str).unwrap_or("");
            if slot.is_empty() {
                out.push(Row::default());
                continue;
            }
            let index = usize::from_str_radix(slot, 16).ok();
            let key = frame == 0 || column.labels.get(frame - 1).map(String::as_str) != Some(slot);
            out.push(Row {
                cel: index.and_then(|index| order.get(index).cloned()),
                filled: true,
                key,
                number: index.map(|index| index + 1),
            });
        }
        return out;
    }
    let numbers: std::collections::HashMap<String, usize> = folder
        .map(|folder| {
            timeline_cels(folder)
                .iter()
                .enumerate()
                .map(|(index, cel)| (cel.id.clone(), index + 1))
                .collect()
        })
        .unwrap_or_default();
    for frame in 0..rows as usize {
        let cel = column.cells.get(frame).cloned().flatten();
        out.push(Row {
            number: cel.as_deref().and_then(|id| numbers.get(id).copied()),
            filled: cel.is_some(),
            key: column.keys.get(frame).copied().unwrap_or(false),
            cel,
        });
    }
    out
}

/// Draw the sheet in `mode` and hand back what the pointer asked for.
pub fn show(
    ui: &mut egui::Ui,
    desk: &Desk,
    state: &mut SheetState,
    mode: Mode,
    theme: &Theme,
) -> Vec<Act> {
    let mut acts = Vec::new();
    clip_bar(ui, desk, state, theme, &mut acts);
    let end = desk.sheet_len();
    let mut columns = desk.sheet_columns();
    // Front level first. The document lists the back first.
    columns.reverse();
    if columns.is_empty() {
        empty_sheet(ui, desk, theme, &mut acts);
        return acts;
    }
    // A stored sheet may run past the cut; the grid shows all of it.
    let rows = columns
        .iter()
        .filter(|column| column.stored)
        .map(|column| column.labels.len() as u32)
        .max()
        .unwrap_or(0)
        .max(end)
        .max(1);
    let grid: Vec<Vec<Row>> = columns
        .iter()
        .map(|column| rows_of(column, find(&desk.art.layers, &column.folder_id), rows))
        .collect();
    let drawn = mode.resolve(ui.ctx().screen_rect().size());
    let zoom = state.zoom.clamp(0.35, 1.6);
    match drawn {
        Mode::Film => film(
            ui, desk, state, theme, &columns, &grid, rows, zoom, &mut acts,
        ),
        _ => xsheet(
            ui, desk, state, theme, &columns, &grid, rows, zoom, &mut acts,
        ),
    }
    acts
}

/// The cut, or the clip we are inside, plus the sheet zoom.
fn clip_bar(
    ui: &mut egui::Ui,
    desk: &Desk,
    state: &mut SheetState,
    theme: &Theme,
    acts: &mut Vec<Act>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
        if let Some(name) = desk.clip_label() {
            ui.label(
                egui::RichText::new(name)
                    .monospace()
                    .small()
                    .color(rgb(theme.ink)),
            );
            if ui
                .small_button("OUT")
                .on_hover_text("Leave this clip. The cut is the longer timeline.")
                .clicked()
            {
                acts.push(Act::Leave);
            }
        } else {
            ui.label(
                egui::RichText::new("CUT")
                    .monospace()
                    .small()
                    .color(rgb(theme.quiet)),
            );
        }
        if ui
            .small_button("-")
            .on_hover_text("Zoom the sheet out. A long cut stays one grid.")
            .clicked()
        {
            state.zoom = (state.zoom / 1.25).clamp(0.35, 1.6);
        }
        if ui
            .small_button("+")
            .on_hover_text("Zoom the sheet in.")
            .clicked()
        {
            state.zoom = (state.zoom * 1.25).clamp(0.35, 1.6);
        }
    });
}

/// Nothing animated yet. The two ways in are the Layer menu's: New frame
/// wraps the selected drawing as frame 1, Animation folder marks a folder.
fn empty_sheet(ui: &mut egui::Ui, desk: &Desk, theme: &Theme, acts: &mut Vec<Act>) {
    ui.label(
        egui::RichText::new("No animation yet.")
            .color(rgb(theme.soft))
            .monospace(),
    );
    let selected_group = desk
        .selected
        .as_deref()
        .and_then(|id| find(&desk.art.layers, id))
        .map(|node| !node.is_layer())
        .unwrap_or(false);
    ui.horizontal(|ui| {
        if ui
            .small_button("New frame    Shift+F")
            .on_hover_text("The selected layer becomes frame 1 of an Animation folder, with an empty frame after it.")
            .clicked()
        {
            acts.push(Act::NewFrame);
        }
        if selected_group
            && ui.small_button("Animation folder").on_hover_text("The selected folder is one sequence. Its layers are its cels.").clicked()
        {
            acts.push(Act::AnimateSelected);
        }
    });
    ui.label(
        egui::RichText::new("Or right-click a folder in Layers and choose Animation.")
            .color(rgb(theme.quiet))
            .small(),
    );
}

fn cell_info<'a>(column: &'a XColumn, grid: &'a [Row], frame: u32) -> CellInfo<'a> {
    match grid.get(frame as usize) {
        Some(row) => CellInfo {
            column,
            frame,
            cel: row.cel.as_deref(),
            filled: row.filled,
            key: row.key,
        },
        None => CellInfo {
            column,
            frame,
            cel: None,
            filled: false,
            key: false,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn xsheet(
    ui: &mut egui::Ui,
    desk: &Desk,
    state: &mut SheetState,
    theme: &Theme,
    columns: &[XColumn],
    grid: &[Vec<Row>],
    rows: u32,
    zoom: f32,
    acts: &mut Vec<Act>,
) {
    let cell_w = CELL_W * zoom;
    let cell_h = (CELL_H * zoom).max(8.0);
    let playhead = desk.local_playhead();
    let width = FRAME_W + columns.len() as f32 * cell_w;
    let height = HEAD + rows as f32 * cell_h;
    let scroll = egui::ScrollArea::both()
        .id_salt("xsheet")
        .auto_shrink([false, false]);
    scroll.show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(width.max(ui.available_width()), height),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, rgb(theme.field));
        // Header: level names, front first.
        for (index, column) in columns.iter().enumerate() {
            let x = rect.min.x + FRAME_W + index as f32 * cell_w;
            let head =
                egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(cell_w, HEAD));
            let head_response = ui.interact(
                head,
                ui.id().with(("col", &column.folder_id)),
                egui::Sense::click(),
            );
            if head_response.double_clicked() {
                acts.push(Act::Enter(column.folder_id.clone()));
            }
            let selected = desk.selected.as_deref() == Some(column.folder_id.as_str())
                || desk
                    .selected
                    .as_deref()
                    .and_then(|id| vmi_core::anim::owning_animation(&desk.art.layers, id))
                    .as_deref()
                    == Some(column.folder_id.as_str());
            let color = if selected {
                rgb(theme.ink)
            } else {
                rgb(theme.soft)
            };
            painter.text(
                head.center(),
                egui::Align2::CENTER_CENTER,
                clip_name(&column.name, 6),
                egui::FontId::monospace(10.0),
                color,
            );
            painter.line_segment(
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(1.0_f32, rgb(theme.rule)),
            );
        }
        painter.line_segment(
            [
                egui::pos2(rect.min.x, rect.min.y + HEAD),
                egui::pos2(rect.min.x + width, rect.min.y + HEAD),
            ],
            egui::Stroke::new(1.0_f32, rgb(theme.rule)),
        );
        // Frame slots down the left, `000` then `001`. Dragging them scrubs.
        let ruler = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, rect.min.y + HEAD),
            egui::vec2(FRAME_W, rows as f32 * cell_h),
        );
        let ruler_response = ui.interact(
            ruler,
            ui.id().with("xsheet-ruler"),
            egui::Sense::click_and_drag(),
        );
        for frame in 0..rows {
            let y = rect.min.y + HEAD + frame as f32 * cell_h;
            let here = frame == playhead;
            let color = if here {
                rgb(theme.ink)
            } else if frame % 4 == 0 {
                rgb(theme.soft)
            } else {
                rgb(theme.quiet)
            };
            painter.text(
                egui::pos2(rect.min.x + FRAME_W - 4.0, y + cell_h * 0.5),
                egui::Align2::RIGHT_CENTER,
                vmi_core::slot_hex(frame),
                egui::FontId::monospace(10.0),
                color,
            );
            if frame % 4 == 0 {
                painter.line_segment(
                    [
                        egui::pos2(rect.min.x + FRAME_W, y),
                        egui::pos2(rect.min.x + width, y),
                    ],
                    egui::Stroke::new(1.0_f32, rgb(theme.rule)),
                );
            }
        }
        if ruler_response.drag_started() || ruler_response.clicked() {
            state.drag = Some(Drag::Scrub);
        }
        if matches!(state.drag, Some(Drag::Scrub)) {
            if let Some(pos) = ruler_response.interact_pointer_pos() {
                let frame = ((pos.y - ruler.min.y) / cell_h).floor().max(0.0) as u32;
                acts.push(Act::SetFrame(frame.min(rows - 1)));
            }
            if !ui.input(|input| input.pointer.primary_down()) {
                state.drag = None;
            }
        }
        // Cells.
        for (index, column) in columns.iter().enumerate() {
            let x = rect.min.x + FRAME_W + index as f32 * cell_w;
            for frame in 0..rows {
                let y = rect.min.y + HEAD + frame as f32 * cell_h;
                let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));
                let info = cell_info(column, &grid[index], frame);
                cell_interact(ui, desk, state, theme, &painter, cell, &info, true, acts);
            }
        }
        // The needle: a red line across the exact frame.
        let y = rect.min.y + HEAD + playhead as f32 * cell_h;
        painter.line_segment(
            [egui::pos2(rect.min.x, y), egui::pos2(rect.min.x + width, y)],
            egui::Stroke::new(1.0_f32, RED),
        );
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(rect.min.x, y - 1.0), egui::vec2(3.0, 3.0)),
            0.0,
            RED,
        );
        // Keep the playhead in sight. Only then does the view move.
        if state.kept != Some(playhead) {
            state.kept = Some(playhead);
            let row =
                egui::Rect::from_min_size(egui::pos2(rect.min.x, y), egui::vec2(FRAME_W, cell_h));
            ui.scroll_to_rect(row, None);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn film(
    ui: &mut egui::Ui,
    desk: &Desk,
    state: &mut SheetState,
    theme: &Theme,
    columns: &[XColumn],
    grid: &[Vec<Row>],
    rows: u32,
    zoom: f32,
    acts: &mut Vec<Act>,
) {
    let film_w = FILM_W * zoom;
    let cell_h = (CELL_H * zoom).max(8.0);
    let playhead = desk.local_playhead();
    let frames = rows;
    let width = LANE_NAME_W + frames as f32 * film_w;
    let height = HEAD + columns.len() as f32 * cell_h;
    let scroll = egui::ScrollArea::both()
        .id_salt("film")
        .auto_shrink([false, false]);
    scroll.show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(width, height.max(ui.available_height())),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, rgb(theme.field));
        // Ruler along the top. Dragging it scrubs.
        let ruler = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + LANE_NAME_W, rect.min.y),
            egui::vec2(frames as f32 * film_w, HEAD),
        );
        let ruler_response = ui.interact(
            ruler,
            ui.id().with("film-ruler"),
            egui::Sense::click_and_drag(),
        );
        for frame in 0..frames {
            let x = rect.min.x + LANE_NAME_W + frame as f32 * film_w;
            let here = frame == playhead;
            let color = if here {
                rgb(theme.ink)
            } else if frame % 4 == 0 {
                rgb(theme.soft)
            } else {
                rgb(theme.quiet)
            };
            painter.text(
                egui::pos2(x + film_w * 0.5, rect.min.y + HEAD * 0.5),
                egui::Align2::CENTER_CENTER,
                vmi_core::slot_hex(frame),
                egui::FontId::monospace(9.0),
                color,
            );
            if frame % 4 == 0 {
                painter.line_segment(
                    [egui::pos2(x, rect.min.y + HEAD), egui::pos2(x, rect.max.y)],
                    egui::Stroke::new(1.0_f32, rgb(theme.rule)),
                );
            }
        }
        if ruler_response.drag_started() || ruler_response.clicked() {
            state.drag = Some(Drag::Scrub);
        }
        if matches!(state.drag, Some(Drag::Scrub)) {
            if let Some(pos) = ruler_response.interact_pointer_pos() {
                let frame = ((pos.x - ruler.min.x) / film_w).floor().max(0.0) as u32;
                acts.push(Act::SetFrame(frame.min(frames - 1)));
            }
            if !ui.input(|input| input.pointer.primary_down()) {
                state.drag = None;
            }
        }
        // Lanes, front first.
        for (index, column) in columns.iter().enumerate() {
            let y = rect.min.y + HEAD + index as f32 * cell_h;
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.min.x + width, y)],
                egui::Stroke::new(1.0_f32, rgb(theme.rule)),
            );
            let name_rect = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, y),
                egui::vec2(LANE_NAME_W, cell_h),
            );
            let name_response = ui.interact(
                name_rect,
                ui.id().with(("lane", &column.folder_id)),
                egui::Sense::click(),
            );
            if name_response.double_clicked() {
                acts.push(Act::Enter(column.folder_id.clone()));
            }
            let selected = desk.selected.as_deref() == Some(column.folder_id.as_str())
                || desk
                    .selected
                    .as_deref()
                    .and_then(|id| vmi_core::anim::owning_animation(&desk.art.layers, id))
                    .as_deref()
                    == Some(column.folder_id.as_str());
            let color = if selected {
                rgb(theme.ink)
            } else {
                rgb(theme.soft)
            };
            painter.text(
                egui::pos2(rect.min.x + 4.0, y + cell_h * 0.5),
                egui::Align2::LEFT_CENTER,
                clip_name(&column.name, 7),
                egui::FontId::monospace(10.0),
                color,
            );
            for frame in 0..frames {
                let x = rect.min.x + LANE_NAME_W + frame as f32 * film_w;
                let cell = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(film_w, cell_h));
                let info = cell_info(column, &grid[index], frame);
                cell_interact(ui, desk, state, theme, &painter, cell, &info, false, acts);
            }
        }
        // The needle.
        let x = rect.min.x + LANE_NAME_W + playhead as f32 * film_w;
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(1.0_f32, RED),
        );
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x - 1.0, rect.min.y), egui::vec2(3.0, 3.0)),
            0.0,
            RED,
        );
        if state.kept != Some(playhead) {
            state.kept = Some(playhead);
            let col =
                egui::Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(film_w, HEAD));
            ui.scroll_to_rect(col, None);
        }
    });
}

/// One cell: paint it, then answer the pointer.
#[allow(clippy::too_many_arguments)]
fn cell_interact(
    ui: &mut egui::Ui,
    desk: &Desk,
    state: &mut SheetState,
    theme: &Theme,
    painter: &egui::Painter,
    cell: egui::Rect,
    info: &CellInfo<'_>,
    vertical: bool,
    acts: &mut Vec<Act>,
) {
    let folder = info.column.folder_id.as_str();
    let id = ui.id().with(("cell", folder, info.frame));
    let response = ui.interact(cell, id, egui::Sense::click_and_drag());
    let picked = desk
        .sheet_pick
        .as_ref()
        .map(|pick| {
            pick.folder == folder
                && info.frame >= pick.start
                && info.frame < pick.start + pick.count
        })
        .unwrap_or(false);
    let moving = match &state.drag {
        Some(Drag::Key {
            folder: f, cel, to, ..
        }) => f == folder && Some(cel.as_str()) == info.cel && *to == info.frame,
        _ => false,
    };
    if picked {
        painter.rect_filled(cell.shrink(0.5), 0.0, rgb(theme.row));
        painter.rect_stroke(
            cell.shrink(0.5),
            0.0,
            egui::Stroke::new(1.0_f32, rgb(theme.ink)),
        );
    } else if response.hovered() {
        painter.rect_filled(cell.shrink(0.5), 0.0, rgb(theme.row));
    }
    let font = egui::FontId::monospace(10.0);
    match (info.filled, info.key) {
        (true, true) => {
            let text = vmi_core::slot_hex(info.frame);
            let color = if moving { RED } else { rgb(theme.ink) };
            painter.text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                text,
                font,
                color,
            );
        }
        (true, false) => {
            // A hold. Dotted down the sheet, dashes along the film.
            let color = rgb(theme.soft);
            if vertical {
                let x = cell.center().x.round() + 0.5;
                let mut y = cell.min.y + 2.0;
                while y < cell.max.y - 1.0 {
                    painter.line_segment(
                        [egui::pos2(x, y), egui::pos2(x, y + 2.0)],
                        egui::Stroke::new(1.0_f32, color),
                    );
                    y += 4.0;
                }
            } else {
                let y = cell.center().y.round() + 0.5;
                let mut x = cell.min.x + 2.0;
                while x < cell.max.x - 1.0 {
                    painter.line_segment(
                        [egui::pos2(x, y), egui::pos2(x + 2.0, y)],
                        egui::Stroke::new(1.0_f32, color),
                    );
                    x += 4.0;
                }
            }
        }
        (false, _) => {
            painter.text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                "---",
                font,
                rgb(theme.quiet),
            );
        }
    }
    // Hover names the cel.
    if response.hovered() {
        if let Some(cel) = info.cel {
            if let Some(node) = find(&desk.art.layers, cel) {
                response.clone().on_hover_text(format!(
                    "{}  {}",
                    node.name,
                    vmi_core::slot_hex(info.frame)
                ));
            }
        }
    }
    let shift = ui.input(|input| input.modifiers.shift);
    if response.double_clicked() {
        if let Some(cel) = info.cel {
            if let Some(node) = find(&desk.art.layers, cel) {
                if !node.is_layer() && node.animation {
                    acts.push(Act::Enter(cel.to_string()));
                }
            }
        }
    }
    if response.clicked() {
        if shift {
            if let Some(pick) = desk
                .sheet_pick
                .as_ref()
                .filter(|pick| pick.folder == folder)
            {
                let (start, last) = (pick.start.min(info.frame), pick.start.max(info.frame));
                acts.push(Act::Range {
                    folder: folder.to_string(),
                    start,
                    count: last - start + 1,
                });
            } else {
                acts.push(Act::Range {
                    folder: folder.to_string(),
                    start: info.frame,
                    count: 1,
                });
            }
        } else {
            acts.push(Act::SetFrame(info.frame));
            acts.push(Act::Pick {
                folder: folder.to_string(),
                frame: info.frame,
            });
            acts.push(Act::Range {
                folder: folder.to_string(),
                start: info.frame,
                count: 1,
            });
        }
    }
    if response.drag_started() {
        // A stored sheet's rows are exposures, not keys; the film moves keys.
        if info.key && info.cel.is_some() && !info.column.stored && !shift {
            state.drag = Some(Drag::Key {
                folder: folder.to_string(),
                cel: info.cel.unwrap_or_default().to_string(),
                from: info.frame,
                to: info.frame,
            });
        } else {
            state.drag = Some(Drag::Pick {
                folder: folder.to_string(),
                anchor: info.frame,
            });
            acts.push(Act::Range {
                folder: folder.to_string(),
                start: info.frame,
                count: 1,
            });
        }
    }
    // Any drag that is over this cell right now lands here.
    let pointer_here = response.hovered()
        || cell.contains(
            ui.input(|input| input.pointer.hover_pos())
                .unwrap_or(egui::pos2(-1.0, -1.0)),
        );
    if pointer_here && ui.input(|input| input.pointer.primary_down()) {
        match state.drag.clone() {
            Some(Drag::Key {
                folder: f,
                cel,
                from,
                to,
            }) if f == folder && to != info.frame => {
                state.drag = Some(Drag::Key {
                    folder: f,
                    cel,
                    from,
                    to: info.frame,
                });
            }
            Some(Drag::Pick { folder: f, anchor }) if f == folder => {
                let (start, last) = (anchor.min(info.frame), anchor.max(info.frame));
                acts.push(Act::Range {
                    folder: folder.to_string(),
                    start,
                    count: last - start + 1,
                });
            }
            _ => {}
        }
    }
    if ui.input(|input| input.pointer.any_released()) {
        if let Some(Drag::Key {
            folder: f,
            cel,
            from,
            to,
        }) = state.drag.clone()
        {
            if from != to {
                acts.push(Act::MoveCel {
                    folder: f,
                    cel,
                    start: to,
                });
            }
            acts.push(Act::SetFrame(to));
            state.drag = None;
        } else if matches!(state.drag, Some(Drag::Pick { .. })) {
            state.drag = None;
        }
    }
    response.context_menu(|ui| {
        // A right-click outside the picked cells picks this one, like the Python sheet.
        let pick = desk
            .sheet_pick
            .clone()
            .filter(|pick| {
                pick.folder == folder
                    && info.frame >= pick.start
                    && info.frame < pick.start + pick.count
            })
            .unwrap_or(crate::desk::SheetPick {
                folder: folder.to_string(),
                start: info.frame,
                count: 1,
            });
        ui.label(
            egui::RichText::new(format!(
                "{}  frames {}–{}",
                info.column.name,
                vmi_core::slot_hex(pick.start),
                vmi_core::slot_hex(pick.start + pick.count.saturating_sub(1))
            ))
            .monospace()
            .color(rgb(theme.soft)),
        );
        if ui.button("New frame here").clicked() {
            acts.push(Act::NewCel {
                folder: folder.to_string(),
                frame: info.frame,
            });
            ui.close_menu();
        }
        if let Some(cel) = info.cel {
            if ui.button("Delete this cel").clicked() {
                acts.push(Act::DeleteCel {
                    folder: folder.to_string(),
                    cel: cel.to_string(),
                });
                ui.close_menu();
            }
        }
        ui.separator();
        // The OpenToonz cell commands, with the Python labels and fixed arguments.
        let command =
            |ui: &mut egui::Ui, acts: &mut Vec<Act>, label: &str, name: &'static str, arg: i64| {
                if ui.button(label).clicked() {
                    acts.push(Act::Command {
                        folder: folder.to_string(),
                        name,
                        start: pick.start,
                        count: pick.count,
                        arg,
                    });
                    ui.close_menu();
                }
            };
        command(ui, acts, "Reverse", "reverse", 0);
        command(ui, acts, "Swing", "swing", 0);
        ui.menu_button("Step", |ui| {
            for step in 2..=4 {
                command(ui, acts, &format!("Step {step}"), "step", step);
            }
        });
        ui.menu_button("Each", |ui| {
            for each in 2..=4 {
                command(ui, acts, &format!("Each {each}"), "each", each);
            }
        });
        command(ui, acts, "Reset Step", "reset", 0);
        command(ui, acts, "Increase Step", "increase", 0);
        command(ui, acts, "Decrease Step", "decrease", 0);
        command(ui, acts, "Autoexpose", "autoexpose", 0);
        ui.menu_button("Reframe", |ui| {
            for on in 1..=4 {
                command(ui, acts, &format!("Reframe on {on}'s"), "reframe", on);
            }
        });
        command(ui, acts, "Repeat", "repeat", 1);
        command(ui, acts, "Insert", "insert", 0);
        command(ui, acts, "Clear", "clear", 0);
        command(ui, acts, "Cut", "cut", 0);
    });
}

fn clip_name(name: &str, max: usize) -> String {
    let mut out: String = name.chars().take(max).collect();
    if name.chars().count() > max {
        out.pop();
        out.push('…');
    }
    out
}

fn rgb(color: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(color[0], color[1], color[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_turns_with_the_window() {
        assert_eq!(Mode::Auto.resolve(egui::vec2(1600.0, 900.0)), Mode::XSheet);
        assert_eq!(Mode::Auto.resolve(egui::vec2(800.0, 1200.0)), Mode::Film);
        assert_eq!(Mode::Film.resolve(egui::vec2(1600.0, 900.0)), Mode::Film);
        assert_eq!(Mode::XSheet.label(), "X-SHEET");
        assert_eq!(clip_name("Background", 6), "Backg…");
        assert_eq!(clip_name("Cut", 6), "Cut");
    }

    #[test]
    fn a_stored_sheet_reads_in_the_same_grammar_as_the_keys() {
        let mut folder = vmi_core::Node::group("f", "Shot");
        folder.animation = true;
        folder.children = vec![
            vmi_core::Node::layer("a", "1", 2, 2),
            vmi_core::Node::layer("b", "2", 2, 2),
        ];
        folder.timeline = vec!["a".into(), "b".into()];
        folder.frames = vec![0, 2];
        let column = XColumn {
            folder_id: "f".into(),
            name: "Shot".into(),
            cells: vec![
                Some("a".into()),
                Some("a".into()),
                Some("b".into()),
                Some("b".into()),
            ],
            keys: vec![true, false, true, false],
            labels: vec![],
            stored: false,
        };
        let keyed = rows_of(&column, Some(&folder), 4);
        assert_eq!(
            keyed
                .iter()
                .map(|row| (row.filled, row.key, row.number))
                .collect::<Vec<_>>(),
            [
                (true, true, Some(1)),
                (true, false, Some(1)),
                (true, true, Some(2)),
                (true, false, Some(2)),
            ]
        );

        // Swing: 1 2 2 1, then a blank and an id the keys never issued.
        let stored = XColumn {
            labels: vec![
                "000".into(),
                "001".into(),
                "001".into(),
                "000".into(),
                String::new(),
                "002".into(),
            ],
            stored: true,
            ..column
        };
        let rows = rows_of(&stored, Some(&folder), 7);
        assert_eq!(
            rows.iter()
                .map(|row| (row.filled, row.key, row.number))
                .collect::<Vec<_>>(),
            [
                (true, true, Some(1)),
                (true, true, Some(2)),
                (true, false, Some(2)),
                (true, true, Some(1)),
                (false, false, None),
                (true, true, Some(3)),
                (false, false, None),
            ]
        );
        assert_eq!(
            rows[3].cel.as_deref(),
            Some("a"),
            "a reused drawing is still its cel"
        );
        assert_eq!(
            rows[5].cel, None,
            "an id without a cel prints its number alone"
        );
    }
}
