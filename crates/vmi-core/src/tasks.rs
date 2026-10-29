//! Tasks for the drawing that is open. A port of the data side of
//! `vmi_studio/tasks.py`: the rows and the two colors that the blueprint
//! carries. The board that shows them lives in the app.
//!
//! Open task text is the board's text color, white unless the gear changes
//! it. An executed task is struck through in dark grey.

use serde_json::{json, Value};

pub const DONE_GREY: &str = "#3A3A3A";
pub const DEFAULT_BACKGROUND: &str = "#000000";
pub const DEFAULT_TEXT: &str = "#FFFFFF";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub text: String,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskColors {
    pub background: String,
    pub text: String,
}

impl Default for TaskColors {
    fn default() -> Self {
        Self { background: DEFAULT_BACKGROUND.into(), text: DEFAULT_TEXT.into() }
    }
}

/// `#RGB` or `#RRGGBB`, returned as upper-case `#RRGGBB`. Anything else is None.
pub fn clean_hex(value: &str) -> Option<String> {
    let text = value.trim();
    let text = text.strip_prefix('#').unwrap_or(text);
    let hex = |text: &str| text.chars().all(|ch| ch.is_ascii_hexdigit());
    if text.len() == 3 && hex(text) {
        let wide: String = text.chars().flat_map(|ch| [ch, ch]).collect();
        return Some(format!("#{}", wide.to_ascii_uppercase()));
    }
    if text.len() == 6 && hex(text) {
        return Some(format!("#{}", text.to_ascii_uppercase()));
    }
    None
}

/// Blueprint rows. Blank text and repeated ids are dropped.
pub fn pack_tasks(raw: &Value) -> Vec<Task> {
    let Some(rows) = raw.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<Task> = Vec::new();
    for row in rows {
        let id = row.get("id").map(value_text).unwrap_or_default().trim().to_string();
        let text = row.get("text").map(value_text).unwrap_or_default().trim().to_string();
        if id.is_empty() || text.is_empty() || out.iter().any(|task| task.id == id) {
            continue;
        }
        out.push(Task { id, text, done: row.get("done").and_then(Value::as_bool).unwrap_or(false) });
    }
    out
}

/// The same drop rule on rows that are already typed.
pub fn tidy_tasks(tasks: &[Task]) -> Vec<Task> {
    let mut out: Vec<Task> = Vec::new();
    for task in tasks {
        let id = task.id.trim();
        let text = task.text.trim();
        if id.is_empty() || text.is_empty() || out.iter().any(|known| known.id == id) {
            continue;
        }
        out.push(Task { id: id.to_string(), text: text.to_string(), done: task.done });
    }
    out
}

pub fn pack_colors(raw: &Value) -> TaskColors {
    let pick = |name: &str, fallback: &str| {
        raw.get(name).and_then(Value::as_str).and_then(clean_hex).unwrap_or_else(|| fallback.to_string())
    };
    TaskColors { background: pick("background", DEFAULT_BACKGROUND), text: pick("text", DEFAULT_TEXT) }
}

pub fn tidy_colors(colors: &TaskColors) -> TaskColors {
    TaskColors {
        background: clean_hex(&colors.background).unwrap_or_else(|| DEFAULT_BACKGROUND.to_string()),
        text: clean_hex(&colors.text).unwrap_or_else(|| DEFAULT_TEXT.to_string()),
    }
}

pub fn tasks_value(tasks: &[Task]) -> Value {
    Value::Array(
        tidy_tasks(tasks).iter().map(|task| json!({"id": task.id, "text": task.text, "done": task.done})).collect(),
    )
}

pub fn colors_value(colors: &TaskColors) -> Value {
    let colors = tidy_colors(colors);
    json!({"background": colors.background, "text": colors.text})
}

/// Open tasks use the chosen text color. Executed tasks are dark grey.
pub fn task_color(done: bool, text_hex: &str) -> String {
    if done {
        DONE_GREY.to_string()
    } else {
        clean_hex(text_hex).unwrap_or_else(|| DEFAULT_TEXT.to_string())
    }
}

/// `t1`, `t2`… one past the highest numbered id.
pub fn fresh_id(tasks: &[Task]) -> String {
    let top = tasks
        .iter()
        .filter_map(|task| task.id.strip_prefix('t').and_then(|rest| rest.parse::<u64>().ok()))
        .max()
        .unwrap_or(0);
    format!("t{}", top + 1)
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_cleans_short_and_long_forms() {
        assert_eq!(clean_hex("#abc").as_deref(), Some("#AABBCC"));
        assert_eq!(clean_hex(" 123456 ").as_deref(), Some("#123456"));
        assert_eq!(clean_hex("#12345"), None);
        assert_eq!(clean_hex("#GGGGGG"), None);
        assert_eq!(task_color(true, "#ffffff"), DONE_GREY);
        assert_eq!(task_color(false, "nope"), DEFAULT_TEXT);
    }

    #[test]
    fn tasks_pack_like_python() {
        let rows = json!([
            {"id": "t1", "text": " ink the door ", "done": true},
            {"id": "t1", "text": "twice"},
            {"id": "", "text": "no id"},
            {"id": "t3", "text": "   "},
            {"id": 7, "text": "number id"},
        ]);
        let tasks = pack_tasks(&rows);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0], Task { id: "t1".into(), text: "ink the door".into(), done: true });
        assert_eq!(tasks[1].id, "7");
        assert_eq!(fresh_id(&tasks), "t2");
        assert_eq!(fresh_id(&[]), "t1");
        let colors = pack_colors(&json!({"background": "#111", "text": "junk"}));
        assert_eq!(colors, TaskColors { background: "#111111".into(), text: DEFAULT_TEXT.into() });
        assert_eq!(pack_colors(&colors_value(&colors)), colors);
        assert_eq!(pack_tasks(&tasks_value(&tasks)), tasks);
    }
}
