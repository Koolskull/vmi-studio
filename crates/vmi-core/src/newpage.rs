//! A new picture. A preset fills the pixel size. The two fields are the
//! custom size. A port of the model half of `vmi_studio/newpage.py`.
//!
//! The working plate is 1920 by 1080. Larger presets are in the list. Each
//! side stops at 8192 pixels, which is the largest plate the composite will
//! hold.
//!
//! Every new picture has a Paper layer under an empty Ink layer. The 2kool
//! grid preset adds the cyan template from the Clip Studio file, on top of Ink.

use std::path::Path;

use crate::color::{hex_of, parse_hex};
use crate::doc::{Art, Node};
use crate::koolgrid::{grid_raster, PLATE};

/// Name, width, height. The first row is the working plate.
pub const PRESETS: &[(&str, u32, u32)] = &[
    ("HD 1080", 1920, 1080),
    ("HD 720", 1280, 720),
    ("QHD", 2560, 1440),
    ("4K", 3840, 2160),
    ("Square", 1080, 1080),
    ("Portrait", 1080, 1920),
    ("Story", 720, 1280),
    ("Square 2K", 2048, 2048),
    ("2kool grid", 3840, 3840),
];

pub const GRID_PRESET: &str = "2kool grid";
pub const PAPER_WHITE: &str = "#FFFFFF";
pub const PAPER_GREY: &str = "#808080";
pub const PAPER_BLACK: &str = "#000000";
pub const PAPER_CHOICES: &[(&str, &str)] = &[("White", PAPER_WHITE), ("Grey", PAPER_GREY), ("Black", PAPER_BLACK)];

pub const MIN_EDGE: u32 = 1;
pub const MAX_EDGE: u32 = 8192;

/// Pixel size, or the message Python raises when a side is outside 1 to 8192.
pub fn parse_plate(width: i64, height: i64) -> Result<(u32, u32), String> {
    let low = i64::from(MIN_EDGE);
    let high = i64::from(MAX_EDGE);
    if width < low || height < low || width > high || height > high {
        return Err(format!("Each side is {MIN_EDGE} to {MAX_EDGE} pixels."));
    }
    Ok((width as u32, height as u32))
}

/// The preset name for this size, or an empty string when it is custom.
pub fn match_preset(width: u32, height: u32) -> &'static str {
    PRESETS.iter().find(|(_, wide, high)| *wide == width && *high == height).map(|(name, ..)| *name).unwrap_or("")
}

/// `#RRGGBB` for a paper choice, falling back to white.
pub fn paper_hex(background: &str) -> String {
    parse_hex(background).map(hex_of).unwrap_or_else(|| PAPER_WHITE.to_string())
}

fn paper_pixels(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for _ in 0..(width as usize) * (height as usize) {
        pixels.extend_from_slice(&[color[0], color[1], color[2], 255]);
    }
    pixels
}

/// A picture file resized to the plate, Lanczos like Python's PIL call.
pub fn texture_pixels(width: u32, height: u32, path: &Path) -> Result<Vec<u8>, String> {
    let image = image::open(path).map_err(|_| "That texture could not be read.".to_string())?;
    let rgba = if image.width() == width && image.height() == height {
        image.into_rgba8()
    } else {
        image::imageops::resize(&image.into_rgba8(), width, height, image::imageops::FilterType::Lanczos3)
    };
    Ok(rgba.into_raw())
}

/// An unsaved picture. Paper is the back. Ink is the empty draw layer.
///
/// `grid` adds the 2kool template, and only on the 3840 plate that template
/// was measured from. Save writes the blueprint.
pub fn blank_picture(
    width: i64,
    height: i64,
    name: &str,
    background: &str,
    texture: Option<&Path>,
    grid: bool,
) -> Result<Art, String> {
    let (wide, high) = parse_plate(width, height)?;
    let title = if name.trim().is_empty() { format!("Untitled {wide}×{high}") } else { name.trim().to_string() };
    let color = paper_hex(background);
    let pixels = match texture {
        Some(path) => texture_pixels(wide, high, path)?,
        None => paper_pixels(wide, high, parse_hex(&color).unwrap_or([255, 255, 255])),
    };
    let mut paper = Node::layer("n0", "Paper", 0, 0);
    paper.pixels = pixels;
    paper.color = color;
    let ink = Node::layer("n1", "Ink", wide, high);
    let mut art = Art::blank(wide, high);
    art.file_name = title;
    art.layers = vec![paper, ink];
    if grid && wide == PLATE && high == PLATE {
        let mut grid = Node::layer("n2", "2Kool Grid", 0, 0);
        grid.pixels = grid_raster();
        art.layers.push(grid);
    }
    art.next_id = art.layers.len() as u64;
    Ok(art)
}

/// The status line Python shows after a new picture opens.
pub fn opened_note(art: &Art) -> String {
    let mut note = format!("New picture {}×{}. Draw on Ink.", art.width, art.height);
    if art.layers.iter().any(|node| node.name == "2Kool Grid") {
        note.push_str(" The 2kool grid sits on top.");
    }
    note.push_str(" Save blueprint writes the file.");
    note
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_picture_is_paper_under_ink() {
        let art = blank_picture(16, 8, "", "#808080", None, false).unwrap();
        assert_eq!(art.file_name, "Untitled 16×8");
        assert_eq!((art.width, art.height), (16, 8));
        let names: Vec<&str> = art.layers.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(names, ["Paper", "Ink"]);
        assert_eq!(art.layers[0].id, "n0");
        assert_eq!(art.layers[0].color, "#808080");
        assert_eq!(&art.layers[0].pixels[..4], &[128, 128, 128, 255]);
        assert_eq!(art.layers[0].pixels.len(), 16 * 8 * 4);
        assert!(art.layers[1].pixels.iter().all(|byte| *byte == 0));
        assert_eq!(art.next_id, 2);
        assert_eq!(opened_note(&art), "New picture 16×8. Draw on Ink. Save blueprint writes the file.");
    }

    #[test]
    fn the_grid_only_lands_on_the_3840_plate() {
        let plain = blank_picture(1920, 1080, "", PAPER_WHITE, None, true).unwrap();
        assert_eq!(plain.layers.len(), 2);
        let grid = blank_picture(3840, 3840, "", PAPER_WHITE, None, true).unwrap();
        assert_eq!(grid.layers.len(), 3);
        assert_eq!(grid.layers[2].name, "2Kool Grid");
        assert_eq!(grid.layers[2].id, "n2");
        assert_eq!(grid.next_id, 3);
        let at = ((381 * 3840 + 1277) * 4) as usize;
        assert_eq!(&grid.layers[2].pixels[at..at + 4], &[44, 176, 255, 255]);
        assert!(opened_note(&grid).contains("The 2kool grid sits on top."));
    }

    #[test]
    fn sizes_and_presets_follow_python() {
        assert_eq!(parse_plate(0, 10).unwrap_err(), "Each side is 1 to 8192 pixels.");
        assert!(parse_plate(8193, 10).is_err());
        assert_eq!(parse_plate(1, 8192).unwrap(), (1, 8192));
        assert_eq!(match_preset(3840, 3840), GRID_PRESET);
        assert_eq!(match_preset(1920, 1080), "HD 1080");
        assert_eq!(match_preset(7, 7), "");
        assert_eq!(paper_hex("junk"), PAPER_WHITE);
        assert_eq!(paper_hex("#abc"), "#AABBCC");
        assert_eq!(PRESETS.len(), 9);
    }
}
