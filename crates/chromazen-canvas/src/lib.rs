//! Window-independent GPU painting canvas.

mod renderer;
mod smoothing;

pub use renderer::{
    BrushCursor, Canvas, CanvasDocument, CanvasSizeConstraints, DEFAULT_CANVAS_SIZE,
    DocumentVersions, DropEdge, LayerContentBounds, LayerId, LayerInfo, LayerReadback,
    LayerResourceId, LayerSnapshot, LayerTransform, PaintViewSnapshot, merge_down_target_index,
};
pub use smoothing::{StrokePositionFilter, StrokeSmoother};

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
