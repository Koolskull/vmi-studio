//! Layers, the tool grid, the viewfinder, and objects.
//! The grid is the toolbox. Color and size sit on that same panel.
//! Left-click selects a tool. Right-click opens every setting for that tool.

use eframe::egui;

use crate::desk::{Button, Desk, Display};
use crate::gpu::{Frame, PictureFilter, PICTURE_FILTER};
use crate::icons;
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prompt {
    Open,
    Save,
    Export,
}

impl StudioApp {
    fn new() -> Self {
        Self {
            desk: Desk::launch(),
            texture: None,
            prompt: None,
            path_text: String::new(),
            rotate_from: None,
            text_focused: false,
        }
    }

    fn watch_text(&mut self, response: egui::Response) -> egui::Response {
        if response.has_focus() {
            self.text_focused = true;
        }
        response
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // egui 0.29 reports any focused widget, including the canvas and the tool
        // cells, as wanting the keyboard. Undo still has to run after a stroke.
        let typing = self.text_focused;
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
        let mut shortcut = String::new();
        ctx.input_mut(|input| {
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
        let before = (name.clone(), opacity, blend.clone(), clip, visible, mute, solo);
        ui.label("Layer");
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
                        Prompt::Open => self.desk.open_path(&path),
                        Prompt::Save => self.desk.save_blueprint(&path),
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

    fn palette(&mut self, ui: &mut egui::Ui) {
        let theme = self.desk.theme();
        ui.label(egui::RichText::new("Tools").color(rgb(theme.ink)).strong());
        ui.horizontal(|ui| {
            if ui.button("Undo").on_hover_text("Ctrl+Z").clicked() {
                self.desk.undo();
            }
            if ui.button("Redo").on_hover_text("Ctrl+Shift+Z").clicked() {
                self.desk.redo();
            }
        });
        self.color_box(ui, theme);
        self.shape_controls(ui, theme);
        if ui.button("All settings").on_hover_text("Every field for this tool. Right-click a cell does the same.").clicked() {
            let tool = self.desk.tool();
            self.desk.palette_click(tool, true);
        }
        let search = ui.add(egui::TextEdit::singleline(&mut self.desk.query).hint_text("Find a tool"));
        self.watch_text(search);
        egui::ScrollArea::vertical().id_salt("tool-grid").show(ui, |ui| {
            self.tool_grid(ui, theme);
        });
    }

    fn color_box(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let active = self.desk.tool();
        let color_tool = if active.fields().contains(&Field::Color) { active } else { Tool::Pen };
        let mut color = self.desk.preset_of(color_tool).color;
        let before = color;
        let label = if color_tool == active { "Color" } else { "Pen color" };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).color(rgb(theme.ink)));
            ui.color_edit_button_srgb(&mut color);
            let mut hex = vmi_core::hex_of(color);
            let response = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(72.0));
            if response.has_focus() {
                self.text_focused = true;
            }
            if response.changed() {
                if let Some(parsed) = parse_hex(&hex) {
                    color = parsed;
                }
            }
        });
        let ink = rgb(theme.ink);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
            for swatch in SWATCHES {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 1.0, rgb(swatch));
                if color == swatch {
                    ui.painter().rect_stroke(rect.shrink(0.5), 1.0, egui::Stroke::new(1.5_f32, ink));
                }
                if response.clicked() {
                    color = swatch;
                }
                response.on_hover_text(vmi_core::hex_of(swatch));
            }
        });
        if color != before {
            self.desk.edit_preset(color_tool, |slot| slot.color = color);
        }
    }

    fn shape_controls(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let tool = self.desk.tool();
        let mut settings = self.desk.preset_of(tool).clone();
        let before = settings.clone();
        let shortcut = tool.shortcut();
        let title = if shortcut.is_empty() { tool.name().to_string() } else { format!("{}   {shortcut}", tool.name()) };
        ui.label(egui::RichText::new(title).strong().color(rgb(theme.ink)));
        ui.label(egui::RichText::new(tool.usage()).small().color(rgb(theme.soft)));
        if tool.fields().contains(&Field::Size) {
            ui.add(egui::Slider::new(&mut settings.size, 0.5..=256.0).text("Size"));
        }
        if tool.fields().contains(&Field::Softness) {
            ui.add(egui::Slider::new(&mut settings.softness, 0.0..=1.0).text("Soft"));
        }
        if tool.fields().contains(&Field::Opacity) {
            ui.add(egui::Slider::new(&mut settings.opacity, 0.0..=1.0).text("Opacity"));
        }
        if tool.fields().contains(&Field::Tip) {
            tip_row(ui, &mut settings);
        }
        if tool.fields().contains(&Field::Texture) {
            ui.add(egui::Slider::new(&mut settings.texture, 0.0..=1.0).text("Texture"));
        }
        if tool.fields().contains(&Field::Edge) {
            ui.horizontal(|ui| {
                ui.label("Edge");
                for edge in Edge::all() {
                    if ui.selectable_label(settings.edge == edge, edge.label()).clicked() {
                        settings.edge = edge;
                    }
                }
            });
        }
        if tool.fields().contains(&Field::Chunk) {
            ui.horizontal(|ui| {
                ui.label("Chunk");
                for chunk in [1u32, 2, 4] {
                    if ui.selectable_label(settings.chunk == chunk, chunk.to_string()).clicked() {
                        settings.chunk = chunk;
                    }
                }
            });
        }
        if tool.fields().contains(&Field::FillTolerance) {
            let mut tolerance = i32::from(settings.fill_tolerance);
            if ui.add(egui::Slider::new(&mut tolerance, 0..=255).text("Tolerance")).changed() {
                settings.fill_tolerance = tolerance as u8;
            }
        }
        if tool.fields().contains(&Field::PickRadius) {
            ui.add(egui::DragValue::new(&mut settings.pick_radius).range(2.0..=64.0).speed(0.25).prefix("Pick "));
        }
        if tool.fields().contains(&Field::BoneReach) {
            ui.add(egui::DragValue::new(&mut settings.bone_reach).range(8.0..=800.0).speed(1.0).prefix("Reach "));
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
            let gap = 3.0;
            let columns = 4.0;
            let side = ((ui.available_width() - gap * (columns - 1.0)) / columns).floor().clamp(34.0, 72.0);
            let cell = egui::vec2(side, side);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                for tool in rows {
                    let response = tool_cell(ui, tool, cell, theme, tool == current);
                    if response.clicked() {
                        self.desk.palette_click(tool, false);
                    }
                    if response.secondary_clicked() {
                        self.desk.palette_click(tool, true);
                    }
                }
            });
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
        for row in rows {
            ui.horizontal(|ui| {
                ui.add_space(row.depth as f32 * 12.0);
                let mut visible = row.visible;
                if ui.checkbox(&mut visible, "").changed() {
                    self.desk.update_layer(&row.id, |node| node.visible = visible);
                }
                let active = selected.as_deref() == Some(row.id.as_str());
                let prefix = if row.group { "▸ " } else { "" };
                let fill = if active { rgb(theme.ink) } else { rgb(theme.row) };
                let text = if active { rgb(theme.field) } else { rgb(theme.ink) };
                let response = ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::Button::new(egui::RichText::new(format!("{prefix}{}", row.name)).color(text)).fill(fill),
                );
                if response.clicked() {
                    self.desk.select_layer(row.id.clone());
                }
                if response.secondary_clicked() {
                    self.desk.open_layer_window(row.id.clone());
                }
            });
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
                1.15_f32.powf(scroll.y / 40.0)
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
            if response.dragged() && !shift && !space && !middle {
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

const SWATCHES: [[u8; 3]; 16] = [
    [0, 0, 0],
    [255, 255, 255],
    [0x44, 0x44, 0x44],
    [0x88, 0x88, 0x88],
    [0xCC, 0xCC, 0xCC],
    [0xE0, 0x20, 0x20],
    [0xFF, 0x8A, 0x00],
    [0xFF, 0xE1, 0x00],
    [0x22, 0xC5, 0x5A],
    [0x14, 0xB8, 0xC4],
    [0x3B, 0x6C, 0xFF],
    [0x8A, 0x3F, 0xF5],
    [0xFF, 0x3E, 0xB8],
    [0xF4, 0xA4, 0xC0],
    [0x8B, 0x45, 0x13],
    [0x00, 0x3A, 0x70],
];

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
    let px = ((rect.width().min(rect.height()) - 6.0) / side).floor().max(1.0);
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
    ctx.set_visuals(visuals);
}

impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, self.desk.theme());
        self.keys(ctx);
        self.text_focused = false;
        self.windows(ctx);
        let layers_default = self.desk.session.layers_width;
        let brush_default = self.desk.session.brush_width;
        let layers = egui::SidePanel::left("layers")
            .resizable(true)
            .default_width(layers_default)
            .width_range(180.0..=480.0)
            .show(ctx, |ui| {
                ui.heading("Layers");
                self.layers(ui);
            });
        let brushes = egui::SidePanel::left("brushes")
            .resizable(true)
            .default_width(brush_default)
            .width_range(200.0..=420.0)
            .show(ctx, |ui| self.palette(ui));
        egui::SidePanel::right("objects")
            .resizable(true)
            .default_width(220.0)
            .width_range(160.0..=420.0)
            .show(ctx, |ui| {
                ui.heading("Objects");
                self.objects(ui);
            });
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        self.desk.new_document();
                    }
                    if ui.button("Open…").clicked() {
                        self.path_text = self.desk.session.path.clone();
                        self.prompt = Some(Prompt::Open);
                    }
                    if ui.button("Save blueprint…").clicked() {
                        self.path_text = if self.desk.session.path.is_empty() {
                            "drawing.vmib".into()
                        } else {
                            self.desk.session.path.clone()
                        };
                        self.prompt = Some(Prompt::Save);
                    }
                    if ui.button("Export PNG…").clicked() {
                        self.path_text = "drawing.png".into();
                        self.prompt = Some(Prompt::Export);
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
                ui.menu_button("Theme", |ui| {
                    for theme in THEMES {
                        if ui.button(theme.name).clicked() {
                            self.desk.set_theme(theme.id);
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Fit").clicked() {
                        self.desk.fit(self.desk.last_frame);
                    }
                    if ui.button("Actual size").clicked() {
                        self.desk.actual();
                    }
                });
            });
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.desk.art.file_name.as_str());
                ui.separator();
                ui.label(self.desk.status.as_str());
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.picture_bar(ui);
            self.picture(ui, ctx);
        });
        self.desk.note_widths(layers.response.rect.width(), brushes.response.rect.width());
        self.desk.flush_session();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.desk.flush_session();
    }
}
