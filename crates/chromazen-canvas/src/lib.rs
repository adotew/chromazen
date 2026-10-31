mod blend;
mod renderer;
mod smoothing;

pub use blend::{BlendMode, LayerSample, composite_samples};
pub use renderer::{
    BrushCursor, Canvas, CanvasDocument, CanvasOrientation, CanvasSizeConstraints,
    DEFAULT_CANVAS_SIZE, DocumentVersions, DropEdge, LayerContentBounds, LayerId, LayerInfo,
    LayerReadback, LayerResourceId, LayerSnapshot, LayerTransform, PaintViewSnapshot,
    merge_down_target_index,
};
pub use smoothing::{StrokePositionFilter, StrokeSmoother};

/// How brush and smudge strokes combine their color with existing paint.
///
/// Discriminants are shared with `pigment.wgsl`; serialized names are persisted in configs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMixing {
    /// Composites in sRGB, like most digital painting software.
    #[default]
    Digital = 0,
    /// Mixes like physical paint using Kubelka-Munk reflectance from spectral.js.
    Pigment = 1,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaintTool {
    #[default]
    Brush,
    Eraser,
    Smudge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSpacing {
    pub ratio: f32,
    pub minimum: f32,
}

impl Default for BrushSpacing {
    fn default() -> Self {
        Self {
            ratio: 0.03,
            minimum: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub opacity: f32,
}
