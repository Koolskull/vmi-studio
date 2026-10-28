//! Straight RGBA. Channels are 0 to 255.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const CLEAR: Self = Self { r: 0, g: 0, b: 0, a: 0 };

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    pub fn channels(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// `#RGB` or `#RRGGBB`, hash optional. Empty when the text is not a color.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let raw = text.trim().trim_start_matches('#');
    let hex = if raw.len() == 3 {
        let mut out = String::with_capacity(6);
        for ch in raw.chars() {
            out.push(ch);
            out.push(ch);
        }
        out
    } else {
        raw.to_string()
    };
    if hex.len() != 6 || !hex.bytes().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

pub fn hex_of(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// `#RRGGBBAA`. Alpha is the fourth byte.
pub fn hex_rgba(rgba: [u8; 4]) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        rgba[0], rgba[1], rgba[2], rgba[3]
    )
}

/// Three hex digits per channel, zero-padded. `FF3EB8` at full alpha is `0FF03E0B80FF`.
pub fn hex_rgba_wide(rgba: [u8; 4]) -> String {
    format!("{:03X}{:03X}{:03X}{:03X}", rgba[0], rgba[1], rgba[2], rgba[3])
}

/// One channel. One digit doubles (`F` is `FF`). Two digits are the byte.
/// Three digits keep the low byte, so `0FF` is 255.
pub fn parse_channel_hex(text: &str) -> Option<u8> {
    let raw = text.trim().trim_start_matches('#');
    if raw.is_empty() || raw.len() > 3 || !raw.bytes().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let value = u16::from_str_radix(raw, 16).ok()?;
    Some(match raw.len() {
        1 => ((value << 4) | value) as u8,
        2 => value as u8,
        _ => value as u8,
    })
}

/// `RRGGBB`, `RRGGBBAA`, `RRRGGGBBB`, or `RRRGGGBBBAAA`. A hash is optional.
/// Forms without alpha keep `alpha`.
pub fn parse_hex_rgba(text: &str, alpha: u8) -> Option<[u8; 4]> {
    let raw = text.trim().trim_start_matches('#');
    if raw.is_empty() || !raw.bytes().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let pair = |index: usize| u8::from_str_radix(&raw[index..index + 2], 16).ok();
    let triple = |index: usize| u16::from_str_radix(&raw[index..index + 3], 16).ok().map(|value| value as u8);
    match raw.len() {
        6 => Some([pair(0)?, pair(2)?, pair(4)?, alpha]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        9 => Some([triple(0)?, triple(3)?, triple(6)?, alpha]),
        12 => Some([triple(0)?, triple(3)?, triple(6)?, triple(9)?]),
        _ => None,
    }
}

/// Same hue, peak channel pulled to 92. Empty when `text` is not a color.
pub fn quiet_color(text: &str) -> String {
    let Some(channels) = parse_hex(text) else {
        return String::new();
    };
    let peak = channels.iter().copied().max().unwrap_or(0);
    if peak < 1 {
        return "#000000".into();
    }
    let scale = 92.0_f64 / f64::from(peak);
    let mut out = [0u8; 3];
    for (slot, channel) in out.iter_mut().zip(channels) {
        let rounded = (f64::from(channel) * scale).round_ties_even();
        *slot = rounded.clamp(0.0, 255.0) as u8;
    }
    hex_of(out)
}

pub fn rint_u8(value: f32) -> u8 {
    value.round_ties_even().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_expands_and_quiets() {
        assert_eq!(parse_hex("#F3A"), Some([255, 51, 170]));
        assert_eq!(parse_hex("ff3eb8"), Some([255, 62, 184]));
        assert!(parse_hex("nope").is_none());
        assert_eq!(quiet_color("#FF3EB8"), "#5C1642");
        let pink = [255, 62, 184, 255];
        assert_eq!(hex_rgba(pink), "#FF3EB8FF");
        assert_eq!(hex_rgba_wide(pink), "0FF03E0B80FF");
        assert_eq!(parse_hex_rgba("0FF03E0B80FF", 0), Some(pink));
        assert_eq!(parse_hex_rgba("#FF3EB8FF", 0), Some(pink));
        assert_eq!(parse_hex_rgba("FF3EB8", 128), Some([255, 62, 184, 128]));
        assert_eq!(parse_channel_hex("0FF"), Some(255));
        assert_eq!(parse_channel_hex("3E"), Some(62));
        assert_eq!(parse_channel_hex("F"), Some(255));
    }
}
