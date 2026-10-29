//! Desk session. This file is separate from the Python session so the two
//! programs do not overwrite each other.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::palette::Palette;
use crate::tool::{Tool, ToolSettings};

/// How many picked colors the session remembers. RGBA is four bytes, so
/// this list is a few kilobytes in the session file. It is the desk's list,
/// one for every project: a clip file is never written back.
pub const RECENT_COLORS: usize = 512;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub theme: String,
    pub tool: Tool,
    pub presets: Vec<ToolSettings>,
    pub path: String,
    pub layers_width: f32,
    pub brush_width: f32,
    pub export_percent: u32,
    /// Palettes the user added. The palette window lists them after the presets.
    #[serde(default)]
    pub palettes: Vec<Palette>,
    /// The open palette tab, counting the presets first and then `palettes`.
    #[serde(default)]
    pub palette_tab: usize,
    /// Colors picked lately, most recent first. At most `RECENT_COLORS`.
    /// Each entry is R, G, B, A. Older sessions stored three channels.
    #[serde(default, deserialize_with = "de_recent_colors")]
    pub recent_colors: Vec<[u8; 4]>,
    /// The right area's width.
    #[serde(default = "default_right_width")]
    pub right_width: f32,
    /// The bottom area's height.
    #[serde(default = "default_bottom_height")]
    pub bottom_height: f32,
    /// The view in each area: left, center, right, bottom. Empty means the defaults.
    #[serde(default)]
    pub areas: Vec<String>,
    /// The UI face id. `monospace` is the house stack.
    #[serde(default = "default_font")]
    pub font: String,
    /// Onion skin colour for cels behind the playhead. A view, not part of a drawing.
    #[serde(default = "default_onion_past")]
    pub onion_past_color: [u8; 3],
    /// Onion skin colour for cels ahead of the playhead.
    #[serde(default = "default_onion_future")]
    pub onion_future_color: [u8; 3],
}

fn default_onion_past() -> [u8; 3] {
    crate::anim::PAST
}

fn default_onion_future() -> [u8; 3] {
    crate::anim::FUTURE
}

fn default_font() -> String {
    "monospace".into()
}

fn default_right_width() -> f32 {
    236.0
}

fn default_bottom_height() -> f32 {
    132.0
}

impl Default for Session {
    fn default() -> Self {
        Self {
            theme: "studio".into(),
            tool: Tool::Pen,
            presets: ToolSettings::rack(),
            path: String::new(),
            layers_width: 280.0,
            brush_width: 124.0,
            export_percent: 100,
            palettes: Vec::new(),
            palette_tab: 0,
            recent_colors: Vec::new(),
            right_width: default_right_width(),
            bottom_height: default_bottom_height(),
            areas: Vec::new(),
            font: default_font(),
            onion_past_color: default_onion_past(),
            onion_future_color: default_onion_future(),
        }
    }
}

impl Session {
    /// Moves `color` to the front of the recent list, dropping the oldest past `RECENT_COLORS`.
    /// False when it was already the newest.
    pub fn note_recent_color(&mut self, color: [u8; 4]) -> bool {
        if self.recent_colors.first() == Some(&color) {
            return false;
        }
        self.recent_colors.retain(|known| *known != color);
        self.recent_colors.insert(0, color);
        self.recent_colors.truncate(RECENT_COLORS);
        true
    }
}

/// Three-channel history from older sessions gains alpha `FF`.
fn de_recent_colors<'de, D>(deserializer: D) -> Result<Vec<[u8; 4]>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Visit;
    impl<'de> Visitor<'de> for Visit {
        type Value = Vec<[u8; 4]>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a list of RGB or RGBA colors")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some(channels) = seq.next_element::<Vec<u8>>()? {
                let color = match channels.len() {
                    3 => [channels[0], channels[1], channels[2], 255],
                    4 => [channels[0], channels[1], channels[2], channels[3]],
                    _ => {
                        return Err(de::Error::invalid_length(
                            channels.len(),
                            &"3 or 4 channels",
                        ))
                    }
                };
                out.push(color);
            }
            Ok(out)
        }
    }
    deserializer.deserialize_seq(Visit)
}

pub fn session_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".cache")
        .join("vmi-studio")
        .join("rust-desk.json")
}

pub fn load() -> Session {
    load_from(&session_path()).unwrap_or_default()
}

pub fn save(session: &Session) -> Result<(), String> {
    save_to(&session_path(), session)
}

pub fn load_from(path: &Path) -> Result<Session, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut session: Session = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    session.presets = ToolSettings::merge(session.presets);
    Ok(session)
}

pub fn save_to(path: &Path, session: &Session) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let text = serde_json::to_string_pretty(session).map_err(|err| err.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, text).map_err(|err| err.to_string())?;
    fs::rename(&temp, path).map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn a_session_round_trips_the_active_tool() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-session-{stamp}.json"));
        let mut session = Session::default();
        session.tool = Tool::Dither;
        session
            .presets
            .iter_mut()
            .find(|preset| preset.tool == Tool::Dither)
            .unwrap()
            .chunk = 4;
        save_to(&path, &session).unwrap();
        let loaded = load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.tool, Tool::Dither);
        assert_eq!(
            loaded
                .presets
                .iter()
                .find(|preset| preset.tool == Tool::Dither)
                .unwrap()
                .chunk,
            4
        );
        assert!(loaded
            .presets
            .iter()
            .any(|preset| preset.tool == Tool::Vector));
    }

    #[test]
    fn an_old_session_without_palette_keys_still_loads() {
        let mut old = serde_json::to_value(Session::default()).unwrap();
        let fields = old.as_object_mut().unwrap();
        fields.remove("palettes");
        fields.remove("palette_tab");
        fields.remove("recent_colors");
        fields.remove("font");
        fields.remove("onion_past_color");
        fields.remove("onion_future_color");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-session-old-{stamp}.json"));
        fs::write(&path, old.to_string()).unwrap();
        let loaded = load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert!(loaded.palettes.is_empty());
        assert_eq!(loaded.palette_tab, 0);
        assert!(loaded.recent_colors.is_empty());
        assert_eq!(loaded.font, "monospace");
        assert_eq!(loaded.onion_past_color, crate::anim::PAST);
        assert_eq!(loaded.onion_future_color, crate::anim::FUTURE);
    }

    #[test]
    fn palettes_and_recent_colors_round_trip() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-session-palette-{stamp}.json"));
        let mut session = Session::default();
        session
            .palettes
            .push(Palette::new("Mine", vec![[1, 2, 3], [4, 5, 6]]));
        session.palette_tab = 3;
        session.note_recent_color([9, 9, 9, 128]);
        save_to(&path, &session).unwrap();
        let loaded = load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.palettes, session.palettes);
        assert_eq!(loaded.palette_tab, 3);
        assert_eq!(loaded.recent_colors, vec![[9, 9, 9, 128]]);
    }

    #[test]
    fn an_old_three_channel_history_gains_full_alpha() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-session-rgb-{stamp}.json"));
        let mut value = serde_json::to_value(Session::default()).unwrap();
        value["recent_colors"] = serde_json::json!([[1, 2, 3], [4, 5, 6, 7]]);
        fs::write(&path, value.to_string()).unwrap();
        let loaded = load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.recent_colors, vec![[1, 2, 3, 255], [4, 5, 6, 7]]);
    }

    #[test]
    fn recent_colors_move_to_the_front_and_stay_capped() {
        let mut session = Session::default();
        for step in 0..(RECENT_COLORS + 20) {
            let channel = (step % 256) as u8;
            let high = (step / 256) as u8;
            session.note_recent_color([channel, high, 0, 255]);
        }
        assert_eq!(session.recent_colors.len(), RECENT_COLORS);
        let newest = RECENT_COLORS + 19;
        assert_eq!(
            session.recent_colors[0],
            [(newest % 256) as u8, (newest / 256) as u8, 0, 255]
        );
        let oldest = newest - (RECENT_COLORS - 1);
        assert_eq!(
            session.recent_colors[RECENT_COLORS - 1],
            [(oldest % 256) as u8, (oldest / 256) as u8, 0, 255]
        );
        let again = [10, 0, 0, 255];
        assert!(session.note_recent_color(again));
        assert_eq!(session.recent_colors.len(), RECENT_COLORS);
        assert_eq!(session.recent_colors[0], again);
        assert_eq!(
            session
                .recent_colors
                .iter()
                .filter(|color| **color == again)
                .count(),
            1
        );
        assert!(!session.note_recent_color(again));
    }
}
