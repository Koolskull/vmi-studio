//! The tool palette. Every tool has a name, a place in the grid, and the
//! fields a right-click settings window is allowed to edit.
//!
//! Release is the commit. Raster tools become pixels. Vector pens keep a stroke.

use serde::{Deserialize, Serialize};

use crate::coverage::{curve_value, Edge};
use crate::tip::Tip;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Pen,
    Pencil,
    Charcoal,
    Paint,
    Air,
    Eraser,
    Smudge,
    Blend,
    Fill,
    Dither,
    Line,
    Vector,
    Curve,
    Point,
    Bone,
    Grain,
    WarpTarget,
    WarpMask,
    /// Click a painted pixel. The outliner selects that layer.
    Select,
    /// Click a pixel. The shared ink becomes that colour.
    Dropper,
    CutRect,
    CutLasso,
    CutPolygon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Raster,
    Vector,
    Picture,
    Cut,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Raster => "Raster",
            Self::Vector => "Vector",
            Self::Picture => "Picture",
            Self::Cut => "Cut",
        }
    }
}

/// What a pen-up keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// The path is dropped. The layer keeps the pixels.
    Pixels,
    /// The stroke record stays editable. Pixels are a cache.
    Stroke,
    /// Warp corners stay on the layer.
    Polygon,
    /// The tool edits something that is already there.
    Edit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveKind {
    Poly,
    Quadratic,
    Cubic,
    Spline,
}

impl CurveKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Poly => "Poly",
            Self::Quadratic => "Quadratic",
            Self::Cubic => "Cubic",
            Self::Spline => "Spline",
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Poly, Self::Quadratic, Self::Cubic, Self::Spline]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrainKind {
    None,
    Stipple,
    Hatch,
    Liquid,
}

impl GrainKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Stipple => "Stipple",
            Self::Hatch => "Hatch",
            Self::Liquid => "Liquid",
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::None, Self::Stipple, Self::Hatch, Self::Liquid]
    }
}

/// One row in the settings window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    ReleaseNote,
    Color,
    Size,
    Min,
    Max,
    Softness,
    Tip,
    Texture,
    Edge,
    Fringe,
    Chunk,
    Tones,
    PressureSize,
    PressureOpacity,
    Opacity,
    SizeCurve,
    OpacityCurve,
    CurveKind,
    Grain,
    Seed,
    Closed,
    BoneReach,
    FillTolerance,
    PickRadius,
    /// PhotoCraft-style tip scatter. 0 keeps every dab on the stroke.
    Scatter,
    /// Pulled-string correction, in pixels. 0 follows the pen.
    Stabilize,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReleaseNote => "Release",
            Self::Color => "Color",
            Self::Size => "Size",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Softness => "Softness",
            Self::Tip => "Tip",
            Self::Texture => "Texture",
            Self::Edge => "Edge",
            Self::Fringe => "Fringe",
            Self::Chunk => "Chunk",
            Self::Tones => "Three tones",
            Self::PressureSize => "Pressure size",
            Self::PressureOpacity => "Pressure opacity",
            Self::Opacity => "Opacity",
            Self::SizeCurve => "Size curve",
            Self::OpacityCurve => "Opacity curve",
            Self::CurveKind => "Curve",
            Self::Grain => "Grain",
            Self::Seed => "Seed",
            Self::Closed => "Closed",
            Self::BoneReach => "Bone reach",
            Self::FillTolerance => "Fill tolerance",
            Self::PickRadius => "Pick radius",
            Self::Scatter => "Scatter",
            Self::Stabilize => "Stabilize",
        }
    }
}

const SHAPE: &[Field] = &[
    Field::ReleaseNote,
    Field::Color,
    Field::Size,
    Field::Min,
    Field::Max,
    Field::Softness,
    Field::Tip,
    Field::Texture,
    Field::Edge,
    Field::Fringe,
    Field::PressureSize,
    Field::PressureOpacity,
    Field::Opacity,
    Field::Scatter,
    Field::Stabilize,
    Field::SizeCurve,
    Field::OpacityCurve,
];

const NO_COLOR: &[Field] = &[
    Field::ReleaseNote,
    Field::Size,
    Field::Min,
    Field::Max,
    Field::Softness,
    Field::Tip,
    Field::Texture,
    Field::Edge,
    Field::Fringe,
    Field::PressureSize,
    Field::PressureOpacity,
    Field::Opacity,
    Field::Scatter,
    Field::Stabilize,
    Field::SizeCurve,
    Field::OpacityCurve,
];

const DITHER: &[Field] = &[
    Field::ReleaseNote,
    Field::Color,
    Field::Size,
    Field::Min,
    Field::Max,
    Field::Softness,
    Field::Tip,
    Field::Texture,
    Field::Edge,
    Field::Fringe,
    Field::Chunk,
    Field::Tones,
    Field::PressureSize,
    Field::PressureOpacity,
    Field::Opacity,
    Field::Scatter,
    Field::Stabilize,
    Field::SizeCurve,
    Field::OpacityCurve,
];

const VECTOR: &[Field] = &[
    Field::ReleaseNote,
    Field::Color,
    Field::Size,
    Field::Min,
    Field::Max,
    Field::Softness,
    Field::Edge,
    Field::Fringe,
    Field::Chunk,
    Field::Tones,
    Field::PressureSize,
    Field::PressureOpacity,
    Field::Opacity,
    Field::Stabilize,
    Field::CurveKind,
    Field::Grain,
    Field::Seed,
    Field::Closed,
    Field::BoneReach,
    Field::SizeCurve,
    Field::OpacityCurve,
];

const FILL: &[Field] = &[Field::ReleaseNote, Field::Color, Field::FillTolerance];
const POINT: &[Field] = &[Field::ReleaseNote, Field::PickRadius];
const BONE: &[Field] = &[Field::ReleaseNote, Field::BoneReach];
const TARGET: &[Field] = &[Field::ReleaseNote, Field::Color];
const CUT: &[Field] = &[Field::ReleaseNote];

impl Tool {
    pub const PALETTE: &'static [Self] = &[
        Self::Pen,
        Self::Pencil,
        Self::Charcoal,
        Self::Paint,
        Self::Air,
        Self::Eraser,
        Self::Smudge,
        Self::Blend,
        Self::Fill,
        Self::Dither,
        Self::Line,
        Self::Vector,
        Self::Curve,
        Self::Point,
        Self::Bone,
        Self::Grain,
        Self::WarpTarget,
        Self::WarpMask,
        Self::Select,
        Self::Dropper,
        Self::CutRect,
        Self::CutLasso,
        Self::CutPolygon,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pen => "Pen",
            Self::Pencil => "Pencil",
            Self::Charcoal => "Charcoal",
            Self::Paint => "Paint",
            Self::Air => "Air",
            Self::Eraser => "Eraser",
            Self::Smudge => "Smudge",
            Self::Blend => "Blend",
            Self::Fill => "Fill",
            Self::Dither => "Dither",
            Self::Line => "Line",
            Self::Vector => "Vector",
            Self::Curve => "Curve",
            Self::Point => "Point",
            Self::Bone => "Bone",
            Self::Grain => "Grain",
            Self::WarpTarget => "Warp target",
            Self::WarpMask => "Warp mask",
            Self::Select => "Select",
            Self::Dropper => "Dropper",
            Self::CutRect => "Rect",
            Self::CutLasso => "Lasso",
            Self::CutPolygon => "Polygon",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Self::Pen => "P",
            Self::Pencil => "N",
            Self::Charcoal => "C",
            Self::Paint => "B",
            Self::Air => "A",
            Self::Eraser => "E",
            Self::Smudge => "S",
            Self::Blend => "D",
            Self::Fill => "F",
            Self::Dither => "",
            Self::Line => "L",
            Self::Vector => "V",
            Self::Curve => "",
            Self::Point => "",
            Self::Bone => "",
            Self::Grain => "",
            Self::WarpTarget => "T",
            Self::WarpMask => "M",
            Self::Select => "",
            Self::Dropper => "I",
            Self::CutRect => "",
            Self::CutLasso => "",
            Self::CutPolygon => "",
        }
    }

    pub fn usage(self) -> &'static str {
        match self {
            Self::Pen => "Hard freehand ink. Release writes pixels.",
            Self::Pencil => "Graphite on paper tooth. Pressure sets the width.",
            Self::Charcoal => "A rough stick. Grain breaks the edge. Release writes pixels.",
            Self::Paint => "Bristle streaks of soft paint. Release writes pixels.",
            Self::Air => "Spray paint. Speckle piles up while the can stays.",
            Self::Eraser => "Lifts alpha. Release writes pixels.",
            Self::Smudge => "Drags paint in bristle streaks. Each hair carries its own color.",
            Self::Blend => "Mixes the color already on the layer.",
            Self::Fill => "Fills a flat area on click.",
            Self::Dither => "KOOLDRAW chunks. Ink or a tone, never a gray edge.",
            Self::Line => "One straight stroke. Release writes pixels and keeps no points.",
            Self::Vector => "A smooth pen. Pressure sets the width. Stabilize pulls the string. Points stay hidden until the Point tool.",
            Self::Curve => "A curve with handles. Stabilize pulls the string. Points stay hidden until the Point tool.",
            Self::Point => "Shows the points and handles, and moves the one under the pen.",
            Self::Bone => "Bends a vector stroke around one bone.",
            Self::Grain => "A vector stroke with a generated texture.",
            Self::WarpTarget => "Click corners. Enter keeps the polygon.",
            Self::WarpMask => "Paints a mask. Release writes pixels.",
            Self::Select => "Click painted ink. A drag stays put. The outliner selects that layer and opens the folders around it.",
            Self::Dropper => "Click or drag a pixel. The ink becomes that color. Checker and onion stay out.",
            Self::CutRect => "A rectangle marquee for cut.",
            Self::CutLasso => "A freehand marquee for cut.",
            Self::CutPolygon => "Click corners for a cut.",
        }
    }

    pub fn release_note(self) -> &'static str {
        match self.release() {
            Release::Pixels => "Release writes pixels and drops the path.",
            Release::Stroke => {
                "Release keeps a smooth curve. The Point tool edits the points and handles."
            }
            Release::Polygon => "Enter keeps the corners and refills the raster from them.",
            Release::Edit => {
                "Release changes the thing under the pen. It does not start a new stroke."
            }
        }
    }

    pub fn release(self) -> Release {
        match self {
            Self::Vector | Self::Curve | Self::Grain => Release::Stroke,
            Self::WarpTarget => Release::Polygon,
            Self::Point
            | Self::Bone
            | Self::Select
            | Self::Dropper
            | Self::CutRect
            | Self::CutLasso
            | Self::CutPolygon => Release::Edit,
            _ => Release::Pixels,
        }
    }

    pub fn section(self) -> Section {
        match self {
            Self::Vector | Self::Curve | Self::Point | Self::Bone | Self::Grain => Section::Vector,
            Self::WarpTarget | Self::WarpMask | Self::Select | Self::Dropper => Section::Picture,
            Self::CutRect | Self::CutLasso | Self::CutPolygon => Section::Cut,
            _ => Section::Raster,
        }
    }

    pub fn fields(self) -> &'static [Field] {
        match self {
            Self::Dither => DITHER,
            Self::Vector | Self::Curve | Self::Grain => VECTOR,
            Self::Fill => FILL,
            Self::Point => POINT,
            Self::Bone => BONE,
            Self::WarpTarget => TARGET,
            Self::Select | Self::Dropper | Self::CutRect | Self::CutLasso | Self::CutPolygon => CUT,
            Self::Eraser | Self::Smudge => NO_COLOR,
            _ => SHAPE,
        }
    }

    pub fn by_shortcut(key: &str) -> Option<Self> {
        Self::PALETTE
            .iter()
            .copied()
            .find(|tool| !tool.shortcut().is_empty() && tool.shortcut().eq_ignore_ascii_case(key))
    }
}

/// Bumped when a saved preset needs new traits filled in from the defaults.
pub const SETTINGS_REV: u32 = 4;

/// Settings for one palette tool. A right-click edits this record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSettings {
    #[serde(default)]
    pub rev: u32,
    pub tool: Tool,
    pub color: [u8; 3],
    pub size: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub softness: f32,
    #[serde(default)]
    pub tip: Tip,
    /// 0 is a plain disc, 1 is the full generated texture.
    #[serde(default)]
    pub texture: f32,
    pub fringe: f32,
    pub edge: Edge,
    pub chunk: u32,
    pub tones: bool,
    /// Ordered pattern used when the edge is dither.
    #[serde(default)]
    pub dither_pattern: crate::coverage::DitherPattern,
    /// Slides the dither gate. Negative keeps more ink.
    #[serde(default)]
    pub dither_bias: f32,
    pub size_pressure: bool,
    pub opacity_pressure: bool,
    pub opacity: f32,
    pub size_curve: Vec<(f32, f32)>,
    pub opacity_curve: Vec<(f32, f32)>,
    pub curve: CurveKind,
    pub grain: GrainKind,
    pub seed: u64,
    pub closed: bool,
    pub bone_reach: f32,
    pub fill_tolerance: u8,
    pub pick_radius: f32,
    /// How far a dab may leave the stroke, in fractions of the size. 0 is none.
    #[serde(default)]
    pub scatter: f32,
    /// Pulled-string length in pixels. 0 follows the pen.
    #[serde(default)]
    pub stabilize: f32,
}

impl ToolSettings {
    pub fn default_for(tool: Tool) -> Self {
        let mut settings = Self {
            rev: SETTINGS_REV,
            tool,
            color: [0xFF, 0x3E, 0xB8],
            // Python's Brush starts at 24. The pen keeps that, not a smaller default.
            size: 24.0,
            size_min: 1.0,
            size_max: 128.0,
            softness: 0.0,
            tip: Tip::Round,
            texture: 0.0,
            fringe: 0.5,
            edge: Edge::Smooth,
            chunk: 1,
            tones: false,
            dither_pattern: crate::coverage::DitherPattern::Bayer,
            dither_bias: 0.0,
            size_pressure: true,
            opacity_pressure: true,
            opacity: 1.0,
            size_curve: vec![(0.0, 0.0), (1.0, 1.0)],
            opacity_curve: vec![(0.0, 0.0), (1.0, 1.0)],
            curve: CurveKind::Poly,
            grain: GrainKind::None,
            seed: 1,
            closed: false,
            bone_reach: 160.0,
            fill_tolerance: 0,
            pick_radius: 8.0,
            scatter: 0.0,
            stabilize: 0.0,
        };
        match tool {
            Tool::Pencil => {
                settings.size = 5.0;
                settings.fringe = 0.5;
                settings.tip = Tip::Pencil;
                settings.texture = 0.85;
            }
            Tool::Charcoal => {
                settings.size = 18.0;
                settings.softness = 0.25;
                settings.tip = Tip::Charcoal;
                settings.texture = 0.9;
            }
            Tool::Paint => {
                settings.size = 24.0;
                settings.softness = 0.55;
                settings.tip = Tip::Bristle;
                settings.texture = 0.7;
            }
            Tool::Air => {
                settings.size = 48.0;
                settings.softness = 1.0;
                settings.opacity = 0.5;
                settings.tip = Tip::Spray;
                settings.texture = 1.0;
            }
            Tool::Eraser => settings.size = 16.0,
            Tool::Smudge => {
                settings.size = 20.0;
                settings.tip = Tip::Bristle;
                settings.texture = 0.8;
            }
            Tool::Blend => {
                settings.size = 24.0;
                settings.softness = 0.35;
            }
            Tool::Dither => {
                settings.size = 24.0;
                settings.softness = 0.7;
                settings.edge = Edge::Dither;
                settings.chunk = 2;
                settings.tones = true;
            }
            Tool::Line => settings.size = 4.0,
            Tool::Vector => {
                settings.size = 12.0;
                // Clip Studio's vector pen is a curve. Poly is still in the Curve menu.
                settings.curve = CurveKind::Cubic;
                settings.stabilize = 12.0;
            }
            Tool::Curve => {
                settings.size = 12.0;
                settings.curve = CurveKind::Cubic;
                settings.stabilize = 12.0;
            }
            Tool::Grain => {
                settings.size = 18.0;
                settings.stabilize = 12.0;
                settings.grain = GrainKind::Liquid;
                settings.edge = Edge::Dither;
                settings.tones = true;
                settings.chunk = 2;
                settings.seed = 7;
            }
            Tool::Point => settings.pick_radius = 8.0,
            Tool::Bone => settings.bone_reach = 180.0,
            _ => {}
        }
        settings
    }

    pub fn rack() -> Vec<Self> {
        Tool::PALETTE
            .iter()
            .copied()
            .map(Self::default_for)
            .collect()
    }

    pub fn merge(saved: Vec<Self>) -> Vec<Self> {
        let mut rack = Self::rack();
        for mut item in saved {
            if let Some(slot) = rack.iter_mut().find(|preset| preset.tool == item.tool) {
                item.upgrade(slot);
                *slot = item;
            }
        }
        rack
    }

    /// A preset saved before the tip engine knows nothing about tips or
    /// pressure. Take those traits from the default and keep the rest.
    fn upgrade(&mut self, default: &Self) {
        if self.rev < 2 {
            self.tip = default.tip;
            self.texture = default.texture;
            self.size_pressure = true;
            self.opacity_pressure = true;
        }
        // The vector pen used to store a polyline. A saved Poly becomes the curve.
        if self.rev < 3 && self.tool == Tool::Vector && self.curve == CurveKind::Poly {
            self.curve = CurveKind::Cubic;
        }
        if self.rev < 4
            && matches!(self.tool, Tool::Vector | Tool::Curve | Tool::Grain)
            && self.stabilize <= 0.0
        {
            self.stabilize = default.stabilize;
        }
        self.rev = SETTINGS_REV;
    }

    pub fn diameter_for(&self, pressure: f32) -> f32 {
        let low = self.size_min.max(0.5);
        let high = self.size_max.max(low);
        let size = self.size.clamp(low, high);
        if !self.size_pressure {
            return size;
        }
        let mix = curve_value(&self.size_curve, pressure);
        low + (size - low) * mix
    }

    pub fn opacity_for(&self, pressure: f32) -> f32 {
        let base = self.opacity.clamp(0.0, 1.0);
        if !self.opacity_pressure {
            return base;
        }
        base * curve_value(&self.opacity_curve, pressure)
    }

    /// Paint never draws fully hard when the knob is still near zero.
    pub fn softness_for(&self, tool: Tool) -> f32 {
        let value = self.softness.clamp(0.0, 1.0);
        if tool == Tool::Paint && value < 0.35 {
            0.55
        } else {
            value
        }
    }

    pub fn radius_for(&self, pressure: f32) -> f32 {
        self.diameter_for(pressure) * 0.5
    }
}

pub fn ink_spacing(diameter: f32, tool: Tool) -> f32 {
    let width = diameter.max(0.5);
    let ratio = match tool {
        Tool::Paint | Tool::Smudge => 0.12,
        Tool::Charcoal | Tool::Pencil => 0.10,
        Tool::Air => 0.08,
        Tool::Vector | Tool::Curve | Tool::Grain => 0.22,
        Tool::WarpMask => 0.35,
        _ => 0.15,
    };
    // A wide brush keeps its ratio. A thin one spaces inside its own width,
    // so the first dabs join instead of sitting as separate dots.
    let spaced = width * ratio;
    spaced.max((width.min(0.75)) * 0.45).min(width.max(0.35))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    Select(Tool),
    Edit(Tool),
}

/// Left click selects. Right click opens the settings window and does not change the selection by itself.
pub fn palette_action(tool: Tool, secondary: bool) -> PaletteAction {
    if secondary {
        PaletteAction::Edit(tool)
    } else {
        PaletteAction::Select(tool)
    }
}

pub fn palette_matching(query: &str) -> Vec<Tool> {
    let needle = query.trim().to_lowercase();
    Tool::PALETTE
        .iter()
        .copied()
        .filter(|tool| {
            needle.is_empty()
                || tool.name().to_lowercase().contains(&needle)
                || tool.usage().to_lowercase().contains(&needle)
                || tool.section().label().to_lowercase().contains(&needle)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_named_and_has_settings_fields() {
        let mut names = Vec::new();
        for tool in Tool::PALETTE {
            assert!(!tool.name().is_empty(), "{tool:?}");
            assert!(!tool.usage().is_empty(), "{tool:?}");
            assert!(
                tool.fields().contains(&Field::ReleaseNote),
                "{}",
                tool.name()
            );
            names.push(tool.name());
        }
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    #[test]
    fn right_click_edits_and_vector_release_keeps_traits() {
        assert_eq!(
            palette_action(Tool::Pen, false),
            PaletteAction::Select(Tool::Pen)
        );
        assert_eq!(
            palette_action(Tool::Smudge, true),
            PaletteAction::Edit(Tool::Smudge)
        );
        assert_eq!(Tool::Pen.release(), Release::Pixels);
        assert_eq!(Tool::Line.release(), Release::Pixels);
        assert_eq!(Tool::Vector.release(), Release::Stroke);
        assert_eq!(Tool::Curve.release(), Release::Stroke);
        assert_eq!(Tool::Grain.release(), Release::Stroke);
        assert_eq!(Tool::WarpMask.release(), Release::Pixels);
        assert_eq!(Tool::WarpTarget.release(), Release::Polygon);
        assert_eq!(Tool::Point.release(), Release::Edit);
        assert_eq!(Tool::Dropper.release(), Release::Edit);
        assert_eq!(Tool::Dropper.section(), Section::Picture);
        assert_eq!(Tool::by_shortcut("i"), Some(Tool::Dropper));
        assert!(!Tool::Dropper.fields().contains(&Field::Color));
        assert!(Tool::Vector.fields().contains(&Field::Grain));
        assert!(Tool::Vector.fields().contains(&Field::Stabilize));
        assert!(Tool::Pen.fields().contains(&Field::Stabilize));
        assert!(Tool::Dither.fields().contains(&Field::Chunk));
        assert!(Tool::Fill.fields().contains(&Field::FillTolerance));
    }

    #[test]
    fn the_palette_search_finds_smudge_and_dither() {
        assert_eq!(palette_matching("smu"), vec![Tool::Smudge]);
        assert!(palette_matching("kooldraw").contains(&Tool::Dither));
        assert!(palette_matching("vector").contains(&Tool::Vector));
        assert_eq!(palette_matching("").len(), Tool::PALETTE.len());
    }

    #[test]
    fn paint_softness_has_a_floor_and_a_mouse_is_full_size() {
        let paint = ToolSettings::default_for(Tool::Paint);
        assert!((paint.softness_for(Tool::Paint) - 0.55).abs() < 1e-5);
        let mut hard = paint.clone();
        hard.softness = 0.1;
        assert!((hard.softness_for(Tool::Paint) - 0.55).abs() < 1e-5);
        assert!((hard.softness_for(Tool::Pen) - 0.1).abs() < 1e-5);
        let pen = ToolSettings::default_for(Tool::Pen);
        assert!((pen.diameter_for(1.0) - pen.size).abs() < 1e-4);
        assert!(pen.diameter_for(0.0) < pen.diameter_for(1.0));
        let pencil = ToolSettings::default_for(Tool::Pencil);
        assert!(
            pencil.size_pressure,
            "pressure is on by default for every tool"
        );
        assert!(pencil.diameter_for(0.2) < pencil.size);
        for tool in Tool::PALETTE {
            let settings = ToolSettings::default_for(*tool);
            assert!(
                settings.size_pressure && settings.opacity_pressure,
                "{tool:?}"
            );
        }
    }

    #[test]
    fn an_old_saved_preset_picks_up_the_tip_and_pressure_but_keeps_its_size() {
        let mut old = ToolSettings::default_for(Tool::Paint);
        old.rev = 0;
        old.tip = Tip::Round;
        old.texture = 0.0;
        old.size_pressure = false;
        old.size = 99.0;
        let rack = ToolSettings::merge(vec![old]);
        let paint = rack
            .iter()
            .find(|preset| preset.tool == Tool::Paint)
            .unwrap();
        assert_eq!(paint.tip, Tip::Bristle);
        assert!(paint.texture > 0.0);
        assert!(paint.size_pressure);
        assert_eq!(paint.size, 99.0);
        assert_eq!(paint.rev, SETTINGS_REV);

        let mut kept = ToolSettings::default_for(Tool::Paint);
        kept.tip = Tip::Round;
        let rack = ToolSettings::merge(vec![kept]);
        assert_eq!(
            rack.iter()
                .find(|preset| preset.tool == Tool::Paint)
                .unwrap()
                .tip,
            Tip::Round
        );

        let mut vector = ToolSettings::default_for(Tool::Vector);
        vector.rev = 3;
        vector.stabilize = 0.0;
        let rack = ToolSettings::merge(vec![vector]);
        let vector = rack
            .iter()
            .find(|preset| preset.tool == Tool::Vector)
            .unwrap();
        assert!((vector.stabilize - 12.0).abs() < 1e-4);
        assert_eq!(vector.rev, SETTINGS_REV);
    }

    #[test]
    fn the_drawing_tools_carry_a_generated_tip() {
        assert_eq!(ToolSettings::default_for(Tool::Pencil).tip, Tip::Pencil);
        assert_eq!(ToolSettings::default_for(Tool::Charcoal).tip, Tip::Charcoal);
        assert_eq!(ToolSettings::default_for(Tool::Paint).tip, Tip::Bristle);
        assert_eq!(ToolSettings::default_for(Tool::Air).tip, Tip::Spray);
        assert_eq!(ToolSettings::default_for(Tool::Smudge).tip, Tip::Bristle);
        assert_eq!(ToolSettings::default_for(Tool::Pen).tip, Tip::Round);
        assert!(Tool::Pen.fields().contains(&Field::Tip));
        assert!(Tool::Smudge.fields().contains(&Field::Texture));
        assert_eq!(Tool::Charcoal.release(), Release::Pixels);
        assert_eq!(Tool::by_shortcut("c"), Some(Tool::Charcoal));
    }

    #[test]
    fn a_thin_brush_spaces_inside_its_width() {
        let thin = ink_spacing(1.0, Tool::Pen);
        assert!(thin < 1.0 && thin > 0.2, "{thin}");
        let wide = ink_spacing(24.0, Tool::Pen);
        assert!((wide - 3.6).abs() < 1e-4, "{wide}");
    }
}
