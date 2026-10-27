//! The New picture dialog. A port of `vmi_studio/newpage.py`.
//!
//! A preset fills the size. Width and height stay editable. Paper is the back
//! layer. A texture path fills Paper instead of the flat color. The 2kool grid
//! is added only for the 3840 template. The dialog returns the choice; the
//! desk builds the picture.

use std::path::PathBuf;

use eframe::egui;

use vmi_core::newpage::{self, GRID_PRESET, PAPER_CHOICES, PAPER_WHITE};
use vmi_core::Theme;

/// What Create hands the desk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPageChoice {
    pub width: u32,
    pub height: u32,
    pub background: String,
    pub texture: Option<PathBuf>,
    pub grid: bool,
}

pub enum NewPageAct {
    None,
    Create(NewPageChoice),
    Cancel,
}

/// The preset list, the two size fields, paper, and the texture path.
pub struct NewPage {
    open: bool,
    row: Option<usize>,
    width: u32,
    height: u32,
    paper: String,
    paper_text: String,
    texture: String,
    error: String,
}

impl Default for NewPage {
    fn default() -> Self {
        Self::new()
    }
}

impl NewPage {
    pub fn new() -> Self {
        let mut page = Self {
            open: false,
            row: Some(0),
            width: 1920,
            height: 1080,
            paper: PAPER_WHITE.into(),
            paper_text: PAPER_WHITE.into(),
            texture: String::new(),
            error: String::new(),
        };
        page.reset();
        page
    }

    /// Show the dialog at the Python defaults: the first preset, white paper, no texture.
    pub fn open(&mut self) {
        self.reset();
        self.open = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn reset(&mut self) {
        let (_, width, height) = newpage::PRESETS[0];
        self.row = Some(0);
        self.width = width;
        self.height = height;
        self.paper = PAPER_WHITE.into();
        self.paper_text = PAPER_WHITE.into();
        self.texture.clear();
        self.error.clear();
    }

    /// Paint the dialog. `typing` is set while a text field has focus.
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, typing: &mut bool) -> NewPageAct {
        if !self.open {
            return NewPageAct::None;
        }
        let mut act = NewPageAct::None;
        let mut open = true;
        egui::Window::new("New picture")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(520.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(520.0);
                ui.label(egui::RichText::new(
                    "A preset fills the size. Type any width and height. Paper is the back layer. Draw on Ink. 2kool grid is the 3840 template.",
                ).color(rgb(theme.quiet)));
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        egui::ScrollArea::vertical().max_height(240.0).max_width(220.0).id_salt("new-picture-presets").show(ui, |ui| {
                            ui.set_min_width(200.0);
                            for (index, (name, wide, high)) in newpage::PRESETS.iter().enumerate() {
                                if ui.selectable_label(self.row == Some(index), format!("{name}    {wide}×{high}")).clicked() {
                                    self.row = Some(index);
                                    self.width = *wide;
                                    self.height = *high;
                                    self.error.clear();
                                }
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("WIDTH").color(rgb(theme.quiet)));
                        if ui.add(egui::DragValue::new(&mut self.width).range(newpage::MIN_EDGE..=newpage::MAX_EDGE).speed(1.0)).changed() {
                            self.sync_row();
                        }
                        ui.label(egui::RichText::new("HEIGHT").color(rgb(theme.quiet)));
                        if ui.add(egui::DragValue::new(&mut self.height).range(newpage::MIN_EDGE..=newpage::MAX_EDGE).speed(1.0)).changed() {
                            self.sync_row();
                        }
                        ui.label(egui::RichText::new("PAPER").color(rgb(theme.quiet)));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (label, color) in PAPER_CHOICES {
                                let on = self.paper.eq_ignore_ascii_case(color);
                                if ui.selectable_label(on, *label).on_hover_text(*color).clicked() {
                                    self.paper = (*color).to_string();
                                    self.paper_text = (*color).to_string();
                                }
                            }
                        });
                        let hex = ui.add(egui::TextEdit::singleline(&mut self.paper_text).desired_width(84.0).hint_text(PAPER_WHITE));
                        if hex.has_focus() {
                            *typing = true;
                        }
                        if hex.lost_focus() {
                            self.commit_paper();
                        }
                        hex.on_hover_text("Paper color. White, grey, black, or any hex.");
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(egui::RichText::new("Texture").color(rgb(theme.quiet)))
                                .on_hover_text("A picture fills Paper instead of the flat color.");
                            let path = ui.add(
                                egui::TextEdit::singleline(&mut self.texture)
                                    .desired_width(180.0)
                                    .hint_text("Flat color"),
                            );
                            if path.has_focus() {
                                *typing = true;
                            }
                            if ui.button("Clear").clicked() {
                                self.texture.clear();
                            }
                        });
                        ui.label(egui::RichText::new(count_label(self.width, self.height)).color(rgb(theme.soft)));
                        if !self.error.is_empty() {
                            ui.label(egui::RichText::new(&self.error).color(egui::Color32::WHITE));
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        act = self.create();
                    }
                    if ui.button("Cancel").clicked() {
                        act = NewPageAct::Cancel;
                    }
                });
                if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    act = self.create();
                }
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    act = NewPageAct::Cancel;
                }
            });
        if !open {
            act = NewPageAct::Cancel;
        }
        if !matches!(act, NewPageAct::None) {
            self.open = false;
        }
        act
    }

    fn sync_row(&mut self) {
        self.row = newpage::PRESETS
            .iter()
            .position(|(_, wide, high)| *wide == self.width && *high == self.height);
        self.error.clear();
    }

    fn commit_paper(&mut self) {
        self.paper = newpage::paper_hex(&self.paper_text);
        self.paper_text = self.paper.clone();
    }

    fn create(&mut self) -> NewPageAct {
        self.commit_paper();
        match newpage::parse_plate(i64::from(self.width), i64::from(self.height)) {
            Ok((width, height)) => {
                let texture = {
                    let trimmed = self.texture.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(trimmed))
                    }
                };
                let grid = newpage::match_preset(width, height) == GRID_PRESET;
                NewPageAct::Create(NewPageChoice {
                    width,
                    height,
                    background: self.paper.clone(),
                    texture,
                    grid,
                })
            }
            Err(err) => {
                self.error = err;
                self.open = true;
                NewPageAct::None
            }
        }
    }
}

/// The line under the fields: the preset or Custom, the pixel count, and the grid note.
pub fn count_label(width: u32, height: u32) -> String {
    let pixels = format!("{} pixels", grouped(u64::from(width) * u64::from(height)));
    let name = newpage::match_preset(width, height);
    let mut label = if name.is_empty() {
        format!("Custom    {pixels}")
    } else {
        format!("{name}    {pixels}")
    };
    if name == GRID_PRESET {
        label = format!("{label}    grid on top");
    }
    label
}

fn grouped(value: u64) -> String {
    let digits: Vec<char> = value.to_string().chars().collect();
    let mut out = String::new();
    for (index, ch) in digits.iter().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
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
    fn the_count_line_names_the_preset_and_the_grid() {
        assert_eq!(count_label(1920, 1080), "HD 1080    2,073,600 pixels");
        assert_eq!(count_label(100, 100), "Custom    10,000 pixels");
        assert_eq!(
            count_label(3840, 3840),
            "2kool grid    14,745,600 pixels    grid on top"
        );
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
    }

    #[test]
    fn create_carries_the_grid_only_for_the_3840_plate() {
        let mut page = NewPage::new();
        page.open();
        page.width = 3840;
        page.height = 3840;
        page.paper_text = "not a color".into();
        match page.create() {
            NewPageAct::Create(choice) => {
                assert!(choice.grid);
                assert_eq!(choice.background, PAPER_WHITE);
                assert_eq!(choice.width, 3840);
                assert!(choice.texture.is_none());
            }
            NewPageAct::None | NewPageAct::Cancel => panic!("create"),
        }
        let mut page = NewPage::new();
        page.texture = "  paper.png  ".into();
        match page.create() {
            NewPageAct::Create(choice) => {
                assert!(!choice.grid);
                assert_eq!(choice.texture.unwrap(), PathBuf::from("paper.png"));
            }
            NewPageAct::None | NewPageAct::Cancel => panic!("create"),
        }
    }
}
