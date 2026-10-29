//! The load gate. A port of `vmi_studio/loadgate.py`.
//!
//! A black veil at 158/255 covers the studio and eats input. The panel is the
//! dithered liquid picture, the file name, LOADING with stepping dots, the two
//! notes, and the ASCII bar. Catch-up mode keeps the picture and the bar and
//! hides the name line and the notes. The bar closes with the panel.

use std::time::Duration;

use eframe::egui;

use vmi_core::Theme;

const DIM: u8 = 158;
const WIDTH: usize = 96;
const HEIGHT: usize = 54;
const SCALE: f32 = 3.0;
const TONE: [[u8; 3]; 3] = [[0, 0, 0], [0x44, 0x44, 0x44], [0xCC, 0xCC, 0xCC]];
const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const GLYPHS: [&str; 8] = ["#", "0", "o", ",", "_", ",", "*", "%"];
const BAR_CAP: f32 = 0.92;
const BAR_STEP: f32 = 0.004;
const BAR_MS: f64 = 0.040;
const DITHER_MS: f64 = 0.070;
const DOT_MS: f64 = 0.300;
const DOTS: [&str; 3] = [".", "..", "..."];
const CELL: f32 = 8.0;
const PANEL_W: f32 = 340.0;

/// How full the bar may jump when a read step is named. Never the end.
pub fn note_fraction(text: &str) -> f32 {
    let lower = text.to_lowercase();
    if lower.contains("picture") || lower.contains("composite") || lower.contains("painting") {
        0.84
    } else if lower.contains("layer") {
        0.7
    } else if lower.contains("convert") {
        0.36
    } else if lower.contains("reading") || lower.contains("blueprint") {
        0.5
    } else {
        0.16
    }
}

/// One dithered frame. Each cell is 0 black, 1 #444444, or 2 #CCCCCC.
pub fn liquid_levels(tick: u32) -> Vec<u8> {
    let mut rows = Vec::with_capacity(WIDTH * HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            rows.push(level_at(tick, x as f64, y as f64));
        }
    }
    rows
}

fn hash(ix: i32, iy: i32, iz: i32) -> f64 {
    let mut n = (ix as u32)
        .wrapping_mul(374761393)
        .wrapping_add((iy as u32).wrapping_mul(668265263))
        .wrapping_add((iz as u32).wrapping_mul(1440673973));
    n ^= n >> 13;
    n = n.wrapping_mul(1274126177);
    f64::from(n & 255) / 255.0
}

fn fade(value: f64) -> f64 {
    value * value * (3.0 - 2.0 * value)
}

fn noise(x: f64, y: f64, z: f64) -> f64 {
    let x = x * 0.13;
    let y = y * 0.13;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let iz = z.floor() as i32;
    let fx = fade(x - f64::from(ix));
    let fy = fade(y - f64::from(iy));
    let fz = fade(z - f64::from(iz));
    let at = |dx, dy, dz| hash(ix + dx, iy + dy, iz + dz);
    let lerp = |a, b, t| a + (b - a) * t;
    let front = lerp(
        lerp(at(0, 0, 0), at(1, 0, 0), fx),
        lerp(at(0, 1, 0), at(1, 1, 0), fx),
        fy,
    );
    let back = lerp(
        lerp(at(0, 0, 1), at(1, 0, 1), fx),
        lerp(at(0, 1, 1), at(1, 1, 1), fx),
        fy,
    );
    lerp(front, back, fz)
}

fn level_at(tick: u32, x: f64, y: f64) -> u8 {
    let steps = (TONE.len() - 1) as f64;
    let t = f64::from(tick) * 0.17;
    let wave_y = 10.0 * (y * 0.11 + t).sin();
    let u = x + wave_y;
    let v = y + 8.0 * (x * 0.09 - t * 1.2).sin();
    let sample = noise(u, v, t * 2.0);
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let limit = (f64::from(BAYER[(iy & 3) as usize][(ix & 3) as usize]) + 0.5) / 16.0;
    let scaled = sample * steps;
    let mut level = scaled.floor() as i32;
    if scaled - f64::from(level) > limit {
        level += 1;
    }
    level.clamp(0, steps as i32) as u8
}

/// Bar text, and the index of the animated cell. None when the bar is full.
pub fn bar_line(fraction: f32, tick: u32, count: usize) -> (String, Option<usize>) {
    let count = count.max(1);
    let mut chars = vec!["."; count];
    let head = if fraction >= 1.0 {
        chars.fill("#");
        None
    } else {
        let filled = ((fraction * count as f32) + 1e-6).floor() as usize;
        let filled = filled.min(count);
        for cell in chars.iter_mut().take(filled) {
            *cell = "#";
        }
        let at = if filled < count { filled } else { count - 1 };
        chars[at] = GLYPHS[(tick as usize) % GLYPHS.len()];
        Some(at + 1)
    };
    let mut text = String::from("[");
    text.extend(chars);
    text.push(']');
    (text, head)
}

fn bar_count(width: f32) -> usize {
    let inner = width - CELL * 2.0;
    if inner < CELL * 8.0 {
        24
    } else {
        ((inner / CELL).floor() as usize).max(8)
    }
}

/// The modal veil over the studio while a drawing is read.
pub struct Gate {
    active: bool,
    catchup: bool,
    name: String,
    fraction: f32,
    target: f32,
    bar_tick: u32,
    dither_tick: u32,
    dot_index: usize,
    clock: bool,
    dither_at: f64,
    bar_at: f64,
    dot_at: f64,
    texture: Option<egui::TextureHandle>,
    painted_tick: Option<u32>,
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    pub fn new() -> Self {
        Self {
            active: false,
            catchup: false,
            name: String::new(),
            fraction: 0.0,
            target: 0.08,
            bar_tick: 0,
            dither_tick: 0,
            dot_index: 0,
            clock: false,
            dither_at: 0.0,
            bar_at: 0.0,
            dot_at: 0.0,
            texture: None,
            painted_tick: None,
        }
    }

    /// Open the gate for one file. `catchup` is the tab-switch mode: picture and bar only.
    pub fn start(&mut self, file_name: &str, catchup: bool) {
        self.active = true;
        self.catchup = catchup;
        self.name = file_name.to_string();
        self.fraction = 0.0;
        self.target = 0.08;
        self.bar_tick = 0;
        self.dither_tick = 0;
        self.dot_index = 0;
        self.clock = false;
        self.painted_tick = None;
    }

    /// A stage note from the loading thread. The bar jumps forward and never back.
    pub fn hear(&mut self, note: &str) {
        let value = note_fraction(note).clamp(0.0, BAR_CAP);
        if value > self.target {
            self.target = value;
        }
        if self.fraction < 1.0 && self.target > self.fraction {
            self.fraction = self.target;
        }
    }

    /// The picture is on screen. Fill the bar and close.
    pub fn complete(&mut self) {
        self.fraction = 1.0;
        self.active = false;
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// How full the bar is, 0 to 1.
    pub fn stage_fraction(&self) -> f32 {
        self.fraction
    }

    pub fn tick_interval(&self) -> Duration {
        Duration::from_millis(40)
    }

    fn step_bar(&mut self) {
        self.bar_tick = self.bar_tick.wrapping_add(1);
        if self.fraction < 1.0 {
            let mut next = (self.fraction + BAR_STEP).min(BAR_CAP);
            if self.target > next {
                next = self.target.min(BAR_CAP);
            }
            self.fraction = next;
        }
    }

    fn advance(&mut self, now: f64) {
        if !self.clock {
            self.clock = true;
            self.dither_at = now;
            self.bar_at = now;
            self.dot_at = now;
            return;
        }
        let dither_steps = ((now - self.dither_at) / DITHER_MS).floor() as u32;
        let dither_steps = dither_steps.min(8);
        if dither_steps > 0 {
            self.dither_tick = self.dither_tick.wrapping_add(dither_steps);
            self.dither_at += f64::from(dither_steps) * DITHER_MS;
        }
        let bar_steps = ((now - self.bar_at) / BAR_MS).floor() as u32;
        let bar_steps = bar_steps.min(8);
        for _ in 0..bar_steps {
            self.step_bar();
        }
        if bar_steps > 0 {
            self.bar_at += f64::from(bar_steps) * BAR_MS;
        }
        if !self.catchup {
            let dot_steps = ((now - self.dot_at) / DOT_MS).floor() as u32;
            let dot_steps = dot_steps.min(8);
            if dot_steps > 0 {
                self.dot_index = (self.dot_index + dot_steps as usize) % DOTS.len();
                self.dot_at += f64::from(dot_steps) * DOT_MS;
            }
        }
    }

    /// Paint the veil and the panel. True while the gate is up.
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, now: f64) -> bool {
        if !self.active {
            return false;
        }
        self.advance(now);
        let screen = ctx.screen_rect();
        let levels = if self.painted_tick != Some(self.dither_tick) {
            Some(liquid_levels(self.dither_tick))
        } else {
            None
        };
        if let Some(levels) = levels {
            let mut pixels = vec![0u8; WIDTH * HEIGHT * 4];
            for (index, level) in levels.iter().enumerate() {
                let tone = TONE[(*level as usize).min(TONE.len() - 1)];
                let at = index * 4;
                pixels[at] = tone[0];
                pixels[at + 1] = tone[1];
                pixels[at + 2] = tone[2];
                pixels[at + 3] = 255;
            }
            let image = egui::ColorImage::from_rgba_unmultiplied([WIDTH, HEIGHT], &pixels);
            match &mut self.texture {
                Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
                None => {
                    self.texture = Some(ctx.load_texture(
                        "load-gate-dither",
                        image,
                        egui::TextureOptions::NEAREST,
                    ));
                }
            }
            self.painted_tick = Some(self.dither_tick);
        }
        let texture = self.texture.as_ref().map(|texture| texture.id());
        let catchup = self.catchup;
        let name = self.name.clone();
        let fraction = self.fraction;
        let bar_tick = self.bar_tick;
        let dots = DOTS[self.dot_index];
        egui::Area::new(egui::Id::new("load-gate"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .movable(false)
            .interactable(true)
            .fade_in(false)
            .show(ctx, |ui| {
                let (response, painter) =
                    ui.allocate_painter(screen.size(), egui::Sense::click_and_drag());
                let _ = response;
                painter.rect_filled(
                    screen,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, DIM),
                );
                paint_panel(
                    &painter,
                    ui,
                    theme,
                    screen.center(),
                    catchup,
                    &name,
                    dots,
                    fraction,
                    bar_tick,
                    texture,
                );
            });
        true
    }
}

fn paint_panel(
    painter: &egui::Painter,
    ui: &egui::Ui,
    theme: &Theme,
    center: egui::Pos2,
    catchup: bool,
    name: &str,
    dots: &str,
    fraction: f32,
    bar_tick: u32,
    texture: Option<egui::TextureId>,
) {
    let font = egui::FontId::monospace(12.0);
    let white = egui::Color32::WHITE;
    let quiet = egui::Color32::from_rgb(theme.quiet[0], theme.quiet[1], theme.quiet[2]);
    let soft = egui::Color32::from_rgb(theme.soft[0], theme.soft[1], theme.soft[2]);
    let rule = egui::Color32::from_rgb(0x2a, 0x2a, 0x2a);
    let inner_w = PANEL_W - 32.0;
    let quiet_galley = ui.fonts(|fonts| {
        fonts.layout_job(wrap_job(
            "The studio cannot be used until this file is loaded.",
            font.clone(),
            quiet,
            inner_w,
        ))
    });
    let soft_galley = ui.fonts(|fonts| {
        fonts.layout_no_wrap("thank you for your patience".into(), font.clone(), soft)
    });
    let quiet_h = quiet_galley.size().y;
    let soft_h = soft_galley.size().y;
    let header_h = 18.0;
    let dither_h = HEIGHT as f32 * SCALE;
    let bar_h = 16.0;
    let mut height = 16.0 + dither_h + 8.0 + bar_h + 16.0;
    if !catchup {
        height += header_h + 8.0 + quiet_h + 8.0 + soft_h + 8.0;
    }
    let panel = egui::Rect::from_center_size(center, egui::vec2(PANEL_W, height));
    painter.rect_filled(panel, 0.0, egui::Color32::BLACK);
    painter.rect_stroke(panel, 0.0, egui::Stroke::new(1.0_f32, rule));
    let mut y = panel.min.y + 16.0;
    if !catchup {
        let loading = ui.fonts(|fonts| fonts.layout_no_wrap("LOADING".into(), font.clone(), white));
        let dot_galley =
            ui.fonts(|fonts| fonts.layout_no_wrap(DOTS[2].into(), font.clone(), white));
        let name_width = (inner_w - 12.0 - loading.size().x - dot_galley.size().x).max(24.0);
        let shown = elide(ui, name, name_width, font.clone(), white);
        painter.text(
            egui::pos2(panel.min.x + 16.0, y),
            egui::Align2::LEFT_TOP,
            shown,
            font.clone(),
            white,
        );
        let dots_w = ui
            .fonts(|fonts| fonts.layout_no_wrap(dots.into(), font.clone(), white))
            .size()
            .x;
        painter.text(
            egui::pos2(panel.max.x - 16.0 - dots_w, y),
            egui::Align2::LEFT_TOP,
            dots,
            font.clone(),
            white,
        );
        painter.text(
            egui::pos2(
                panel.max.x - 16.0 - dot_galley.size().x - loading.size().x,
                y,
            ),
            egui::Align2::LEFT_TOP,
            "LOADING",
            font.clone(),
            white,
        );
        y += header_h + 8.0;
    }
    let dither = egui::Rect::from_min_size(
        egui::pos2(panel.center().x - (WIDTH as f32 * SCALE) / 2.0, y),
        egui::vec2(WIDTH as f32 * SCALE, dither_h),
    );
    if let Some(texture) = texture {
        painter.image(
            texture,
            dither,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            white,
        );
    }
    y += dither_h + 8.0;
    if !catchup {
        painter.galley(egui::pos2(panel.min.x + 16.0, y), quiet_galley, quiet);
        y += quiet_h + 8.0;
        painter.galley(egui::pos2(panel.min.x + 16.0, y), soft_galley, soft);
        y += soft_h + 8.0;
    }
    paint_bar(
        painter,
        ui,
        egui::pos2(panel.min.x + 16.0, y),
        inner_w,
        fraction,
        bar_tick,
        font,
        quiet,
        white,
    );
}

fn wrap_job(
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job
}

fn elide(
    ui: &egui::Ui,
    text: &str,
    width: f32,
    font: egui::FontId,
    color: egui::Color32,
) -> String {
    let full = ui.fonts(|fonts| fonts.layout_no_wrap(text.into(), font.clone(), color));
    if full.size().x <= width {
        return text.to_string();
    }
    let mut kept: String = text.chars().collect();
    while !kept.is_empty() {
        kept.pop();
        let candidate = format!("{kept}…");
        let size = ui
            .fonts(|fonts| fonts.layout_no_wrap(candidate.clone().into(), font.clone(), color))
            .size()
            .x;
        if size <= width {
            return candidate;
        }
    }
    "…".into()
}

fn paint_bar(
    painter: &egui::Painter,
    ui: &egui::Ui,
    origin: egui::Pos2,
    width: f32,
    fraction: f32,
    tick: u32,
    font: egui::FontId,
    quiet: egui::Color32,
    white: egui::Color32,
) {
    let filled = egui::Color32::from_rgb(0xBB, 0xBB, 0xBB);
    let (text, head) = bar_line(fraction, tick, bar_count(width));
    for (index, ch) in text.chars().enumerate() {
        let color = if Some(index) == head {
            white
        } else if ch == '#' {
            filled
        } else if ch == '.' || ch == '[' || ch == ']' {
            quiet
        } else {
            white
        };
        let rect = egui::Rect::from_min_size(
            origin + egui::vec2(index as f32 * CELL, 0.0),
            egui::vec2(CELL, 16.0),
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            ch.to_string(),
            font.clone(),
            color,
        );
    }
    let _ = ui;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_fills_the_bar_to_the_step_and_never_past_the_cap() {
        assert_eq!(note_fraction("Painting the picture"), 0.84);
        assert_eq!(note_fraction("Reading vector layers"), 0.7);
        assert_eq!(note_fraction("Converting Clip Studio paint"), 0.36);
        assert_eq!(note_fraction("Reading page.clip"), 0.5);
        assert_eq!(note_fraction("blueprint"), 0.5);
        assert_eq!(note_fraction("Downloading page.clip from iCloud."), 0.16);
        assert_eq!(note_fraction(""), 0.16);
    }

    #[test]
    fn liquid_frames_are_stable_and_stay_inside_the_three_tones() {
        let first = liquid_levels(0);
        assert_eq!(first, liquid_levels(0));
        assert_eq!(first.len(), WIDTH * HEIGHT);
        assert!(first.iter().all(|level| *level <= 2));
        assert_ne!(first, liquid_levels(4));
    }

    #[test]
    fn the_bar_creeps_then_jumps_and_a_full_bar_is_hashes() {
        let (empty, head) = bar_line(0.0, 0, 8);
        assert_eq!(empty, "[#.......]");
        assert_eq!(head, Some(1));
        let (half, head) = bar_line(0.5, 1, 8);
        assert_eq!(half, "[####0...]");
        assert_eq!(head, Some(5));
        let (full, head) = bar_line(1.0, 3, 8);
        assert_eq!(full, "[########]");
        assert!(head.is_none());

        let mut gate = Gate::new();
        assert!(!gate.active());
        gate.start("page.clip", false);
        assert!(gate.active());
        assert_eq!(gate.stage_fraction(), 0.0);
        gate.step_bar();
        assert!((gate.stage_fraction() - 0.08).abs() < 1e-5);
        gate.hear("Reading vector layers");
        assert!((gate.stage_fraction() - 0.7).abs() < 1e-5);
        gate.hear("Reading page.clip");
        assert!((gate.stage_fraction() - 0.7).abs() < 1e-5);
        gate.complete();
        assert!(!gate.active());
        assert_eq!(gate.stage_fraction(), 1.0);
    }
}
