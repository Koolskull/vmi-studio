//! Layer names from the VMI grammar. SSSXXX, hex state and frame. A port of
//! `vmi_studio/naming.py`.

use std::sync::OnceLock;

use regex::Regex;

pub const STATE_LABELS: &[(u16, &str)] = &[(0, "rest"), (1, "hover"), (2, "pressed"), (3, "after")];

pub const LABEL_TO_STATE: &[(&str, u16)] = &[
    ("default", 0),
    ("idle", 0),
    ("normal", 0),
    ("rest", 0),
    ("hover", 1),
    ("active", 1),
    ("pressed", 2),
    ("press", 2),
    ("down", 2),
    ("after", 3),
];

fn omit_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:ref(?:[_\-\s]|$)|sketch(?:[_\-\s]|$)|guide(?:[_\-\s]|$)|uigrid|paper\b|_|\s*\[ref\])|(?:\b(?:reference|uigrid|guide|sketch|template)\b)",
        )
        .expect("omit pattern")
    })
}

fn state_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:#|\.|__|_)(hover|pressed|press|after|active|down|idle|normal|default|rest|s[0-9a-fA-F]{1,3})\b")
            .expect("state pattern")
    })
}

fn frame_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:@f|[#._]f|frame[_-]?)([0-9]+)\b").expect("frame pattern"))
}

fn vague_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(?:\d+|(?:layer|folder|group|copy)(?:\s*\d+)?|(?:レイヤー|コピー)(?:\s*\d+)?)$")
            .expect("vague pattern")
    })
}

fn unsafe_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"[<>:"/\\|?*]+"#).expect("unsafe pattern"))
}

fn tail_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[#@]+$").expect("tail pattern"))
}

fn space_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").expect("space pattern"))
}

/// Sketch, guide, reference, template, paper, a leading underscore: these
/// stay in the drawing when the scene is written.
pub fn is_omitted_name(name: &str) -> bool {
    omit_re().is_match(name.trim())
}

/// Layer 1, Folder 1, a bare number: a name that will not make sense next month.
pub fn is_vague_name(name: &str) -> bool {
    vague_re().is_match(name.trim())
}

/// The object stem: the position tokens and the file-unsafe characters gone.
pub fn sanitize(name: &str) -> String {
    let text = state_re().replace_all(name, "");
    let text = frame_re().replace_all(&text, "");
    let text = tail_re().replace_all(&text, "");
    let text = text.trim_matches(|ch| " ._-/".contains(ch));
    let text = unsafe_re().replace_all(text, "_").into_owned();
    if text.is_empty() {
        "layer".to_string()
    } else {
        text
    }
}

/// The (state, frame) a name carries, each masked to 12 bits.
pub fn parse_sf(raw: &str) -> (u16, u16) {
    let mut state = 0u16;
    let mut frame = 0u16;
    if let Some(found) = state_re().captures(raw) {
        let token = found[1].to_ascii_lowercase();
        if let Some(hex) = token.strip_prefix('s').filter(|hex| hex.chars().all(|ch| ch.is_ascii_hexdigit())) {
            state = u16::from_str_radix(hex, 16).unwrap_or(0);
        } else {
            state = LABEL_TO_STATE.iter().find(|(label, _)| *label == token).map(|(_, state)| *state).unwrap_or(0);
        }
    }
    if let Some(found) = frame_re().captures(raw) {
        frame = found[1].parse::<u64>().map(|number| (number & 0xFFF) as u16).unwrap_or(0);
    }
    (state & 0xFFF, frame & 0xFFF)
}

pub fn slot_key(state: u16, frame: u16) -> String {
    format!("{:03X}{:03X}", state & 0xFFF, frame & 0xFFF)
}

pub fn state_label(state: u16) -> &'static str {
    STATE_LABELS.iter().find(|(known, _)| *known == (state & 0xFFF)).map(|(_, label)| *label).unwrap_or("custom")
}

/// A folder name on disk. Unsafe characters become one space.
pub fn folder_name(name: &str) -> String {
    let clean = unsafe_re().replace_all(name, " ");
    let clean = space_re().replace_all(&clean, " ").trim().to_string();
    if clean.is_empty() {
        "object".to_string()
    } else {
        clean
    }
}

/// `scene` for a name that is empty or a dot path.
pub fn safe_folder(name: &str) -> String {
    let clean = name.trim().replace(['\\', '/'], " ");
    if clean.is_empty() || clean == "." || clean == ".." {
        "scene".to_string()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_read_the_way_python_reads_them() {
        assert_eq!(parse_sf("door#hover"), (1, 0));
        assert_eq!(parse_sf("door_pressed"), (2, 0));
        assert_eq!(parse_sf("wave@f2"), (0, 2));
        assert_eq!(parse_sf("sky.s0a frame_3"), (10, 3));
        assert_eq!(parse_sf("plain"), (0, 0));
        assert_eq!(slot_key(1, 0), "001000");
        assert_eq!(slot_key(0x123, 0xABC), "123ABC");
        assert_eq!(state_label(2), "pressed");
        assert_eq!(state_label(9), "custom");
    }

    #[test]
    fn names_sanitize_and_the_omit_rule_holds() {
        assert_eq!(sanitize("door#hover"), "door");
        assert_eq!(sanitize("wave@f2"), "wave");
        assert_eq!(sanitize("  shop/front: open  "), "shop_front_ open");
        assert_eq!(sanitize("#@"), "layer");
        assert!(is_omitted_name("sketch 1"));
        assert!(is_omitted_name("_hidden"));
        assert!(is_omitted_name("Paper"));
        assert!(is_omitted_name("the reference plate"));
        assert!(!is_omitted_name("door"));
        assert!(is_vague_name("Layer 12"));
        assert!(is_vague_name("7"));
        assert!(is_vague_name("レイヤー 3"));
        assert!(!is_vague_name("door"));
        assert_eq!(folder_name("a/b:c"), "a b c");
        assert_eq!(folder_name("   "), "object");
        assert_eq!(safe_folder(".."), "scene");
        assert_eq!(safe_folder("a/b"), "a b");
    }
}
