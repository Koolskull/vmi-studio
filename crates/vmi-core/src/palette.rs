//! Color palettes: the built-in sets and the files Aseprite reads and writes.
//!
//! A palette is a named, ordered list of opaque RGB colors. [`parse`] reads GIMP
//! `.gpl` (including Aseprite's `Channels: RGBA` dialect), `.hex` lists, JASC and
//! Microsoft RIFF `.pal`, Paint.NET `.txt`, Adobe `.act`, Aseprite sprites, Adobe
//! Swatch Exchange `.ase`, and palette `.png` images. The extension picks the
//! format and the content is sniffed as a fallback, so a misnamed file still
//! opens. [`save_file`] writes `.gpl`, `.hex`, `.pal`, `.txt`, `.act` and `.png`;
//! Aseprite reads the first four, so a saved palette round-trips into Aseprite.
//!
//! Formats with an explicit order (everything but PNG) keep every entry, including
//! repeats. PNG images collect their unique colors instead.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

impl Palette {
    pub fn new(name: impl Into<String>, colors: Vec<[u8; 3]>) -> Self {
        Self { name: name.into(), colors }
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

/// Extensions the open prompt should accept, lowercase without dots.
pub const EXTENSIONS: &[&str] = &["gpl", "hex", "pal", "txt", "act", "ase", "aseprite", "png"];

/// Entries a palette file may hold. PNG and ACT stop at 256 on their own; this
/// bounds the Aseprite and swatch readers against damaged size fields.
const MAX_COLORS: usize = 65_536;

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

const KOOL_SKULL: &[u32] = &[
    0x000000, 0xFFFFFF, 0x444444, 0x888888, 0xCCCCCC, 0xE02020, 0xFF8A00, 0xFFE100, 0x22C55A, 0x14B8C4, 0x3B6CFF,
    0x8A3FF5, 0xFF3EB8, 0xF4A4C0, 0x8B4513, 0x003A70,
];

/// DawnBringer 32, Aseprite's default palette.
const DB32: &[u32] = &[
    0x000000, 0x222034, 0x45283C, 0x663931, 0x8F563B, 0xDF7126, 0xD9A066, 0xEEC39A, 0xFBF236, 0x99E550, 0x6ABE30,
    0x37946E, 0x4B692F, 0x524B24, 0x323C39, 0x3F3F74, 0x306082, 0x5B6EE1, 0x639BFF, 0x5FCDE4, 0xCBDBFC, 0xFFFFFF,
    0x9BADB7, 0x847E87, 0x696A6A, 0x595652, 0x76428A, 0xAC3232, 0xD95763, 0xD77BBA, 0x8F974A, 0x8A6F30,
];

/// DawnBringer 16.
const DB16: &[u32] = &[
    0x140C1C, 0x442434, 0x30346D, 0x4E4A4E, 0x854C30, 0x346524, 0xD04648, 0x757161, 0x597DCE, 0xD27D2C, 0x8595A1,
    0x6DAA2C, 0xD2AA99, 0x6DC2CA, 0xDAD45E, 0xDEEED6,
];

const SWEETIE_16: &[u32] = &[
    0x1A1C2C, 0x5D275D, 0xB13E53, 0xEF7D57, 0xFFCD75, 0xA7F070, 0x38B764, 0x257179, 0x29366F, 0x3B5DC9, 0x41A6F6,
    0x73EFF7, 0xF4F4F4, 0x94B0C2, 0x566C86, 0x333C57,
];

const PICO_8: &[u32] = &[
    0x000000, 0x1D2B53, 0x7E2553, 0x008751, 0xAB5236, 0x5F574F, 0xC2C3C7, 0xFFF1E8, 0xFF004D, 0xFFA300, 0xFFEC27,
    0x00E436, 0x29ADFF, 0x83769C, 0xFF77A8, 0xFFCCAA,
];

const ENDESGA_32: &[u32] = &[
    0xBE4A2F, 0xD77643, 0xEAD4AA, 0xE4A672, 0xB86F50, 0x733E39, 0x3E2731, 0xA22633, 0xE43B44, 0xF77622, 0xFEAE34,
    0xFEE761, 0x63C74D, 0x3E8948, 0x265C42, 0x193C3E, 0x124E89, 0x0099DB, 0x2CE8F5, 0xFFFFFF, 0xC0CBDC, 0x8B9BB4,
    0x5A6988, 0x3A4466, 0x262B44, 0x181425, 0xFF0044, 0x68386C, 0xB55088, 0xF6757A, 0xE8B796, 0xC28569,
];

/// The original DMG greens.
const GAME_BOY: &[u32] = &[0x0F380F, 0x306230, 0x8BAC0F, 0x9BBC0F];

const GAME_BOY_POCKET: &[u32] = &[0x000000, 0x555555, 0xAAAAAA, 0xFFFFFF];

/// The classic 2C02 RGB table (the 64-entry list on Wikipedia's console palette
/// page) in PPU order $00..$3F, with every black after the first dropped: $0D stays,
/// $0E $0F $1D $1E $1F $2E $2F $3E $3F go. That leaves the 54 colors plus one black.
const NES: &[u32] = &[
    0x7C7C7C, 0x0000FC, 0x0000BC, 0x4428BC, 0x940084, 0xA80020, 0xA81000, 0x881400, 0x503000, 0x007800, 0x006800,
    0x005800, 0x004058, 0x000000, 0xBCBCBC, 0x0078F8, 0x0058F8, 0x6844FC, 0xD800CC, 0xE40058, 0xF83800, 0xE45C10,
    0xAC7C00, 0x00B800, 0x00A800, 0x00A844, 0x008888, 0xF8F8F8, 0x3CBCFC, 0x6888FC, 0x9878F8, 0xF878F8, 0xF85898,
    0xF87858, 0xFCA044, 0xF8B800, 0xB8F818, 0x58D854, 0x58F898, 0x00E8D8, 0x787878, 0xFCFCFC, 0xA4E4FC, 0xB8B8F8,
    0xD8B8F8, 0xF8B8F8, 0xF8A4C0, 0xF0D0B0, 0xFCE0A8, 0xF8D878, 0xD8F878, 0xB8F8B8, 0xB8F8D8, 0x00FCFC, 0xF8D8F8,
];

const COMMODORE_64: &[u32] = &[
    0x000000, 0xFFFFFF, 0x880000, 0xAAFFEE, 0xCC44CC, 0x00CC55, 0x0000AA, 0xEEEE77, 0xDD8855, 0x664400, 0xFF7777,
    0x333333, 0x777777, 0xAAFF66, 0x0088FF, 0xBBBBBB,
];

const CGA: &[u32] = &[
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xAA5500, 0xAAAAAA, 0x555555, 0x5555FF, 0x55FF55,
    0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// Normal and bright sets. Black is the same in both, so it appears once.
const ZX_SPECTRUM: &[u32] = &[
    0x000000, 0x0000D7, 0xD70000, 0xD700D7, 0x00D700, 0x00D7D7, 0xD7D700, 0xD7D7D7, 0x0000FF, 0xFF0000, 0xFF00FF,
    0x00FF00, 0x00FFFF, 0xFFFF00, 0xFFFFFF,
];

/// Lo-res colors 0..15 in hardware order. Entries 5 and 10 are both grey, as on the machine.
const APPLE_II: &[u32] = &[
    0x000000, 0x6C2940, 0x403578, 0xD93CF3, 0x135740, 0x808080, 0x2697F3, 0xBFB4F8, 0x404B07, 0xD9680F, 0x808080,
    0xECA8BF, 0x26C30F, 0xBFCA87, 0x93D6BF, 0xFFFFFF,
];

const fn rgb(hex: u32) -> [u8; 3] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
}

fn preset(name: &str, table: &[u32]) -> Palette {
    Palette::new(name, table.iter().copied().map(rgb).collect())
}

/// Built-in sets, in display order. The first is the house palette.
pub fn presets() -> Vec<Palette> {
    vec![
        preset("Kool Skull", KOOL_SKULL),
        preset("DB32", DB32),
        preset("DB16", DB16),
        preset("Sweetie 16", SWEETIE_16),
        preset("PICO-8", PICO_8),
        preset("Endesga 32", ENDESGA_32),
        preset("Game Boy", GAME_BOY),
        preset("Game Boy Pocket", GAME_BOY_POCKET),
        preset("NES", NES),
        preset("Commodore 64", COMMODORE_64),
        preset("CGA", CGA),
        preset("ZX Spectrum", ZX_SPECTRUM),
        preset("Apple II", APPLE_II),
        Palette::new("Greys", (0..16u8).map(|step| [step * 17; 3]).collect()),
    ]
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Gpl,
    Hex,
    Jasc,
    Riff,
    PaintNet,
    Act,
    Aseprite,
    Asef,
    Png,
}

impl Format {
    /// Formats a file extension may hold, most likely first.
    fn candidates(extension: &str) -> &'static [Format] {
        match extension {
            "gpl" => &[Format::Gpl],
            "hex" => &[Format::Hex],
            "pal" => &[Format::Jasc, Format::Riff],
            "txt" => &[Format::PaintNet],
            "act" => &[Format::Act],
            "ase" | "aseprite" => &[Format::Aseprite, Format::Asef],
            "png" => &[Format::Png],
            _ => &[],
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Format::Gpl => "a GIMP palette",
            Format::Hex => "a hex color list",
            Format::Jasc => "a JASC palette",
            Format::Riff => "a Microsoft RIFF palette",
            Format::PaintNet => "a Paint.NET palette",
            Format::Act => "an Adobe Color Table",
            Format::Aseprite => "an Aseprite sprite",
            Format::Asef => "an Adobe Swatch Exchange file",
            Format::Png => "a PNG image",
        }
    }

    fn parse(self, bytes: &[u8]) -> Result<Palette, String> {
        match self {
            Format::Gpl => parse_gpl(bytes),
            Format::Hex => parse_hex(bytes),
            Format::Jasc => parse_jasc_pal(bytes),
            Format::Riff => parse_riff_pal(bytes),
            Format::PaintNet => parse_paint_net(bytes),
            Format::Act => parse_act(bytes),
            Format::Aseprite => parse_aseprite(bytes),
            Format::Asef => parse_asef(bytes),
            Format::Png => parse_png(bytes),
        }
    }

    /// Formats that announce themselves. These win over the extension.
    fn sniff_magic(bytes: &[u8]) -> Option<Format> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some(Format::Png);
        }
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"PAL " {
            return Some(Format::Riff);
        }
        if bytes.starts_with(b"ASEF") {
            return Some(Format::Asef);
        }
        if bytes.len() >= 128 && u16_le(bytes, 4) == Some(0xA5E0) {
            return Some(Format::Aseprite);
        }
        if !looks_like_text(bytes) {
            return None;
        }
        let text = text_of(bytes);
        let first = text.lines().map(str::trim).find(|line| !line.is_empty())?;
        if first.eq_ignore_ascii_case("GIMP Palette") {
            return Some(Format::Gpl);
        }
        if first.eq_ignore_ascii_case("JASC-PAL") {
            return Some(Format::Jasc);
        }
        if first.starts_with(';') && first.to_ascii_lowercase().contains("paint.net") {
            return Some(Format::PaintNet);
        }
        None
    }

    /// Text that is nothing but hex colors and comments. A guess, so the extension wins.
    fn sniff_text(bytes: &[u8]) -> Option<Format> {
        if !looks_like_text(bytes) {
            return None;
        }
        let text = text_of(bytes);
        let mut colors = 0;
        let mut semicolons = false;
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if hex_digits(first_token(line)).is_some() {
                colors += 1;
            } else if line.starts_with(';') {
                semicolons = true;
            } else if !(line.starts_with('#') || line.starts_with("//")) {
                return None;
            }
        }
        if colors == 0 {
            return None;
        }
        Some(if semicolons { Format::PaintNet } else { Format::Hex })
    }
}

/// Parse any supported palette file from bytes. `file_name` picks the format by
/// extension, with content sniffing as fallback, so a misnamed file still loads.
/// Errors are plain English for the status bar.
///
/// The palette's name is its own when the format carries one (GPL `Name:`, a
/// Lospec or Paint.NET `Palette Name:` comment, a lone swatch group), else the
/// file stem.
pub fn parse(bytes: &[u8], file_name: &str) -> Result<Palette, String> {
    let path = Path::new(file_name);
    let extension = path.extension().map(|ext| ext.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let candidates = Format::candidates(&extension);

    // Most certain first: a magic number, the extension when it names one format,
    // text that is plainly a color list, and last a bare 768 or 772 byte blob.
    let mut attempts: Vec<Format> = Vec::new();
    let mut consider = |format: Option<Format>| {
        if let Some(format) = format {
            if !attempts.contains(&format) {
                attempts.push(format);
            }
        }
    };
    consider(Format::sniff_magic(bytes));
    if let [single] = candidates {
        consider(Some(*single));
    }
    consider(Format::sniff_text(bytes));
    if (bytes.len() == 768 || bytes.len() == 772) && !looks_like_text(bytes) {
        consider(Some(Format::Act));
    }

    if attempts.is_empty() {
        return Err(match candidates {
            [first, second] => format!("This file is neither {} nor {}.", first.describe(), second.describe()),
            _ if extension.is_empty() => "This file is not a palette the studio can read.".to_string(),
            _ => format!(".{extension} files are not palettes the studio can read."),
        });
    }

    let mut first_error = None;
    for format in attempts {
        match format.parse(bytes) {
            Ok(mut palette) => {
                if palette.colors.is_empty() {
                    return Err("The palette has no colors.".into());
                }
                if palette.name.trim().is_empty() {
                    palette.name = stem_of(path);
                }
                return Ok(palette);
            }
            Err(err) => {
                first_error.get_or_insert(err);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| "This file is not a palette the studio can read.".to_string()))
}

pub fn load_file(path: &Path) -> Result<Palette, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    parse(&bytes, &file_name)
}

fn stem_of(path: &Path) -> String {
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().trim().to_string()).unwrap_or_default();
    if stem.is_empty() {
        "Palette".to_string()
    } else {
        stem
    }
}

// --- GIMP .gpl ---------------------------------------------------------------

/// GIMP palette, including Aseprite's `Channels: RGBA` dialect where rows carry a
/// fourth number. Rows are `R G B [A] [name]`; tabs, runs of spaces and missing
/// names are fine. `#` lines are comments; a Lospec `#Palette Name:` comment names
/// the palette when there is no `Name:` header.
pub fn parse_gpl(bytes: &[u8]) -> Result<Palette, String> {
    let text = text_of(bytes);
    let mut lines = text.lines().map(str::trim);
    let header = lines.find(|line| !line.is_empty()).unwrap_or_default();
    if !header.eq_ignore_ascii_case("GIMP Palette") {
        return Err("Not a GIMP palette: the first line should be 'GIMP Palette'.".into());
    }
    let mut name = String::new();
    let mut has_alpha = false;
    let mut colors = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if name.is_empty() {
                if let Some(found) = comment_name(line) {
                    name = found;
                }
            }
            continue;
        }
        if !line.starts_with(|ch: char| ch.is_ascii_digit()) {
            if let Some((key, value)) = line.split_once(':') {
                match key.trim().to_ascii_lowercase().as_str() {
                    "name" => name = value.trim().to_string(),
                    "channels" => has_alpha = value.trim().eq_ignore_ascii_case("RGBA"),
                    _ => {}
                }
            }
            continue;
        }
        let mut tokens = line.split_whitespace();
        let mut channel = || tokens.next().and_then(|token| token.parse::<i64>().ok()).map(|value| value.clamp(0, 255) as u8);
        let (Some(r), Some(g), Some(b)) = (channel(), channel(), channel()) else {
            continue;
        };
        if has_alpha {
            let _alpha = channel();
        }
        colors.push([r, g, b]);
    }
    if colors.is_empty() {
        return Err("No colors found in this GIMP palette.".into());
    }
    Ok(Palette::new(name, colors))
}

/// `GIMP Palette` with `Name:` and `Columns:` headers, then `R G B<tab>RRGGBB` rows.
/// GIMP and Aseprite both read this.
pub fn write_gpl(palette: &Palette) -> String {
    let mut out = String::from("GIMP Palette\n");
    let name = one_line(&palette.name);
    if !name.is_empty() {
        out.push_str(&format!("Name: {name}\n"));
    }
    out.push_str(&format!("Columns: {}\n#\n", palette.colors.len().clamp(1, 16)));
    for [r, g, b] in &palette.colors {
        out.push_str(&format!("{r:>3} {g:>3} {b:>3}\t{r:02X}{g:02X}{b:02X}\n"));
    }
    out
}

// --- .hex and Paint.NET .txt -------------------------------------------------

/// One color per line as `RRGGBB` or `RRGGBBAA` (alpha dropped), `#` or `0x`
/// optional. Blank lines and comment lines are skipped. Lospec and Aseprite write this.
pub fn parse_hex(bytes: &[u8]) -> Result<Palette, String> {
    parse_color_lines(bytes, AlphaSlot::Last, "hex color list")
}

/// Paint.NET palette: `AARRGGBB` lines (plain `RRGGBB` is accepted too) with `;` comments.
pub fn parse_paint_net(bytes: &[u8]) -> Result<Palette, String> {
    parse_color_lines(bytes, AlphaSlot::First, "Paint.NET palette")
}

/// Where an eight-digit hex color keeps its alpha.
#[derive(Clone, Copy)]
enum AlphaSlot {
    /// `RRGGBBAA`, as in `.hex` files.
    Last,
    /// `AARRGGBB`, as in Paint.NET files.
    First,
}

fn parse_color_lines(bytes: &[u8], alpha: AlphaSlot, what: &str) -> Result<Palette, String> {
    let text = text_of(bytes);
    let mut name = String::new();
    let mut colors = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(color) = hex_color(first_token(line), alpha) {
            colors.push(color);
        } else if name.is_empty() {
            if let Some(found) = comment_name(line) {
                name = found;
            }
        }
    }
    if colors.is_empty() {
        return Err(format!("No colors found in this {what}."));
    }
    Ok(Palette::new(name, colors))
}

/// Lowercase `rrggbb` lines and nothing else, so Aseprite and Lospec read it.
pub fn write_hex(palette: &Palette) -> String {
    let mut out = String::new();
    for [r, g, b] in &palette.colors {
        out.push_str(&format!("{r:02x}{g:02x}{b:02x}\n"));
    }
    out
}

/// Paint.NET palette: `;` comments then `FFRRGGBB` lines.
pub fn write_txt(palette: &Palette) -> String {
    let mut out = String::from("; paint.net Palette File\n");
    let name = one_line(&palette.name);
    if !name.is_empty() {
        out.push_str(&format!("; Palette Name: {name}\n"));
    }
    out.push_str(&format!("; Colors: {}\n", palette.colors.len()));
    for [r, g, b] in &palette.colors {
        out.push_str(&format!("FF{r:02X}{g:02X}{b:02X}\n"));
    }
    out
}

fn first_token(line: &str) -> &str {
    line.split([' ', '\t', ',', ';']).next().unwrap_or("")
}

/// The hex digits of a color token: six or eight of them after an optional `#` or `0x`.
fn hex_digits(token: &str) -> Option<&str> {
    let digits = token
        .strip_prefix('#')
        .or_else(|| token.strip_prefix("0x"))
        .or_else(|| token.strip_prefix("0X"))
        .unwrap_or(token);
    let plain = matches!(digits.len(), 6 | 8) && digits.bytes().all(|byte| byte.is_ascii_hexdigit());
    plain.then_some(digits)
}

fn hex_color(token: &str, alpha: AlphaSlot) -> Option<[u8; 3]> {
    let digits = hex_digits(token)?;
    let rgb = match (digits.len(), alpha) {
        (8, AlphaSlot::Last) => &digits[..6],
        (8, AlphaSlot::First) => &digits[2..],
        _ => digits,
    };
    let channel = |at: usize| u8::from_str_radix(&rgb[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// `Palette Name: X` or `Name: X` inside a comment line, as Lospec and Paint.NET write.
fn comment_name(line: &str) -> Option<String> {
    let body = line.trim_start_matches(['#', ';', '/']).trim();
    let (key, value) = body.split_once(':')?;
    let key = key.trim().to_ascii_lowercase();
    if key != "palette name" && key != "name" {
        return None;
    }
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn one_line(name: &str) -> String {
    name.lines().next().unwrap_or("").trim().to_string()
}

// --- JASC and RIFF .pal ------------------------------------------------------

/// JASC-PAL: `JASC-PAL`, a version line (`0100`), a count, then `R G B` rows.
pub fn parse_jasc_pal(bytes: &[u8]) -> Result<Palette, String> {
    let text = text_of(bytes);
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if !lines.next().unwrap_or_default().eq_ignore_ascii_case("JASC-PAL") {
        return Err("Not a JASC palette: the first line should be 'JASC-PAL'.".into());
    }
    let _version = lines.next().ok_or("This JASC palette ends after its header.")?;
    let count = lines.next().and_then(|line| line.parse::<usize>().ok()).unwrap_or(0);
    let mut colors = Vec::new();
    for line in lines {
        if count > 0 && colors.len() >= count {
            break;
        }
        let mut tokens = line.split_whitespace();
        let mut channel = || tokens.next().and_then(|token| token.parse::<i64>().ok()).map(|value| value.clamp(0, 255) as u8);
        if let (Some(r), Some(g), Some(b)) = (channel(), channel(), channel()) {
            colors.push([r, g, b]);
        }
    }
    if colors.is_empty() {
        return Err("No colors found in this JASC palette.".into());
    }
    Ok(Palette::new("", colors))
}

/// Microsoft RIFF palette: a `PAL ` form whose `data` chunk holds a LOGPALETTE of
/// 4-byte `R G B flags` entries.
pub fn parse_riff_pal(bytes: &[u8]) -> Result<Palette, String> {
    if bytes.len() < 12 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"PAL " {
        return Err("Not a Microsoft RIFF palette.".into());
    }
    let mut at = 12;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), u32_le(bytes, at + 4)) {
        let start = at + 8;
        let end = bytes.len().min(start.saturating_add(size as usize));
        if id == b"data" {
            let chunk = &bytes[start..end];
            let count = u16_le(chunk, 2).ok_or("This RIFF palette has an empty data chunk.")? as usize;
            let colors: Vec<[u8; 3]> =
                chunk[4..].chunks_exact(4).take(count).map(|entry| [entry[0], entry[1], entry[2]]).collect();
            if colors.is_empty() {
                return Err("No colors found in this RIFF palette.".into());
            }
            return Ok(Palette::new("", colors));
        }
        // RIFF pads odd chunks to an even length.
        at = end + (size as usize & 1);
    }
    Err("This RIFF palette has no data chunk.".into())
}

/// JASC-PAL text, the `.pal` Aseprite and Paint Shop Pro read.
pub fn write_pal(palette: &Palette) -> String {
    let mut out = format!("JASC-PAL\n0100\n{}\n", palette.colors.len());
    for [r, g, b] in &palette.colors {
        out.push_str(&format!("{r} {g} {b}\n"));
    }
    out
}

// --- Adobe .act --------------------------------------------------------------

/// Adobe Color Table: 768 bytes of RGB triples, optionally followed by a big-endian
/// color count and transparent index. The count wins when present. A bare 768-byte
/// table loses its trailing run of black, which is how Photoshop pads to 256, but
/// keeps at least its first color. Shorter files that are whole triples load as-is.
pub fn parse_act(bytes: &[u8]) -> Result<Palette, String> {
    let triples = |slice: &[u8]| slice.chunks_exact(3).map(|rgb| [rgb[0], rgb[1], rgb[2]]).collect::<Vec<_>>();
    let colors = if bytes.len() >= 772 {
        let count = u16::from_be_bytes([bytes[768], bytes[769]]) as usize;
        let count = if count == 0 || count > 256 { 256 } else { count };
        let mut colors = triples(&bytes[..768]);
        colors.truncate(count);
        colors
    } else if bytes.len() == 768 {
        let mut colors = triples(bytes);
        while colors.len() > 1 && colors.last() == Some(&[0, 0, 0]) {
            colors.pop();
        }
        colors
    } else if !bytes.is_empty() && bytes.len() % 3 == 0 {
        triples(bytes)
    } else {
        return Err("Not an Adobe Color Table: expected 768 or 772 bytes of colors.".into());
    };
    if colors.is_empty() {
        return Err("No colors found in this Adobe Color Table.".into());
    }
    Ok(Palette::new("", colors))
}

/// 772 bytes: 256 RGB triples padded with black, then the color count and
/// `0xFFFF` for no transparent index. Palettes past 256 colors are cut.
pub fn write_act(palette: &Palette) -> Vec<u8> {
    let count = palette.colors.len().min(256);
    let mut out = Vec::with_capacity(772);
    for color in palette.colors.iter().take(count) {
        out.extend_from_slice(color);
    }
    out.resize(768, 0);
    out.extend_from_slice(&(count as u16).to_be_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out
}

// --- Aseprite sprite ---------------------------------------------------------

/// The palette of an Aseprite sprite, read from the first frame only. The new
/// palette chunk (0x2019) wins, then the old 8-bit chunk (0x0004), then the old
/// 6-bit chunk (0x0011, scaled like Aseprite does: `v << 2 | v >> 4`). Entry names
/// and alpha are skipped. Cels are not touched.
pub fn parse_aseprite(bytes: &[u8]) -> Result<Palette, String> {
    if bytes.len() < 128 || u16_le(bytes, 4) != Some(0xA5E0) {
        return Err("Not an Aseprite sprite.".into());
    }
    if u16_le(bytes, 6) == Some(0) {
        return Err("This Aseprite sprite has no frames.".into());
    }
    let declared = u16_le(bytes, 32).unwrap_or(0) as usize;
    let header_colors = if declared == 0 { 256 } else { declared };
    let damaged = || "The first frame of this Aseprite sprite is damaged.".to_string();
    let frame_size = u32_le(bytes, 128).ok_or_else(damaged)? as usize;
    if u16_le(bytes, 132) != Some(0xF1FA) {
        return Err(damaged());
    }
    let frame_end = bytes.len().min(128usize.saturating_add(frame_size));
    let old_count = u32::from(u16_le(bytes, 134).unwrap_or(0));
    let new_count = u32_le(bytes, 140).unwrap_or(0);
    let chunk_count = if new_count != 0 { new_count } else { old_count };

    let mut new_palette: Option<Vec<[u8; 3]>> = None;
    let mut old_palette: Option<Vec<[u8; 3]>> = None;
    let mut old_palette_6bit: Option<Vec<[u8; 3]>> = None;
    let mut at = 144;
    for _ in 0..chunk_count {
        let (Some(size), Some(kind)) = (u32_le(bytes, at), u16_le(bytes, at + 4)) else {
            break;
        };
        let size = size as usize;
        if size < 6 {
            break;
        }
        let data_end = frame_end.min(at.saturating_add(size));
        let data = bytes.get(at + 6..data_end).unwrap_or(&[]);
        match kind {
            0x2019 => apply_new_palette_chunk(data, new_palette.get_or_insert_with(Vec::new))?,
            0x0004 => apply_old_palette_chunk(data, false, old_palette.get_or_insert_with(Vec::new)),
            0x0011 => apply_old_palette_chunk(data, true, old_palette_6bit.get_or_insert_with(Vec::new)),
            _ => {}
        }
        at = data_end;
        if at >= frame_end {
            break;
        }
    }

    let colors = match (new_palette, old_palette.or(old_palette_6bit)) {
        (Some(colors), _) => colors,
        (None, Some(mut colors)) => {
            colors.truncate(header_colors);
            colors
        }
        (None, None) => return Err("This Aseprite sprite has no palette.".into()),
    };
    if colors.is_empty() {
        return Err("This Aseprite sprite has an empty palette.".into());
    }
    Ok(Palette::new("", colors))
}

fn apply_new_palette_chunk(data: &[u8], palette: &mut Vec<[u8; 3]>) -> Result<(), String> {
    let size = u32_le(data, 0).unwrap_or(0) as usize;
    let first = u32_le(data, 4).unwrap_or(0) as usize;
    let last = u32_le(data, 8).unwrap_or(0) as usize;
    if size > MAX_COLORS {
        return Err("This Aseprite palette is impossibly large.".into());
    }
    if palette.len() < size {
        palette.resize(size, [0, 0, 0]);
    }
    let mut at = 20;
    let mut index = first;
    while index <= last && at + 6 <= data.len() {
        let flags = u16_le(data, at).unwrap_or(0);
        let color = [data[at + 2], data[at + 3], data[at + 4]];
        at += 6;
        if flags & 1 != 0 {
            let name_len = u16_le(data, at).unwrap_or(0) as usize;
            at += 2 + name_len;
        }
        if index < palette.len() {
            palette[index] = color;
        }
        index += 1;
    }
    Ok(())
}

fn apply_old_palette_chunk(data: &[u8], six_bit: bool, palette: &mut Vec<[u8; 3]>) {
    let packets = u16_le(data, 0).unwrap_or(0);
    let mut at = 2;
    let mut index = 0usize;
    for _ in 0..packets {
        let (Some(&skip), Some(&count)) = (data.get(at), data.get(at + 1)) else {
            return;
        };
        at += 2;
        index += usize::from(skip);
        let count = if count == 0 { 256 } else { usize::from(count) };
        for _ in 0..count {
            let Some(rgb) = data.get(at..at + 3) else {
                return;
            };
            at += 3;
            let mut color = [rgb[0], rgb[1], rgb[2]];
            if six_bit {
                for channel in &mut color {
                    *channel = scale_6_to_8(*channel);
                }
            }
            if index >= MAX_COLORS {
                return;
            }
            if index >= palette.len() {
                palette.resize(index + 1, [0, 0, 0]);
            }
            palette[index] = color;
            index += 1;
        }
    }
}

fn scale_6_to_8(value: u8) -> u8 {
    let value = value & 0x3F;
    (value << 2) | (value >> 4)
}

// --- Adobe Swatch Exchange ---------------------------------------------------

/// Adobe Swatch Exchange: big-endian `ASEF` blocks. Color entries convert RGB
/// directly, CMYK naively (`(1 - c) * (1 - k)`), Gray as intensity (1.0 is white),
/// and Lab through a proper D50 conversion, see [`lab_to_rgb`]. When the file has a
/// single named group, that is the palette's name.
pub fn parse_asef(bytes: &[u8]) -> Result<Palette, String> {
    if !bytes.starts_with(b"ASEF") {
        return Err("Not an Adobe Swatch Exchange file.".into());
    }
    let blocks = u32_be(bytes, 8).ok_or("This swatch file ends after its header.")?;
    let mut colors = Vec::new();
    let mut groups = Vec::new();
    let mut at = 12;
    for _ in 0..blocks {
        let (Some(kind), Some(size)) = (u16_be(bytes, at), u32_be(bytes, at + 2)) else {
            break;
        };
        let start = at + 6;
        let end = bytes.len().min(start.saturating_add(size as usize));
        let block = &bytes[start.min(end)..end];
        match kind {
            0x0001 => {
                if let Some(color) = asef_color(block) {
                    colors.push(color);
                }
            }
            0xC001 => groups.push(utf16_string(block)),
            0xC002 => {}
            _ => break,
        }
        if colors.len() >= MAX_COLORS {
            break;
        }
        at = end;
    }
    if colors.is_empty() {
        return Err("No colors found in this swatch file.".into());
    }
    let name = match groups.as_slice() {
        [only] => only.trim().to_string(),
        _ => String::new(),
    };
    Ok(Palette::new(name, colors))
}

/// A swatch color entry: UTF-16 name, four-letter model, big-endian floats.
fn asef_color(block: &[u8]) -> Option<[u8; 3]> {
    let name_len = u16_be(block, 0)? as usize;
    let at = 2 + name_len * 2;
    let model = block.get(at..at + 4)?;
    let value = |slot: usize| f32_be(block, at + 4 + slot * 4);
    let unit = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    match model {
        b"RGB " => Some([unit(value(0)?), unit(value(1)?), unit(value(2)?)]),
        b"CMYK" => {
            let (c, m, y, k) = (value(0)?, value(1)?, value(2)?, value(3)?);
            Some([unit((1.0 - c) * (1.0 - k)), unit((1.0 - m) * (1.0 - k)), unit((1.0 - y) * (1.0 - k))])
        }
        b"LAB " => {
            // Swatch files store L* scaled to 0..1; a and b stay in -128..127.
            let lightness = value(0)?;
            let lightness = if lightness <= 1.0 { lightness * 100.0 } else { lightness };
            Some(lab_to_rgb(lightness, value(1)?, value(2)?))
        }
        b"Gray" => {
            let grey = unit(value(0)?);
            Some([grey, grey, grey])
        }
        _ => None,
    }
}

/// A swatch string: big-endian length in UTF-16 units including the terminating zero.
fn utf16_string(block: &[u8]) -> String {
    let len = u16_be(block, 0).unwrap_or(0) as usize;
    let units: Vec<u16> = block
        .get(2..)
        .unwrap_or(&[])
        .chunks_exact(2)
        .take(len)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// CIE L*a*b* to sRGB. Photoshop's Lab is relative to D50, so the XYZ step uses the
/// D50 white (0.96422, 1.0, 0.82521) and Lindbloom's Bradford-adapted sRGB(D50)
/// matrix, then the sRGB transfer curve. L is 0..100, a and b about -128..127.
pub fn lab_to_rgb(l: f32, a: f32, b: f32) -> [u8; 3] {
    let (l, a, b) = (f64::from(l), f64::from(a), f64::from(b));
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let inverse = |t: f64| {
        let delta = 6.0 / 29.0;
        if t > delta {
            t * t * t
        } else {
            3.0 * delta * delta * (t - 4.0 / 29.0)
        }
    };
    let x = 0.96422 * inverse(fx);
    let y = inverse(fy);
    let z = 0.82521 * inverse(fz);
    let red = 3.1338561 * x - 1.6168667 * y - 0.4906146 * z;
    let green = -0.9787684 * x + 1.9161415 * y + 0.0334540 * z;
    let blue = 0.0719453 * x - 0.2289914 * y + 1.4052427 * z;
    [srgb_encode(red), srgb_encode(green), srgb_encode(blue)]
}

fn srgb_encode(linear: f64) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 { 12.92 * linear } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

// --- PNG ---------------------------------------------------------------------

/// The unique colors of a PNG in reading order, at most 256. Indexed images use
/// their PLTE order instead of the pixels. Fully transparent pixels and entries
/// are skipped. Any depth and color type the png crate can normalize to 8 bits loads.
pub fn parse_png(bytes: &[u8]) -> Result<Palette, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|err| format!("Could not read this PNG: {err}"))?;
    let mut colors = UniqueColors::default();
    let info = reader.info();
    if info.color_type == png::ColorType::Indexed {
        let plte = info.palette.as_deref().ok_or("This indexed PNG has no palette.")?;
        let trns = info.trns.as_deref().unwrap_or(&[]);
        for (index, entry) in plte.chunks_exact(3).enumerate() {
            let alpha = trns.get(index).copied().unwrap_or(255);
            if alpha != 0 && !colors.push([entry[0], entry[1], entry[2]]) {
                break;
            }
        }
    } else {
        let mut buffer = vec![0u8; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut buffer).map_err(|err| format!("Could not read this PNG: {err}"))?;
        let channels = frame.color_type.samples();
        let pixels = &buffer[..(frame.line_size * frame.height as usize).min(buffer.len())];
        for pixel in pixels.chunks_exact(channels) {
            let color = match frame.color_type {
                png::ColorType::Grayscale => [pixel[0]; 3],
                png::ColorType::GrayscaleAlpha => {
                    if pixel[1] == 0 {
                        continue;
                    }
                    [pixel[0]; 3]
                }
                png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2]],
                png::ColorType::Rgba => {
                    if pixel[3] == 0 {
                        continue;
                    }
                    [pixel[0], pixel[1], pixel[2]]
                }
                png::ColorType::Indexed => return Err("Could not expand this indexed PNG.".into()),
            };
            if !colors.push(color) {
                break;
            }
        }
    }
    if colors.order.is_empty() {
        return Err("This PNG has no opaque pixels to take colors from.".into());
    }
    Ok(Palette::new("", colors.order))
}

/// Colors in first-seen order, stopping at 256.
#[derive(Default)]
struct UniqueColors {
    seen: HashSet<[u8; 3]>,
    order: Vec<[u8; 3]>,
}

impl UniqueColors {
    /// False once the palette is full.
    fn push(&mut self, color: [u8; 3]) -> bool {
        if self.seen.insert(color) {
            self.order.push(color);
        }
        self.order.len() < 256
    }
}

/// A one-pixel-tall RGBA strip, one pixel per color. Aseprite and Lospec read these.
/// An empty palette becomes a single transparent pixel.
pub fn write_png(palette: &Palette) -> Vec<u8> {
    let width = palette.colors.len().max(1) as u32;
    let mut pixels = Vec::with_capacity(width as usize * 4);
    for [r, g, b] in &palette.colors {
        pixels.extend_from_slice(&[*r, *g, *b, 255]);
    }
    pixels.resize(width as usize * 4, 0);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Writing into a Vec cannot fail short of running out of memory.
        if let Ok(mut writer) = encoder.write_header() {
            let _ = writer.write_image_data(&pixels);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Write by extension: `.gpl` (also the default when the extension is unknown),
/// `.hex`, `.pal` (JASC), `.txt` (Paint.NET), `.act`, `.png`. Aseprite reads
/// `.gpl`, `.hex`, `.pal` and `.act`, so saving round-trips into Aseprite.
pub fn save_file(palette: &Palette, path: &Path) -> Result<(), String> {
    if palette.colors.is_empty() {
        return Err("The palette has no colors to save.".into());
    }
    let extension = path.extension().map(|ext| ext.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let bytes = match extension.as_str() {
        "hex" => write_hex(palette).into_bytes(),
        "pal" => write_pal(palette).into_bytes(),
        "txt" => write_txt(palette).into_bytes(),
        "act" => write_act(palette),
        "png" => write_png(palette),
        "ase" | "aseprite" => {
            return Err("Saving Aseprite sprites is not supported. Save a .gpl, .hex, .pal or .act instead; Aseprite opens those.".into());
        }
        _ => write_gpl(palette).into_bytes(),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    std::fs::write(path, bytes).map_err(|err| format!("Could not write {}: {err}", path.display()))
}

// ---------------------------------------------------------------------------
// Bytes and text helpers
// ---------------------------------------------------------------------------

fn u16_le(bytes: &[u8], at: usize) -> Option<u16> {
    bytes.get(at..at + 2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
}

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|quad| u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

fn u16_be(bytes: &[u8], at: usize) -> Option<u16> {
    bytes.get(at..at + 2).map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
}

fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|quad| u32::from_be_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

fn f32_be(bytes: &[u8], at: usize) -> Option<f32> {
    bytes.get(at..at + 4).map(|quad| f32::from_be_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

/// Text files carry no zero bytes in their first few kilobytes; UTF-16 files start with a BOM.
fn looks_like_text(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) || !bytes.iter().take(4096).any(|&byte| byte == 0)
}

/// Bytes as text: UTF-8 (lossy, BOM dropped) or UTF-16 when a BOM says so.
fn text_of(bytes: &[u8]) -> String {
    let utf16 = |body: &[u8], read: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = body.chunks_exact(2).map(|pair| read([pair[0], pair[1]])).collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(body) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(body, u16::from_le_bytes);
    }
    if let Some(body) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(body, u16::from_be_bytes);
    }
    String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(colors: &[[u8; 3]]) -> Vec<[u8; 3]> {
        colors.to_vec()
    }

    // --- fixtures -----------------------------------------------------------

    fn aseprite_sprite(ncolors: u16, chunks: &[(u16, Vec<u8>)], use_new_count: bool) -> Vec<u8> {
        let mut body = Vec::new();
        for (kind, data) in chunks {
            body.extend_from_slice(&((6 + data.len()) as u32).to_le_bytes());
            body.extend_from_slice(&kind.to_le_bytes());
            body.extend_from_slice(data);
        }
        let frame_size = (16 + body.len()) as u32;
        let mut out = vec![0u8; 128];
        out[0..4].copy_from_slice(&(128 + frame_size).to_le_bytes());
        out[4..6].copy_from_slice(&0xA5E0u16.to_le_bytes());
        out[6..8].copy_from_slice(&1u16.to_le_bytes());
        out[8..10].copy_from_slice(&1u16.to_le_bytes());
        out[10..12].copy_from_slice(&1u16.to_le_bytes());
        out[12..14].copy_from_slice(&8u16.to_le_bytes());
        out[32..34].copy_from_slice(&ncolors.to_le_bytes());
        out.extend_from_slice(&frame_size.to_le_bytes());
        out.extend_from_slice(&0xF1FAu16.to_le_bytes());
        let count = chunks.len();
        if use_new_count {
            out.extend_from_slice(&0xFFFFu16.to_le_bytes());
        } else {
            out.extend_from_slice(&(count as u16).to_le_bytes());
        }
        out.extend_from_slice(&100u16.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(if use_new_count { count as u32 } else { 0 }).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn new_palette_chunk(entries: &[([u8; 4], Option<&str>)]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(entries.len() as u32 - 1).to_le_bytes());
        data.extend_from_slice(&[0u8; 8]);
        for (rgba, name) in entries {
            data.extend_from_slice(&(if name.is_some() { 1u16 } else { 0 }).to_le_bytes());
            data.extend_from_slice(rgba);
            if let Some(name) = name {
                data.extend_from_slice(&(name.len() as u16).to_le_bytes());
                data.extend_from_slice(name.as_bytes());
            }
        }
        data
    }

    fn old_palette_chunk(colors: &[[u8; 3]]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes());
        data.push(0);
        data.push(if colors.len() == 256 { 0 } else { colors.len() as u8 });
        for color in colors {
            data.extend_from_slice(color);
        }
        data
    }

    fn asef_name(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let units: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        out.extend_from_slice(&(units.len() as u16).to_be_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_be_bytes());
        }
        out
    }

    fn asef_file(blocks: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"ASEF".to_vec();
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&(blocks.len() as u32).to_be_bytes());
        for (kind, data) in blocks {
            out.extend_from_slice(&kind.to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
        }
        out
    }

    fn asef_color_block(name: &str, model: &[u8; 4], values: &[f32]) -> (u16, Vec<u8>) {
        let mut data = asef_name(name);
        data.extend_from_slice(model);
        for value in values {
            data.extend_from_slice(&value.to_be_bytes());
        }
        data.extend_from_slice(&2u16.to_be_bytes());
        (0x0001, data)
    }

    fn riff_pal(entries: &[[u8; 3]]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&0x0300u16.to_le_bytes());
        data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for entry in entries {
            data.extend_from_slice(entry);
            data.push(0);
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&((4 + 8 + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"PAL ");
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    fn png_bytes(color: png::ColorType, depth: png::BitDepth, width: u32, pixels: &[u8], plte: Option<(&[u8], &[u8])>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, 1);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if let Some((palette, trns)) = plte {
            encoder.set_palette(palette.to_vec());
            if !trns.is_empty() {
                encoder.set_trns(trns.to_vec());
            }
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
        drop(writer);
        out
    }

    // --- presets -------------------------------------------------------------

    #[test]
    fn presets_have_the_right_sizes_and_unique_names() {
        let presets = presets();
        let sizes: Vec<(&str, usize)> = presets.iter().map(|palette| (palette.name.as_str(), palette.len())).collect();
        assert_eq!(
            sizes,
            vec![
                ("Kool Skull", 16),
                ("DB32", 32),
                ("DB16", 16),
                ("Sweetie 16", 16),
                ("PICO-8", 16),
                ("Endesga 32", 32),
                ("Game Boy", 4),
                ("Game Boy Pocket", 4),
                ("NES", 55),
                ("Commodore 64", 16),
                ("CGA", 16),
                ("ZX Spectrum", 15),
                ("Apple II", 16),
                ("Greys", 16),
            ]
        );
        let names: HashSet<&str> = presets.iter().map(|palette| palette.name.as_str()).collect();
        assert_eq!(names.len(), presets.len());
        assert!(presets.iter().all(|palette| !palette.is_empty()));
    }

    #[test]
    fn preset_values_match_the_published_tables() {
        let presets = presets();
        assert_eq!(presets[0].colors[12], [0xFF, 0x3E, 0xB8]);
        assert_eq!(presets[1].colors[1], [0x22, 0x20, 0x34]);
        assert_eq!(presets[1].colors[31], [0x8A, 0x6F, 0x30]);
        assert_eq!(presets[4].colors[8], [0xFF, 0x00, 0x4D]);
        assert_eq!(presets[6].colors, named(&[[15, 56, 15], [48, 98, 48], [139, 172, 15], [155, 188, 15]]));
        let nes = &presets[8];
        assert_eq!(nes.colors[0], [0x7C, 0x7C, 0x7C]);
        assert_eq!(nes.colors.iter().filter(|color| **color == [0, 0, 0]).count(), 1);
        assert_eq!(nes.colors[54], [0xF8, 0xD8, 0xF8]);
        let greys = &presets[13];
        assert_eq!(greys.colors[0], [0, 0, 0]);
        assert_eq!(greys.colors[1], [17, 17, 17]);
        assert_eq!(greys.colors[15], [255, 255, 255]);
        for palette in &presets {
            if palette.name != "Apple II" {
                let unique: HashSet<_> = palette.colors.iter().collect();
                assert_eq!(unique.len(), palette.len(), "{} repeats a color", palette.name);
            }
        }
    }

    // --- gpl -----------------------------------------------------------------

    #[test]
    fn gimp_palettes_read_names_comments_tabs_and_missing_names() {
        let text = "GIMP Palette\nName: Night Shift\nColumns: 4\n#\n# a comment\n  0   0   0\tBlack\n255\t255\t255 White\n 12  34  56\n300 -5 7 Clamped\n";
        let palette = parse(text.as_bytes(), "whatever.gpl").unwrap();
        assert_eq!(palette.name, "Night Shift");
        assert_eq!(palette.colors, named(&[[0, 0, 0], [255, 255, 255], [12, 34, 56], [255, 0, 7]]));
    }

    #[test]
    fn aseprite_gimp_palettes_carry_alpha_and_lospec_names_live_in_comments() {
        let text = "GIMP Palette\nChannels: RGBA\n#\n  0   0   0 255\tUntitled\n255   0   0 128\tUntitled\n";
        let palette = parse(text.as_bytes(), "/tmp/export.gpl").unwrap();
        assert_eq!(palette.name, "export");
        assert_eq!(palette.colors, named(&[[0, 0, 0], [255, 0, 0]]));

        let lospec = "GIMP Palette\n#Palette Name: Sweetie 16\n#Description: by GrafxKid\n#Colors: 2\n26\t28\t44\t1a1c2c\n93\t39\t93\t5d275d\n";
        let palette = parse(lospec.as_bytes(), "sweetie-16-1x.gpl").unwrap();
        assert_eq!(palette.name, "Sweetie 16");
        assert_eq!(palette.colors, named(&[[26, 28, 44], [93, 39, 93]]));
    }

    #[test]
    fn a_gimp_palette_without_rows_is_an_error_in_plain_english() {
        let err = parse(b"GIMP Palette\nName: Empty\n", "empty.gpl").unwrap_err();
        assert!(err.contains("No colors"), "{err}");
        let err = parse(b"not a palette\n1 2 3\n", "bad.gpl").unwrap_err();
        assert!(err.contains("GIMP Palette"), "{err}");
    }

    // --- hex and txt ---------------------------------------------------------

    #[test]
    fn hex_lists_take_hashes_alpha_blank_lines_and_comments() {
        let text = "\u{feff}# Lospec export\n\n#1a1c2c\n5D275D\nb13e5380 ; alpha dropped\n0xef7d57\n// trailing comment\n";
        let palette = parse(text.as_bytes(), "lospec.hex").unwrap();
        assert_eq!(palette.name, "lospec");
        assert_eq!(palette.colors, named(&[[0x1A, 0x1C, 0x2C], [0x5D, 0x27, 0x5D], [0xB1, 0x3E, 0x53], [0xEF, 0x7D, 0x57]]));
    }

    #[test]
    fn paint_net_palettes_are_aarrggbb_with_semicolon_comments() {
        let text = "; paint.net Palette File\n; Palette Name: Dusk\n; Colors: 3\nFF102030\n80FFFFFF\n445566\n";
        let palette = parse(text.as_bytes(), "dusk.txt").unwrap();
        assert_eq!(palette.name, "Dusk");
        assert_eq!(palette.colors, named(&[[0x10, 0x20, 0x30], [0xFF, 0xFF, 0xFF], [0x44, 0x55, 0x66]]));
        // A Paint.NET file announces itself, so a .hex name does not flip the alpha slot.
        let misnamed = parse(text.as_bytes(), "dusk.hex").unwrap();
        assert_eq!(misnamed.colors[0], [0x10, 0x20, 0x30]);
    }

    // --- pal -----------------------------------------------------------------

    #[test]
    fn jasc_palettes_read_with_crlf_and_honour_the_count() {
        let text = "JASC-PAL\r\n0100\r\n2\r\n0 0 0\r\n255 128 64\r\n9 9 9\r\n";
        let palette = parse(text.as_bytes(), "C:\\pals\\two.pal").unwrap();
        assert_eq!(palette.colors, named(&[[0, 0, 0], [255, 128, 64]]));
        assert_eq!(parse(text.as_bytes(), "two.pal").unwrap().name, "two");
    }

    #[test]
    fn riff_palettes_read_their_data_chunk() {
        let bytes = riff_pal(&[[1, 2, 3], [4, 5, 6], [7, 8, 9]]);
        let palette = parse(&bytes, "windows.pal").unwrap();
        assert_eq!(palette.colors, named(&[[1, 2, 3], [4, 5, 6], [7, 8, 9]]));
        let err = parse(b"nothing to see here\n", "odd.pal").unwrap_err();
        assert!(err.contains("neither a JASC palette nor a Microsoft RIFF palette"), "{err}");
    }

    // --- act -----------------------------------------------------------------

    #[test]
    fn color_tables_honour_the_count_and_trim_photoshop_padding() {
        let mut long = vec![0u8; 768];
        long[0..6].copy_from_slice(&[10, 20, 30, 40, 50, 60]);
        long.extend_from_slice(&[0, 2, 0xFF, 0xFF]);
        assert_eq!(parse(&long, "two.act").unwrap().colors, named(&[[10, 20, 30], [40, 50, 60]]));

        let mut padded = vec![0u8; 768];
        padded[0..9].copy_from_slice(&[0, 0, 0, 7, 7, 7, 0, 0, 0]);
        let palette = parse(&padded, "padded.act").unwrap();
        assert_eq!(palette.colors, named(&[[0, 0, 0], [7, 7, 7]]));

        let all_black = vec![0u8; 768];
        assert_eq!(parse(&all_black, "black.act").unwrap().colors, named(&[[0, 0, 0]]));

        let short = [1u8, 2, 3, 4, 5, 6];
        assert_eq!(parse(&short, "short.act").unwrap().colors, named(&[[1, 2, 3], [4, 5, 6]]));
        assert!(parse(&[1u8, 2, 3, 4], "broken.act").is_err());

        // Bytes 4 and 5 happen to spell the Aseprite magic; the extension still wins.
        let mut unlucky = vec![0u8; 772];
        unlucky[0..6].copy_from_slice(&[1, 2, 3, 4, 0xE0, 0xA5]);
        unlucky[769] = 2;
        assert_eq!(parse(&unlucky, "unlucky.act").unwrap().colors, named(&[[1, 2, 3], [4, 0xE0, 0xA5]]));
    }

    // --- aseprite ------------------------------------------------------------

    #[test]
    fn aseprite_sprites_prefer_the_new_palette_chunk_and_skip_names() {
        let new_chunk = new_palette_chunk(&[
            ([10, 20, 30, 255], Some("Dusk")),
            ([40, 50, 60, 0], None),
            ([70, 80, 90, 255], Some("A longer swatch name")),
        ]);
        let old_chunk = old_palette_chunk(&[[1, 1, 1], [2, 2, 2], [3, 3, 3]]);
        let color_profile = (0x2007u16, vec![0u8; 16]);
        let bytes = aseprite_sprite(3, &[color_profile, (0x0004, old_chunk), (0x2019, new_chunk)], false);
        let palette = parse(&bytes, "swatches.aseprite").unwrap();
        assert_eq!(palette.name, "swatches");
        assert_eq!(palette.colors, named(&[[10, 20, 30], [40, 50, 60], [70, 80, 90]]));
    }

    #[test]
    fn aseprite_sprites_fall_back_to_old_palette_chunks() {
        let colors: Vec<[u8; 3]> = (0..256u32).map(|index| [index as u8, 0, 255 - index as u8]).collect();
        let bytes = aseprite_sprite(0, &[(0x0004, old_palette_chunk(&colors))], true);
        let palette = parse(&bytes, "old.ase").unwrap();
        assert_eq!(palette.colors, colors);

        let bytes = aseprite_sprite(4, &[(0x0004, old_palette_chunk(&colors))], false);
        assert_eq!(parse(&bytes, "four.ase").unwrap().len(), 4);

        let six_bit = aseprite_sprite(3, &[(0x0011, old_palette_chunk(&[[0, 32, 63], [63, 63, 63], [1, 2, 3]]))], false);
        let palette = parse(&six_bit, "vga.ase").unwrap();
        assert_eq!(palette.colors, named(&[[0, 130, 255], [255, 255, 255], [4, 8, 12]]));
    }

    #[test]
    fn a_sprite_without_a_palette_or_frames_is_refused() {
        let bytes = aseprite_sprite(1, &[(0x2007, vec![0u8; 16])], false);
        assert!(parse(&bytes, "blank.aseprite").unwrap_err().contains("no palette"));
        let mut no_frames = aseprite_sprite(1, &[], false);
        no_frames[6..8].copy_from_slice(&0u16.to_le_bytes());
        assert!(parse(&no_frames, "none.aseprite").unwrap_err().contains("no frames"));
    }

    // --- asef ----------------------------------------------------------------

    #[test]
    fn swatch_exchange_files_convert_every_color_model() {
        let bytes = asef_file(&[
            (0xC001, asef_name("Brand")),
            asef_color_block("Red", b"RGB ", &[1.0, 0.0, 0.0]),
            asef_color_block("Cyan ink", b"CMYK", &[1.0, 0.0, 0.0, 0.0]),
            asef_color_block("Half", b"Gray", &[0.5]),
            asef_color_block("Paper", b"LAB ", &[1.0, 0.0, 0.0]),
            asef_color_block("Mid", b"LAB ", &[0.5, 0.0, 0.0]),
            (0xC002, Vec::new()),
        ]);
        let palette = parse(&bytes, "brand.ase").unwrap();
        assert_eq!(palette.name, "Brand");
        assert_eq!(palette.colors[0], [255, 0, 0]);
        assert_eq!(palette.colors[1], [0, 255, 255]);
        assert_eq!(palette.colors[2], [128, 128, 128]);
        assert_eq!(palette.colors[3], [255, 255, 255]);
        let mid = palette.colors[4];
        assert!(mid.iter().all(|channel| (118..=120).contains(channel)), "{mid:?}");
    }

    #[test]
    fn an_ase_that_is_really_a_swatch_file_is_sniffed_not_trusted_by_extension() {
        let bytes = asef_file(&[asef_color_block("Only", b"RGB ", &[0.0, 0.5, 1.0])]);
        let palette = parse(&bytes, "exported.ase").unwrap();
        assert_eq!(palette.name, "exported");
        assert_eq!(palette.colors, named(&[[0, 128, 255]]));
        let err = parse(b"ASEF\0\x01\0\0\0\0\0\0", "empty.ase").unwrap_err();
        assert!(err.contains("No colors"), "{err}");
        let err = parse(b"garbage", "mystery.ase").unwrap_err();
        assert!(err.contains("neither an Aseprite sprite nor an Adobe Swatch Exchange file"), "{err}");
    }

    #[test]
    fn lab_to_srgb_uses_the_d50_white() {
        assert_eq!(lab_to_rgb(100.0, 0.0, 0.0), [255, 255, 255]);
        assert_eq!(lab_to_rgb(0.0, 0.0, 0.0), [0, 0, 0]);
        let red = lab_to_rgb(54.29, 80.81, 69.89);
        assert!(red[0] >= 252 && red[1] <= 3 && red[2] <= 3, "{red:?}");
    }

    // --- png -----------------------------------------------------------------

    #[test]
    fn png_images_collect_unique_opaque_colors_in_reading_order() {
        let pixels = [255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 9, 9, 9, 0, 0, 0, 255, 255];
        let bytes = png_bytes(png::ColorType::Rgba, png::BitDepth::Eight, 5, &pixels, None);
        let palette = parse(&bytes, "strip.png").unwrap();
        assert_eq!(palette.name, "strip");
        assert_eq!(palette.colors, named(&[[255, 0, 0], [0, 255, 0], [0, 0, 255]]));

        let grey = png_bytes(png::ColorType::Grayscale, png::BitDepth::Sixteen, 2, &[0x80, 0x00, 0xFF, 0xFF], None);
        assert_eq!(parse(&grey, "grey.png").unwrap().colors, named(&[[128, 128, 128], [255, 255, 255]]));
    }

    #[test]
    fn indexed_png_images_use_their_plte_order() {
        let plte = [10, 10, 10, 20, 20, 20, 30, 30, 30, 20, 20, 20];
        let trns = [0, 255];
        let bytes = png_bytes(png::ColorType::Indexed, png::BitDepth::Eight, 2, &[3, 3], Some((&plte, &trns)));
        let palette = parse(&bytes, "indexed.png").unwrap();
        assert_eq!(palette.colors, named(&[[20, 20, 20], [30, 30, 30]]));
    }

    #[test]
    fn a_png_with_only_transparent_pixels_is_an_error() {
        let bytes = png_bytes(png::ColorType::Rgba, png::BitDepth::Eight, 1, &[1, 2, 3, 0], None);
        assert!(parse(&bytes, "clear.png").unwrap_err().contains("no opaque pixels"));
    }

    // --- writers -------------------------------------------------------------

    fn sample() -> Palette {
        Palette::new("Round Trip", vec![[0, 0, 0], [255, 255, 255], [0x12, 0x34, 0x56], [200, 100, 50]])
    }

    #[test]
    fn writers_round_trip_through_the_parsers() {
        let palette = sample();
        let gpl = parse(write_gpl(&palette).as_bytes(), "x.gpl").unwrap();
        assert_eq!(gpl, palette);
        let hex = parse(write_hex(&palette).as_bytes(), "Round Trip.hex").unwrap();
        assert_eq!(hex, palette);
        let pal = parse(write_pal(&palette).as_bytes(), "Round Trip.pal").unwrap();
        assert_eq!(pal, palette);
        let txt = parse(write_txt(&palette).as_bytes(), "x.txt").unwrap();
        assert_eq!(txt, palette);
        let act = parse(&write_act(&palette), "Round Trip.act").unwrap();
        assert_eq!(act, palette);
        let png = parse(&write_png(&palette), "Round Trip.png").unwrap();
        assert_eq!(png, palette);
    }

    #[test]
    fn the_gimp_writer_speaks_gimp_and_aseprite() {
        let text = write_gpl(&sample());
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("GIMP Palette"));
        assert_eq!(lines.next(), Some("Name: Round Trip"));
        assert_eq!(lines.next(), Some("Columns: 4"));
        assert_eq!(lines.next(), Some("#"));
        assert_eq!(lines.next(), Some("  0   0   0\t000000"));
        assert_eq!(write_hex(&sample()).lines().nth(2), Some("123456"));
        assert_eq!(write_pal(&sample()).lines().take(4).collect::<Vec<_>>(), vec!["JASC-PAL", "0100", "4", "0 0 0"]);
        let act = write_act(&sample());
        assert_eq!(act.len(), 772);
        assert_eq!(&act[768..], &[0, 4, 0xFF, 0xFF]);
    }

    #[test]
    fn the_color_table_writer_cuts_at_256() {
        let big = Palette::new("Big", (0..300u32).map(|index| [index as u8, 1, 2]).collect());
        let act = write_act(&big);
        assert_eq!(&act[768..770], &[1, 0]);
        assert_eq!(parse(&act, "big.act").unwrap().len(), 256);
    }

    // --- parse and files -----------------------------------------------------

    #[test]
    fn misnamed_files_still_load_by_content() {
        let gpl = write_gpl(&sample());
        assert_eq!(parse(gpl.as_bytes(), "colors.hex").unwrap(), sample());
        assert_eq!(parse(gpl.as_bytes(), "colors.txt").unwrap(), sample());
        assert_eq!(parse(gpl.as_bytes(), "colors").unwrap(), sample());
        let hex = write_hex(&sample());
        assert_eq!(parse(hex.as_bytes(), "colors.gpl").unwrap().colors, sample().colors);
        assert_eq!(parse(hex.as_bytes(), "no-extension").unwrap().name, "no-extension");
        let jasc = write_pal(&sample());
        assert_eq!(parse(jasc.as_bytes(), "colors.hex").unwrap().colors, sample().colors);
        assert_eq!(parse(&write_png(&sample()), "colors.gpl").unwrap().colors, sample().colors);
        assert_eq!(parse(&write_act(&sample()), "mystery").unwrap().colors, sample().colors);
    }

    #[test]
    fn unknown_files_are_refused_in_plain_english() {
        let err = parse(b"hello there\n", "notes.md").unwrap_err();
        assert_eq!(err, ".md files are not palettes the studio can read.");
        let err = parse(b"hello there\n", "notes").unwrap_err();
        assert_eq!(err, "This file is not a palette the studio can read.");
        assert!(parse(b"", "empty.hex").unwrap_err().contains("No colors"));
        assert_eq!(parse(b"", "").unwrap_err(), "This file is not a palette the studio can read.");
    }

    #[test]
    fn files_save_and_load_in_every_written_format() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let folder = std::env::temp_dir().join(format!("vmi-palette-{stamp}"));
        for extension in ["gpl", "hex", "pal", "txt", "act", "png", "palette"] {
            let path = folder.join(format!("Round Trip.{extension}"));
            save_file(&sample(), &path).unwrap();
            let loaded = load_file(&path).unwrap();
            assert_eq!(loaded, sample(), "{extension}");
        }
        let err = save_file(&sample(), &folder.join("x.aseprite")).unwrap_err();
        assert!(err.contains("not supported"), "{err}");
        assert!(save_file(&Palette::new("Empty", Vec::new()), &folder.join("empty.gpl")).is_err());
        assert!(load_file(&folder.join("missing.gpl")).unwrap_err().contains("Could not read"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn extensions_cover_every_reader() {
        assert_eq!(EXTENSIONS, &["gpl", "hex", "pal", "txt", "act", "ase", "aseprite", "png"]);
        assert!(EXTENSIONS.iter().all(|ext| ext.chars().all(|ch| ch.is_ascii_lowercase())));
    }
}
