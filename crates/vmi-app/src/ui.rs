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
    blend_label, palette_matching, parse_hex, CurveKind, Edge, Field, GrainKind, Section, Theme, Tip, Tool,
    ToolSettings, BLENDS, THEMES,
};

pub fn run() -> eframe::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1440.0, 900.0])
        .with_title("KOOL SKULL STUDIO");
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "kool-skull-studio",
        options,
        Box::new(|cc| {
            // Ctrl+= and Ctrl+- zoom the picture, never the widgets.
            cc.egui_ctx.options_mut(|options| options.zoom_with_keyboard = false);
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
            Self::Opacity => "Opacity.",
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

    const ALL: [Self; 7] = [Self::Size, Self::Soft, Self::Opacity, Self::Texture, Self::Tolerance, Self::Pick, Self::Reach];

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
    const ALL: [Self; 5] = [Self::Picture, Self::Layers, Self::XSheet, Self::Film, Self::Objects];

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
        Self { left: View::Layers, center: View::Picture, right: View::XSheet, bottom: View::Film }
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
        if let Some(other) = Area::ALL.iter().copied().find(|other| self.get(*other) == view) {
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
                    areas = Self { left, center, right, bottom };
                }
            }
        }
        areas
    }

    fn ids(&self) -> Vec<String> {
        [self.left, self.center, self.right, self.bottom].iter().map(|view| view.id().to_string()).collect()
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
            palette_folder: palette_folder().to_string_lossy().into_owned(),
            palette_path: String::new(),
            areas,
            sheet_auto: [false; 4],
            sheet_state: SheetState::default(),
            gate: Gate::new(),
            newpage: NewPage::new(),
            settings: SettingsBar::new(),
            applied_font: String::new(),
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
        let mut first = false;
        let mut last = false;
        let mut onion = false;
        let mut play = false;
        let mut new_cel = false;
        let mut shortcut = String::new();
        ctx.input_mut(|input| {
            // The sheet: comma back, period forward, W first, Shift+W last,
            // O onion skin, Shift+Space play, Shift+F a new frame.
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
                if input.key_pressed(egui::Key::O) && !input.modifiers.command && !input.modifiers.shift {
                    onion = true;
                }
                if input.modifiers.shift && input.key_pressed(egui::Key::Space) {
                    play = true;
                }
            }
            if input.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z) {
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
            // `=`, `+`, Ctrl+`=`, and Ctrl+`+` zoom in; `-` and Ctrl+`-` zoom out.
            let zoom_keys = !typing && !input.modifiers.alt;
            if zoom_keys && (input.key_pressed(egui::Key::Equals) || input.key_pressed(egui::Key::Plus)) {
                zoom_in = true;
            }
            if zoom_keys && input.key_pressed(egui::Key::Minus) {
                zoom_out = true;
            }
            if !typing && plain && !shift {
                for (key, name) in [
                    (egui::Key::P, "P"),
                    (egui::Key::N, "N"),
                    (egui::Key::C, "C"),
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
                ] {
                    if input.key_pressed(key) {
                        shortcut = name.to_string();
                    }
                }
            }
        });
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
        let before = settings.clone();
        ui.label(egui::RichText::new(tool.usage()).color(rgb(theme.soft)));
        ui.label(egui::RichText::new(tool.release_note()).color(rgb(theme.ink)));
        ui.separator();
        for field in tool.fields() {
            if *field == Field::ReleaseNote {
                continue;
            }
            edit_field(ui, tool, *field, &mut settings, theme, &mut self.text_focused);
        }
        if ui.button("Reset").clicked() {
            settings = ToolSettings::default_for(tool);
        }
        if settings != before {
            self.desk.edit_preset(tool, |slot| *slot = settings);
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
        let before = (name.clone(), opacity, blend.clone(), clip, visible, mute, solo);
        ui.label(if is_group { "Folder" } else { "Layer" });
        self.watch_text(ui.text_edit_singleline(&mut name));
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
                .on_hover_text("This folder is one sequence. Its layers are its cels, on the sheet.")
                .changed()
            {
                let id = id.to_string();
                self.desk.set_animation(&id, animation);
                return;
            }
        }
        let now = (name.clone(), opacity, blend.clone(), clip, visible, mute, solo);
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
        let (color0, size0, softness0, fringe0, edge0, chunk0, curve0, grain0, seed0, closed0, tones0, reach0) =
            snapshot;
        ui.label("The picture is a cache. These traits are the stroke.");
        let mut color = color0;
        if ui.color_edit_button_srgb(&mut color).changed() {
            self.desk.edit_stroke(|stroke| stroke.color = color);
        }
        let mut size = size0;
        if ui
            .add(egui::DragValue::new(&mut size).range(0.5..=256.0).speed(0.25).prefix("Size "))
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
        if ui.add(egui::Slider::new(&mut softness, 0.0..=1.0).text("Softness")).changed() {
            self.desk.edit_stroke(|stroke| stroke.softness = softness);
        }
        let mut fringe = fringe0;
        if ui
            .add(egui::DragValue::new(&mut fringe).range(0.0..=8.0).speed(0.05).prefix("Fringe "))
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
                if ui.selectable_label(chunk0 == chunk, chunk.to_string()).clicked() {
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
        if ui.add(egui::DragValue::new(&mut seed).range(0..=u32::MAX).prefix("Seed ")).changed() {
            self.desk.edit_stroke(|stroke| stroke.seed = u64::from(seed));
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
            if ui.add(egui::DragValue::new(&mut reach).range(8.0..=800.0).prefix("Bone reach ")).changed() {
                self.desk.edit_stroke(|stroke| {
                    if let Some(bone) = stroke.bone.as_mut() {
                        bone.reach = reach;
                    }
                });
            }
        }
    }

    fn object_settings(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(object) = self.desk.art.objects.iter().find(|object| object.id == id) else {
            return;
        };
        let mut name = object.name.clone();
        let mut kind = object.kind.clone();
        let before = (name.clone(), kind.clone());
        ui.label("Object");
        self.watch_text(ui.text_edit_singleline(&mut name));
        ui.label("Kind");
        self.watch_text(ui.text_edit_singleline(&mut kind));
        if (name.clone(), kind.clone()) != before {
            self.desk.edit_object(|object| {
                object.name = name;
                object.kind = kind;
            });
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
                let name = vmi_core::fonts::face(&id).map(|face| face.label).unwrap_or_else(|| id.clone());
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
        let name = vmi_core::fonts::face(&id).map(|face| face.label).unwrap_or_else(|| id.clone());
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
        let shown = if self.desk.on_browser { None } else { self.desk.shown };
        let mut pick: Option<Option<usize>> = None;
        let mut close: Option<usize> = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if project_tab(ui, theme, "browser", shown.is_none(), false).on_hover_text("Projects, tasks, and drawings").clicked() {
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
                    ui.add(egui::Label::new(egui::RichText::new(&path).color(egui::Color32::WHITE)).truncate())
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

    /// The launcher page, in the window where the editor otherwise is.
    fn browser_page(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("New picture").clicked() {
                self.new_picture();
            }
            if ui.button("Open…").clicked() {
                self.path_text = self.desk.session.path.clone();
                self.prompt = Some(Prompt::Open);
            }
        });
        ui.label(egui::RichText::new("Open a .clip, .psd, .kra, .xcf, or .vmib file.").color(rgb(theme.soft)));
    }

    fn windows(&mut self, ctx: &egui::Context) {
        if let Some(tool) = self.desk.tool_window {
            let mut open = true;
            egui::Window::new(format!("{} settings", tool.name()))
                .open(&mut open)
                .resizable(true)
                .default_width(280.0)
                .show(ctx, |ui| self.tool_settings(ui, tool));
            if !open {
                self.desk.close_tool_window();
            }
        }
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
        if let Some(id) = self.desk.object_window.clone() {
            let mut open = true;
            egui::Window::new("Object settings")
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| self.object_settings(ui, &id));
            if !open {
                self.desk.close_object_window();
            }
        }
        self.palette_window(ctx);
        if let Some(prompt) = self.prompt {
            let title = match prompt {
                Prompt::Open => "Open",
                Prompt::Save => "Save blueprint",
                Prompt::Export => "Export PNG",
            };
            let mut open = true;
            egui::Window::new(title).open(&mut open).resizable(true).show(ctx, |ui| {
                ui.label("Path");
                let path_edit = ui.text_edit_singleline(&mut self.path_text);
                self.watch_text(path_edit);
                if matches!(prompt, Prompt::Export) {
                    let mut percent = i32::try_from(self.desk.session.export_percent).unwrap_or(100);
                    if ui.add(egui::Slider::new(&mut percent, 1..=100).text("Percent")).changed() {
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
                            let path = if is_vmib { path } else { PathBuf::from(format!("{}.vmib", path.display())) };
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
                .chain(self.desk.session.palettes.iter().map(|palette| palette.name.clone()))
                .enumerate()
            {
                if tab_label(ui, theme, &name, index == tab).clicked() {
                    tab = index;
                }
            }
            if tab_label(ui, theme, "+", false).on_hover_text("A new palette of your own.").clicked() {
                let name = unique_palette_name(&self.desk.session.palettes, "Palette");
                self.desk.session.palettes.push(Palette::new(name, Vec::new()));
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
        let color_tool = color_tool_for(self.desk.tool());
        let current = self.desk.preset_of(color_tool).color;
        ui.add_space(2.0);
        // The swatches. Fourteen pixels, one apart, as many across as fit.
        let mut picked: Option<[u8; 3]> = None;
        let mut remove: Option<usize> = None;
        let cell = 14.0;
        let step = cell + 1.0;
        let across = ((ui.available_width() + 1.0) / step).floor().max(1.0) as usize;
        let rows = palette.colors.len().div_ceil(across).max(1);
        let (grid, _) = ui.allocate_exact_size(egui::vec2(across as f32 * step - 1.0, rows as f32 * step - 1.0), egui::Sense::hover());
        for (index, swatch) in palette.colors.iter().enumerate() {
            let col = (index % across) as f32;
            let row = (index / across) as f32;
            let rect = egui::Rect::from_min_size(
                egui::pos2(grid.min.x + col * step, grid.min.y + row * step),
                egui::vec2(cell, cell),
            );
            let response = ui.interact(rect, ui.id().with(("swatch", tab, index)), egui::Sense::click());
            ui.painter().rect_filled(rect, 0.0, rgb(*swatch));
            if *swatch == current {
                ui.painter().rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0_f32, rgb(theme.ink)));
                ui.painter().rect_stroke(rect.shrink(1.5), 0.0, egui::Stroke::new(1.0_f32, rgb(theme.field)));
            }
            if response.clicked() {
                picked = Some(*swatch);
            }
            if response.secondary_clicked() && user_index.is_some() {
                remove = Some(index);
            }
            let hint = if user_index.is_some() {
                format!("{}  {}\nRight-click removes it.", vmi_core::hex_of(*swatch), index)
            } else {
                format!("{}  {}", vmi_core::hex_of(*swatch), index)
            };
            response.on_hover_text(hint);
        }
        if palette.colors.is_empty() {
            ui.label(egui::RichText::new("Empty. + color adds the current color.").color(rgb(theme.soft)).small());
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
            ui.spacing_mut().interact_size = egui::vec2(22.0, 18.0);
            ui.color_edit_button_srgb(&mut color);
            let mut hex = vmi_core::hex_of(color);
            let response = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(64.0).font(egui::TextStyle::Monospace));
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
            let recent: Vec<[u8; 3]> = self.desk.session.recent_colors.iter().copied().take(12).collect();
            let mut pick = None;
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
            for (index, swatch) in recent.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(cell, cell), egui::Sense::click());
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
        ui.horizontal_wrapped(|ui| {
            match user_index {
                Some(index) => {
                    if ui.small_button("+ color").on_hover_text("Add the current color to this palette.").clicked() {
                        if let Some(user) = self.desk.session.palettes.get_mut(index) {
                            if !user.colors.contains(&current) {
                                user.colors.push(current);
                                self.desk.touch_session();
                            }
                        }
                    }
                    if ui.small_button("Save .gpl").on_hover_text("Write it into the folder below. Aseprite reads it.").clicked() {
                        let path = Path::new(&self.palette_folder).join(format!("{}.gpl", file_stem_for(&palette.name)));
                        match vmi_core::palette::save_file(&palette, &path) {
                            Ok(()) => self.desk.status = format!("Saved {}", path.display()),
                            Err(err) => self.desk.status = err,
                        }
                    }
                    if ui.small_button("Delete").on_hover_text("Remove this palette. Files stay.").clicked()
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
                    if ui.small_button("Copy to mine").on_hover_text("A copy you can edit and save.").clicked() {
                        let name = unique_palette_name(&self.desk.session.palettes, &palette.name);
                        self.desk.session.palettes.push(Palette::new(name, palette.colors.clone()));
                        self.desk.session.palette_tab = presets.len() + self.desk.session.palettes.len() - 1;
                        self.desk.touch_session();
                    }
                    ui.label(egui::RichText::new(format!("{} colors", palette.colors.len())).color(rgb(theme.soft)).small());
                }
            }
        });
        ui.separator();
        // Files. Aseprite's palette folder when there is one.
        ui.label(egui::RichText::new("Folder").color(rgb(theme.soft)).small());
        let folder_edit = ui.add(egui::TextEdit::singleline(&mut self.palette_folder).desired_width(f32::INFINITY));
        if folder_edit.has_focus() {
            self.text_focused = true;
        }
        let files = palette_files(Path::new(&self.palette_folder));
        if files.is_empty() {
            ui.label(egui::RichText::new("No palette files here yet. Save .gpl writes one.").color(rgb(theme.soft)).small());
        }
        let mut load: Option<PathBuf> = None;
        egui::ScrollArea::vertical().id_salt("palette-files").max_height(96.0).show(ui, |ui| {
            for file in &files {
                let name = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let known = self.desk.session.palettes.iter().any(|palette| palette.name == stem_of(file));
                if tab_label(ui, theme, &name, known).on_hover_text("Open as a palette of your own.").clicked() {
                    load = Some(file.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Path").color(rgb(theme.soft)).small());
            let response = ui.add(egui::TextEdit::singleline(&mut self.palette_path).desired_width(160.0));
            if response.has_focus() {
                self.text_focused = true;
            }
            if ui.small_button("Load").clicked() && !self.palette_path.trim().is_empty() {
                load = Some(PathBuf::from(self.palette_path.trim()));
            }
        });
        ui.label(
            egui::RichText::new(format!("Reads {}.", vmi_core::palette::EXTENSIONS.join(" ")))
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
                let slot = self.desk.session.palettes.iter().position(|item| item.name == palette.name);
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
        egui::ScrollArea::vertical().id_salt("tool-grid").show(ui, |ui| {
            self.tool_grid(ui, theme);
        });
    }

    /// Undo, redo, and the settings gear: three cells that together are as
    /// wide as the tool grid.
    fn head_row(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let side = (PANEL_INNER - 2.0) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
            if small_cell(ui, theme, side, false, |painter, rect, ink| paint_bitmap(painter, rect, &icons::UNDO, ink))
                .on_hover_text("Undo    Ctrl+Z    two-finger tap")
                .clicked()
            {
                self.desk.undo();
            }
            if small_cell(ui, theme, side, false, |painter, rect, ink| paint_bitmap(painter, rect, &icons::REDO, ink))
                .on_hover_text("Redo    Ctrl+Shift+Z    three-finger tap")
                .clicked()
            {
                self.desk.redo();
            }
            let tool = self.desk.tool();
            if small_cell(ui, theme, side, self.desk.tool_window == Some(tool), |painter, rect, ink| {
                paint_bitmap(painter, rect, &icons::GEAR, ink)
            })
            .on_hover_text(format!("{}: every setting. Right-click a cell does the same.", tool.name()))
            .clicked()
            {
                self.desk.palette_click(tool, true);
            }
        });
    }

    /// The color cell and the palette cell. Click the color for the picker,
    /// right-click it for the palette window. The palette cell shows the
    /// first colors of the current palette and opens that window.
    fn color_row(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let active = self.desk.tool();
        let color_tool = color_tool_for(active);
        let mut color = self.desk.preset_of(color_tool).color;
        let before = color;
        let height = 22.0;
        let half = (PANEL_INNER - CELL_GAP) / 2.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(CELL_GAP, 0.0);
            ui.spacing_mut().interact_size = egui::vec2(half, height);
            let response = ui.color_edit_button_srgb(&mut color).on_hover_text(if color_tool == active {
                format!("Color {}\nClick: picker. Right-click: palette window.", vmi_core::hex_of(color))
            } else {
                format!("Pen color {}. This tool has none of its own.", vmi_core::hex_of(color))
            });
            if response.secondary_clicked() {
                self.palette_open = !self.palette_open;
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
            // Six by three, five pixels each, centered: the palette in miniature.
            let px = 5.0;
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
        if color != before {
            self.pick_color(color);
        }
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

    /// Set the color of the tool that owns one, and remember it.
    fn pick_color(&mut self, color: [u8; 3]) {
        let tool = color_tool_for(self.desk.tool());
        self.desk.edit_preset(tool, |slot| slot.color = color);
        self.desk.session.note_recent_color(color);
    }

    /// The rotaries. 7 o'clock is 0, the long way around to 5 o'clock is 1.
    fn dials(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let tool = self.desk.tool();
        let fields = tool.fields();
        let shown: Vec<Dial> = Dial::ALL.iter().copied().filter(|dial| fields.contains(&dial.field())).collect();
        if shown.is_empty() {
            return;
        }
        let mut settings = self.desk.preset_of(tool).clone();
        let before = settings.clone();
        let defaults = ToolSettings::default_for(tool);
        // Three dials fill the panel width: 3 × 22 and two 4 px gaps is 74.
        for row in shown.chunks(3) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                for dial in row.iter().copied() {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click_and_drag());
                        let value = dial.get(&settings);
                        paint_dial(ui.painter(), rect, value, &dial.digits(&settings), theme, value > 0.0);
                        if response.drag_started() {
                            self.dial_drag = Some((dial, value));
                        }
                        if response.dragged() {
                            if let Some((which, start)) = self.dial_drag {
                                if which == dial {
                                    // Up raises. The whole sweep is `travel` pixels.
                                    let delta = response.interact_pointer_pos().map(|pos| pos.y).unwrap_or(0.0);
                                    let origin =
                                        ui.input(|input| input.pointer.press_origin()).map(|pos| pos.y).unwrap_or(delta);
                                    dial.set(&mut settings, start + (origin - delta) / dial.travel());
                                }
                            }
                        }
                        if response.drag_stopped() {
                            self.dial_drag = None;
                        }
                        if response.double_clicked() {
                            dial.set(&mut settings, dial.get(&defaults));
                        }
                        let scroll = if response.hovered() { ui.input(|input| input.raw_scroll_delta.y) } else { 0.0 };
                        if scroll != 0.0 {
                            dial.set(&mut settings, value + scroll / (dial.travel() * 2.0));
                        }
                        response.on_hover_text(dial.hint());
                        let label =
                            egui::RichText::new(dial.label()).color(rgb(theme.soft)).font(egui::FontId::monospace(8.0));
                        ui.allocate_ui(egui::vec2(22.0, 10.0), |ui| {
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
        // Five tips across: 5 × 14 and four 1 px gaps is 74. Three edges or
        // three chunks across: 3 × 24 and two gaps.
        let five = egui::vec2((PANEL_INNER - 4.0) / 5.0, 16.0);
        let three = egui::vec2((PANEL_INNER - 2.0) / 3.0, 16.0);
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
        if fields.contains(&Field::Chunk) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                for chunk in [1u32, 2, 4] {
                    if letter_cell(ui, theme, three, &chunk.to_string(), settings.chunk == chunk)
                        .on_hover_text(format!("Chunk {chunk}: dither cells {chunk} px square"))
                        .clicked()
                    {
                        settings.chunk = chunk;
                    }
                }
            });
        }
        if settings != before {
            self.desk.edit_preset(tool, |slot| *slot = settings);
        }
    }

    fn tool_grid(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let tools = palette_matching(&self.desk.query);
        let current = self.desk.tool();
        for section in [Section::Raster, Section::Vector, Section::Picture, Section::Cut] {
            let rows: Vec<Tool> = tools.iter().copied().filter(|tool| tool.section() == section).collect();
            if rows.is_empty() {
                continue;
            }
            if section != Section::Raster {
                ui.add_space(6.0);
                ui.separator();
            }
            // Two across, like the Clip Studio tool palette. A cell is 36 px
            // and its icon is 32 px, two picture pixels per icon pixel.
            let gap = CELL_GAP;
            let cell = egui::vec2(CELL_SIDE, CELL_SIDE);
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for pair in rows.chunks(2) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                    for tool in pair {
                        let response = tool_cell(ui, *tool, cell, theme, *tool == current);
                        if response.clicked() {
                            self.desk.palette_click(*tool, false);
                        }
                        if response.secondary_clicked() {
                            self.desk.palette_click(*tool, true);
                        }
                    }
                });
            }
        }
    }

    fn layers(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        ui.horizontal(|ui| {
            if ui.button("Layer").clicked() {
                self.desk.add_layer();
            }
            if ui.button("Group").clicked() {
                self.desk.add_group();
            }
        });
        ui.label(egui::RichText::new("Front is on top. Right-click a row for its settings.").color(rgb(theme.soft)).small());
        let rows = self.desk.layer_rows();
        let selected = self.desk.selected.clone();
        let mut toggle_animation: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("layer-rows").auto_shrink([false, false]).show(ui, |ui| {
            for row in rows {
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 12.0);
                    let mut visible = row.visible;
                    if ui.checkbox(&mut visible, "").changed() {
                        self.desk.update_layer(&row.id, |node| node.visible = visible);
                    }
                    if row.group {
                        // A, like M, S, and C on the Python rows: this folder is a sequence.
                        if letter_cell(ui, theme, egui::vec2(16.0, 16.0), "A", row.animation)
                            .on_hover_text("Animation: this folder is one sequence. Its layers are its cels.")
                            .clicked()
                        {
                            toggle_animation = Some(row.id.clone());
                        }
                    }
                    let active = selected.as_deref() == Some(row.id.as_str());
                    let prefix = if row.group { "▸ " } else { "" };
                    let suffix = match row.frame {
                        Some(frame) => format!("  f{frame}"),
                        None => String::new(),
                    };
                    let fill = if active { rgb(theme.ink) } else { rgb(theme.row) };
                    let text = if active { rgb(theme.field) } else { rgb(theme.ink) };
                    let response = ui.add_sized(
                        [ui.available_width(), 24.0],
                        egui::Button::new(egui::RichText::new(format!("{prefix}{}{suffix}", row.name)).color(text)).fill(fill),
                    );
                    if response.clicked() {
                        self.desk.select_layer(row.id.clone());
                    }
                    if response.secondary_clicked() {
                        self.desk.open_layer_window(row.id.clone());
                    }
                });
            }
        });
        if let Some(id) = toggle_animation {
            self.desk.toggle_animation(&id);
        }
    }

    fn objects(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        if ui.button("Add object").clicked() {
            self.desk.add_object();
        }
        ui.label(egui::RichText::new("Drag a row to reorder. Right-click for name and kind.").color(rgb(theme.soft)).small());
        let rows: Vec<(String, String, String)> = self
            .desk
            .art
            .objects
            .iter()
            .map(|object| (object.id.clone(), object.name.clone(), object.kind.clone()))
            .collect();
        let mut reorder = None;
        for (index, (id, name, kind)) in rows.iter().enumerate() {
            let response = ui.add_sized(
                [ui.available_width(), 24.0],
                egui::Button::new(format!("{name}  {kind}")),
            );
            if response.secondary_clicked() {
                self.desk.open_object_window(id.clone());
            }
            if response.drag_started() {
                // The drop target is recorded when the pointer is released over another row.
            }
            if let Some(pos) = response.interact_pointer_pos() {
                if ui.input(|input| input.pointer.any_released()) && response.rect.contains(pos) {
                    // handled below with drag payload via memory
                }
            }
            let drag_id = egui::Id::new("object-drag");
            if response.drag_started() {
                ui.memory_mut(|memory| memory.data.insert_temp(drag_id, index));
            }
            if response.hovered() && ui.input(|input| input.pointer.any_released()) {
                let from = ui.memory(|memory| memory.data.get_temp::<usize>(drag_id));
                if let Some(from) = from {
                    if from != index {
                        reorder = Some((from, index));
                    }
                    ui.memory_mut(|memory| memory.data.remove::<usize>(drag_id));
                }
            }
        }
        if let Some((from, to)) = reorder {
            self.desk.reorder_object(from, to);
        }
    }

    /// The strip at the top of an area: its name opens the list of views.
    /// A sheet area carries the transport too.
    fn area_header(&mut self, ui: &mut egui::Ui, area: Area) {
        let theme = self.desk.theme();
        let view = self.areas.get(area);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 2.0);
            let title = egui::RichText::new(view.label()).monospace().small().color(rgb(theme.ink));
            ui.menu_button(title, |ui| {
                for candidate in View::ALL {
                    if ui.selectable_label(candidate == view, candidate.label()).clicked() {
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
        if letter_cell(ui, theme, cell, "<", false).on_hover_text("Back one frame    ,    (W: first frame)").clicked() {
            self.desk.step_frame(-1);
        }
        let playing = self.desk.playing;
        if letter_cell(ui, theme, word, if playing { "STOP" } else { "PLAY" }, playing)
            .on_hover_text("Play the cut at its frame rate    Shift+Space")
            .clicked()
        {
            self.desk.toggle_play();
        }
        if letter_cell(ui, theme, cell, ">", false).on_hover_text("Forward one frame    .    (Shift+W: last frame)").clicked() {
            self.desk.step_frame(1);
        }
        if letter_cell(ui, theme, word, "LOOP", self.desk.loop_play)
            .on_hover_text("Play returns to the first frame. Off stops on the last.")
            .clicked()
        {
            self.desk.toggle_loop();
        }
        if letter_cell(ui, theme, word, "SKIN", self.desk.art.onion)
            .on_hover_text("Onion skin: three back in purple, three ahead in green    O")
            .clicked()
        {
            self.desk.toggle_onion();
        }
        let counter = format!("{:03}/{:03}", self.desk.playhead() + 1, self.desk.cut_end());
        ui.label(egui::RichText::new(counter).monospace().small().color(rgb(theme.ink)));
        let mut fps = self.desk.art.fps.clamp(1, 60);
        let response = ui.add(egui::DragValue::new(&mut fps).range(1..=60).speed(0.1).prefix("FPS "));
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
        let shown = if self.sheet_auto[index] { "AUTO" } else { mode.label() };
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
                let acts = sheet::show(ui, &self.desk, &mut self.sheet_state, mode, theme);
                for act in acts {
                    self.apply(act);
                }
            }
        }
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::SetFrame(frame) => self.desk.set_playhead(frame),
            Act::Pick { folder, frame } => self.desk.pick_cel(&folder, frame),
            Act::Range { folder, start, count } => self.desk.pick_range(&folder, start, count),
            Act::MoveCel { folder, cel, start } => self.desk.move_cel(&folder, &cel, start),
            Act::NewCel { folder, frame } => self.desk.new_cel(Some(&folder), Some(frame)),
            Act::DeleteCel { folder, cel } => self.desk.delete_cel(&folder, &cel),
            Act::Command { folder, name, start, count, arg } => self.desk.sheet_command(&folder, name, start, count, arg),
            Act::NewFrame => self.desk.new_cel(None, None),
            Act::AnimateSelected => self.desk.animate_selected(),
        }
    }

    fn picture_bar(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        ui.horizontal_wrapped(|ui| {
            let tool = self.desk.tool();
            ui.label(egui::RichText::new(tool.name()).color(rgb(theme.ink)).strong());
            ui.label(egui::RichText::new(tool.usage()).color(rgb(theme.soft)));
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
                egui::Event::Touch { phase: egui::TouchPhase::End | egui::TouchPhase::Cancel, .. } => Some(1.0),
                egui::Event::Touch { force: Some(force), .. } => Some(*force),
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
            if let egui::Event::Touch { id, phase, pos, force, .. } = event {
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
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [self.desk.art.width as usize, self.desk.art.height as usize],
                    &pixels,
                );
                match &mut self.texture {
                    Some(texture) => texture.set(image, texture_options()),
                    None => self.texture = Some(ctx.load_texture("plate", image, texture_options())),
                }
            }
            Some(Display::Part { x, y, w, h, rgba }) => {
                if let Some(texture) = &mut self.texture {
                    let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    texture.set_partial([x as usize, y as usize], image, texture_options());
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
        let navigating = response.hovered() && (space || middle || (shift && response.dragged()));
        if navigating && (space || middle) && response.dragged() {
            let delta = ui.input(|input| input.pointer.delta());
            self.desk.pan_by(delta.x, delta.y);
            self.rotate_from = None;
        } else if shift && response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let center = response.rect.center();
                if let Some(prev) = self.rotate_from {
                    let a0 = (prev.y - center.y).atan2(prev.x - center.x);
                    let a1 = (pos.y - center.y).atan2(pos.x - center.x);
                    self.desk.rotate(a1 - a0);
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
                    let (x, y) = doc_at(pos);
                    self.desk.down_doc(x, y, Button::Secondary);
                }
            } else if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = doc_at(pos);
                    self.desk.down_doc(x, y, Button::Primary);
                    self.desk.up_doc(x, y);
                }
            } else if response.drag_started() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = doc_at(pos);
                    self.desk.down_doc(x, y, Button::Primary);
                }
            }
            let painting = response.dragged() && !shift && !space && !middle && !self.fingers.many();
            if painting {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = doc_at(pos);
                    self.desk.move_doc(x, y);
                }
            }
            if response.drag_stopped() && !shift && !space {
                if let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = doc_at(pos);
                    self.desk.up_doc(x, y);
                }
            }
        }
    }
}

/// A tool cell is 36 px; two across with a 2 px gap is 74. That is the panel.
const CELL_SIDE: f32 = 36.0;
const CELL_GAP: f32 = 2.0;
const PANEL_INNER: f32 = CELL_SIDE * 2.0 + CELL_GAP;
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
    let measure = |candidate: &str| ui.fonts(|fonts| fonts.layout_no_wrap(candidate.to_owned(), font.clone(), egui::Color32::WHITE).size().x);
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
fn project_tab(ui: &mut egui::Ui, theme: &Theme, name: &str, selected: bool, with_close: bool) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let close_width = if with_close { TAB_CLOSE.x } else { TAB_PAD_LEFT };
    let room = TAB_MAX_WIDTH - TAB_PAD_LEFT - close_width;
    let text = elide_right(ui, &font, name, room);
    let text_width = ui.fonts(|fonts| fonts.layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE).size().x);
    let width = (TAB_PAD_LEFT + text_width + close_width).min(TAB_MAX_WIDTH);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::click());
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
fn project_tab_with_close(ui: &mut egui::Ui, theme: &Theme, name: &str, selected: bool) -> (egui::Response, egui::Response) {
    let tab = project_tab(ui, theme, name, selected, true);
    let rect = tab.rect;
    let mark_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - TAB_CLOSE.x, rect.center().y - TAB_CLOSE.y / 2.0),
        TAB_CLOSE,
    );
    let mark = ui.interact(mark_rect, tab.id.with("close"), egui::Sense::click());
    let lit = mark.hovered() || mark.is_pointer_button_down_on();
    let color = rgb(if lit { theme.ink } else { theme.quiet });
    let stroke = egui::Stroke::new(1.0_f32, color);
    let origin = mark_rect.min;
    let painter = ui.painter();
    painter.line_segment([origin + egui::vec2(6.0, 3.0), origin + egui::vec2(11.0, 8.0)], stroke);
    painter.line_segment([origin + egui::vec2(11.0, 3.0), origin + egui::vec2(6.0, 8.0)], stroke);
    (tab, mark)
}

fn tab_label(ui: &mut egui::Ui, theme: &Theme, text: &str, chosen: bool) -> egui::Response {
    let color = if chosen { rgb(theme.ink) } else { rgb(theme.soft) };
    let label = egui::Label::new(egui::RichText::new(text).monospace().color(color)).sense(egui::Sense::click());
    let response = ui.add(label);
    if chosen {
        let rect = response.rect;
        let y = rect.bottom() + 1.0;
        ui.painter().line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], egui::Stroke::new(1.0_f32, rgb(theme.ink)));
    } else if response.hovered() {
        ui.painter().rect_filled(response.rect.expand(1.0), 0.0, rgb(theme.frame));
        ui.painter().text(response.rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::monospace(ui.text_style_height(&egui::TextStyle::Monospace) * 0.75), rgb(theme.ink));
    }
    response
}

fn unique_palette_name(palettes: &[Palette], base: &str) -> String {
    if !palettes.iter().any(|palette| palette.name.eq_ignore_ascii_case(base)) {
        return base.to_string();
    }
    let mut number = 2;
    loop {
        let candidate = format!("{base} {number}");
        if !palettes.iter().any(|palette| palette.name.eq_ignore_ascii_case(&candidate)) {
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
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    if stem.is_empty() {
        "palette".into()
    } else {
        stem
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Aseprite's palette folder when it exists, else the desk's own.
fn palette_folder() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut candidates = vec![home.join(".config").join("aseprite").join("palettes")];
    candidates.push(home.join("Library").join("Application Support").join("Aseprite").join("palettes"));
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
                    .map(|ext| vmi_core::palette::EXTENSIONS.contains(&ext.to_string_lossy().to_ascii_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

/// The tool whose color the color cell edits: this one, or the pen.
fn color_tool_for(active: Tool) -> Tool {
    if active.fields().contains(&Field::Color) {
        active
    } else {
        Tool::Pen
    }
}

fn tool_cell(ui: &mut egui::Ui, tool: Tool, cell: egui::Vec2, theme: &Theme, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::click());
    let fill = if active {
        rgb(theme.ink)
    } else if response.hovered() {
        rgb(theme.frame)
    } else {
        rgb(theme.row)
    };
    let ink = if active { rgb(theme.field) } else { rgb(theme.ink) };
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, rgb(theme.frame)));
    paint_icon(&painter, rect, tool, ink);
    let shortcut = tool.shortcut();
    let title = if shortcut.is_empty() { tool.name().to_string() } else { format!("{}  [{shortcut}]", tool.name()) };
    response.on_hover_text(format!("{title}\n{}\nRight-click: settings", tool.usage()))
}

/// Whole-pixel scaling keeps the icon crisp. The bitmap is centered in the cell.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, tool: Tool, color: egui::Color32) {
    let side = icons::SIDE as f32;
    let px = ((rect.width().min(rect.height()) - 4.0) / side).floor().max(1.0);
    let size = px * side;
    let origin = egui::pos2(
        (rect.center().x - size * 0.5).round(),
        (rect.center().y - size * 0.5).round(),
    );
    for (col, row) in icons::cells(icons::icon(tool)) {
        let min = egui::pos2(origin.x + col as f32 * px, origin.y + row as f32 * px);
        painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(px, px)), 0.0, color);
    }
}

fn tip_row(ui: &mut egui::Ui, settings: &mut ToolSettings) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Tip");
        for tip in Tip::all() {
            if ui.selectable_label(settings.tip == tip, tip.label()).on_hover_text(tip.usage()).clicked() {
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
    let ink = if lit { rgb(theme.field) } else { rgb(theme.ink) };
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0_f32, rgb(theme.frame)));
    draw(&painter, rect, ink);
    response
}

fn letter_cell(ui: &mut egui::Ui, theme: &Theme, size: egui::Vec2, letter: &str, lit: bool) -> egui::Response {
    let text = letter.to_string();
    sized_cell(ui, theme, size, lit, move |painter, rect, ink| {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::monospace(10.0), ink);
    })
}

fn paint_bitmap(painter: &egui::Painter, rect: egui::Rect, bitmap: &icons::Icon, color: egui::Color32) {
    let side = icons::SIDE as f32;
    let px = ((rect.width().min(rect.height()) - 4.0) / side).floor().max(1.0);
    let size = px * side;
    let origin = egui::pos2((rect.center().x - size * 0.5).round(), (rect.center().y - size * 0.5).round());
    for (col, row) in icons::cells(bitmap) {
        let min = egui::pos2(origin.x + col as f32 * px, origin.y + row as f32 * px);
        painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(px, px)), 0.0, color);
    }
}

/// 7 o'clock at 0, the long way around to 5 o'clock at 1. Same sweep as Thrash Tracker.
fn dial_angle(amount: f32) -> f32 {
    let span = amount.clamp(0.0, 1.0);
    2.0 * std::f32::consts::PI / 3.0 + span * 5.0 * std::f32::consts::PI / 3.0
}

/// A rotary. The bump rides the rim. Two hex digits sit in the middle.
fn paint_dial(painter: &egui::Painter, rect: egui::Rect, amount: f32, digits: &str, theme: &Theme, lit: bool) {
    let color = if lit { rgb(theme.ink) } else { rgb(theme.quiet) };
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5 - 1.0;
    painter.circle_stroke(center, radius, egui::Stroke::new(1.0_f32, color));
    let angle = dial_angle(amount);
    let bump = egui::pos2((center.x + angle.cos() * radius).round(), (center.y + angle.sin() * radius).round());
    painter.rect_filled(egui::Rect::from_center_size(bump, egui::vec2(3.0, 3.0)), 0.0, color);
    painter.text(center, egui::Align2::CENTER_CENTER, digits, egui::FontId::monospace(8.0), color);
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
        let color = if mark.hot { rgb(theme.ink) } else { rgb(theme.soft) };
        painter.circle_stroke(egui::pos2(sx, sy), 4.0, egui::Stroke::new(1.0_f32, color));
    }
    if desk.cut_points.len() >= 2 {
        let mut last = None;
        for point in &desk.cut_points {
            let (sx, sy) = desk.camera.to_screen(frame, width, height, point[0], point[1]);
            if let Some((px, py)) = last {
                painter.line_segment([egui::pos2(px, py), egui::pos2(sx, sy)], egui::Stroke::new(1.0_f32, rgb(theme.ink)));
            }
            last = Some((sx, sy));
        }
    }
}

fn edit_field(ui: &mut egui::Ui, tool: Tool, field: Field, settings: &mut ToolSettings, theme: &Theme, typing: &mut bool) {
    match field {
        Field::ReleaseNote => {
            ui.label(tool.release_note());
        }
        Field::Color => {
            ui.horizontal(|ui| {
                ui.label("Color");
                ui.color_edit_button_srgb(&mut settings.color);
                let mut hex = vmi_core::hex_of(settings.color);
                let response = ui.text_edit_singleline(&mut hex);
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
            ui.add(egui::DragValue::new(&mut settings.size).range(0.5..=256.0).speed(0.25).prefix("Size "));
        }
        Field::Min => {
            ui.add(egui::DragValue::new(&mut settings.size_min).range(0.5..=256.0).speed(0.25).prefix("Min "));
        }
        Field::Max => {
            ui.add(egui::DragValue::new(&mut settings.size_max).range(0.5..=512.0).speed(0.25).prefix("Max "));
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
                    if ui.selectable_label(settings.edge == edge, edge.label()).clicked() {
                        settings.edge = edge;
                    }
                }
            });
        }
        Field::Fringe => {
            ui.add(egui::DragValue::new(&mut settings.fringe).range(0.0..=8.0).speed(0.05).prefix("Fringe "));
        }
        Field::Chunk => {
            ui.horizontal(|ui| {
                ui.label("Chunk");
                for chunk in [1u32, 2, 4] {
                    if ui.selectable_label(settings.chunk == chunk, chunk.to_string()).clicked() {
                        settings.chunk = chunk;
                    }
                }
            });
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
            if ui.add(egui::DragValue::new(&mut seed).range(0..=u32::MAX).prefix("Seed ")).changed() {
                settings.seed = u64::from(seed);
            }
        }
        Field::Closed => {
            ui.checkbox(&mut settings.closed, "Closed");
        }
        Field::BoneReach => {
            ui.add(egui::DragValue::new(&mut settings.bone_reach).range(8.0..=800.0).speed(1.0).prefix("Bone reach "));
        }
        Field::FillTolerance => {
            let mut tolerance = i32::from(settings.fill_tolerance);
            if ui.add(egui::Slider::new(&mut tolerance, 0..=255).text("Fill tolerance")).changed() {
                settings.fill_tolerance = tolerance as u8;
            }
        }
        Field::PickRadius => {
            ui.add(egui::DragValue::new(&mut settings.pick_radius).range(2.0..=64.0).prefix("Pick radius "));
        }
    }
    let _ = theme;
}

fn curve_editor(ui: &mut egui::Ui, label: &str, curve: &mut Vec<(f32, f32)>) {
    ui.label(label);
    for point in curve.iter_mut() {
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut point.0).range(0.0..=1.0).speed(0.01).prefix("t "));
            ui.add(egui::DragValue::new(&mut point.1).range(0.0..=1.0).speed(0.01).prefix("v "));
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

fn texture_options() -> egui::TextureOptions {
    match PICTURE_FILTER {
        PictureFilter::Nearest => egui::TextureOptions::NEAREST,
    }
}

fn rgb(color: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(color[0], color[1], color[2])
}

fn apply_theme(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = if theme.field[0] > 180 {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    let ink = rgb(theme.ink);
    let field = rgb(theme.field);
    visuals.panel_fill = rgb(theme.gutter);
    visuals.window_fill = rgb(theme.row);
    visuals.extreme_bg_color = field;
    visuals.faint_bg_color = rgb(theme.grid);
    visuals.override_text_color = Some(ink);
    let stroke = egui::Stroke::new(1.0_f32, ink);
    visuals.widgets.noninteractive.bg_fill = rgb(theme.gutter);
    visuals.widgets.noninteractive.fg_stroke = stroke;
    visuals.widgets.inactive.bg_fill = rgb(theme.row);
    visuals.widgets.inactive.fg_stroke = stroke;
    visuals.widgets.hovered.bg_fill = rgb(theme.frame);
    visuals.widgets.hovered.fg_stroke = stroke;
    visuals.widgets.active.bg_fill = rgb(theme.frame);
    visuals.widgets.active.fg_stroke = stroke;
    visuals.selection.bg_fill = rgb(theme.frame);
    visuals.selection.stroke = stroke;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, rgb(theme.frame));
    // K-OS chrome: square corners, no shadows, no hover swell.
    visuals.window_rounding = 0.0.into();
    visuals.menu_rounding = 0.0.into();
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.rounding = 0.0.into();
        widget.expansion = 0.0;
    }
    ctx.set_visuals(visuals);
}

/// The house face when the font cache has it, else one mono face for all
/// the chrome. The Python app unpacks faces into `~/.cache/vmi-studio/fonts`.
fn install_fonts(ctx: &egui::Context) {
    install_face(ctx, vmi_core::fonts::house_bytes().or_else(house_font));
}

fn install_face(ctx: &egui::Context, named: Option<(String, Vec<u8>)>) {
    let mut fonts = egui::FontDefinitions::default();
    let mono = fonts.families.get(&egui::FontFamily::Monospace).cloned().unwrap_or_default();
    match named {
        Some((name, bytes)) => {
            fonts.font_data.insert(name.clone(), egui::FontData::from_owned(bytes));
            let mut stack = vec![name];
            stack.extend(mono);
            fonts.families.insert(egui::FontFamily::Monospace, stack.clone());
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
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)?;
    let folder = home.join(".cache").join("vmi-studio").join("fonts");
    let entries: Vec<PathBuf> = std::fs::read_dir(folder).ok()?.flatten().map(|entry| entry.path()).collect();
    let face = |wanted: &[&str]| {
        entries.iter().find(|path| {
            let name = path.file_name().map(|name| name.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            (name.ends_with(".ttf") || name.ends_with(".otf")) && wanted.iter().all(|part| name.contains(part))
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
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(self.desk.next_frame_in().max(0.001)));
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
            .frame(egui::Frame::none().fill(rgb(self.desk.theme().field)).inner_margin(egui::Margin {
                left: 8.0,
                right: 8.0,
                top: 2.0,
                bottom: 0.0,
            }))
            .show(ctx, |ui| self.tab_bar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if !self.desk.on_browser {
                    ui.label(self.desk.art.file_name.as_str());
                    ui.separator();
                }
                ui.label(self.desk.status.as_str());
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
                self.area_header(ui, Area::Right);
                self.area_body(ui, ctx, Area::Right);
            });
        // The sheet sits under the picture, between the side panes.
        let bottom = egui::TopBottomPanel::bottom("bottom-area")
            .resizable(true)
            .default_height(self.desk.session.bottom_height)
            .height_range(48.0..=520.0)
            .show(ctx, |ui| {
                self.area_header(ui, Area::Bottom);
                self.area_body(ui, ctx, Area::Bottom);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.areas.center != View::Picture {
                self.area_header(ui, Area::Center);
            }
            self.area_body(ui, ctx, Area::Center);
        });
        self.desk.note_widths(left.response.rect.width(), brushes.response.rect.width());
        self.desk.note_areas(right.response.rect.width(), bottom.response.rect.height(), &self.areas.ids());
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
        assert_eq!(Areas::from_ids(&["layers".into(), "layers".into(), "film".into(), "xsheet".into()]), Areas::default());
        let kept = Areas::from_ids(&areas.ids());
        assert_eq!(kept, areas);
        assert_eq!(color_tool_for(Tool::Pen), Tool::Pen);
        assert_eq!(file_stem_for("DB 32 / mine"), "DB-32---mine");
        assert_eq!(unique_palette_name(&[Palette::new("Mine", vec![])], "Mine"), "Mine 2");
    }
}
