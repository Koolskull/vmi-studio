//! Layers, the tool grid, the viewfinder, and objects.
//! The grid is the toolbox. Color and size sit on that same panel.
//! Left-click selects a tool. Right-click opens every setting for that tool.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::desk::{Button, Desk, Display, LoadEvent};
use crate::fingers::{Fingers, Tap};
use crate::gate::Gate;
use crate::gpu::{Frame, PictureFilter, PICTURE_FILTER};
use crate::icons;
use crate::newpage::{NewPage, NewPageAct};
use crate::settings::{SettingsAct, SettingsBar};
use crate::sheet::{self, Act, SheetState};
use crate::tablet;
use vmi_core::palette::Palette;
use vmi_core::{
    blend_label, palette_matching, parse_hex, CurveKind, DitherPattern, Edge, Field, GrainKind,
    Release, Section, Theme, Tip, Tool, ToolSettings, BLENDS, THEMES,
};

pub fn run() -> eframe::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1440.0, 900.0])
        .with_title("KOOL SKULL STUDIO");
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "kool-skull-studio",
        options,
        Box::new(|cc| {
            // Ctrl+= and Ctrl+- zoom the picture, never the widgets.
            cc.egui_ctx
                .options_mut(|options| options.zoom_with_keyboard = false);
            install_fonts(&cc.egui_ctx);
            tablet::install(cc);
            Ok(Box::new(StudioApp::new()))
        }),
    )
}

struct StudioApp {
    desk: Desk,
    texture: Option<egui::TextureHandle>,
    prompt: Option<Prompt>,
    path_text: String,
    rotate_from: Option<egui::Pos2>,
    /// A text field had focus last frame. Letter shortcuts stay quiet then.
    text_focused: bool,
    /// Finger taps on a touch screen: two undo, three redo.
    fingers: Fingers,
    /// A dial drag in progress: which dial, and the value where the drag began.
    dial_drag: Option<(Dial, f32)>,
    /// The palette window is up.
    palette_open: bool,
    /// The Color window is up. It sits on the layers column, not over the picture.
    color_open: bool,
    /// The folder the palette window lists. Aseprite's own, when it exists.
    palette_folder: String,
    /// A path typed into the palette window's load field.
    palette_path: String,
    /// Which view each area shows.
    areas: Areas,
    /// A sheet area on AUTO turns with the window instead of staying put.
    sheet_auto: [bool; 4],
    /// Pointer state on the sheets.
    sheet_state: SheetState,
    /// The veil while a drawing is read.
    gate: Gate,
    /// File → New picture.
    newpage: NewPage,
    /// Settings, font, theme, at the right of the menu bar.
    settings: SettingsBar,
    /// The face id last handed to egui. Empty until the first apply.
    applied_font: String,
    /// Folder the browser is listing.
    browse_path: String,
    browse_back: Vec<String>,
    browse_filter: String,
    list_filter: String,
    /// 0 recent, 1 projects, 2 tasks.
    browse_mode: usize,
    project_index: usize,
    /// A layer-opacity dial drag. One undo step when the pointer lets go.
    opacity_drag: Option<OpacityDrag>,
    /// Library folders the object list has closed. Absent means open.
    library_closed: std::collections::HashSet<String>,
    /// How much of the Objects panel the list keeps. The rest is that object's settings.
    object_split: f32,
    /// Space was used to pan or zoom. A bare tap still plays.
    space_dragged: bool,
}

struct OpacityDrag {
    id: String,
    start_y: f32,
    start: u8,
    before: Option<vmi_core::Art>,
}

/// One rotary on the panel. Every value is 0..=1 on the dial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dial {
    Size,
    Soft,
    Opacity,
    Texture,
    Tolerance,
    Pick,
    Reach,
}

impl Dial {
    fn label(self) -> &'static str {
        match self {
            Self::Size => "SIZ",
            Self::Soft => "SOF",
            Self::Opacity => "OPA",
            Self::Texture => "TEX",
            Self::Tolerance => "TOL",
            Self::Pick => "PIK",
            Self::Reach => "RCH",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Size => "Size. Drag up for a bigger brush. Double-click resets.",
            Self::Soft => "Softness. Drag up to soften the edge.",
            Self::Opacity => {
                "Opacity of the whole stroke. Dabs inside one stroke do not stack past it."
            }
            Self::Texture => "Texture. How much of the generated tip shows.",
            Self::Tolerance => "Fill tolerance.",
            Self::Pick => "Pick radius for points and handles.",
            Self::Reach => "Bone reach.",
        }
    }

    fn field(self) -> Field {
        match self {
            Self::Size => Field::Size,
            Self::Soft => Field::Softness,
            Self::Opacity => Field::Opacity,
            Self::Texture => Field::Texture,
            Self::Tolerance => Field::FillTolerance,
            Self::Pick => Field::PickRadius,
            Self::Reach => Field::BoneReach,
        }
    }

    const ALL: [Self; 7] = [
        Self::Size,
        Self::Soft,
        Self::Opacity,
        Self::Texture,
        Self::Tolerance,
        Self::Pick,
        Self::Reach,
    ];

    /// 0..=1 from the setting.
    fn get(self, settings: &ToolSettings) -> f32 {
        match self {
            Self::Size => (settings.size / 255.0).clamp(0.0, 1.0),
            Self::Soft => settings.softness.clamp(0.0, 1.0),
            Self::Opacity => settings.opacity.clamp(0.0, 1.0),
            Self::Texture => settings.texture.clamp(0.0, 1.0),
            Self::Tolerance => f32::from(settings.fill_tolerance) / 255.0,
            Self::Pick => ((settings.pick_radius - 2.0) / 62.0).clamp(0.0, 1.0),
            Self::Reach => ((settings.bone_reach - 8.0) / 792.0).clamp(0.0, 1.0),
        }
    }

    fn set(self, settings: &mut ToolSettings, value: f32) {
        let value = value.clamp(0.0, 1.0);
        match self {
            Self::Size => settings.size = (value * 255.0).round().max(1.0),
            Self::Soft => settings.softness = value,
            Self::Opacity => settings.opacity = value,
            Self::Texture => settings.texture = value,
            Self::Tolerance => settings.fill_tolerance = (value * 255.0).round() as u8,
            Self::Pick => settings.pick_radius = 2.0 + value * 62.0,
            Self::Reach => settings.bone_reach = 8.0 + value * 792.0,
        }
    }

    /// Two hex digits, Thrash Tracker style. Size shows its pixel count.
    fn digits(self, settings: &ToolSettings) -> String {
        let byte = match self {
            Self::Size => settings.size.round().clamp(0.0, 255.0) as u32,
            _ => (self.get(settings) * 255.0).round() as u32,
        };
        format!("{byte:02X}")
    }

    /// Pixels of drag for the full sweep.
    fn travel(self) -> f32 {
        match self {
            Self::Size => 200.0,
            _ => 120.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prompt {
    Open,
    Save,
    Export,
}

/// What an area shows. Four areas, like Blender: left, center, right, bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Picture,
    Layers,
    XSheet,
    Film,
    Objects,
}

impl View {
    const ALL: [Self; 5] = [
        Self::Picture,
        Self::Layers,
        Self::XSheet,
        Self::Film,
        Self::Objects,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Picture => "PICTURE",
            Self::Layers => "LAYERS",
            Self::XSheet => "X-SHEET",
            Self::Film => "FILM",
            Self::Objects => "OBJECTS",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Picture => "picture",
            Self::Layers => "layers",
            Self::XSheet => "xsheet",
            Self::Film => "film",
            Self::Objects => "objects",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|view| view.id() == id)
    }

    fn is_sheet(self) -> bool {
        matches!(self, Self::XSheet | Self::Film)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    Left,
    Center,
    Right,
    Bottom,
}

impl Area {
    const ALL: [Self; 4] = [Self::Left, Self::Center, Self::Right, Self::Bottom];

    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Center => 1,
            Self::Right => 2,
            Self::Bottom => 3,
        }
    }
}

/// The view in each area. Defaults: layers, picture, X-sheet, film.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Areas {
    left: View,
    center: View,
    right: View,
    bottom: View,
}

impl Default for Areas {
    fn default() -> Self {
        Self {
            left: View::Layers,
            center: View::Picture,
            right: View::XSheet,
            bottom: View::Film,
        }
    }
}

impl Areas {
    fn get(&self, area: Area) -> View {
        match area {
            Area::Left => self.left,
            Area::Center => self.center,
            Area::Right => self.right,
            Area::Bottom => self.bottom,
        }
    }

    fn slot(&mut self, area: Area) -> &mut View {
        match area {
            Area::Left => &mut self.left,
            Area::Center => &mut self.center,
            Area::Right => &mut self.right,
            Area::Bottom => &mut self.bottom,
        }
    }

    /// Show `view` in `area`. The area that had it takes this area's old
    /// view, so every view is in one place and the picture is always live.
    fn set(&mut self, area: Area, view: View) {
        let old = self.get(area);
        if old == view {
            return;
        }
        if let Some(other) = Area::ALL
            .iter()
            .copied()
            .find(|other| self.get(*other) == view)
        {
            *self.slot(other) = old;
        }
        *self.slot(area) = view;
    }

    fn from_ids(ids: &[String]) -> Self {
        let mut areas = Self::default();
        if ids.len() == 4 {
            let parsed: Vec<Option<View>> = ids.iter().map(|id| View::from_id(id)).collect();
            if let [Some(left), Some(center), Some(right), Some(bottom)] = parsed[..] {
                let mut seen = vec![left, center, right, bottom];
                seen.sort_by_key(|view| view.id());
                seen.dedup();
                if seen.len() == 4 && seen.contains(&View::Picture) {
                    areas = Self {
                        left,
                        center,
                        right,
                        bottom,
                    };
                }
            }
        }
        areas
    }

    fn ids(&self) -> Vec<String> {
        [self.left, self.center, self.right, self.bottom]
            .iter()
            .map(|view| view.id().to_string())
            .collect()
    }
}

impl StudioApp {
    fn new() -> Self {
        let desk = Desk::launch();
        let areas = Areas::from_ids(&desk.session.areas);
        Self {
            desk,
            texture: None,
            prompt: None,
            path_text: String::new(),
            rotate_from: None,
            text_focused: false,
            fingers: Fingers::default(),
            dial_drag: None,
            palette_open: false,
            color_open: false,
            palette_folder: palette_folder().to_string_lossy().into_owned(),
            palette_path: String::new(),
            areas,
            sheet_auto: [false; 4],
            sheet_state: SheetState::default(),
            gate: Gate::new(),
            newpage: NewPage::new(),
            settings: SettingsBar::new(),
            applied_font: String::new(),
            browse_path: String::new(),
            browse_back: Vec::new(),
            browse_filter: String::new(),
            list_filter: String::new(),
            browse_mode: 0,
            project_index: 0,
            opacity_drag: None,
            library_closed: std::collections::HashSet::new(),
            object_split: 0.5,
            space_dragged: false,
        }
    }

    fn watch_text(&mut self, response: egui::Response) -> egui::Response {
        if response.has_focus() {
            self.text_focused = true;
        }
        response
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // The gate and the New picture dialog are modal. Shortcuts wait.
        if self.gate.active() || self.newpage.is_open() {
            return;
        }
        // egui 0.29 reports any focused widget, including the canvas and the tool
        // cells, as wanting the keyboard. Undo still has to run after a stroke.
        let typing = self.text_focused;
        let mut new_picture = false;
        let mut open = false;
        let mut save = false;
        let mut quit = false;
        let mut export_png = false;
        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::N) {
                new_picture = true;
            }
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::O) {
                open = true;
            }
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::S) {
                save = true;
            }
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Q) {
                quit = true;
            }
            if !typing && input.consume_key(egui::Modifiers::SHIFT, egui::Key::E) {
                export_png = true;
            }
        });
        if new_picture {
            self.new_picture();
        }
        if open {
            self.desk.show_browser();
        }
        if save {
            self.save_blueprint();
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if export_png && !self.desk.on_browser {
            self.path_text = "drawing.png".into();
            self.prompt = Some(Prompt::Export);
        }
        // The editor's keys sleep while the launcher page is up.
        if self.desk.on_browser {
            return;
        }
        let mut undo = false;
        let mut redo = false;
        let mut fit = false;
        let mut actual = false;
        let mut cut = false;
        let mut enter = false;
        let mut escape = false;
        let mut delete = false;
        let mut backspace = false;
        let mut zoom_in = false;
        let mut zoom_out = false;
        let mut step: i64 = 0;
        let mut size_step: i32 = 0;
        let mut swap_inks = false;
        let mut clear_ink = false;
        let mut first = false;
        let mut last = false;
        let mut onion = false;
        let mut play = false;
        let mut new_cel = false;
        let mut shortcut = String::new();
        let mut space_down = false;
        let mut space_up = false;
        ctx.input_mut(|input| {
            // The sheet: comma back, period forward, W first, Shift+W last,
            // O onion skin, Space plays on release, Shift+F a new frame.
            if !typing && input.consume_key(egui::Modifiers::SHIFT, egui::Key::F) {
                new_cel = true;
            }
            if !typing {
                if input.key_pressed(egui::Key::Comma) {
                    step -= 1;
                }
                if input.key_pressed(egui::Key::Period) {
                    step += 1;
                }
                if input.key_pressed(egui::Key::W) && !input.modifiers.command {
                    if input.modifiers.shift {
                        last = true;
                    } else {
                        first = true;
                    }
                }
                if input.key_pressed(egui::Key::O)
                    && !input.modifiers.command
                    && !input.modifiers.shift
                {
                    onion = true;
                }
                if input.key_pressed(egui::Key::Space) {
                    space_down = true;
                }
                if input.key_released(egui::Key::Space) {
                    space_up = true;
                }
            }
            if input.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ) {
                redo = true;
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) {
                undo = true;
            } else if input.consume_key(egui::Modifiers::COMMAND, egui::Key::Y) {
                redo = true;
            }
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            let plain = !command && !input.modifiers.alt;
            if input.key_pressed(egui::Key::Num0) && command {
                fit = true;
            }
            if input.key_pressed(egui::Key::Num1) && command {
                actual = true;
            }
            if !typing && input.key_pressed(egui::Key::X) && shift && !command {
                cut = true;
            }
            if input.key_pressed(egui::Key::Enter) {
                enter = true;
            }
            if input.key_pressed(egui::Key::Escape) {
                escape = true;
            }
            if input.key_pressed(egui::Key::Delete) {
                delete = true;
            }
            if !typing && input.key_pressed(egui::Key::Backspace) && plain {
                backspace = true;
            }
            // Python's draw bar: [ ] step the size, Shift+[ ] by ten, X trades the two inks,
            // C clears. Repeat is allowed on the brackets. Shift+X stays the cut.
            if !typing && plain && input.key_pressed(egui::Key::OpenBracket) {
                size_step = if shift { -10 } else { -1 };
            }
            if !typing && plain && input.key_pressed(egui::Key::CloseBracket) {
                size_step = if shift { 10 } else { 1 };
            }
            if !typing && plain && !shift && input.key_pressed(egui::Key::X) {
                swap_inks = true;
            }
            if !typing && plain && !shift && input.key_pressed(egui::Key::C) {
                clear_ink = true;
            }
            // `=`, `+`, Ctrl+`=`, and Ctrl+`+` zoom in; `-` and Ctrl+`-` zoom out.
            let zoom_keys = !typing && !input.modifiers.alt;
            if zoom_keys
                && (input.key_pressed(egui::Key::Equals) || input.key_pressed(egui::Key::Plus))
            {
                zoom_in = true;
            }
            if zoom_keys && input.key_pressed(egui::Key::Minus) {
                zoom_out = true;
            }
            if !typing && plain && !shift {
                for (key, name) in [
                    (egui::Key::P, "P"),
                    (egui::Key::N, "N"),
                    (egui::Key::B, "B"),
                    (egui::Key::A, "A"),
                    (egui::Key::E, "E"),
                    (egui::Key::S, "S"),
                    (egui::Key::D, "D"),
                    (egui::Key::F, "F"),
                    (egui::Key::L, "L"),
                    (egui::Key::V, "V"),
                    (egui::Key::T, "T"),
                    (egui::Key::M, "M"),
                    (egui::Key::I, "I"),
                ] {
                    if input.key_pressed(key) {
                        shortcut = name.to_string();
                    }
                }
            }
        });
        // A tap plays. Holding space to pan or zoom does not, because the drag
        // marks the key before it is released.
        if space_down {
            self.space_dragged = false;
        }
        if space_up && !self.space_dragged {
            play = true;
        }
        if undo {
            self.desk.undo();
        }
        if redo {
            self.desk.redo();
        }
        if fit {
            self.desk.fit(self.desk.last_frame);
        }
        if actual {
            self.desk.actual();
        }
        if cut {
            self.desk.toggle_cut();
        }
        if enter {
            self.desk.enter();
        }
        if escape {
            self.desk.escape();
        }
        if delete {
            self.desk.delete_selection();
        }
        if backspace {
            self.desk.backspace();
        }
        if zoom_in {
            self.desk.zoom_at_center(1.25);
        }
        if zoom_out {
            self.desk.zoom_at_center(1.0 / 1.25);
        }
        if !shortcut.is_empty() {
            self.desk.shortcut(&shortcut);
        }
        if size_step != 0 {
            self.desk.nudge_size(size_step);
        }
        if swap_inks {
            self.desk.swap_inks();
        }
        if clear_ink {
            self.desk.toggle_clear();
        }
        if step != 0 {
            self.desk.step_frame(step);
        }
        if first {
            self.desk.first_frame();
        }
        if last {
            self.desk.last_frame();
        }
        if onion {
            self.desk.toggle_onion();
        }
        if play {
            self.desk.toggle_play();
        }
        if new_cel {
            self.desk.new_cel(None, None);
        }
    }

    fn tool_settings(&mut self, ui: &mut egui::Ui, tool: Tool) {
        let theme = self.desk.theme();
        let mut settings = self.desk.preset_of(tool).clone();
        settings.color = self.desk.ink;
        let before = settings.clone();
        ui.label(egui::RichText::new(tool.usage()).color(rgb(theme.soft)));
        ui.label(egui::RichText::new(tool.release_note()).color(rgb(theme.ink)));
        ui.separator();
        let mut rows: Vec<Field> = tool.fields().to_vec();
        if settings.edge == Edge::Dither && !rows.contains(&Field::Chunk) {
            let at = rows
                .iter()
                .position(|field| *field == Field::Edge)
                .map(|index| index + 1)
                .unwrap_or(rows.len());
            rows.insert(at, Field::Chunk);
            rows.insert(at + 1, Field::Tones);
        }
        for field in rows {
            if field == Field::ReleaseNote {
                continue;
            }
            edit_field(
                ui,
                tool,
                field,
                &mut settings,
                theme,
                &mut self.text_focused,
                &mut self.color_open,
            );
        }
        if ui.button("Reset").clicked() {
            let color = self.desk.ink;
            settings = ToolSettings::default_for(tool);
            settings.color = color;
        }
        if settings != before {
            let color = settings.color;
            self.desk.edit_preset(tool, |slot| *slot = settings);
            self.desk.set_ink(color);
        }
    }

    fn layer_settings(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(node) = vmi_core::find(&self.desk.art.layers, id) else {
            ui.label("That layer is gone.");
            return;
        };
        let mut name = node.name.clone();
        let mut opacity = i32::from(node.opacity);
        let mut blend = node.blend.clone();
        let mut clip = node.clip;
        let mut visible = node.visible;
        let mut mute = node.mute;
        let mut solo = node.solo;
        let is_group = !node.is_layer();
        let animation0 = node.animation;
        let before = (
            name.clone(),
            opacity,
            blend.clone(),
            clip,
            visible,
            mute,
            solo,
        );
        ui.label(if is_group { "Folder" } else { "Layer" });
        self.watch_text(field_edit(ui, &mut name));
        ui.add(egui::Slider::new(&mut opacity, 0..=255).text("Opacity"));
        egui::ComboBox::from_label("Blend")
            .selected_text(blend_label(&blend))
            .show_ui(ui, |ui| {
                for mode in BLENDS {
                    ui.selectable_value(&mut blend, (*mode).to_string(), blend_label(mode));
                }
            });
        ui.checkbox(&mut clip, "Clip");
        ui.checkbox(&mut visible, "Visible");
        ui.checkbox(&mut mute, "Mute");
        ui.checkbox(&mut solo, "Solo");
        if is_group {
            let mut animation = animation0;
            if ui
                .checkbox(&mut animation, "Animation")
                .on_hover_text(
                    "This folder is one sequence. Its layers are its cels, on the sheet.",
                )
                .changed()
            {
                let id = id.to_string();
                self.desk.set_animation(&id, animation);
                return;
            }
        }
        let now = (
            name.clone(),
            opacity,
            blend.clone(),
            clip,
            visible,
            mute,
            solo,
        );
        if now != before {
            let opacity = opacity.clamp(0, 255) as u8;
            self.desk.edit_layer(id, |node| {
                node.name = name;
                node.opacity = opacity;
                node.blend = blend;
                node.clip = clip;
                node.visible = visible;
                node.mute = mute;
                node.solo = solo;
            });
        }
    }

    fn stroke_settings(&mut self, ui: &mut egui::Ui, layer: &str, stroke_id: u64) {
        let snapshot = {
            let Some(node) = vmi_core::find(&self.desk.art.layers, layer) else {
                return;
            };
            let Some(stroke) = node.strokes.iter().find(|stroke| stroke.id == stroke_id) else {
                ui.label("That stroke is gone.");
                return;
            };
            (
                stroke.color,
                stroke.size,
                stroke.softness,
                stroke.fringe,
                stroke.edge,
                stroke.chunk,
                stroke.curve,
                stroke.grain,
                stroke.seed,
                stroke.closed,
                stroke.tones,
                stroke.bone.map(|bone| bone.reach),
            )
        };
        let (
            color0,
            size0,
            softness0,
            fringe0,
            edge0,
            chunk0,
            curve0,
            grain0,
            seed0,
            closed0,
            tones0,
            reach0,
        ) = snapshot;
        ui.label("The picture is a cache. These traits are the stroke.");
        let mut color = color0;
        if ui.color_edit_button_srgb(&mut color).changed() {
            self.desk.edit_stroke(|stroke| stroke.color = color);
        }
        let mut size = size0;
        if ui
            .add(
                egui::DragValue::new(&mut size)
                    .range(0.5..=256.0)
                    .speed(0.25)
                    .prefix("Size "),
            )
            .changed()
        {
            self.desk.edit_stroke(|stroke| {
                let scale = size / size0.max(0.5);
                stroke.size = size;
                for vertex in &mut stroke.vertices {
                    vertex.width = (vertex.width * scale).max(0.5);
                }
            });
        }
        let mut softness = softness0;
        if ui
            .add(egui::Slider::new(&mut softness, 0.0..=1.0).text("Softness"))
            .changed()
        {
            self.desk.edit_stroke(|stroke| stroke.softness = softness);
        }
        let mut fringe = fringe0;
        if ui
            .add(
                egui::DragValue::new(&mut fringe)
                    .range(0.0..=8.0)
                    .speed(0.05)
                    .prefix("Fringe "),
            )
            .changed()
        {
            self.desk.edit_stroke(|stroke| stroke.fringe = fringe);
        }
        ui.horizontal(|ui| {
            ui.label("Edge");
            for edge in Edge::all() {
                if ui.selectable_label(edge0 == edge, edge.label()).clicked() {
                    self.desk.edit_stroke(|stroke| stroke.edge = edge);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Chunk");
            for chunk in [1u32, 2, 4] {
                if ui
                    .selectable_label(chunk0 == chunk, chunk.to_string())
                    .clicked()
                {
                    self.desk.edit_stroke(|stroke| stroke.chunk = chunk);
                }
            }
        });
        let mut curve = curve0;
        egui::ComboBox::from_label("Curve")
            .selected_text(curve.label())
            .show_ui(ui, |ui| {
                for kind in CurveKind::all() {
                    ui.selectable_value(&mut curve, kind, kind.label());
                }
            });
        if curve != curve0 {
            self.desk.edit_stroke(|stroke| stroke.set_curve(curve));
        }
        let mut grain = grain0;
        egui::ComboBox::from_label("Grain")
            .selected_text(grain.label())
            .show_ui(ui, |ui| {
                for kind in GrainKind::all() {
                    ui.selectable_value(&mut grain, kind, kind.label());
                }
            });
        if grain != grain0 {
            self.desk.edit_stroke(|stroke| stroke.grain = grain);
        }
        let mut seed = seed0.min(u64::from(u32::MAX)) as u32;
        if ui
            .add(
                egui::DragValue::new(&mut seed)
                    .range(0..=u32::MAX)
                    .prefix("Seed "),
            )
            .changed()
        {
            self.desk
                .edit_stroke(|stroke| stroke.seed = u64::from(seed));
        }
        let mut closed = closed0;
        if ui.checkbox(&mut closed, "Closed").changed() {
            self.desk.edit_stroke(|stroke| stroke.closed = closed);
        }
        let mut tones = tones0;
        if ui.checkbox(&mut tones, "Three tones").changed() {
            self.desk.edit_stroke(|stroke| stroke.tones = tones);
        }
        if let Some(reach0) = reach0 {
            let mut reach = reach0;
            if ui
                .add(
                    egui::DragValue::new(&mut reach)
                        .range(8.0..=800.0)
                        .prefix("Bone reach "),
                )
                .changed()
            {
                self.desk.edit_stroke(|stroke| {
                    if let Some(bone) = stroke.bone.as_mut() {
                        bone.reach = reach;
                    }
                });
            }
        }
    }

    fn onion_settings(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        ui.label(egui::RichText::new("PAST").monospace().small());
        self.onion_color_row(ui, true);
        ui.label(egui::RichText::new("FUTURE").monospace().small());
        self.onion_color_row(ui, false);
        ui.label(
            egui::RichText::new("Past is behind the playhead. Future is ahead. The drawing keeps its own colour under the tint.")
                .small()
                .color(rgb(theme.soft)),
        );
        ui.separator();
        let mut back = i32::from(self.desk.art.onion_past.min(vmi_core::ONION_LIMIT));
        let mut ahead = i32::from(self.desk.art.onion_future.min(vmi_core::ONION_LIMIT));
        let limit = i32::from(vmi_core::ONION_LIMIT);
        ui.horizontal(|ui| {
            ui.label("Back");
            if ui
                .add(egui::DragValue::new(&mut back).range(0..=limit))
                .changed()
            {
                self.desk.set_onion_reach(back as u8, ahead as u8);
            }
            ui.label("Ahead");
            if ui
                .add(egui::DragValue::new(&mut ahead).range(0..=limit))
                .changed()
            {
                self.desk.set_onion_reach(back as u8, ahead as u8);
            }
        });
        if ui.button("KoolDraw colors").clicked() {
            self.desk.set_onion_color(true, vmi_core::PAST);
            self.desk.set_onion_color(false, vmi_core::FUTURE);
        }
    }

    fn onion_color_row(&mut self, ui: &mut egui::Ui, past: bool) {
        let current = if past {
            self.desk.session.onion_past_color
        } else {
            self.desk.session.onion_future_color
        };
        let mut color = current;
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size = egui::vec2(22.0, 18.0);
            ui.color_edit_button_srgb(&mut color);
            let mut hex = vmi_core::hex_of(color);
            let response = ui.add(
                egui::TextEdit::singleline(&mut hex)
                    .desired_width(72.0)
                    .font(egui::TextStyle::Monospace),
            );
            self.watch_text(response.clone());
            if response.changed() {
                if let Some(parsed) = parse_hex(&hex) {
                    color = parsed;
                }
            }
        });
        if color != current {
            self.desk.set_onion_color(past, color);
        }
    }

    fn object_settings(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(object) = self
            .desk
            .art
            .objects
            .iter()
            .find(|object| object.id == id)
            .cloned()
        else {
            return;
        };
        let theme = self.desk.theme();
        let mut name = object.name.clone();
        let mut kind = object.kind.clone();
        let mut sound = object.sound.clone();
        let stored = (
            object.name.clone(),
            object.kind.clone(),
            object.sound.clone(),
        );
        let rows: Vec<(u16, Vec<u16>, String, String, Vec<String>)> = object
            .states()
            .into_iter()
            .map(|state| {
                let layers = object
                    .layer_ids
                    .iter()
                    .filter(|layer| {
                        object
                            .assign
                            .get(*layer)
                            .map(|(found, _)| found & 0xFFF == state)
                            .unwrap_or(state == 0)
                    })
                    .filter_map(|layer| {
                        vmi_core::find(&self.desk.art.layers, layer).map(|node| node.name.clone())
                    })
                    .collect();
                (
                    state,
                    object.frames_of(state),
                    object.label_of(state, 0),
                    object.own_sample(state, 0),
                    layers,
                )
            })
            .collect();
        let folders: Vec<(String, String)> = self
            .desk
            .art
            .objects
            .iter()
            .filter(|other| {
                other.is_folder()
                    && other.id != id
                    && !vmi_core::folder_contains(&self.desk.art.objects, id, &other.id)
            })
            .map(|other| (other.id.clone(), other.name.clone()))
            .collect();
        ui.label("Object");
        self.watch_text(field_edit(ui, &mut name));
        ui.label("Kind");
        let mut kind_pick: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            for word in vmi_core::OBJECT_KINDS {
                if ui.selectable_label(kind == *word, *word).clicked() {
                    kind_pick = Some((*word).to_string());
                }
            }
            if !kind.is_empty()
                && !vmi_core::OBJECT_KINDS.contains(&kind.as_str())
                && ui.selectable_label(true, &kind).clicked()
            {
                kind_pick = Some(kind.clone());
            }
        });
        if let Some(picked) = kind_pick {
            kind = picked;
        }
        ui.label(
            egui::RichText::new(
                "A state swaps the picture: a button, a mouth, a hand, a prop. fx is the sample.",
            )
            .small()
            .color(rgb(theme.soft)),
        );
        ui.label("Folder");
        let mut folder_pick: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(object.folder.is_empty(), "(library)")
                .clicked()
            {
                folder_pick = Some(String::new());
            }
            for (folder_id, folder_name) in &folders {
                if ui
                    .selectable_label(object.folder == *folder_id, folder_name)
                    .clicked()
                {
                    folder_pick = Some(folder_id.clone());
                }
            }
        });
        ui.label("Sample");
        self.watch_text(field_edit(ui, &mut sound));
        ui.label(
            egui::RichText::new(
                "A file path, or sfx:tik. A state sample below wins for that state.",
            )
            .small()
            .color(rgb(theme.soft)),
        );
        if (name.clone(), kind.clone(), sound.clone()) != stored {
            self.desk.edit_object(|object| {
                object.name = name;
                object.kind = kind;
                object.sound = sound;
            });
        }
        if let Some(folder) = folder_pick {
            if folder != object.folder {
                self.desk.place_object_in(id, &folder);
            }
        }
        if object.is_folder() {
            ui.label(
                egui::RichText::new(
                    "A folder holds symbols. Click it in the list and new objects join it.",
                )
                .small()
                .color(rgb(theme.soft)),
            );
            return;
        }
        ui.separator();
        ui.label(egui::RichText::new("STATES").monospace().small());
        ui.label(
            egui::RichText::new(
                "000 idle, 001 hover, 002 click, 003 after. A mouth or a hand uses the same slots.",
            )
            .small()
            .color(rgb(theme.soft)),
        );
        let mut renamed: Option<(u16, String)> = None;
        let mut sample_edit: Option<(u16, String)> = None;
        let mut add_frame: Option<u16> = None;
        let mut add_state = false;
        let mut use_layer: Option<u16> = None;
        for (state, frames, mut state_name, mut state_sample, layers) in rows {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(vmi_core::slot_hex(u32::from(state)))
                        .monospace()
                        .small(),
                );
                let response = field_edit(ui, &mut state_name);
                self.watch_text(response.clone());
                if response.changed() {
                    renamed = Some((state, state_name));
                }
                if ui
                    .small_button("Frame")
                    .on_hover_text("The next frame of this state, in hex.")
                    .clicked()
                {
                    add_frame = Some(state);
                }
            });
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Sample")
                        .small()
                        .color(rgb(theme.quiet)),
                );
                let response = field_edit(ui, &mut state_sample);
                self.watch_text(response.clone());
                if response.changed() {
                    sample_edit = Some((state, state_sample));
                }
            });
            ui.horizontal_wrapped(|ui| {
                for frame in frames {
                    ui.label(
                        egui::RichText::new(vmi_core::slot_key(state, frame))
                            .monospace()
                            .small()
                            .color(rgb(theme.quiet)),
                    );
                }
                if layers.is_empty() {
                    ui.label(
                        egui::RichText::new("No layer yet.")
                            .small()
                            .color(rgb(theme.quiet)),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(layers.join(", "))
                            .small()
                            .color(rgb(theme.quiet)),
                    );
                }
                if ui
                    .small_button("Use layer")
                    .on_hover_text("The selected layer becomes this state's picture.")
                    .clicked()
                {
                    use_layer = Some(state);
                }
            });
        }
        if ui
            .small_button("State")
            .on_hover_text("The next state, 000 through FFF.")
            .clicked()
        {
            add_state = true;
        }
        if let Some((state, state_name)) = renamed {
            self.desk
                .edit_object(|object| object.rename_slot(state, 0, &state_name));
        }
        if let Some((state, path)) = sample_edit {
            self.desk
                .edit_object(|object| object.set_sample(state, 0, &path));
        }
        if let Some(state) = add_frame {
            self.desk.edit_object(|object| {
                object.add_frame(state);
            });
            self.desk.status = "Frame added. The slot is the hex number.".into();
        }
        if add_state {
            self.desk.edit_object(|object| {
                object.add_state();
            });
            self.desk.status = "State added.".into();
        }
        if let Some(state) = use_layer {
            self.desk.give_layer_to_state(state);
        }
    }

    /// The loading thread's notes and the tab changes, once per frame.
    fn hear_loader(&mut self, ctx: &egui::Context) {
        self.desk.poll_load();
        if self.desk.is_loading() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
        for event in self.desk.take_load_events() {
            match event {
                LoadEvent::Started(name) => self.gate.start(&name, false),
                LoadEvent::Catchup(name) => self.gate.start(&name, true),
                LoadEvent::Note(note) => self.gate.hear(&note),
                LoadEvent::Shown => self.gate.complete(),
                LoadEvent::Failed(message) => {
                    self.gate.complete();
                    self.desk.status = message;
                }
            }
        }
    }

    /// File → New picture, and the launcher button. The dialog chooses the plate.
    fn new_picture(&mut self) {
        self.newpage.open();
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        let theme = self.desk.theme();
        if self.gate.show(ctx, theme, now) {
            ctx.request_repaint_after(self.gate.tick_interval());
        }
        let mut typing = false;
        let act = self.newpage.show(ctx, theme, &mut typing);
        if typing {
            self.text_focused = true;
        }
        if let NewPageAct::Create(choice) = act {
            let texture = choice.texture.clone();
            if let Err(err) = self.desk.open_blank(
                i64::from(choice.width),
                i64::from(choice.height),
                &choice.background,
                texture.as_deref(),
                choice.grid,
            ) {
                self.desk.status = err;
            }
        }
    }

    fn apply_settings(&mut self, ctx: &egui::Context, act: SettingsAct) {
        match act {
            SettingsAct::None => {}
            SettingsAct::Font(id) => {
                let name = vmi_core::fonts::face(&id)
                    .map(|face| face.label)
                    .unwrap_or_else(|| id.clone());
                self.desk.status = format!("Loading {name}");
                self.desk.set_font(&id);
                self.applied_font.clear();
                self.sync_font(ctx, true);
            }
            SettingsAct::Theme(id) => self.desk.set_theme(&id),
        }
    }

    /// Apply `session.font` once it changes. `announce` writes the Python status lines.
    fn sync_font(&mut self, ctx: &egui::Context, announce: bool) {
        let id = if self.desk.session.font.is_empty() {
            vmi_core::fonts::DEFAULT_FACE.to_string()
        } else {
            self.desk.session.font.clone()
        };
        if self.applied_font == id {
            return;
        }
        let name = vmi_core::fonts::face(&id)
            .map(|face| face.label)
            .unwrap_or_else(|| id.clone());
        match face_bytes(&id) {
            Ok(bytes) => {
                install_face(ctx, bytes);
                self.applied_font = id;
                if announce {
                    self.desk.status = format!("UI font: {name}");
                }
            }
            Err(_) => {
                if announce {
                    self.desk.status = format!("Could not load {name}.");
                }
                if id != vmi_core::fonts::DEFAULT_FACE {
                    self.desk.set_font(vmi_core::fonts::DEFAULT_FACE);
                }
                self.applied_font = vmi_core::fonts::DEFAULT_FACE.to_string();
                install_face(ctx, vmi_core::fonts::house_bytes());
            }
        }
    }

    /// Ctrl+S. An open blueprint is overwritten. A drawing is saved beside it.
    fn save_blueprint(&mut self) {
        if self.desk.on_browser {
            self.desk.status = "Open a drawing first.".into();
            return;
        }
        match self.desk.blueprint_path() {
            Some(path) => {
                if let Err(err) = self.desk.save_blueprint(&path) {
                    self.desk.status = err;
                }
            }
            None => self.save_blueprint_as(),
        }
    }

    fn save_blueprint_as(&mut self) {
        if self.desk.on_browser {
            self.desk.status = "Open a drawing first.".into();
            return;
        }
        self.path_text = self.desk.suggested_blueprint().display().to_string();
        self.prompt = Some(Prompt::Save);
    }

    /// The tab row: `browser` first, then one short tab per drawing with its
    /// close mark, and the shown tab's export folder in white at the right.
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        let names = self.desk.tab_names();
        let shown = if self.desk.on_browser {
            None
        } else {
            self.desk.shown
        };
        let mut pick: Option<Option<usize>> = None;
        let mut close: Option<usize> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if project_tab(ui, theme, "browser", shown.is_none(), false)
                .on_hover_text("Projects, tasks, and drawings")
                .clicked()
            {
                pick = Some(None);
            }
            for (index, name) in names.iter().enumerate() {
                let export_dir = self.desk.tabs[index].export_dir.display().to_string();
                let (tab, mark) = project_tab_with_close(ui, theme, name, shown == Some(index));
                if tab.on_hover_text(export_dir).clicked() {
                    pick = Some(Some(index));
                }
                if mark.on_hover_text("Close this tab").clicked() {
                    close = Some(index);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.desk.on_browser && self.desk.shown.is_some() {
                    let path = self.desk.export_dir().display().to_string();
                    ui.add(
                        egui::Label::new(egui::RichText::new(&path).color(egui::Color32::WHITE))
                            .truncate(),
                    )
                    .on_hover_text(path);
                }
            });
        });
        if let Some(index) = close {
            self.desk.close_tab(index);
        } else if let Some(choice) = pick {
            self.desk.select_tab(choice);
        }
    }

    /// Projects, recent files, and a folder browser. Nothing loads until a drawing is opened.
    fn browser_page(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        let book = vmi_core::catalog::load_book(&vmi_core::catalog::book_path());
        if self.browse_path.is_empty() {
            self.browse_path = vmi_core::catalog::default_browse(&book)
                .display()
                .to_string();
        }
        if self.project_index >= book.projects.len() {
            self.project_index = 0;
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Pick a drawing. Nothing loads until you open it.")
                .color(rgb(theme.quiet)),
        );
        ui.horizontal(|ui| {
            if ui.button("New picture").clicked() {
                self.new_picture();
            }
            if ui.button("Open…").clicked() {
                self.path_text = self.desk.session.path.clone();
                self.prompt = Some(Prompt::Open);
            }
        });
        let mut open_path: Option<String> = None;
        let mut enter: Option<String> = None;
        let mut up = false;
        let mut back = false;
        let places = vmi_core::catalog::places();
        ui.columns(3, |columns| {
            columns[0].label(egui::RichText::new("LISTS").color(rgb(theme.quiet)));
            columns[0].horizontal(|ui| {
                if ui
                    .selectable_label(self.browse_mode == 0, "Recent")
                    .clicked()
                {
                    self.browse_mode = 0;
                }
                if ui
                    .selectable_label(self.browse_mode == 1, "Projects")
                    .clicked()
                {
                    self.browse_mode = 1;
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("browser-projects")
                .auto_shrink([false, false])
                .max_height(220.0)
                .show(&mut columns[0], |ui| {
                    for (index, project) in book.projects.iter().enumerate() {
                        if ui
                            .selectable_label(
                                self.browse_mode == 1 && index == self.project_index,
                                &project.title,
                            )
                            .clicked()
                        {
                            self.browse_mode = 1;
                            self.project_index = index;
                        }
                    }
                    if book.projects.is_empty() {
                        ui.label(egui::RichText::new("No projects yet.").color(rgb(theme.quiet)));
                    }
                });

            let title = if self.browse_mode == 1 {
                book.projects
                    .get(self.project_index)
                    .map(|project| project.title.as_str())
                    .unwrap_or("PROJECT")
            } else {
                "RECENTLY USED"
            };
            columns[1].label(egui::RichText::new(title).color(rgb(theme.quiet)));
            let filter = field_edit(&mut columns[1], &mut self.list_filter);
            self.watch_text(filter);
            let files: Vec<(String, String)> = if self.browse_mode == 1 {
                book.projects
                    .get(self.project_index)
                    .map(|project| project.files.clone())
                    .unwrap_or_default()
            } else {
                book.recent.clone()
            }
            .into_iter()
            .map(|file| (file.label, file.path))
            .filter(|(label, path)| list_match(&self.list_filter, label, path))
            .collect();
            egui::ScrollArea::vertical()
                .id_salt("browser-files")
                .auto_shrink([false, false])
                .show(&mut columns[1], |ui| {
                    for (label, path) in &files {
                        let name = if label.is_empty() {
                            path.clone()
                        } else {
                            label.clone()
                        };
                        if ui.button(name).on_hover_text(path).clicked() {
                            open_path = Some(path.clone());
                        }
                    }
                    if files.is_empty() {
                        ui.label(
                            egui::RichText::new("No drawings in this list.")
                                .color(rgb(theme.quiet)),
                        );
                    }
                });

            columns[2].label(egui::RichText::new("BROWSER").color(rgb(theme.quiet)));
            columns[2].horizontal_wrapped(|ui| {
                for (label, path) in &places {
                    if ui.button(*label).clicked() {
                        enter = Some(path.display().to_string());
                    }
                }
            });
            columns[2].horizontal(|ui| {
                if ui.button("Back").clicked() {
                    back = true;
                }
                if ui.button("Up").clicked() {
                    up = true;
                }
                let path_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.browse_path)
                        .desired_width(ui.available_width())
                        .text_color(rgb(theme.ink)),
                );
                self.watch_text(path_edit);
            });
            let folder_filter = field_edit(&mut columns[2], &mut self.browse_filter);
            self.watch_text(folder_filter);
            let rows = folder_rows(std::path::Path::new(&self.browse_path), &self.browse_filter);
            egui::ScrollArea::vertical()
                .id_salt("browser-folders")
                .auto_shrink([false, false])
                .show(&mut columns[2], |ui| {
                    for row in &rows {
                        let mark = if row.dir { "+ " } else { "" };
                        if ui
                            .button(format!("{mark}{}", row.name))
                            .on_hover_text(row.path.display().to_string())
                            .clicked()
                        {
                            if row.dir {
                                enter = Some(row.path.display().to_string());
                            } else {
                                open_path = Some(row.path.display().to_string());
                            }
                        }
                    }
                    if rows.is_empty() {
                        ui.label(
                            egui::RichText::new("No folders or drawings here.")
                                .color(rgb(theme.quiet)),
                        );
                    }
                });
        });
        if back {
            if let Some(previous) = self.browse_back.pop() {
                self.browse_path = previous;
            }
        } else if up {
            if let Some(parent) = std::path::Path::new(&self.browse_path).parent() {
                if parent != std::path::Path::new(&self.browse_path) {
                    self.browse_back.push(self.browse_path.clone());
                    self.browse_path = parent.display().to_string();
                }
            }
        } else if let Some(path) = enter {
            if path != self.browse_path {
                self.browse_back.push(self.browse_path.clone());
                self.browse_path = path;
            }
        }
        if let Some(path) = open_path {
            let file = std::path::PathBuf::from(&path);
            if file.is_file() {
                self.desk.begin_open(&file, false);
            } else {
                self.desk.status = "That file is missing.".into();
            }
        }
    }

    /// The lower left, under the layers. Every setting of the current tool.
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        let tool = self.desk.tool();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            ui.label(
                egui::RichText::new("TOOL OPTIONS")
                    .monospace()
                    .small()
                    .color(rgb(theme.ink)),
            );
            ui.label(
                egui::RichText::new(tool.name())
                    .monospace()
                    .small()
                    .color(rgb(theme.soft)),
            );
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("tool-options")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.tool_settings(ui, tool);
            });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.desk.layer_window.clone() {
            let mut open = true;
            egui::Window::new("Layer settings")
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| self.layer_settings(ui, &id));
            if !open {
                self.desk.close_layer_window();
            }
        }
        if let Some((layer, stroke)) = self.desk.stroke_window.clone() {
            let mut open = true;
            egui::Window::new("Stroke settings")
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| self.stroke_settings(ui, &layer, stroke));
            if !open {
                self.desk.close_stroke_window();
            }
        }
        if self.desk.onion_window {
            let mut open = true;
            egui::Window::new("Onion skin")
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| self.onion_settings(ui));
            if !open {
                self.desk.close_onion_window();
            }
        }
        self.color_window(ctx);
        self.palette_window(ctx);
        if let Some(prompt) = self.prompt {
            let title = match prompt {
                Prompt::Open => "Open",
                Prompt::Save => "Save blueprint",
                Prompt::Export => "Export PNG",
            };
            let mut open = true;
            egui::Window::new(title)
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.label("Path");
                    let path_edit = field_edit(ui, &mut self.path_text);
                    self.watch_text(path_edit);
                    if matches!(prompt, Prompt::Export) {
                        let mut percent =
                            i32::try_from(self.desk.session.export_percent).unwrap_or(100);
                        if ui
                            .add(egui::Slider::new(&mut percent, 1..=100).text("Percent"))
                            .changed()
                        {
                            self.desk.set_export_percent(percent as u32);
                        }
                        ui.label("Nearest sample. Dither stays square.");
                    }
                    if ui.button("OK").clicked() {
                        let path = std::path::PathBuf::from(self.path_text.trim());
                        let result = match prompt {
                            Prompt::Open => {
                                if path.is_file() {
                                    self.desk.begin_open(&path, false);
                                    Ok(())
                                } else {
                                    Err("That file is missing.".to_string())
                                }
                            }
                            Prompt::Save => {
                                let is_vmib = path
                                    .extension()
                                    .and_then(|ext| ext.to_str())
                                    .map(|ext| ext.eq_ignore_ascii_case("vmib"))
                                    .unwrap_or(false);
                                let path = if is_vmib {
                                    path
                                } else {
                                    PathBuf::from(format!("{}.vmib", path.display()))
                                };
                                self.desk.save_blueprint(&path)
                            }
                            Prompt::Export => self.desk.export_png_file(&path),
                        };
                        if let Err(err) = result {
                            self.desk.status = err;
                        } else {
                            self.prompt = None;
                        }
                    }
                });
            if !open {
                self.prompt = None;
            }
        }
    }

    /// One ink. The window opens on the layers column. A drag here is not a stroke.
    fn color_window(&mut self, ctx: &egui::Context) {
        if !self.color_open {
            return;
        }
        let mut open = true;
        let mut picked = None;
        let mut typed = None;
        let mut hex_focus = false;
        let current = self.desk.ink;
        egui::Window::new("Color")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_pos(egui::pos2(12.0, 40.0))
            .default_width(220.0)
            .max_width(240.0)
            .show(ctx, |ui| {
                ui.spacing_mut().slider_width = 168.0;
                let mut color = egui::Color32::from_rgb(current[0], current[1], current[2]);
                if egui::color_picker::color_picker_color32(
                    ui,
                    &mut color,
                    egui::color_picker::Alpha::Opaque,
                ) {
                    picked = Some([color.r(), color.g(), color.b()]);
                }
                let shown = picked.unwrap_or(current);
                let mut hex = vmi_core::hex_of(shown);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut hex)
                        .desired_width(72.0)
                        .font(egui::TextStyle::Monospace),
                );
                hex_focus = response.has_focus();
                if response.changed() {
                    if let Some(parsed) = parse_hex(&hex) {
                        typed = Some(parsed);
                    }
                }
            });
        if let Some(color) = typed.or(picked) {
            self.desk.set_ink(color);
        }
        if hex_focus {
            self.text_focused = true;
        }
        if !open {
            self.color_open = false;
        }
    }

    /// The palette window. Tabs are the preset sets and then your own. A
    /// swatch click picks that color. Files come and go in Aseprite's formats.
    fn palette_window(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }
        let theme = self.desk.theme();
        let mut open = true;
        egui::Window::new("Palette")
            .open(&mut open)
            .resizable(true)
            .default_width(272.0)
            .show(ctx, |ui| self.palette_body(ui, theme));
        if !open {
            self.palette_open = false;
        }
    }

    fn palette_body(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let presets = vmi_core::palette::presets();
        let user_count = self.desk.session.palettes.len();
        let total = presets.len() + user_count;
        let mut tab = self.desk.session.palette_tab.min(total.saturating_sub(1));
        let tab_before = tab;
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
        // Tabs. The open one is white; the rest wait in the quiet color.
        ui.horizontal_wrapped(|ui| {
            for (index, name) in presets
                .iter()
                .map(|palette| palette.name.clone())
                .chain(
                    self.desk
                        .session
                        .palettes
                        .iter()
                        .map(|palette| palette.name.clone()),
                )
                .enumerate()
            {
                if tab_label(ui, theme, &name, index == tab).clicked() {
                    tab = index;
                }
            }
            if tab_label(ui, theme, "+", false)
                .on_hover_text("A new palette of your own.")
                .clicked()
            {
                let name = unique_palette_name(&self.desk.session.palettes, "Palette");
                self.desk
                    .session
                    .palettes
                    .push(Palette::new(name, Vec::new()));
                tab = presets.len() + self.desk.session.palettes.len() - 1;
            }
        });
        if tab != tab_before {
            self.desk.session.palette_tab = tab;
            self.desk.touch_session();
        }
        let user_index = tab.checked_sub(presets.len());
        let palette = match user_index {
            Some(index) => self.desk.session.palettes.get(index).cloned(),
            None => presets.get(tab).cloned(),
        };
        let Some(palette) = palette else {
            return;
        };
        let current = self.desk.ink;
        ui.add_space(2.0);
        // The swatches. Fourteen pixels, one apart, as many across as fit.
        let mut picked: Option<[u8; 3]> = None;
        let mut remove: Option<usize> = None;
        let cell = 14.0;
        let step = cell + 1.0;
        let across = ((ui.available_width() + 1.0) / step).floor().max(1.0) as usize;
        let rows = palette.colors.len().div_ceil(across).max(1);
        let (grid, _) = ui.allocate_exact_size(
            egui::vec2(across as f32 * step - 1.0, rows as f32 * step - 1.0),
            egui::Sense::hover(),
        );
        for (index, swatch) in palette.colors.iter().enumerate() {
            let col = (index % across) as f32;
            let row = (index / across) as f32;
            let rect = egui::Rect::from_min_size(
                egui::pos2(grid.min.x + col * step, grid.min.y + row * step),
                egui::vec2(cell, cell),
            );
            let response = ui.interact(
                rect,
                ui.id().with(("swatch", tab, index)),
                egui::Sense::click(),
            );
            ui.painter().rect_filled(rect, 0.0, rgb(*swatch));
            if *swatch == current {
                ui.painter().rect_stroke(
                    rect.shrink(0.5),
                    0.0,
                    egui::Stroke::new(1.0_f32, rgb(theme.ink)),
                );
                ui.painter().rect_stroke(
                    rect.shrink(1.5),
                    0.0,
                    egui::Stroke::new(1.0_f32, rgb(theme.field)),
                );
            }
            if response.clicked() {
                picked = Some(*swatch);
            }
            if response.secondary_clicked() && user_index.is_some() {
                remove = Some(index);
            }
            let hint = if user_index.is_some() {
                format!(
                    "{}  {}\nRight-click removes it.",
                    vmi_core::hex_of(*swatch),
                    index
                )
            } else {
                format!("{}  {}", vmi_core::hex_of(*swatch), index)
            };
            response.on_hover_text(hint);
        }
        if palette.colors.is_empty() {
            ui.label(
                egui::RichText::new("Empty. + color adds the current color.")
                    .color(rgb(theme.soft))
                    .small(),
            );
        }
        if let Some(color) = picked {
            self.pick_color(color);
        }
        if let (Some(index), Some(at)) = (user_index, remove) {
            if let Some(user) = self.desk.session.palettes.get_mut(index) {
                if at < user.colors.len() {
                    user.colors.remove(at);
                    self.desk.touch_session();
                }
            }
        }
        ui.add_space(4.0);
        // The current color, as hex, and the recent ones.
        ui.horizontal(|ui| {
            let mut color = current;
            let chip = ink_chip(ui, current, egui::vec2(22.0, 18.0), theme.frame)
                .on_hover_text("Opens the Color window.");
            if chip.clicked() {
                self.color_open = true;
            }
            let mut hex = vmi_core::hex_of(color);
            let response = ui.add(
                egui::TextEdit::singleline(&mut hex)
                    .desired_width(64.0)
                    .font(egui::TextStyle::Monospace),
            );
            if response.has_focus() {
                self.text_focused = true;
            }
            if response.changed() {
                if let Some(parsed) = parse_hex(&hex) {
                    color = parsed;
                }
            }
            if color != current {
                self.pick_color(color);
            }
            ui.label(egui::RichText::new("recent").color(rgb(theme.soft)).small());
            let recent: Vec<[u8; 3]> = self
                .desk
                .session
                .recent_colors
                .iter()
                .copied()
                .take(12)
                .collect();
            let mut pick = None;
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
            for (index, swatch) in recent.iter().enumerate() {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(cell, cell), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, rgb(*swatch));
                if response.on_hover_text(vmi_core::hex_of(*swatch)).clicked() {
                    pick = Some((index, *swatch));
                }
            }
            if let Some((_, color)) = pick {
                self.pick_color(color);
            }
        });
        ui.add_space(4.0);
        // What this palette can do.
        ui.horizontal_wrapped(|ui| match user_index {
            Some(index) => {
                if ui
                    .small_button("+ color")
                    .on_hover_text("Add the current color to this palette.")
                    .clicked()
                {
                    if let Some(user) = self.desk.session.palettes.get_mut(index) {
                        if !user.colors.contains(&current) {
                            user.colors.push(current);
                            self.desk.touch_session();
                        }
                    }
                }
                if ui
                    .small_button("Save .gpl")
                    .on_hover_text("Write it into the folder below. Aseprite reads it.")
                    .clicked()
                {
                    let path = Path::new(&self.palette_folder)
                        .join(format!("{}.gpl", file_stem_for(&palette.name)));
                    match vmi_core::palette::save_file(&palette, &path) {
                        Ok(()) => self.desk.status = format!("Saved {}", path.display()),
                        Err(err) => self.desk.status = err,
                    }
                }
                if ui
                    .small_button("Delete")
                    .on_hover_text("Remove this palette. Files stay.")
                    .clicked()
                    && index < self.desk.session.palettes.len()
                {
                    self.desk.session.palettes.remove(index);
                    self.desk.session.palette_tab = tab.saturating_sub(1);
                    self.desk.touch_session();
                }
                let mut name = palette.name.clone();
                let response = ui.add(egui::TextEdit::singleline(&mut name).desired_width(96.0));
                if response.has_focus() {
                    self.text_focused = true;
                }
                if response.changed() {
                    if let Some(user) = self.desk.session.palettes.get_mut(index) {
                        user.name = name;
                        self.desk.touch_session();
                    }
                }
            }
            None => {
                if ui
                    .small_button("Copy to mine")
                    .on_hover_text("A copy you can edit and save.")
                    .clicked()
                {
                    let name = unique_palette_name(&self.desk.session.palettes, &palette.name);
                    self.desk
                        .session
                        .palettes
                        .push(Palette::new(name, palette.colors.clone()));
                    self.desk.session.palette_tab =
                        presets.len() + self.desk.session.palettes.len() - 1;
                    self.desk.touch_session();
                }
                ui.label(
                    egui::RichText::new(format!("{} colors", palette.colors.len()))
                        .color(rgb(theme.soft))
                        .small(),
                );
            }
        });
        ui.separator();
        // Files. Aseprite's palette folder when there is one.
        ui.label(egui::RichText::new("Folder").color(rgb(theme.soft)).small());
        let folder_edit = ui
            .add(egui::TextEdit::singleline(&mut self.palette_folder).desired_width(f32::INFINITY));
        if folder_edit.has_focus() {
            self.text_focused = true;
        }
        let files = palette_files(Path::new(&self.palette_folder));
        if files.is_empty() {
            ui.label(
                egui::RichText::new("No palette files here yet. Save .gpl writes one.")
                    .color(rgb(theme.soft))
                    .small(),
            );
        }
        let mut load: Option<PathBuf> = None;
        egui::ScrollArea::vertical()
            .id_salt("palette-files")
            .max_height(96.0)
            .show(ui, |ui| {
                for file in &files {
                    let name = file
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let known = self
                        .desk
                        .session
                        .palettes
                        .iter()
                        .any(|palette| palette.name == stem_of(file));
                    if tab_label(ui, theme, &name, known)
                        .on_hover_text("Open as a palette of your own.")
                        .clicked()
                    {
                        load = Some(file.clone());
                    }
                }
            });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Path").color(rgb(theme.soft)).small());
            let response =
                ui.add(egui::TextEdit::singleline(&mut self.palette_path).desired_width(160.0));
            if response.has_focus() {
                self.text_focused = true;
            }
            if ui.small_button("Load").clicked() && !self.palette_path.trim().is_empty() {
                load = Some(PathBuf::from(self.palette_path.trim()));
            }
        });
        ui.label(
            egui::RichText::new(format!(
                "Reads {}.",
                vmi_core::palette::EXTENSIONS.join(" ")
            ))
            .color(rgb(theme.soft))
            .small(),
        );
        if let Some(path) = load {
            self.load_palette(&path, presets.len());
        }
    }

    /// Open a palette file as one of your own. A same-named one is replaced.
    fn load_palette(&mut self, path: &Path, preset_count: usize) {
        match vmi_core::palette::load_file(path) {
            Ok(mut palette) => {
                if palette.name.trim().is_empty() {
                    palette.name = stem_of(path);
                }
                let count = palette.colors.len();
                let slot = self
                    .desk
                    .session
                    .palettes
                    .iter()
                    .position(|item| item.name == palette.name);
                let index = match slot {
                    Some(index) => {
                        self.desk.session.palettes[index] = palette;
                        index
                    }
                    None => {
                        self.desk.session.palettes.push(palette);
                        self.desk.session.palettes.len() - 1
                    }
                };
                self.desk.session.palette_tab = preset_count + index;
                self.desk.touch_session();
                self.desk.status = format!("{count} colors from {}", path.display());
            }
            Err(err) => self.desk.status = err,
        }
    }

    /// The tool panel. Two tool cells across is the widest row on it; every
    /// other row wraps to that same width, so nothing to the right is wasted.
    fn palette(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        ui.set_max_width(PANEL_INNER);
        ui.spacing_mut().item_spacing = egui::vec2(1.0, 3.0);
        self.head_row(ui, theme);
        self.color_row(ui, theme);
        self.dials(ui, theme);
        self.toggles(ui, theme);
        ui.add_space(2.0);
        egui::ScrollArea::vertical()
            .id_salt("tool-grid")
            .show(ui, |ui| {
                self.tool_grid(ui, theme);
            });
    }

    /// Undo and redo. The full settings live under the layers.
    fn head_row(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let side = (PANEL_INNER - 1.0) / 2.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
            if small_cell(ui, theme, side, false, |painter, rect, ink| {
                paint_bitmap(painter, rect, &icons::UNDO, ink)
            })
            .on_hover_text("Undo    Ctrl+Z    two-finger tap")
            .clicked()
            {
                self.desk.undo();
            }
            if small_cell(ui, theme, side, false, |painter, rect, ink| {
                paint_bitmap(painter, rect, &icons::REDO, ink)
            })
            .on_hover_text("Redo    Ctrl+Shift+Z    three-finger tap")
            .clicked()
            {
                self.desk.redo();
            }
        });
    }

    /// The color cell and the palette cell. Click the color for the Color
    /// window, right-click it for the palette window. The palette cell shows
    /// the first colors of the current palette and opens that window.
    fn color_row(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        // One ink. The swatch is not the tool's saved preset, so Pen and Pencil stay the same color.
        let color = self.desk.ink;
        let height = 18.0;
        let half = (PANEL_INNER - CELL_GAP) / 2.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(CELL_GAP, 0.0);
            let response = ink_chip(ui, color, egui::vec2(half, height), theme.frame).on_hover_text(
                format!(
                    "Color {}\nClick: Color window. Right-click: palette window.\nOne ink for every tool. X trades it.",
                    vmi_core::hex_of(color)
                ),
            );
            if response.secondary_clicked() {
                self.palette_open = !self.palette_open;
            } else if response.clicked() {
                self.color_open = true;
            }
            let swatches: Vec<[u8; 3]> = self.current_palette().colors.iter().copied().take(18).collect();
            let (rect, response) = ui.allocate_exact_size(egui::vec2(half, height), egui::Sense::click());
            let lit = self.palette_open;
            let fill = if lit {
                rgb(theme.ink)
            } else if response.hovered() {
                rgb(theme.frame)
            } else {
                rgb(theme.row)
            };
            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, rgb(theme.frame)));
            // Six by three, centered: the palette in miniature. The cell got smaller with the grid.
            let px = ((half - 4.0) / 6.0).floor().max(2.0);
            let origin = egui::pos2((rect.center().x - 3.0 * px).round(), (rect.center().y - 1.5 * px).round());
            for (index, swatch) in swatches.iter().enumerate() {
                let col = (index % 6) as f32;
                let row = (index / 6) as f32;
                let min = egui::pos2(origin.x + col * px, origin.y + row * px);
                painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(px, px)), 0.0, rgb(*swatch));
            }
            if response.on_hover_text("Palettes: presets, your own, and Aseprite files.").clicked() {
                self.palette_open = !self.palette_open;
            }
        });
    }

    /// The palette the window has open.
    fn current_palette(&self) -> Palette {
        let presets = vmi_core::palette::presets();
        let tab = self.desk.session.palette_tab;
        if tab < presets.len() {
            presets[tab].clone()
        } else if let Some(palette) = self.desk.session.palettes.get(tab - presets.len()) {
            palette.clone()
        } else {
            presets[0].clone()
        }
    }

    /// One ink for every tool. Remember it.
    fn pick_color(&mut self, color: [u8; 3]) {
        self.desk.set_ink(color);
    }

    /// The rotaries. 7 o'clock is 0, the long way around to 5 o'clock is 1.
    fn dials(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let tool = self.desk.tool();
        let fields = tool.fields();
        let shown: Vec<Dial> = Dial::ALL
            .iter()
            .copied()
            .filter(|dial| fields.contains(&dial.field()))
            .collect();
        if shown.is_empty() {
            return;
        }
        let mut settings = self.desk.preset_of(tool).clone();
        let before = settings.clone();
        let defaults = ToolSettings::default_for(tool);
        // Three dials fill the panel. The grid shrunk, so the dials follow it.
        for row in shown.chunks(3) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(DIAL_GAP, 0.0);
                for dial in row.iter().copied() {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(DIAL_SIDE, DIAL_SIDE),
                            egui::Sense::click_and_drag(),
                        );
                        let value = dial.get(&settings);
                        paint_dial(
                            ui.painter(),
                            rect,
                            value,
                            &dial.digits(&settings),
                            theme,
                            value > 0.0,
                        );
                        if response.drag_started() {
                            self.dial_drag = Some((dial, value));
                        }
                        if response.dragged() {
                            if let Some((which, start)) = self.dial_drag {
                                if which == dial {
                                    // Up raises. The whole sweep is `travel` pixels.
                                    let delta = response
                                        .interact_pointer_pos()
                                        .map(|pos| pos.y)
                                        .unwrap_or(0.0);
                                    let origin = ui
                                        .input(|input| input.pointer.press_origin())
                                        .map(|pos| pos.y)
                                        .unwrap_or(delta);
                                    dial.set(
                                        &mut settings,
                                        start + (origin - delta) / dial.travel(),
                                    );
                                }
                            }
                        }
                        if response.drag_stopped() {
                            self.dial_drag = None;
                        }
                        if response.double_clicked() {
                            dial.set(&mut settings, dial.get(&defaults));
                        }
                        let scroll = if response.hovered() {
                            ui.input(|input| input.raw_scroll_delta.y)
                        } else {
                            0.0
                        };
                        if scroll != 0.0 {
                            dial.set(&mut settings, value + scroll / (dial.travel() * 2.0));
                        }
                        response.on_hover_text(dial.hint());
                        let label = egui::RichText::new(dial.label())
                            .color(rgb(theme.soft))
                            .font(egui::FontId::monospace(7.0));
                        ui.allocate_ui(egui::vec2(DIAL_SIDE, 9.0), |ui| {
                            ui.centered_and_justified(|ui| ui.label(label));
                        });
                    });
                }
            });
        }
        if settings != before {
            self.desk.edit_preset(tool, |slot| *slot = settings);
        }
    }

    /// Letter boxes: tip, edge, chunk. Lit when chosen.
    fn toggles(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let tool = self.desk.tool();
        let fields = tool.fields();
        let mut settings = self.desk.preset_of(tool).clone();
        let before = settings.clone();
        let five = egui::vec2((PANEL_INNER - 4.0) / 5.0, 14.0);
        let three = egui::vec2((PANEL_INNER - 2.0) / 3.0, 14.0);
        if fields.contains(&Field::Tip) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                for tip in Tip::all() {
                    let letter = &tip.label()[..1];
                    if letter_cell(ui, theme, five, letter, settings.tip == tip)
                        .on_hover_text(format!("{}\n{}", tip.label(), tip.usage()))
                        .clicked()
                    {
                        settings.tip = tip;
                    }
                }
            });
        }
        if fields.contains(&Field::Edge) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                for edge in Edge::all() {
                    let letter = &edge.label()[..1];
                    if letter_cell(ui, theme, three, letter, settings.edge == edge)
                        .on_hover_text(format!("Edge: {}", edge.label()))
                        .clicked()
                    {
                        settings.edge = edge;
                    }
                }
            });
        }
        let show_dither = fields.contains(&Field::Chunk) || settings.edge == Edge::Dither;
        if show_dither {
            let four = egui::vec2((PANEL_INNER - 3.0) / 4.0, 14.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                for chunk in [1u32, 2, 4, 8] {
                    if letter_cell(ui, theme, four, &chunk.to_string(), settings.chunk == chunk)
                        .on_hover_text(format!("Chunk {chunk}: dither cells {chunk} px square"))
                        .clicked()
                    {
                        settings.chunk = chunk;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                for pattern in DitherPattern::all() {
                    if letter_cell(
                        ui,
                        theme,
                        five,
                        pattern.letter(),
                        settings.dither_pattern == pattern,
                    )
                    .on_hover_text(format!("Dither: {}", pattern.label()))
                    .clicked()
                    {
                        settings.dither_pattern = pattern;
                    }
                }
            });
            ui.add(egui::Slider::new(&mut settings.dither_bias, -1.0..=1.0).text("Bias"));
            if letter_cell(ui, theme, three, "T", settings.tones)
                .on_hover_text("Three tones: black, #444444, #CCCCCC")
                .clicked()
            {
                settings.tones = !settings.tones;
            }
        }
        if settings != before {
            self.desk.edit_preset(tool, |slot| *slot = settings);
        }
    }

    fn tool_grid(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let tools = palette_matching(&self.desk.query);
        let current = self.desk.tool();
        for section in [
            Section::Raster,
            Section::Vector,
            Section::Picture,
            Section::Cut,
        ] {
            let rows: Vec<Tool> = tools
                .iter()
                .copied()
                .filter(|tool| tool.section() == section)
                .collect();
            if rows.is_empty() {
                continue;
            }
            if section != Section::Raster {
                ui.add_space(6.0);
                ui.separator();
            }
            // Two across. A cell is 25 px, thirty percent under the old 36.
            let gap = CELL_GAP;
            let cell = egui::vec2(CELL_SIDE, CELL_SIDE);
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for pair in rows.chunks(2) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                    for tool in pair {
                        let response = tool_cell(ui, *tool, cell, theme, *tool == current);
                        if response.clicked() || response.secondary_clicked() {
                            self.desk.palette_click(*tool, false);
                        }
                    }
                });
            }
        }
    }

    fn layers(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        let rows = self.desk.layer_rows();
        let selected = self.desk.selected.clone();
        let chip = blend_chip_width(ui);
        let mut toggle_folder: Option<String> = None;
        let mut mark: Option<(String, &'static str)> = None;
        let mut blend_pick: Option<(String, String)> = None;
        let mut opacity_start: Option<OpacityDrag> = None;
        egui::ScrollArea::vertical()
            .id_salt("layer-rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &rows {
                    let width = ui.available_width();
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(width, ROW_H),
                        egui::Sense::click_and_drag(),
                    );
                    let active = selected.as_deref() == Some(row.id.as_str());
                    let geom = row_geom(rect, row.depth, row.group, chip);
                    paint_layer_row(ui, theme, row, active, &geom, rect);
                    if self.desk.reveal.as_deref() == Some(row.id.as_str()) {
                        response.scroll_to_me(Some(egui::Align::Center));
                        self.desk.reveal = None;
                    }
                    let pointer = response.interact_pointer_pos();
                    if response.secondary_clicked() {
                        self.desk.open_layer_window(row.id.clone());
                    }
                    if response.clicked() {
                        if let Some(pos) = pointer {
                            if geom.disclosure.is_some_and(|box_| box_.contains(pos)) {
                                toggle_folder = Some(row.id.clone());
                            } else if geom.eye.contains(pos) {
                                let visible = !row.visible;
                                self.desk
                                    .update_layer(&row.id, |node| node.visible = visible);
                            } else if geom.mute.contains(pos) {
                                mark = Some((row.id.clone(), "mute"));
                            } else if geom.solo.contains(pos) {
                                mark = Some((row.id.clone(), "solo"));
                            } else if geom.clip.contains(pos) {
                                mark = Some((row.id.clone(), "clip"));
                            } else if geom.blend.is_some_and(|box_| box_.contains(pos)) {
                                let popup = ui.make_persistent_id(("blend", row.id.as_str()));
                                ui.memory_mut(|memory| memory.toggle_popup(popup));
                            } else if !geom.opacity.contains(pos) {
                                self.desk.select_layer(row.id.clone());
                            }
                        }
                    }
                    if response.drag_started() {
                        if let Some(pos) = pointer {
                            if geom.opacity.contains(pos) {
                                opacity_start = Some(OpacityDrag {
                                    id: row.id.clone(),
                                    start_y: pos.y,
                                    start: row.opacity,
                                    before: None,
                                });
                            }
                        }
                    }
                    if let Some(blend) = geom.blend {
                        let popup = ui.make_persistent_id(("blend", row.id.as_str()));
                        let mut anchor = response.clone();
                        anchor.rect = blend;
                        let picked = egui::popup::popup_below_widget(
                            ui,
                            popup,
                            &anchor,
                            egui::popup::PopupCloseBehavior::CloseOnClick,
                            |ui| {
                                let mut chosen = None;
                                for mode in BLENDS {
                                    if ui.button(blend_label(mode)).clicked() {
                                        chosen = Some((*mode).to_string());
                                    }
                                }
                                chosen
                            },
                        );
                        if let Some(Some(mode)) = picked {
                            blend_pick = Some((row.id.clone(), mode));
                        }
                    }
                }
            });
        if opacity_start.is_some() {
            self.opacity_drag = opacity_start;
        }
        if let Some(drag) = self.opacity_drag.as_mut() {
            if let Some(pos) = ui.input(|input| input.pointer.interact_pos()) {
                let delta = drag.start_y - pos.y;
                let value = (f32::from(drag.start) + delta * 255.0 / 100.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                self.desk.preview_opacity(&drag.id, value, &mut drag.before);
            }
            if ui.input(|input| input.pointer.any_released()) {
                let before = self.opacity_drag.take().and_then(|drag| drag.before);
                self.desk.finish_opacity(before);
            }
        }
        if let Some(id) = toggle_folder {
            self.desk.toggle_folder(&id);
        }
        if let Some((id, which)) = mark {
            self.desk.update_layer(&id, |node| match which {
                "mute" => node.mute = !node.mute,
                "solo" => node.solo = !node.solo,
                _ => node.clip = !node.clip,
            });
        }
        if let Some((id, mode)) = blend_pick {
            self.desk.update_layer(&id, |node| node.blend = mode);
        }
    }

    fn objects(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        let available = ui.available_height();
        let list_h = (available * self.object_split).clamp(72.0, (available - 96.0).max(72.0));
        let selected = self.desk.object_window.clone();
        egui::ScrollArea::vertical()
            .id_salt("object-list")
            .max_height(list_h)
            .auto_shrink([false, false])
            .show(ui, |ui| self.object_list(ui, theme));
        let (bar, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::drag());
        response
            .clone()
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        let paint = if response.hovered() || response.dragged() {
            theme.ink
        } else {
            theme.rule
        };
        ui.painter().hline(
            bar.x_range(),
            bar.center().y,
            egui::Stroke::new(1.0_f32, rgb(paint)),
        );
        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta().y);
            if available > 1.0 {
                self.object_split = (self.object_split + delta / available).clamp(0.28, 0.72);
            }
        }
        ui.label(
            egui::RichText::new("SETTINGS")
                .monospace()
                .small()
                .color(rgb(theme.ink)),
        );
        egui::ScrollArea::vertical()
            .id_salt("object-settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(id) = selected {
                    if self.desk.art.objects.iter().any(|object| object.id == id) {
                        self.object_settings(ui, &id);
                    } else {
                        ui.label(
                            egui::RichText::new("That object is gone.")
                                .color(rgb(theme.soft))
                                .small(),
                        );
                    }
                } else {
                    ui.label(
                        egui::RichText::new("Select an object above.")
                            .color(rgb(theme.soft))
                            .small(),
                    );
                }
            });
    }

    /// The scene's objects, top to bottom. The top row is behind.
    fn object_list(&mut self, ui: &mut egui::Ui, theme: &vmi_core::Theme) {
        ui.horizontal(|ui| {
            if ui
                .button("Folder")
                .on_hover_text("A library folder. New objects join the folder you click.")
                .clicked()
            {
                self.desk.add_folder();
            }
            if ui
                .button("Object")
                .on_hover_text("A symbol. Static until a state or a frame needs more.")
                .clicked()
            {
                self.desk.add_object();
            }
        });
        let joined = self
            .desk
            .art
            .objects
            .iter()
            .find(|object| object.id == self.desk.library_folder && object.is_folder())
            .map(|object| object.name.clone());
        let note = match joined {
            Some(name) => format!("24 fps. New objects join {name}. The top row is behind."),
            None => "24 fps. The top row is behind. Select a row to edit it below.".into(),
        };
        ui.label(egui::RichText::new(note).color(rgb(theme.soft)).small());
        let closed = self.library_closed.clone();
        let focus = self.desk.library_folder.clone();
        let editing = self.desk.object_window.clone();
        let listed = vmi_core::library_rows(&self.desk.art.objects, &closed);
        let rows: Vec<(String, String, String, bool, u16, i64)> = listed
            .iter()
            .filter_map(|row| {
                let object = self
                    .desk
                    .art
                    .objects
                    .iter()
                    .find(|object| object.id == row.id)?;
                Some((
                    object.id.clone(),
                    object.name.clone(),
                    object.kind.clone(),
                    row.folder,
                    row.depth,
                    object.stack,
                ))
            })
            .collect();
        let mut toggle: Option<String> = None;
        let mut open: Option<String> = None;
        let mut focus_folder: Option<String> = None;
        let mut place: Option<(String, String)> = None;
        let drag_id = egui::Id::new("object-drag");
        for (id, name, kind, folder, depth, stack) in &rows {
            let current = editing.as_ref() == Some(id) || (*folder && focus == *id);
            let response = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(f32::from(*depth) * 12.0);
                if *folder {
                    let shut = closed.contains(id);
                    if ui.small_button(if shut { ">" } else { "v" }).clicked() {
                        toggle = Some(id.clone());
                    }
                } else {
                    ui.label(
                        egui::RichText::new(format!("Z{stack:03}"))
                            .monospace()
                            .small()
                            .color(rgb(theme.quiet)),
                    );
                }
                let title = if *folder {
                    name.clone()
                } else {
                    format!("{name}  {kind}")
                };
                ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::Button::new(title)
                        .selected(current)
                        .sense(egui::Sense::click_and_drag()),
                )
            });
            let response = response.inner;
            if response.clicked() || response.secondary_clicked() {
                open = Some(id.clone());
                if *folder {
                    focus_folder = Some(id.clone());
                }
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                ui.memory_mut(|memory| memory.data.insert_temp(drag_id, id.clone()));
            }
            if response.hovered() && ui.input(|input| input.pointer.any_released()) {
                let from = ui.memory(|memory| memory.data.get_temp::<String>(drag_id));
                if let Some(from) = from {
                    if from != *id {
                        place = Some((from, id.clone()));
                    }
                    ui.memory_mut(|memory| memory.data.remove::<String>(drag_id));
                }
            }
        }
        if let Some(id) = toggle {
            if !self.library_closed.remove(&id) {
                self.library_closed.insert(id);
            }
        }
        if let Some(id) = open {
            self.desk.open_object_window(id);
        }
        if let Some(id) = focus_folder {
            self.desk.library_folder = id;
        }
        if let Some((from, target)) = place {
            self.desk.place_on(&from, &target);
        }
    }

    /// The strip at the top of an area: its name opens the list of views.
    /// A sheet area carries the transport too.
    fn area_header(&mut self, ui: &mut egui::Ui, area: Area) {
        let theme = self.desk.theme();
        let view = self.areas.get(area);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 2.0);
            let title = egui::RichText::new(view.label())
                .monospace()
                .small()
                .color(rgb(theme.ink));
            ui.menu_button(title, |ui| {
                for candidate in View::ALL {
                    if ui
                        .selectable_label(candidate == view, candidate.label())
                        .clicked()
                    {
                        self.areas.set(area, candidate);
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_hover_text("What this area shows.");
            if view.is_sheet() {
                self.transport(ui, area, theme);
            }
        });
    }

    /// Back, Play, Forward, Loop, Onion, in that order like the Python bar,
    /// then the cut's numbers.
    fn transport(&mut self, ui: &mut egui::Ui, area: Area, theme: &Theme) {
        let cell = egui::vec2(18.0, 16.0);
        let word = egui::vec2(34.0, 16.0);
        if letter_cell(ui, theme, cell, "<", false)
            .on_hover_text("Back one frame    ,    (W: first frame)")
            .clicked()
        {
            self.desk.step_frame(-1);
        }
        let playing = self.desk.playing;
        if letter_cell(
            ui,
            theme,
            word,
            if playing { "STOP" } else { "PLAY" },
            playing,
        )
        .on_hover_text("Play the cut at its frame rate    Space")
        .clicked()
        {
            self.desk.toggle_play();
        }
        if letter_cell(ui, theme, cell, ">", false)
            .on_hover_text("Forward one frame    .    (Shift+W: last frame)")
            .clicked()
        {
            self.desk.step_frame(1);
        }
        if letter_cell(ui, theme, word, "LOOP", self.desk.loop_play)
            .on_hover_text("Play returns to the first frame. Off stops on the last.")
            .clicked()
        {
            self.desk.toggle_loop();
        }
        let skin = letter_cell(ui, theme, word, "SKIN", self.desk.art.onion)
            .on_hover_text("Onion skin. Click toggles it. Right-click edits the colors.    O");
        if skin.secondary_clicked() {
            self.desk.open_onion_window();
        } else if skin.clicked() {
            self.desk.toggle_onion();
        }
        let last = self.desk.cut_end().saturating_sub(1);
        let counter = format!(
            "{}/{}",
            vmi_core::slot_hex(self.desk.playhead()),
            vmi_core::slot_hex(last)
        );
        ui.label(
            egui::RichText::new(counter)
                .monospace()
                .small()
                .color(rgb(theme.ink)),
        );
        let mut fps = self.desk.art.fps.clamp(1, 60);
        let response = ui.add(
            egui::DragValue::new(&mut fps)
                .range(1..=60)
                .speed(0.1)
                .prefix("FPS "),
        );
        if response.changed() {
            self.desk.set_fps(fps);
        }
        for hold in [1u32, 2, 3] {
            let name = match hold {
                1 => "ones",
                2 => "twos",
                _ => "threes",
            };
            if letter_cell(ui, theme, cell, &hold.to_string(), self.desk.hold == hold)
                .on_hover_text(format!("On {name}: a new frame holds {hold}."))
                .clicked()
            {
                self.desk.set_hold(hold);
            }
        }
        if letter_cell(ui, theme, word, "+CEL", false)
            .on_hover_text("New frame after this drawing, on the hold picked here    Shift+F")
            .clicked()
        {
            self.desk.new_cel(None, None);
        }
        if letter_cell(ui, theme, word, "WRAP", self.desk.loop_wrap)
            .on_hover_text("Stepping past the end wraps. Separate from LOOP.")
            .clicked()
        {
            self.desk.toggle_wrap();
        }
        let index = area.index();
        let mode = self.sheet_mode(area);
        let shown = if self.sheet_auto[index] {
            "AUTO"
        } else {
            mode.label()
        };
        if letter_cell(ui, theme, egui::vec2(52.0, 16.0), shown, false)
            .on_hover_text("AUTO, FILM, X-SHEET. Auto shows the sheet in a wide window, the film in a tall one.")
            .clicked()
        {
            // AUTO -> FILM -> X-SHEET -> AUTO.
            if self.sheet_auto[index] {
                self.sheet_auto[index] = false;
                self.areas.set(area, View::Film);
            } else if self.areas.get(area) == View::Film {
                self.areas.set(area, View::XSheet);
            } else {
                self.sheet_auto[index] = true;
            }
        }
    }

    /// The orientation a sheet area draws.
    fn sheet_mode(&self, area: Area) -> sheet::Mode {
        if self.sheet_auto[area.index()] {
            sheet::Mode::Auto
        } else if self.areas.get(area) == View::Film {
            sheet::Mode::Film
        } else {
            sheet::Mode::XSheet
        }
    }

    fn area_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, area: Area) {
        match self.areas.get(area) {
            View::Picture => {
                self.picture_bar(ui);
                self.picture(ui, ctx);
            }
            View::Layers => self.layers(ui),
            View::Objects => self.objects(ui),
            View::XSheet | View::Film => {
                let mode = self.sheet_mode(area);
                let theme = self.desk.theme();
                self.desk.drop_lost_clip();
                let acts = sheet::show(ui, &self.desk, &mut self.sheet_state, mode, theme);
                for act in acts {
                    self.apply(act);
                }
            }
        }
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::SetFrame(frame) => self.desk.set_sheet_frame(frame),
            Act::Enter(id) => self.desk.enter_clip(&id),
            Act::Leave => self.desk.leave_clip(),
            Act::Pick { folder, frame } => self.desk.pick_cel(&folder, frame),
            Act::Range {
                folder,
                start,
                count,
            } => self.desk.pick_range(&folder, start, count),
            Act::MoveCel { folder, cel, start } => self.desk.move_cel(&folder, &cel, start),
            Act::NewCel { folder, frame } => self.desk.new_cel(Some(&folder), Some(frame)),
            Act::DeleteCel { folder, cel } => self.desk.delete_cel(&folder, &cel),
            Act::Command {
                folder,
                name,
                start,
                count,
                arg,
            } => self.desk.sheet_command(&folder, name, start, count, arg),
            Act::NewFrame => self.desk.new_cel(None, None),
            Act::AnimateSelected => self.desk.animate_selected(),
        }
    }

    fn picture_bar(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        let tool = self.desk.tool();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(tool.name()).color(rgb(theme.ink)).strong());
            ui.label(egui::RichText::new(tool.usage()).color(rgb(theme.soft)));
            if brush_tool(tool) {
                let preset = self.desk.preset().clone();
                let mut size = preset.size.round() as i32;
                let mut low = preset.size_min.round() as i32;
                let mut high = preset.size_max.round() as i32;
                ui.label(egui::RichText::new("Size").color(rgb(theme.soft)));
                let size_box = ui.add(egui::DragValue::new(&mut size).range(1..=512).speed(1.0));
                if size_box.changed() {
                    self.desk.edit_preset(tool, |slot| slot.size = size as f32);
                }
                self.watch_text(size_box);
                ui.label(egui::RichText::new("Min").color(rgb(theme.soft)));
                if ui.add(egui::DragValue::new(&mut low).range(1..=512).speed(1.0)).changed() {
                    self.desk.edit_preset(tool, |slot| slot.size_min = low as f32);
                }
                ui.label(egui::RichText::new("Max").color(rgb(theme.soft)));
                if ui.add(egui::DragValue::new(&mut high).range(1..=512).speed(1.0)).changed() {
                    self.desk.edit_preset(tool, |slot| slot.size_max = high as f32);
                }
                if ui.button(fringe_label(preset.fringe)).on_hover_text("Antialiasing. Click to cycle off, .5, 1, and 2.").clicked() {
                    let next = cycle_fringe(preset.fringe);
                    self.desk.edit_preset(tool, |slot| slot.fringe = next);
                }
                let mut size_pressure = preset.size_pressure;
                if ui.checkbox(&mut size_pressure, "Pressure size").changed() {
                    self.desk.edit_preset(tool, |slot| slot.size_pressure = size_pressure);
                }
                let mut opacity_pressure = preset.opacity_pressure;
                if ui.checkbox(&mut opacity_pressure, "Flow").on_hover_text("Pressure changes how hard each dab is. Opacity stays the whole stroke.").changed() {
                    self.desk.edit_preset(tool, |slot| slot.opacity_pressure = opacity_pressure);
                }
                let mut percent = (preset.opacity * 100.0).round() as i32;
                if ui
                    .add(egui::DragValue::new(&mut percent).range(0..=100).speed(1.0).suffix("%"))
                    .on_hover_text("Opacity of the whole stroke. One pass, even where the stroke crosses itself.")
                    .changed()
                {
                    self.desk.edit_preset(tool, |slot| slot.opacity = percent as f32 / 100.0);
                }
                let ink = self.desk.ink;
                if ink_chip(ui, ink, egui::vec2(28.0, 18.0), theme.frame)
                    .on_hover_text("Ink for every tool. Opens the Color window on the layers column. X trades it with the other color.")
                    .clicked()
                {
                    self.color_open = true;
                }
                let back = self.desk.ink_back;
                let back_button = egui::Button::new("X").fill(rgb(back)).min_size(egui::vec2(28.0, 18.0));
                if ui.add(back_button).on_hover_text("Other color. X trades the two inks.").clicked() {
                    self.desk.swap_inks();
                }
                let mut clear = self.desk.clear_ink;
                if ui.checkbox(&mut clear, "Clear").on_hover_text("C. Empty pixels, same size.").changed() {
                    if clear != self.desk.clear_ink {
                        self.desk.toggle_clear();
                    }
                }
            }
            if ui.button("Warp target").clicked() {
                self.desk.set_tool(Tool::WarpTarget);
            }
            if ui.button("Warp mask").clicked() {
                self.desk.set_tool(Tool::WarpMask);
            }
            if tool.section() == Section::Cut {
                if ui.button("Rect").clicked() {
                    self.desk.set_tool(Tool::CutRect);
                }
                if ui.button("Lasso").clicked() {
                    self.desk.set_tool(Tool::CutLasso);
                }
                if ui.button("Polygon").clicked() {
                    self.desk.set_tool(Tool::CutPolygon);
                }
                if ui.button("Cut to folder").clicked() {
                    self.desk.cut_to_folder();
                }
                if ui.button("Delete").clicked() {
                    self.desk.delete_selection();
                }
            }
            let zoom = (self.desk.camera.zoom * 100.0).round() as i32;
            ui.label(format!("{zoom}%"));
        });
    }

    /// A pen that reports force arrives as a touch. A mouse does not, and
    /// keeps full pressure.
    fn read_pressure(&mut self, ui: &egui::Ui) {
        let force = ui.input(|input| {
            input.events.iter().rev().find_map(|event| match event {
                egui::Event::Touch {
                    phase: egui::TouchPhase::End | egui::TouchPhase::Cancel,
                    ..
                } => Some(1.0),
                egui::Event::Touch {
                    force: Some(force), ..
                } => Some(*force),
                _ => None,
            })
        });
        if let Some(force) = force {
            self.desk.set_pressure(force);
        } else if let Some(pen) = tablet::pressure() {
            self.desk.set_pressure(pen);
        } else {
            // No pen has spoken, or a mouse spoke after it: a lifted pen's 0
            // must not stick to the mouse.
            self.desk.set_pressure(1.0);
        }
    }

    /// Finger taps: two undo, three redo. Any second finger cancels the
    /// stroke the first one started, so a tap never leaves a dot.
    fn finger_taps(&mut self, ui: &egui::Ui) {
        let now_ms = (ui.input(|input| input.time) * 1000.0) as u64;
        let events: Vec<egui::Event> = ui.input(|input| input.events.clone());
        for event in events {
            if let egui::Event::Touch {
                id,
                phase,
                pos,
                force,
                ..
            } = event
            {
                // A pen reports force. A finger does not.
                if force.is_some() {
                    continue;
                }
                match phase {
                    egui::TouchPhase::Start => self.fingers.start(id.0, pos.x, pos.y, now_ms),
                    egui::TouchPhase::Move => self.fingers.moved(id.0, pos.x, pos.y),
                    egui::TouchPhase::End => self.fingers.end(id.0, now_ms),
                    egui::TouchPhase::Cancel => self.fingers.cancel(),
                }
            }
        }
        if self.fingers.many() {
            self.desk.escape_stroke();
        }
        match self.fingers.take() {
            Some(Tap::Undo) => self.desk.undo(),
            Some(Tap::Redo) => self.desk.redo(),
            None => {}
        }
    }

    fn picture(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let available = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(
            available,
            egui::Sense {
                click: true,
                drag: true,
                focusable: false,
            },
        );
        let frame = Frame {
            x: response.rect.min.x,
            y: response.rect.min.y,
            w: response.rect.width(),
            h: response.rect.height(),
        };
        self.desk.fit_once(frame);
        self.read_pressure(ui);
        self.finger_taps(ui);
        match self.desk.take_display(self.texture.is_none()) {
            Some(Display::Full(pixels)) => {
                upload_plate(
                    &mut self.texture,
                    ctx,
                    self.desk.art.width as usize,
                    self.desk.art.height as usize,
                    &pixels,
                );
            }
            Some(Display::Play { w, h, rgba }) => {
                upload_plate(&mut self.texture, ctx, w as usize, h as usize, &rgba);
            }
            Some(Display::Part { x, y, w, h, rgba }) => {
                if let Some(texture) = &mut self.texture {
                    let full = [self.desk.art.width as usize, self.desk.art.height as usize];
                    if texture.size() == full {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &rgba,
                        );
                        texture.set_partial([x as usize, y as usize], image, texture_options());
                    }
                }
            }
            None => {}
        }
        if let Some(texture) = &self.texture {
            paint_plate(ui, texture, &self.desk, frame);
        }
        if response.hovered() {
            // Ctrl+wheel and a pinch come in as a zoom factor. A plain wheel
            // zooms too; the picture is the only thing under the pointer.
            let (pinch, scroll) = ui.input(|input| (input.zoom_delta(), input.raw_scroll_delta));
            let factor = if (pinch - 1.0).abs() > 1e-4 {
                pinch
            } else if scroll.y != 0.0 {
                1.12_f32.powf(scroll.y / 40.0)
            } else {
                1.0
            };
            if (factor - 1.0).abs() > 1e-4 {
                if let Some(pos) = response.hover_pos() {
                    self.desk.zoom_at(frame, pos.x, pos.y, factor);
                }
            }
        }
        let space = ui.input(|input| input.key_down(egui::Key::Space));
        let shift = ui.input(|input| input.modifiers.shift);
        let middle = ui.input(|input| input.pointer.button_down(egui::PointerButton::Middle));
        // Space pans. Space+Shift drags zoom about the view center. Shift rotates about that center.
        if response.dragged() && space && shift {
            self.space_dragged = true;
            let dy = ui.input(|input| input.pointer.delta().y);
            let factor = (-dy * 0.008).exp();
            let center = response.rect.center();
            self.desk.zoom_at(frame, center.x, center.y, factor);
            self.rotate_from = None;
        } else if response.dragged() && (space || middle) {
            if space {
                self.space_dragged = true;
            }
            let delta = ui.input(|input| input.pointer.delta());
            self.desk.pan_by(delta.x, delta.y);
            self.rotate_from = None;
        } else if shift && response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let center = response.rect.center();
                if let Some(prev) = self.rotate_from {
                    let a0 = (prev.y - center.y).atan2(prev.x - center.x);
                    let a1 = (pos.y - center.y).atan2(pos.x - center.x);
                    self.desk.rotate_about_view(a1 - a0);
                }
                self.rotate_from = Some(pos);
            }
        } else {
            self.rotate_from = None;
            let camera = self.desk.camera;
            let doc_w = self.desk.art.width as f32;
            let doc_h = self.desk.art.height as f32;
            let doc_at = |pos: egui::Pos2| camera.to_doc(frame, doc_w, doc_h, pos.x, pos.y);
            if response.secondary_clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    if pointer_on_picture(ui, &response, pos) {
                        let (x, y) = doc_at(pos);
                        self.desk.down_doc(x, y, Button::Secondary);
                    }
                }
            } else if self.desk.tool() == Tool::Select {
                // Python's picture click is the release, and only when the
                // pointer stayed put. A drag does not select. Ctrl and Command stay free.
                if response.clicked() {
                    let command = ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
                    if !command {
                        if let Some(pos) = response.interact_pointer_pos() {
                            let (x, y) = doc_at(pos);
                            self.desk.down_doc(x, y, Button::Primary);
                            self.desk.up_doc(x, y);
                        }
                    }
                }
            } else if self.desk.tool() == Tool::Dropper && !self.fingers.many() {
                // A drag reads each pixel. It does not start a stroke.
                let primary = ui.input(|input| {
                    input.pointer.button_down(egui::PointerButton::Primary)
                });
                if primary {
                    let positions: Vec<egui::Pos2> = ui.input(|input| {
                        let mut points = Vec::new();
                        if input.pointer.primary_pressed() {
                            if let Some(origin) = input.pointer.press_origin() {
                                points.push(origin);
                            }
                        }
                        for event in &input.events {
                            if let egui::Event::PointerMoved(pos) = event {
                                points.push(*pos);
                            }
                        }
                        if points.is_empty() {
                            if let Some(pos) = input.pointer.latest_pos() {
                                points.push(pos);
                            }
                        }
                        points
                    });
                    for pos in positions {
                        if pointer_on_picture(ui, &response, pos) {
                            let (x, y) = doc_at(pos);
                            self.desk.sample_ink(x, y);
                        }
                    }
                }
            } else if !shift && !space && !middle && !self.fingers.many() {
                // The drag threshold is 6 px and the first decided frame only has
                // the latest point. A thin line then starts as a straight chord.
                // The pen goes down at the press, and every move after that is a dab.
                let pressed = ui.input(|input| input.pointer.primary_pressed());
                if pressed && !self.desk.is_painting() {
                    if let Some(origin) = ui.input(|input| input.pointer.press_origin()) {
                        if pointer_on_picture(ui, &response, origin) {
                            let (x, y) = doc_at(origin);
                            self.desk.down_doc(x, y, Button::Primary);
                        }
                    }
                }
                if self.desk.is_painting() {
                    let positions: Vec<egui::Pos2> = ui.input(|input| {
                        let mut after_press = !pressed;
                        let mut points = Vec::new();
                        for event in &input.events {
                            match event {
                                egui::Event::PointerButton {
                                    pressed: true,
                                    button: egui::PointerButton::Primary,
                                    ..
                                } => {
                                    after_press = true;
                                }
                                egui::Event::PointerMoved(pos) if after_press => points.push(*pos),
                                _ => {}
                            }
                        }
                        points
                    });
                    if positions.is_empty() {
                        if let Some(pos) = ui.input(|input| input.pointer.latest_pos()) {
                            if pressed || response.dragged() {
                                let (x, y) = doc_at(pos);
                                self.desk.move_doc(x, y);
                            }
                        }
                    } else {
                        for pos in positions {
                            let (x, y) = doc_at(pos);
                            self.desk.move_doc(x, y);
                        }
                    }
                    ctx.request_repaint();
                }
                if ui.input(|input| input.pointer.primary_released()) && self.desk.is_painting() {
                    if let Some(pos) = ui.input(|input| input.pointer.latest_pos()) {
                        let (x, y) = doc_at(pos);
                        self.desk.up_doc(x, y);
                    } else {
                        self.desk.release_doc();
                    }
                }
            }
        }
    }
}

/// A tool cell is 25 px, thirty percent under the old 36. Two across with a
/// 2 px gap is 52. That is the panel.
const CELL_SIDE: f32 = 25.0;
const CELL_GAP: f32 = 2.0;
const PANEL_INNER: f32 = CELL_SIDE * 2.0 + CELL_GAP;
/// Three rotaries across the same panel.
const DIAL_GAP: f32 = 2.0;
const DIAL_SIDE: f32 = (PANEL_INNER - DIAL_GAP * 2.0) / 3.0;
/// The panel's own margin on each side.
const PANEL_MARGIN: f32 = 4.0;

/// A name that acts as a tab. The chosen one is white; the rest are quiet.
/// Tabs are 20 tall and at most 240 wide. A long name ends in an ellipsis
/// before the close mark.
const TAB_HEIGHT: f32 = 20.0;
const TAB_MAX_WIDTH: f32 = 240.0;
const TAB_PAD_LEFT: f32 = 8.0;
/// 6px before the mark, the mark, 6px after it so the stroke stays off the border.
const TAB_CLOSE: egui::Vec2 = egui::vec2(18.0, 12.0);

/// Cut `text` so it fits `width` in the tab's font, with an ellipsis at the end.
fn elide_right(ui: &egui::Ui, font: &egui::FontId, text: &str, width: f32) -> String {
    let measure = |candidate: &str| {
        ui.fonts(|fonts| {
            fonts
                .layout_no_wrap(candidate.to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    if measure(text) <= width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let mut candidate: String = chars[..keep].iter().collect();
        candidate.push('…');
        if measure(&candidate) <= width {
            return candidate;
        }
    }
    "…".into()
}

/// One tab in the K-OS tab sheet: field fill, quiet text inside a rule
/// border; ink text and border when selected.
fn project_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    selected: bool,
    with_close: bool,
) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let close_width = if with_close {
        TAB_CLOSE.x
    } else {
        TAB_PAD_LEFT
    };
    let room = TAB_MAX_WIDTH - TAB_PAD_LEFT - close_width;
    let text = elide_right(ui, &font, name, room);
    let text_width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    });
    let width = (TAB_PAD_LEFT + text_width + close_width).min(TAB_MAX_WIDTH);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::click());
    let color = rgb(if selected { theme.ink } else { theme.quiet });
    let border = rgb(if selected { theme.ink } else { theme.rule });
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, rgb(theme.field));
    painter.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0_f32, border));
    painter.text(
        egui::pos2(rect.left() + TAB_PAD_LEFT, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
    response
}

/// A project tab and its close mark. The mark is the same grey as mute and
/// solo, ink under the pointer.
fn project_tab_with_close(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    selected: bool,
) -> (egui::Response, egui::Response) {
    let tab = project_tab(ui, theme, name, selected, true);
    let rect = tab.rect;
    let mark_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - TAB_CLOSE.x,
            rect.center().y - TAB_CLOSE.y / 2.0,
        ),
        TAB_CLOSE,
    );
    let mark = ui.interact(mark_rect, tab.id.with("close"), egui::Sense::click());
    let lit = mark.hovered() || mark.is_pointer_button_down_on();
    let color = rgb(if lit { theme.ink } else { theme.quiet });
    let stroke = egui::Stroke::new(1.0_f32, color);
    let origin = mark_rect.min;
    let painter = ui.painter();
    painter.line_segment(
        [
            origin + egui::vec2(6.0, 3.0),
            origin + egui::vec2(11.0, 8.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            origin + egui::vec2(11.0, 3.0),
            origin + egui::vec2(6.0, 8.0),
        ],
        stroke,
    );
    (tab, mark)
}

fn tab_label(ui: &mut egui::Ui, theme: &Theme, text: &str, chosen: bool) -> egui::Response {
    let color = if chosen {
        rgb(theme.ink)
    } else {
        rgb(theme.soft)
    };
    let label = egui::Label::new(egui::RichText::new(text).monospace().color(color))
        .sense(egui::Sense::click());
    let response = ui.add(label);
    if chosen {
        let rect = response.rect;
        let y = rect.bottom() + 1.0;
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, rgb(theme.ink)),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(response.rect.expand(1.0), 0.0, rgb(theme.frame));
        ui.painter().text(
            response.rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::monospace(ui.text_style_height(&egui::TextStyle::Monospace) * 0.75),
            rgb(theme.ink),
        );
    }
    response
}

fn unique_palette_name(palettes: &[Palette], base: &str) -> String {
    if !palettes
        .iter()
        .any(|palette| palette.name.eq_ignore_ascii_case(base))
    {
        return base.to_string();
    }
    let mut number = 2;
    loop {
        let candidate = format!("{base} {number}");
        if !palettes
            .iter()
            .any(|palette| palette.name.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
        number += 1;
    }
}

/// A file name for a palette name: letters, digits, dash, underscore.
fn file_stem_for(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if stem.is_empty() {
        "palette".into()
    } else {
        stem
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Aseprite's palette folder when it exists, else the desk's own.
fn palette_folder() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut candidates = vec![home.join(".config").join("aseprite").join("palettes")];
    candidates.push(
        home.join("Library")
            .join("Application Support")
            .join("Aseprite")
            .join("palettes"),
    );
    if let Some(appdata) = std::env::var_os("APPDATA") {
        candidates.push(PathBuf::from(appdata).join("Aseprite").join("palettes"));
    }
    candidates
        .into_iter()
        .find(|path| path.is_dir())
        .unwrap_or_else(|| home.join(".cache").join("vmi-studio").join("palettes"))
}

/// Palette files in a folder, by name.
fn palette_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .map(|ext| {
                        vmi_core::palette::EXTENSIONS
                            .contains(&ext.to_string_lossy().to_ascii_lowercase().as_str())
                    })
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

/// The tool whose color the color cell edits: this one, or the pen.
fn list_match(filter: &str, label: &str, path: &str) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty()
        || label.to_lowercase().contains(&filter)
        || path.to_lowercase().contains(&filter)
}

struct FolderRow {
    name: String,
    path: PathBuf,
    dir: bool,
}

fn folder_rows(folder: &Path, filter: &str) -> Vec<FolderRow> {
    let mut rows = Vec::new();
    let Ok(read) = std::fs::read_dir(folder) else {
        return rows;
    };
    let filter = filter.trim().to_lowercase();
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let dir = path.is_dir();
        if !dir && !vmi_core::catalog::is_drawing(&path) {
            continue;
        }
        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
            continue;
        }
        rows.push(FolderRow { name, path, dir });
    }
    rows.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    rows
}

fn tool_cell(
    ui: &mut egui::Ui,
    tool: Tool,
    cell: egui::Vec2,
    theme: &Theme,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::click());
    let fill = if active {
        rgb(theme.ink)
    } else if response.hovered() {
        rgb(theme.frame)
    } else {
        rgb(theme.row)
    };
    let ink = if active {
        rgb(theme.field)
    } else {
        rgb(theme.ink)
    };
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, rgb(theme.frame)));
    paint_icon(&painter, rect, tool, ink);
    let shortcut = tool.shortcut();
    let title = if shortcut.is_empty() {
        tool.name().to_string()
    } else {
        format!("{}  [{shortcut}]", tool.name())
    };
    response.on_hover_text(format!(
        "{title}\n{}\nTool options are under the layers.",
        tool.usage()
    ))
}

/// Whole-pixel scaling keeps the icon crisp. The bitmap is centered in the cell.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, tool: Tool, color: egui::Color32) {
    let side = icons::SIDE as f32;
    let px = ((rect.width().min(rect.height()) - 4.0) / side)
        .floor()
        .max(1.0);
    let size = px * side;
    let origin = egui::pos2(
        (rect.center().x - size * 0.5).round(),
        (rect.center().y - size * 0.5).round(),
    );
    for (col, row) in icons::cells(icons::icon(tool)) {
        let min = egui::pos2(origin.x + col as f32 * px, origin.y + row as f32 * px);
        painter.rect_filled(
            egui::Rect::from_min_size(min, egui::vec2(px, px)),
            0.0,
            color,
        );
    }
}

fn tip_row(ui: &mut egui::Ui, settings: &mut ToolSettings) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Tip");
        for tip in Tip::all() {
            if ui
                .selectable_label(settings.tip == tip, tip.label())
                .on_hover_text(tip.usage())
                .clicked()
            {
                settings.tip = tip;
            }
        }
    });
}

/// A square cell with a painted mark. Lit cells invert.
fn small_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    side: f32,
    lit: bool,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> egui::Response {
    sized_cell(ui, theme, egui::vec2(side, side), lit, draw)
}

/// A cell of any size with a painted mark. Lit cells invert.
fn sized_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    size: egui::Vec2,
    lit: bool,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if lit {
        rgb(theme.ink)
    } else if response.hovered() {
        rgb(theme.frame)
    } else {
        rgb(theme.row)
    };
    let ink = if lit {
        rgb(theme.field)
    } else {
        rgb(theme.ink)
    };
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, rgb(theme.frame)));
    draw(&painter, rect, ink);
    response
}

const ROW_H: f32 = 22.0;
const GUIDE: f32 = 16.0;
const MARK: f32 = 11.0;
const KNOB: f32 = 18.0;
const ROW_GUTTER: f32 = 7.0;
const BLEND_GAP: f32 = 8.0;
const BLEND_TAIL: f32 = 4.0;
const MIN_TREE: f32 = 160.0;

struct RowGeom {
    disclosure: Option<egui::Rect>,
    eye: egui::Rect,
    name_x: f32,
    branch_x: f32,
    mute: egui::Rect,
    solo: egui::Rect,
    clip: egui::Rect,
    opacity: egui::Rect,
    blend: Option<egui::Rect>,
    split: f32,
}

fn blend_chip_width(ui: &egui::Ui) -> f32 {
    let font = egui::FontId::monospace(12.0);
    let mut widest = 0.0_f32;
    for mode in BLENDS {
        let width = ui.fonts(|fonts| {
            fonts
                .layout_no_wrap(
                    blend_label(mode).to_owned(),
                    font.clone(),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        });
        widest = widest.max(width);
    }
    widest + 18.0
}

/// Names on the left. Mute, solo, clip, and the dial stay. The blend chip
/// drops first when the names would fall under 160 px, the same rule as Python.
fn row_geom(rect: egui::Rect, depth: usize, group: bool, chip: f32) -> RowGeom {
    let buttons = 6.0 + MARK + 6.0 + MARK + 6.0 + MARK + 6.0 + KNOB;
    let full = ROW_GUTTER + buttons + BLEND_GAP + chip + BLEND_TAIL;
    let show_blend = rect.width() >= MIN_TREE + full;
    let x0 = rect.left() + 4.0;
    let branch = x0 + depth as f32 * GUIDE + GUIDE / 2.0;
    let top = rect.center().y - MARK / 2.0;
    let disclosure = if group {
        Some(egui::Rect::from_min_size(
            egui::pos2(branch - MARK / 2.0, top),
            egui::vec2(MARK, MARK),
        ))
    } else {
        None
    };
    let eye = egui::Rect::from_min_size(
        egui::pos2(branch + GUIDE - MARK / 2.0, top),
        egui::vec2(MARK, MARK),
    );
    let end = rect.right();
    let after_opacity = if show_blend {
        end - BLEND_TAIL - chip - BLEND_GAP
    } else {
        end - BLEND_TAIL
    };
    let opacity = egui::Rect::from_min_size(
        egui::pos2(after_opacity - KNOB, rect.center().y - KNOB / 2.0),
        egui::vec2(KNOB, KNOB),
    );
    let clip = egui::Rect::from_min_size(
        egui::pos2(opacity.left() - 6.0 - MARK, top),
        egui::vec2(MARK, MARK),
    );
    let solo = egui::Rect::from_min_size(
        egui::pos2(clip.left() - 6.0 - MARK, top),
        egui::vec2(MARK, MARK),
    );
    let mute = egui::Rect::from_min_size(
        egui::pos2(solo.left() - 6.0 - MARK, top),
        egui::vec2(MARK, MARK),
    );
    let split = mute.left() - 6.0;
    let blend = if show_blend {
        Some(egui::Rect::from_min_size(
            egui::pos2(end - BLEND_TAIL - chip, rect.top() + 2.0),
            egui::vec2(chip, (rect.height() - 4.0).max(0.0)),
        ))
    } else {
        None
    };
    RowGeom {
        disclosure,
        eye,
        name_x: eye.right() + 8.0,
        branch_x: branch,
        mute,
        solo,
        clip,
        opacity,
        blend,
        split,
    }
}

fn paint_layer_row(
    ui: &egui::Ui,
    theme: &Theme,
    row: &crate::desk::LayerRow,
    active: bool,
    geom: &RowGeom,
    rect: egui::Rect,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, rgb(if active { theme.row } else { theme.field }));
    let rule = egui::Stroke::new(1.0_f32, rgb(theme.rule));
    let x0 = rect.left() + 4.0;
    let mid = rect.center().y;
    for level in 0..row.depth {
        if row.guides & (1u32 << level.min(31)) == 0 {
            continue;
        }
        let gx = x0 + level as f32 * GUIDE + GUIDE / 2.0;
        painter.line_segment(
            [egui::pos2(gx, rect.top()), egui::pos2(gx, rect.bottom())],
            rule,
        );
    }
    painter.line_segment(
        [
            egui::pos2(geom.branch_x, rect.top()),
            egui::pos2(geom.branch_x, mid),
        ],
        rule,
    );
    if row.has_next {
        painter.line_segment(
            [
                egui::pos2(geom.branch_x, mid),
                egui::pos2(geom.branch_x, rect.bottom()),
            ],
            rule,
        );
    }
    let hook = geom
        .disclosure
        .map(|box_| box_.left())
        .unwrap_or(geom.eye.left());
    painter.line_segment(
        [egui::pos2(geom.branch_x, mid), egui::pos2(hook, mid)],
        rule,
    );
    let mark_color = if active {
        rgb(theme.ink)
    } else {
        rgb(theme.quiet)
    };
    let mark = egui::Stroke::new(1.0_f32, mark_color);
    if let Some(box_) = geom.disclosure {
        painter.rect_stroke(box_.shrink(0.5), 0.0, mark);
        let y = box_.center().y;
        painter.line_segment(
            [
                egui::pos2(box_.left() + 2.0, y),
                egui::pos2(box_.right() - 2.0, y),
            ],
            mark,
        );
        if !row.open {
            let x = box_.center().x;
            painter.line_segment(
                [
                    egui::pos2(x, box_.top() + 2.0),
                    egui::pos2(x, box_.bottom() - 2.0),
                ],
                mark,
            );
        }
    }
    if row.visible {
        painter.rect_filled(geom.eye, 0.0, mark_color);
    }
    painter.rect_stroke(geom.eye.shrink(0.5), 0.0, mark);
    let name_color = if active {
        rgb(theme.ink)
    } else if row.marker == "window" {
        egui::Color32::from_rgb(0x8b, 0x6b, 0xb8)
    } else if row.marker == "target" {
        egui::Color32::from_rgb(0x5c, 0x3e, 0x4a)
    } else {
        rgb(theme.quiet)
    };
    let font = egui::FontId::monospace(12.0);
    let mut label = row.name.clone();
    if let Some(frame) = row.frame {
        label.push_str(&format!("  f{frame}"));
    }
    let reserve = if row.animation { 36.0 } else { 6.0 };
    let shown = elide_right(
        ui,
        &font,
        &label,
        (geom.split - geom.name_x - reserve).max(0.0),
    );
    painter.text(
        egui::pos2(geom.name_x, mid),
        egui::Align2::LEFT_CENTER,
        &shown,
        font.clone(),
        name_color,
    );
    if row.animation {
        let ani = if active {
            rgb(theme.ink)
        } else {
            rgb(theme.quiet)
        };
        painter.text(
            egui::pos2(geom.split - 4.0, mid),
            egui::Align2::RIGHT_CENTER,
            "ANI",
            font.clone(),
            ani,
        );
    }
    let text_w = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(shown, font, egui::Color32::WHITE)
            .size()
            .x
    });
    let rule_start = geom.name_x + text_w + 8.0;
    if rule_start < rect.right() - 2.0 {
        painter.line_segment(
            [
                egui::pos2(rule_start, rect.bottom() - 0.5),
                egui::pos2(rect.right(), rect.bottom() - 0.5),
            ],
            rule,
        );
    }
    paint_square(ui, geom.mute, "M", row.mute, false, theme);
    paint_square(ui, geom.solo, "S", row.solo, false, theme);
    paint_square(ui, geom.clip, "C", row.clip, active, theme);
    paint_opacity_dial(ui, geom.opacity, row.opacity, active, theme);
    if let Some(box_) = geom.blend {
        paint_blend(ui, box_, &row.blend, active, theme);
    }
}

fn paint_square(ui: &egui::Ui, box_: egui::Rect, letter: &str, on: bool, lit: bool, theme: &Theme) {
    if box_.width() < 4.0 {
        return;
    }
    let painter = ui.painter();
    let (frame, fill, text) = if letter == "C" {
        if on && lit {
            (rgb(theme.ink), Some(rgb(theme.ink)), rgb(theme.field))
        } else if on {
            (rgb(theme.quiet), Some(rgb(theme.quiet)), rgb(theme.field))
        } else {
            (rgb(theme.quiet), None, rgb(theme.quiet))
        }
    } else if on {
        (rgb(theme.ink), Some(rgb(theme.ink)), rgb(theme.field))
    } else {
        (rgb(theme.quiet), None, rgb(theme.quiet))
    };
    if let Some(fill) = fill {
        painter.rect_filled(box_, 0.0, fill);
    }
    painter.rect_stroke(box_.shrink(0.5), 0.0, egui::Stroke::new(1.0_f32, frame));
    painter.text(
        box_.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::monospace(9.0),
        text,
    );
}

fn paint_opacity_dial(ui: &egui::Ui, box_: egui::Rect, opacity: u8, lit: bool, theme: &Theme) {
    if box_.width() < 8.0 {
        return;
    }
    let color = if lit {
        rgb(theme.ink)
    } else {
        rgb(theme.quiet)
    };
    let painter = ui.painter();
    let center = box_.center();
    let radius = box_.width().min(box_.height()) / 2.0 - 2.0;
    painter.circle_stroke(center, radius, egui::Stroke::new(1.0_f32, color));
    let amount = f32::from(opacity) / 255.0;
    let angle =
        (2.0_f32 / 3.0) * std::f32::consts::PI + amount * (5.0_f32 / 3.0) * std::f32::consts::PI;
    let bump = center + egui::vec2(angle.cos(), angle.sin()) * (radius - 0.5);
    painter.rect_filled(
        egui::Rect::from_center_size(bump, egui::vec2(3.0, 2.0)),
        0.0,
        color,
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        format!("{opacity:02X}"),
        egui::FontId::monospace(7.0),
        color,
    );
}

fn paint_blend(ui: &egui::Ui, box_: egui::Rect, mode: &str, lit: bool, theme: &Theme) {
    if box_.width() < 8.0 {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(box_, 0.0, rgb(theme.field));
    painter.rect_stroke(
        box_.shrink(0.5),
        0.0,
        egui::Stroke::new(1.0_f32, rgb(theme.rule)),
    );
    let color = if lit {
        rgb(theme.ink)
    } else {
        rgb(theme.quiet)
    };
    let font = egui::FontId::monospace(12.0);
    let label = elide_right(ui, &font, blend_label(mode), (box_.width() - 18.0).max(0.0));
    painter.text(
        egui::pos2(box_.left() + 4.0, box_.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    painter.text(
        egui::pos2(box_.right() - 3.0, box_.center().y),
        egui::Align2::RIGHT_CENTER,
        "v",
        egui::FontId::monospace(12.0),
        color,
    );
}

fn letter_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    size: egui::Vec2,
    letter: &str,
    lit: bool,
) -> egui::Response {
    let text = letter.to_string();
    sized_cell(ui, theme, size, lit, move |painter, rect, ink| {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::monospace(10.0),
            ink,
        );
    })
}

fn paint_bitmap(
    painter: &egui::Painter,
    rect: egui::Rect,
    bitmap: &icons::Icon,
    color: egui::Color32,
) {
    let side = icons::SIDE as f32;
    let px = ((rect.width().min(rect.height()) - 4.0) / side)
        .floor()
        .max(1.0);
    let size = px * side;
    let origin = egui::pos2(
        (rect.center().x - size * 0.5).round(),
        (rect.center().y - size * 0.5).round(),
    );
    for (col, row) in icons::cells(bitmap) {
        let min = egui::pos2(origin.x + col as f32 * px, origin.y + row as f32 * px);
        painter.rect_filled(
            egui::Rect::from_min_size(min, egui::vec2(px, px)),
            0.0,
            color,
        );
    }
}

/// 7 o'clock at 0, the long way around to 5 o'clock at 1. Same sweep as Thrash Tracker.
fn dial_angle(amount: f32) -> f32 {
    let span = amount.clamp(0.0, 1.0);
    2.0 * std::f32::consts::PI / 3.0 + span * 5.0 * std::f32::consts::PI / 3.0
}

/// A rotary. The bump rides the rim. Two hex digits sit in the middle.
fn paint_dial(
    painter: &egui::Painter,
    rect: egui::Rect,
    amount: f32,
    digits: &str,
    theme: &Theme,
    lit: bool,
) {
    let color = if lit {
        rgb(theme.ink)
    } else {
        rgb(theme.quiet)
    };
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5 - 1.0;
    painter.circle_stroke(center, radius, egui::Stroke::new(1.0_f32, color));
    let angle = dial_angle(amount);
    let bump = egui::pos2(
        (center.x + angle.cos() * radius).round(),
        (center.y + angle.sin() * radius).round(),
    );
    painter.rect_filled(
        egui::Rect::from_center_size(bump, egui::vec2(3.0, 3.0)),
        0.0,
        color,
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        digits,
        egui::FontId::monospace(8.0),
        color,
    );
}

/// Python's brush row: size, the two limits, antialiasing, pressure, the two inks.
fn brush_tool(tool: Tool) -> bool {
    tool != Tool::Fill && matches!(tool.release(), Release::Pixels | Release::Stroke)
}

fn cycle_fringe(fringe: f32) -> f32 {
    const STEPS: [f32; 4] = [0.0, 0.5, 1.0, 2.0];
    let index = STEPS
        .iter()
        .position(|step| (fringe - step).abs() < 0.05)
        .unwrap_or(0);
    STEPS[(index + 1) % STEPS.len()]
}

fn fringe_label(fringe: f32) -> &'static str {
    if fringe <= 0.0 {
        "AA off"
    } else if (fringe - 0.5).abs() < 0.05 {
        "AA .5"
    } else if (fringe - 1.0).abs() < 0.05 {
        "AA 1"
    } else if (fringe - 2.0).abs() < 0.05 {
        "AA 2"
    } else {
        "AA"
    }
}

fn paint_plate(ui: &mut egui::Ui, texture: &egui::TextureHandle, desk: &Desk, frame: Frame) {
    let painter = ui.painter().with_clip_rect(egui::Rect::from_min_size(
        egui::pos2(frame.x, frame.y),
        egui::vec2(frame.w, frame.h),
    ));
    let width = desk.art.width as f32;
    let height = desk.art.height as f32;
    let corner = |x: f32, y: f32, u: f32, v: f32| {
        let (sx, sy) = desk.camera.to_screen(frame, width, height, x, y);
        egui::epaint::Vertex {
            pos: egui::pos2(sx, sy),
            uv: egui::pos2(u, v),
            color: egui::Color32::WHITE,
        }
    };
    let mut mesh = egui::Mesh::with_texture(texture.id());
    mesh.vertices.push(corner(0.0, 0.0, 0.0, 0.0));
    mesh.vertices.push(corner(width, 0.0, 1.0, 0.0));
    mesh.vertices.push(corner(width, height, 1.0, 1.0));
    mesh.vertices.push(corner(0.0, height, 0.0, 1.0));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
    let theme = desk.theme();
    for mark in desk.marks() {
        let (sx, sy) = desk.camera.to_screen(frame, width, height, mark.x, mark.y);
        let color = if mark.hot {
            rgb(theme.ink)
        } else {
            rgb(theme.soft)
        };
        if let Some((tx, ty)) = mark.line_to {
            let (hx, hy) = desk.camera.to_screen(frame, width, height, tx, ty);
            painter.line_segment(
                [egui::pos2(sx, sy), egui::pos2(hx, hy)],
                egui::Stroke::new(1.0_f32, color),
            );
        }
        painter.circle_stroke(egui::pos2(sx, sy), 4.0, egui::Stroke::new(1.0_f32, color));
    }
    if desk.cut_points.len() >= 2 {
        let mut last = None;
        for point in &desk.cut_points {
            let (sx, sy) = desk
                .camera
                .to_screen(frame, width, height, point[0], point[1]);
            if let Some((px, py)) = last {
                painter.line_segment(
                    [egui::pos2(px, py), egui::pos2(sx, sy)],
                    egui::Stroke::new(1.0_f32, rgb(theme.ink)),
                );
            }
            last = Some((sx, sy));
        }
    }
}

fn edit_field(
    ui: &mut egui::Ui,
    tool: Tool,
    field: Field,
    settings: &mut ToolSettings,
    theme: &Theme,
    typing: &mut bool,
    color_open: &mut bool,
) {
    match field {
        Field::ReleaseNote => {
            ui.label(tool.release_note());
        }
        Field::Color => {
            ui.horizontal(|ui| {
                ui.label("Color");
                let chip = ink_chip(ui, settings.color, egui::vec2(22.0, 18.0), theme.frame)
                    .on_hover_text("Opens the Color window.");
                if chip.clicked() {
                    *color_open = true;
                }
                let mut hex = vmi_core::hex_of(settings.color);
                let response = field_edit(ui, &mut hex);
                if response.has_focus() {
                    *typing = true;
                }
                if response.changed() {
                    if let Some(color) = parse_hex(&hex) {
                        settings.color = color;
                    }
                }
            });
        }
        Field::Size => {
            ui.add(
                egui::DragValue::new(&mut settings.size)
                    .range(0.5..=256.0)
                    .speed(0.25)
                    .prefix("Size "),
            );
        }
        Field::Min => {
            ui.add(
                egui::DragValue::new(&mut settings.size_min)
                    .range(0.5..=256.0)
                    .speed(0.25)
                    .prefix("Min "),
            );
        }
        Field::Max => {
            ui.add(
                egui::DragValue::new(&mut settings.size_max)
                    .range(0.5..=512.0)
                    .speed(0.25)
                    .prefix("Max "),
            );
        }
        Field::Softness => {
            ui.add(egui::Slider::new(&mut settings.softness, 0.0..=1.0).text("Softness"));
        }
        Field::Tip => tip_row(ui, settings),
        Field::Texture => {
            ui.add(egui::Slider::new(&mut settings.texture, 0.0..=1.0).text("Texture"));
        }
        Field::Edge => {
            ui.horizontal(|ui| {
                ui.label("Edge");
                for edge in Edge::all() {
                    if ui
                        .selectable_label(settings.edge == edge, edge.label())
                        .clicked()
                    {
                        settings.edge = edge;
                    }
                }
            });
        }
        Field::Fringe => {
            ui.add(
                egui::DragValue::new(&mut settings.fringe)
                    .range(0.0..=8.0)
                    .speed(0.05)
                    .prefix("Fringe "),
            );
        }
        Field::Chunk => {
            ui.horizontal(|ui| {
                ui.label("Chunk");
                ui.add(
                    egui::DragValue::new(&mut settings.chunk)
                        .range(1..=16)
                        .speed(0.1),
                );
                for chunk in [1u32, 2, 4, 8] {
                    if ui
                        .selectable_label(settings.chunk == chunk, chunk.to_string())
                        .clicked()
                    {
                        settings.chunk = chunk;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Pattern");
                for pattern in DitherPattern::all() {
                    if ui
                        .selectable_label(settings.dither_pattern == pattern, pattern.letter())
                        .on_hover_text(pattern.label())
                        .clicked()
                    {
                        settings.dither_pattern = pattern;
                    }
                }
            });
            ui.add(egui::Slider::new(&mut settings.dither_bias, -1.0..=1.0).text("Bias"));
        }
        Field::Tones => {
            ui.checkbox(&mut settings.tones, "Three tones");
        }
        Field::PressureSize => {
            ui.checkbox(&mut settings.size_pressure, "Pressure size");
        }
        Field::PressureOpacity => {
            ui.checkbox(&mut settings.opacity_pressure, "Pressure opacity");
        }
        Field::Opacity => {
            ui.add(egui::Slider::new(&mut settings.opacity, 0.0..=1.0).text("Opacity"));
        }
        Field::SizeCurve => curve_editor(ui, "Size curve", &mut settings.size_curve),
        Field::OpacityCurve => curve_editor(ui, "Opacity curve", &mut settings.opacity_curve),
        Field::CurveKind => {
            egui::ComboBox::from_label("Curve")
                .selected_text(settings.curve.label())
                .show_ui(ui, |ui| {
                    for kind in CurveKind::all() {
                        ui.selectable_value(&mut settings.curve, kind, kind.label());
                    }
                });
        }
        Field::Grain => {
            egui::ComboBox::from_label("Grain")
                .selected_text(settings.grain.label())
                .show_ui(ui, |ui| {
                    for kind in GrainKind::all() {
                        ui.selectable_value(&mut settings.grain, kind, kind.label());
                    }
                });
        }
        Field::Seed => {
            let mut seed = settings.seed.min(u64::from(u32::MAX)) as u32;
            if ui
                .add(
                    egui::DragValue::new(&mut seed)
                        .range(0..=u32::MAX)
                        .prefix("Seed "),
                )
                .changed()
            {
                settings.seed = u64::from(seed);
            }
        }
        Field::Closed => {
            ui.checkbox(&mut settings.closed, "Closed");
        }
        Field::BoneReach => {
            ui.add(
                egui::DragValue::new(&mut settings.bone_reach)
                    .range(8.0..=800.0)
                    .speed(1.0)
                    .prefix("Bone reach "),
            );
        }
        Field::FillTolerance => {
            let mut tolerance = i32::from(settings.fill_tolerance);
            if ui
                .add(egui::Slider::new(&mut tolerance, 0..=255).text("Fill tolerance"))
                .changed()
            {
                settings.fill_tolerance = tolerance as u8;
            }
        }
        Field::PickRadius => {
            ui.add(
                egui::DragValue::new(&mut settings.pick_radius)
                    .range(2.0..=64.0)
                    .prefix("Pick radius "),
            );
        }
        Field::Scatter => {
            ui.add(egui::Slider::new(&mut settings.scatter, 0.0..=1.0).text("Scatter"));
        }
        Field::Stabilize => {
            ui.add(egui::Slider::new(&mut settings.stabilize, 0.0..=64.0).text("Stabilize"));
        }
    }
    let _ = theme;
}

fn curve_editor(ui: &mut egui::Ui, label: &str, curve: &mut Vec<(f32, f32)>) {
    ui.label(label);
    for point in curve.iter_mut() {
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut point.0)
                    .range(0.0..=1.0)
                    .speed(0.01)
                    .prefix("t "),
            );
            ui.add(
                egui::DragValue::new(&mut point.1)
                    .range(0.0..=1.0)
                    .speed(0.01)
                    .prefix("v "),
            );
        });
    }
    ui.horizontal(|ui| {
        if ui.button("Add point").clicked() {
            curve.push((0.5, 0.5));
        }
        if curve.len() > 2 && ui.button("Remove point").clicked() {
            curve.pop();
        }
    });
}

fn upload_plate(
    texture: &mut Option<egui::TextureHandle>,
    ctx: &egui::Context,
    w: usize,
    h: usize,
    pixels: &[u8],
) {
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], pixels);
    let same = texture.as_ref().is_some_and(|tex| tex.size() == [w, h]);
    if same {
        if let Some(tex) = texture.as_mut() {
            tex.set(image, texture_options());
        }
    } else {
        *texture = Some(ctx.load_texture("plate", image, texture_options()));
    }
}

fn texture_options() -> egui::TextureOptions {
    match PICTURE_FILTER {
        PictureFilter::Nearest => egui::TextureOptions::NEAREST,
    }
}

fn rgb(color: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(color[0], color[1], color[2])
}

/// A flat ink cell. Click opens the Color window. It is not egui's popup.
fn ink_chip(ui: &mut egui::Ui, color: [u8; 3], size: egui::Vec2, frame: [u8; 3]) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    ui.painter().rect_filled(rect, 0.0, rgb(color));
    ui.painter()
        .rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, rgb(frame)));
    response
}

/// A press on a window that covers the picture is not a stroke.
fn pointer_on_picture(ui: &egui::Ui, response: &egui::Response, pos: egui::Pos2) -> bool {
    response.rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
}

fn widget_look(
    fill: egui::Color32,
    weak: egui::Color32,
    border: egui::Color32,
    text: egui::Color32,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: weak,
        bg_stroke: egui::Stroke::new(1.0_f32, border),
        fg_stroke: egui::Stroke::new(1.0_f32, text),
        rounding: 0.0.into(),
        expansion: 0.0,
    }
}

/// A line edit. Typed text stays the ink. The frame is the rule until focus,
/// then the selection stroke (white on Studio).
fn field_edit(ui: &mut egui::Ui, text: &mut String) -> egui::Response {
    let ink = ui.visuals().selection.stroke.color;
    ui.add(egui::TextEdit::singleline(text).text_color(ink))
}

fn apply_theme(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = if theme.field[0] > 180 {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    let ink = rgb(theme.ink);
    let field = rgb(theme.field);
    let quiet = rgb(theme.quiet);
    let rule = rgb(theme.rule);
    let row = rgb(theme.row);
    // Python Studio: black field, quiet text until hover or focus, then white
    // text and a white border. A selected row is white on #111111.
    visuals.panel_fill = field;
    visuals.window_fill = field;
    visuals.extreme_bg_color = field;
    visuals.faint_bg_color = rgb(theme.grid);
    visuals.code_bg_color = row;
    visuals.override_text_color = None;
    visuals.widgets.noninteractive = widget_look(field, field, rule, quiet);
    visuals.widgets.inactive = widget_look(field, field, rule, quiet);
    visuals.widgets.hovered = widget_look(field, row, ink, ink);
    visuals.widgets.active = widget_look(field, row, ink, ink);
    visuals.widgets.open = widget_look(field, row, ink, ink);
    visuals.selection.bg_fill = row;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, ink);
    visuals.window_stroke = egui::Stroke::new(1.0_f32, ink);
    visuals.window_rounding = 0.0.into();
    visuals.menu_rounding = 0.0.into();
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.text_cursor.stroke.color = ink;
    visuals.text_cursor.stroke.width = 1.0_f32;
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        let mut scroll = egui::style::ScrollStyle::solid();
        scroll.bar_width = 10.0;
        scroll.foreground_color = true;
        style.spacing.scroll = scroll;
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}

/// The house face when the font cache has it, else one mono face for all
/// the chrome. The Python app unpacks faces into `~/.cache/vmi-studio/fonts`.
fn install_fonts(ctx: &egui::Context) {
    install_face(ctx, vmi_core::fonts::house_bytes().or_else(house_font));
}

fn install_face(ctx: &egui::Context, named: Option<(String, Vec<u8>)>) {
    let mut fonts = egui::FontDefinitions::default();
    let mono = fonts
        .families
        .get(&egui::FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    match named {
        Some((name, bytes)) => {
            fonts
                .font_data
                .insert(name.clone(), egui::FontData::from_owned(bytes));
            let mut stack = vec![name];
            stack.extend(mono);
            fonts
                .families
                .insert(egui::FontFamily::Monospace, stack.clone());
            fonts.families.insert(egui::FontFamily::Proportional, stack);
        }
        None => {
            fonts.families.insert(egui::FontFamily::Proportional, mono);
        }
    }
    ctx.set_fonts(fonts);
}

/// Bytes for a face id. `monospace` is the house stack and never downloads.
fn face_bytes(id: &str) -> Result<Option<(String, Vec<u8>)>, String> {
    if id.is_empty() || id == vmi_core::fonts::DEFAULT_FACE {
        return Ok(vmi_core::fonts::house_bytes());
    }
    let face = vmi_core::fonts::face(id).ok_or_else(|| format!("Could not load {id}."))?;
    let path = vmi_core::fonts::ensure(&face)?;
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    if !vmi_core::fonts::looks_like_font(&bytes) {
        return Err(format!("Could not load {}.", face.label));
    }
    Ok(Some((face.id, bytes)))
}

/// courier-pixel first, then sometype-mono, from the shared font cache.
fn house_font() -> Option<(String, Vec<u8>)> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let folder = home.join(".cache").join("vmi-studio").join("fonts");
    let entries: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    let face = |wanted: &[&str]| {
        entries.iter().find(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            (name.ends_with(".ttf") || name.ends_with(".otf"))
                && wanted.iter().all(|part| name.contains(part))
        })
    };
    let path = face(&["courier", "pixel"]).or_else(|| face(&["sometype"]))?;
    let bytes = std::fs::read(path).ok()?;
    Some((stem_of(path), bytes))
}

impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, self.desk.theme());
        self.sync_font(ctx, false);
        self.keys(ctx);
        self.text_focused = false;
        self.windows(ctx);
        // Playback: frames fall due on the cut's clock. The measured frame
        // time, not egui's predicted one, which stays at the monitor's rate
        // while the window only repaints when a frame is due.
        let dt = ctx.input(|input| f64::from(input.unstable_dt));
        if self.desk.tick(dt) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                self.desk.next_frame_in().max(0.001),
            ));
        }
        self.hear_loader(ctx);
        let layers_default = self.desk.session.layers_width;
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New picture    Ctrl+N").clicked() {
                        self.new_picture();
                    }
                    if ui.button("Open    Ctrl+O").on_hover_text("Project files are chosen from the browser tab.").clicked() {
                        self.desk.show_browser();
                    }
                    if ui.button("Save blueprint    Ctrl+S").clicked() {
                        self.save_blueprint();
                    }
                    if ui.button("Save blueprint as").clicked() {
                        self.save_blueprint_as();
                    }
                    if ui.button("Export PNG    Shift+E").clicked() {
                        self.path_text = "drawing.png".into();
                        self.prompt = Some(Prompt::Export);
                    }
                    ui.separator();
                    if ui.button("Launcher").clicked() {
                        self.desk.show_browser();
                    }
                    if ui.button("Quit    Ctrl+Q").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo    Ctrl+Z").clicked() {
                        self.desk.undo();
                    }
                    if ui.button("Redo    Ctrl+Shift+Z").clicked() {
                        self.desk.redo();
                    }
                });
                ui.menu_button("Layer", |ui| {
                    if ui.button("Layer").clicked() {
                        self.desk.add_layer();
                    }
                    if ui.button("Group").clicked() {
                        self.desk.add_group();
                    }
                    ui.separator();
                    let selected = self.desk.selected.clone();
                    let folder = selected.as_deref().and_then(|id| vmi_core::find(&self.desk.art.layers, id)).filter(|node| !node.is_layer());
                    let is_animation = folder.map(|node| node.animation).unwrap_or(false);
                    let has_folder = folder.is_some();
                    if ui.add_enabled(has_folder, egui::Button::new(if is_animation { "Animation folder    ✓" } else { "Animation folder" }))
                        .on_hover_text("The selected folder is one sequence. Its layers are its cels.")
                        .clicked()
                    {
                        if let Some(id) = selected.clone() {
                            self.desk.toggle_animation(&id);
                        }
                    }
                    if ui
                        .button("New frame    Shift+F")
                        .on_hover_text("An empty cel after this drawing; a lone layer becomes frame 1 first. The skin comes on.")
                        .clicked()
                    {
                        self.desk.new_cel(None, None);
                    }
                });
                ui.menu_button("Theme", |ui| {
                    for theme in THEMES {
                        if ui.button(theme.name).clicked() {
                            self.desk.set_theme(theme.id);
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Fit    Ctrl+0").clicked() {
                        self.desk.fit(self.desk.last_frame);
                    }
                    if ui.button("Actual size    Ctrl+1").clicked() {
                        self.desk.actual();
                    }
                    ui.separator();
                    let onion = self.desk.art.onion;
                    if ui.button(if onion { "Onion skin    O  ✓" } else { "Onion skin    O" }).clicked() {
                        self.desk.toggle_onion();
                    }
                    if ui.button(if self.palette_open { "Palette    ✓" } else { "Palette" }).clicked() {
                        self.palette_open = !self.palette_open;
                    }
                    ui.separator();
                    ui.label(egui::RichText::new("Areas").small().color(rgb(self.desk.theme().soft)));
                    for (area, name) in [(Area::Left, "Left"), (Area::Right, "Right"), (Area::Bottom, "Bottom"), (Area::Center, "Center")] {
                        ui.menu_button(format!("{name}: {}", self.areas.get(area).label()), |ui| {
                            for candidate in View::ALL {
                                if ui.selectable_label(self.areas.get(area) == candidate, candidate.label()).clicked() {
                                    self.areas.set(area, candidate);
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                });
                let font_id = self.desk.session.font.clone();
                let theme_id = self.desk.session.theme.clone();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let act = ui
                        .allocate_ui_with_layout(
                            egui::vec2(380.0, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| self.settings.show(ui, &font_id, &theme_id),
                        )
                        .inner;
                    self.apply_settings(ctx, act);
                });
            });
        });
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::none()
                    .fill(rgb(self.desk.theme().field))
                    .inner_margin(egui::Margin {
                        left: 8.0,
                        right: 8.0,
                        top: 2.0,
                        bottom: 0.0,
                    }),
            )
            .show(ctx, |ui| self.tab_bar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if !self.desk.on_browser {
                    ui.label(self.desk.art.file_name.as_str());
                    ui.separator();
                }
                ui.label(
                    egui::RichText::new(self.desk.status.as_str())
                        .color(rgb(self.desk.theme().ink)),
                );
            });
        });
        if self.desk.on_browser {
            egui::CentralPanel::default().show(ctx, |ui| self.browser_page(ui));
            self.overlays(ctx);
            self.desk.flush_session();
            return;
        }
        let left = egui::SidePanel::left("layers")
            .resizable(true)
            .default_width(layers_default)
            .width_range(180.0..=480.0)
            .show(ctx, |ui| {
                // Hold the width the user dragged. A shorter view must not shrink the column.
                ui.set_min_width(ui.available_width());
                let options = egui::Frame::none()
                    .fill(rgb(self.desk.theme().field))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .rounding(0.0);
                egui::TopBottomPanel::bottom("tool-options")
                    .resizable(true)
                    .default_height(260.0)
                    .height_range(140.0..=480.0)
                    .frame(options)
                    .show_inside(ui, |ui| self.tool_options(ui));
                self.area_header(ui, Area::Left);
                self.area_body(ui, ctx, Area::Left);
            });
        // Exactly two tool cells plus the margins. Not a pixel to the right.
        let panel_frame = egui::Frame::side_top_panel(&ctx.style())
            .inner_margin(egui::Margin::symmetric(PANEL_MARGIN, PANEL_MARGIN))
            .rounding(0.0);
        let brushes = egui::SidePanel::left("brushes")
            .resizable(false)
            .frame(panel_frame)
            .exact_width(PANEL_INNER + PANEL_MARGIN * 2.0)
            .show(ctx, |ui| self.palette(ui));
        let right = egui::SidePanel::right("right-area")
            .resizable(true)
            .default_width(self.desk.session.right_width)
            .width_range(120.0..=720.0)
            .show(ctx, |ui| {
                // Hold the width the user dragged. X-sheet, Objects, and the rest stay in this column.
                ui.set_min_width(ui.available_width());
                self.area_header(ui, Area::Right);
                self.area_body(ui, ctx, Area::Right);
            });
        // The sheet sits under the picture, between the side panes.
        let bottom = egui::TopBottomPanel::bottom("bottom-area")
            .resizable(true)
            .default_height(self.desk.session.bottom_height)
            .height_range(48.0..=520.0)
            .show(ctx, |ui| {
                ui.set_min_height(ui.available_height());
                self.area_header(ui, Area::Bottom);
                self.area_body(ui, ctx, Area::Bottom);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.areas.center != View::Picture {
                self.area_header(ui, Area::Center);
            }
            self.area_body(ui, ctx, Area::Center);
        });
        self.desk
            .note_widths(left.response.rect.width(), brushes.response.rect.width());
        self.desk.note_areas(
            right.response.rect.width(),
            bottom.response.rect.height(),
            &self.areas.ids(),
        );
        self.overlays(ctx);
        self.desk.flush_session();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.desk.flush_session();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_view_lives_in_one_area_and_the_picture_never_leaves() {
        let mut areas = Areas::default();
        assert_eq!(areas.ids(), ["layers", "picture", "xsheet", "film"]);
        // Objects on the right: the X-sheet steps aside into the hidden slot.
        areas.set(Area::Right, View::Objects);
        assert_eq!(areas.right, View::Objects);
        assert_eq!(areas.center, View::Picture);
        // The picture on the left swaps with the layers.
        areas.set(Area::Left, View::Picture);
        assert_eq!(areas.left, View::Picture);
        assert_eq!(areas.center, View::Layers);
        // A bad or old session falls back to the defaults.
        assert_eq!(Areas::from_ids(&[]), Areas::default());
        assert_eq!(
            Areas::from_ids(&[
                "layers".into(),
                "layers".into(),
                "film".into(),
                "xsheet".into()
            ]),
            Areas::default()
        );
        let kept = Areas::from_ids(&areas.ids());
        assert_eq!(kept, areas);
        assert_eq!(file_stem_for("DB 32 / mine"), "DB-32---mine");
        assert_eq!(
            unique_palette_name(&[Palette::new("Mine", vec![])], "Mine"),
            "Mine 2"
        );
    }
}
