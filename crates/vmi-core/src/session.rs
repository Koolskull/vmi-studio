//! Desk session. This file is separate from the Python session so the two
//! programs do not overwrite each other.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tool::{Tool, ToolSettings};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub theme: String,
    pub tool: Tool,
    pub presets: Vec<ToolSettings>,
    pub path: String,
    pub layers_width: f32,
    pub brush_width: f32,
    pub export_percent: u32,
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
        }
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
}
