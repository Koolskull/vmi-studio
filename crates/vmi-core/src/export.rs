//! PNG export. Percent is 1 to 100. The scale is nearest, so dither stays square.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use crate::doc::{present_plain, Art};

pub fn scale_nearest(src: &[u8], width: u32, height: u32, percent: u32) -> (u32, u32, Vec<u8>) {
    let percent = percent.clamp(1, 100);
    let dst_w = ((u64::from(width) * u64::from(percent)) / 100).max(1) as u32;
    let dst_h = ((u64::from(height) * u64::from(percent)) / 100).max(1) as u32;
    let mut dst = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    if width == 0 || height == 0 || src.len() < (width as usize) * (height as usize) * 4 {
        return (dst_w, dst_h, dst);
    }
    for y in 0..dst_h {
        let sy = ((u64::from(y) * u64::from(height)) / u64::from(dst_h)) as u32;
        for x in 0..dst_w {
            let sx = ((u64::from(x) * u64::from(width)) / u64::from(dst_w)) as u32;
            let from = ((sy * width + sx) * 4) as usize;
            let to = ((y * dst_w + x) * 4) as usize;
            dst[to..to + 4].copy_from_slice(&src[from..from + 4]);
        }
    }
    (dst_w, dst_h, dst)
}

/// The picture at the playhead, never with onion skin.
pub fn export_png(art: &mut Art, path: &Path, percent: u32) -> Result<(), String> {
    let plate = present_plain(art);
    let (width, height, pixels) = scale_nearest(&plate, art.width, art.height, percent);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    let file = File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer.write_image_data(&pixels).map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_scale_picks_the_nearest_pixel() {
        let mut src = vec![0u8; 2 * 2 * 4];
        src[0] = 255;
        src[3] = 255;
        let (w, h, dst) = scale_nearest(&src, 2, 2, 50);
        assert_eq!((w, h), (1, 1));
        assert_eq!(dst[0], 255);
        assert_eq!(dst[3], 255);
    }
}
