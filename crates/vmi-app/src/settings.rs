//! Font and theme, as two header dropdowns. A port of `vmi_studio/settingsui.py`.
//!
//! The cluster is the word Settings, then the font menu, then the theme menu.
//! A theme is only chosen. Faces are grouped in the catalog's category order,
//! with a separator between groups.

use eframe::egui;

use vmi_core::fonts::{self, Face, CATEGORY_ORDER};
use vmi_core::THEMES;

pub enum SettingsAct {
    None,
    Font(String),
    Theme(String),
}

/// The header cluster. The face list is read once.
pub struct SettingsBar {
    faces: Vec<Face>,
}

impl Default for SettingsBar {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsBar {
    pub fn new() -> Self {
        Self {
            faces: ordered_faces(fonts::catalog()),
        }
    }

    /// Paint the cluster in a left-to-right row. The caller puts that row at the menu bar's right.
    pub fn show(&mut self, ui: &mut egui::Ui, font_id: &str, theme_id: &str) -> SettingsAct {
        let mut act = SettingsAct::None;
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label("Settings");
        let font_label = self
            .faces
            .iter()
            .find(|face| face.id == font_id)
            .map(|face| face.label.as_str())
            .unwrap_or(font_id);
        egui::ComboBox::from_id_salt("vmi-font-menu")
            .width(168.0)
            .height(320.0)
            .selected_text(font_label)
            .show_ui(ui, |ui| {
                let mut previous: Option<&str> = None;
                for face in &self.faces {
                    if previous.is_some() && previous != Some(face.category.as_str()) {
                        ui.separator();
                    }
                    previous = Some(face.category.as_str());
                    if ui
                        .selectable_label(face.id == font_id, &face.label)
                        .clicked()
                        && face.id != font_id
                    {
                        act = SettingsAct::Font(face.id.clone());
                    }
                }
            });
        let theme_label = THEMES
            .iter()
            .find(|item| item.id == theme_id)
            .map(|item| item.name)
            .unwrap_or(theme_id);
        egui::ComboBox::from_id_salt("vmi-theme-menu")
            .width(120.0)
            .selected_text(theme_label)
            .show_ui(ui, |ui| {
                for item in THEMES {
                    if ui
                        .selectable_label(item.id == theme_id, item.name)
                        .on_hover_text(item.blurb)
                        .clicked()
                        && item.id != theme_id
                    {
                        act = SettingsAct::Theme(item.id.to_string());
                    }
                }
            });
        act
    }
}

/// Faces in category order, then by name. The header draws a separator where the category changes.
pub fn ordered_faces(mut faces: Vec<Face>) -> Vec<Face> {
    let rank = |category: &str| {
        CATEGORY_ORDER
            .iter()
            .position(|name| *name == category)
            .unwrap_or(99)
    };
    faces.sort_by(|left, right| {
        rank(&left.category)
            .cmp(&rank(&right.category))
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });
    faces
}

#[cfg(test)]
mod tests {
    use super::*;
    use vmi_core::fonts::Source;

    fn face(id: &str, label: &str, category: &str) -> Face {
        Face {
            id: id.into(),
            label: label.into(),
            category: category.into(),
            file: format!("{id}.ttf"),
            source: Source::System(id.into()),
        }
    }

    #[test]
    fn faces_group_by_category_then_name() {
        let faces = ordered_faces(vec![
            face("zed", "Zed", "display"),
            face("b", "Beta", "pixel"),
            face("a", "Alpha", "pixel"),
            face("mono", "Mono", "monospace"),
        ]);
        let ids: Vec<&str> = faces.iter().map(|face| face.id.as_str()).collect();
        assert_eq!(ids, ["mono", "a", "b", "zed"]);
    }
}
