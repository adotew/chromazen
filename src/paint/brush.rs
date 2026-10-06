use chromazen_brush::PressureConfig;
use chromazen_canvas::{BrushSpacing, StrokePoint};
use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub color: Color32,
    pub size: f32,
    pub opacity: f32,
    pub pressure: PressureConfig,
    pub spacing: BrushSpacing,
}

impl BrushSettings {
    pub fn rgba(self) -> [f32; 4] {
        color32_to_rgba(self.color)
    }

    pub fn radius(self, pressure: f32) -> f32 {
        self.pressure.radius(self.size, pressure)
    }

    pub fn stroke_point(self, document_point: [f32; 2], pressure: f32) -> StrokePoint {
        self.pressure
            .stroke_point(document_point, self.size, pressure)
    }
}

pub fn color32_to_rgba(color: Color32) -> [f32; 4] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        1.0,
    ]
}
