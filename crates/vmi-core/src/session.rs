//! Desk session. This file is separate from the Python session so the two
//! programs do not overwrite each other.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::palette::Palette;
use crate::tool::{Tool, ToolSettings};

/// How many picked colors the session remembers.
pub const RECENT_COLORS: usize = 16;

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
    #[serde(default)]
    pub recent_colors: Vec<[u8; 3]>,
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
        }
    }
}

impl Session {
    /// Moves `color` to the front of the recent list, dropping the oldest past `RECENT_COLORS`.
    pub fn note_recent_color(&mut self, color: [u8; 3]) {
        self.recent_colors.retain(|known| *known != color);
        self.recent_colors.insert(0, color);
        self.recent_colors.truncate(RECENT_COLORS);
    }
}

pub fn session_path() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".cache").join("vmi-studio").join("rust-desk.json")
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
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-session-{stamp}.json"));
        let mut session = Session::default();
        session.tool = Tool::Dither;
        session.presets.iter_mut().find(|preset| preset.tool == Tool::Dither).unwrap().chunk = 4;
        save_to(&path, &session).unwrap();
        let loaded = load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.tool, Tool::Dither);
        assert_eq!(loaded.presets.iter().find(|preset| preset.tool == Tool::Dither).unwrap().chunk, 4);
        assert!(loaded.presets.iter().any(|preset| preset.tool == Tool::Vector));
    }

    #[test]
    fn an_old_session_without_palette_keys_still_loads() {
        let mut old = serde_json::to_value(Session::default()).unwrap();
        let fields = old.as_object_mut().unwrap();
        fields.remove("palettes");
        fields.remove("palette_tab");
        fields.remove("recent_colors");
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-session-old-{stamp}.json"));
        fs::write(&path, old.to_string()).unwrap();
        let loaded = load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert!(loaded.palettes.is_empty());
        assert_eq!(loaded.palette_tab, 0);
        assert!(loaded.recent_colors.is_empty());
    }

    #[test]
    fn palettes_and_recent_colors_round_trip() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-session-palette-{stamp}.json"));
        let mut session = Session::default();
        session.palettes.push(Palette::new("Mine", vec![[1, 2, 3], [4, 5, 6]]));
        session.palette_tab = 3;
        session.note_recent_color([9, 9, 9]);
        save_to(&path, &session).unwrap();
        let loaded = load_from(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.palettes, session.palettes);
        assert_eq!(loaded.palette_tab, 3);
        assert_eq!(loaded.recent_colors, vec![[9, 9, 9]]);
    }

    #[test]
    fn recent_colors_move_to_the_front_and_stay_capped() {
        let mut session = Session::default();
        for step in 0..20u8 {
            session.note_recent_color([step, 0, 0]);
        }
        assert_eq!(session.recent_colors.len(), RECENT_COLORS);
        assert_eq!(session.recent_colors[0], [19, 0, 0]);
        assert_eq!(session.recent_colors[RECENT_COLORS - 1], [4, 0, 0]);
        session.note_recent_color([10, 0, 0]);
        assert_eq!(session.recent_colors.len(), RECENT_COLORS);
        assert_eq!(session.recent_colors[0], [10, 0, 0]);
        assert_eq!(session.recent_colors.iter().filter(|color| **color == [10, 0, 0]).count(), 1);
    }
}
