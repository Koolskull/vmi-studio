//! The tool palette. Every tool has a name, a place in the grid, and the
//! fields a right-click settings window is allowed to edit.
//!
//! Release is the commit. Raster tools become pixels. Vector pens keep a stroke.

use serde::{Deserialize, Serialize};

use crate::coverage::{curve_value, Edge};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Pen,
    Pencil,
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
    Field::Edge,
    Field::Fringe,
    Field::PressureSize,
    Field::PressureOpacity,
    Field::Opacity,
    Field::SizeCurve,
    Field::OpacityCurve,
];

const NO_COLOR: &[Field] = &[
    Field::ReleaseNote,
    Field::Size,
    Field::Min,
    Field::Max,
    Field::Softness,
    Field::Edge,
    Field::Fringe,
    Field::PressureSize,
    Field::PressureOpacity,
    Field::Opacity,
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
    Field::Edge,
    Field::Fringe,
    Field::Chunk,
    Field::Tones,
    Field::PressureSize,
    Field::PressureOpacity,
    Field::Opacity,
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
        Self::CutRect,
        Self::CutLasso,
        Self::CutPolygon,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pen => "Pen",
            Self::Pencil => "Pencil",
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
            Self::CutRect => "Rect",
            Self::CutLasso => "Lasso",
            Self::CutPolygon => "Polygon",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Self::Pen => "P",
            Self::Pencil => "",
            Self::Paint => "B",
            Self::Air => "",
            Self::Eraser => "E",
            Self::Smudge => "",
            Self::Blend => "",
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
            Self::CutRect => "",
            Self::CutLasso => "",
            Self::CutPolygon => "",
        }
    }

    pub fn usage(self) -> &'static str {
        match self {
            Self::Pen => "Hard freehand ink. Release writes pixels.",
            Self::Pencil => "A small hard stroke with pressure off.",
            Self::Paint => "Soft ink. Release writes pixels.",
            Self::Air => "A thin spray. Release writes pixels.",
            Self::Eraser => "Lifts alpha. Release writes pixels.",
            Self::Smudge => "Pushes color along the stroke.",
            Self::Blend => "Mixes the color already on the layer.",
            Self::Fill => "Fills a flat area on click.",
            Self::Dither => "KOOLDRAW chunks. Ink or a tone, never a gray edge.",
            Self::Line => "One straight stroke. Release writes pixels and keeps no points.",
            Self::Vector => "Freehand points. Release keeps the stroke editable.",
            Self::Curve => "Points and handles. Release keeps the curve.",
            Self::Point => "Moves a point or a handle on a vector stroke.",
            Self::Bone => "Bends a vector stroke around one bone.",
            Self::Grain => "A vector stroke with a generated texture.",
            Self::WarpTarget => "Click corners. Enter keeps the polygon.",
            Self::WarpMask => "Paints a mask. Release writes pixels.",
            Self::CutRect => "A rectangle marquee for cut.",
            Self::CutLasso => "A freehand marquee for cut.",
            Self::CutPolygon => "Click corners for a cut.",
        }
    }

    pub fn release_note(self) -> &'static str {
        match self.release() {
            Release::Pixels => "Release writes pixels and drops the path.",
            Release::Stroke => "Release keeps the points, width, curve, and grain. The picture is a cache.",
            Release::Polygon => "Enter keeps the corners and refills the raster from them.",
            Release::Edit => "Release changes the thing under the pen. It does not start a new stroke.",
        }
    }

    pub fn release(self) -> Release {
        match self {
            Self::Vector | Self::Curve | Self::Grain => Release::Stroke,
            Self::WarpTarget => Release::Polygon,
            Self::Point | Self::Bone | Self::CutRect | Self::CutLasso | Self::CutPolygon => Release::Edit,
            _ => Release::Pixels,
        }
    }

    pub fn section(self) -> Section {
        match self {
            Self::Vector | Self::Curve | Self::Point | Self::Bone | Self::Grain => Section::Vector,
            Self::WarpTarget | Self::WarpMask => Section::Picture,
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
            Self::CutRect | Self::CutLasso | Self::CutPolygon => CUT,
            Self::Eraser | Self::Smudge => NO_COLOR,
            _ => SHAPE,
        }
    }

    pub fn by_shortcut(key: &str) -> Option<Self> {
        Self::PALETTE.iter().copied().find(|tool| !tool.shortcut().is_empty() && tool.shortcut().eq_ignore_ascii_case(key))
    }
}

/// Settings for one palette tool. A right-click edits this record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSettings {
    pub tool: Tool,
    pub color: [u8; 3],
    pub size: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub softness: f32,
    pub fringe: f32,
    pub edge: Edge,
    pub chunk: u32,
    pub tones: bool,
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
}

impl ToolSettings {
    pub fn default_for(tool: Tool) -> Self {
        let mut settings = Self {
            tool,
            color: [0xFF, 0x3E, 0xB8],
            size: 16.0,
            size_min: 1.0,
            size_max: 128.0,
            softness: 0.0,
            fringe: 0.5,
            edge: Edge::Smooth,
            chunk: 1,
            tones: false,
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
        };
        match tool {
            Tool::Pencil => {
                settings.size = 4.0;
                settings.fringe = 0.0;
                settings.edge = Edge::Hard;
                settings.size_pressure = false;
                settings.opacity_pressure = false;
            }
            Tool::Paint => {
                settings.size = 24.0;
                settings.softness = 0.55;
            }
            Tool::Air => {
                settings.size = 48.0;
                settings.softness = 1.0;
                settings.opacity = 48.0 / 255.0;
            }
            Tool::Eraser => settings.size = 16.0,
            Tool::Smudge => settings.size = 20.0,
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
            Tool::Vector => settings.size = 12.0,
            Tool::Curve => {
                settings.size = 12.0;
                settings.curve = CurveKind::Cubic;
            }
            Tool::Grain => {
                settings.size = 18.0;
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
        Tool::PALETTE.iter().copied().map(Self::default_for).collect()
    }

    pub fn merge(saved: Vec<Self>) -> Vec<Self> {
        let mut rack = Self::rack();
        for item in saved {
            if let Some(slot) = rack.iter_mut().find(|preset| preset.tool == item.tool) {
                *slot = item;
            }
        }
        rack
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
        if tool == Tool::Paint && value < 0.35 { 0.55 } else { value }
    }

    pub fn radius_for(&self, pressure: f32) -> f32 {
        self.diameter_for(pressure) * 0.5
    }
}

pub fn ink_spacing(diameter: f32, tool: Tool) -> f32 {
    let width = diameter.max(1.0);
    match tool {
        Tool::Paint => (width * 0.18).max(1.0),
        Tool::Vector | Tool::Curve | Tool::Grain => (width * 0.22).max(1.0),
        Tool::WarpMask => (width * 0.35).max(0.5),
        _ => (width * 0.15).max(0.75),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    Select(Tool),
    Edit(Tool),
}

/// Left click selects. Right click opens the settings window and does not change the selection by itself.
pub fn palette_action(tool: Tool, secondary: bool) -> PaletteAction {
    if secondary { PaletteAction::Edit(tool) } else { PaletteAction::Select(tool) }
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
            assert!(tool.fields().contains(&Field::ReleaseNote), "{}", tool.name());
            names.push(tool.name());
        }
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    #[test]
    fn right_click_edits_and_vector_release_keeps_traits() {
        assert_eq!(palette_action(Tool::Pen, false), PaletteAction::Select(Tool::Pen));
        assert_eq!(palette_action(Tool::Smudge, true), PaletteAction::Edit(Tool::Smudge));
        assert_eq!(Tool::Pen.release(), Release::Pixels);
        assert_eq!(Tool::Line.release(), Release::Pixels);
        assert_eq!(Tool::Vector.release(), Release::Stroke);
        assert_eq!(Tool::Curve.release(), Release::Stroke);
        assert_eq!(Tool::Grain.release(), Release::Stroke);
        assert_eq!(Tool::WarpMask.release(), Release::Pixels);
        assert_eq!(Tool::WarpTarget.release(), Release::Polygon);
        assert_eq!(Tool::Point.release(), Release::Edit);
        assert!(Tool::Vector.fields().contains(&Field::Grain));
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
        assert!(!pencil.size_pressure);
        assert!((pencil.diameter_for(0.2) - pencil.size).abs() < 1e-4);
    }
}
