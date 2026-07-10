mod brush;
mod smoothing;

pub(crate) use brush::{BrushSettings, MAX_BRUSH_SIZE, MIN_BRUSH_SIZE, StrokePoint};
pub(crate) use smoothing::{StrokeSmoother, StrokeSmoothingOptions};
