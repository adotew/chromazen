use egui::Color32;

const MIN_PRESSURE_SIZE: f32 = 0.45;
const MIN_PRESSURE_OPACITY: f32 = 0.08;
const PRESSURE_OPACITY_GAMMA: f32 = 1.35;

#[derive(Clone, Copy, Debug)]
pub struct BrushSettings {
    pub color: Color32,
    pub size: f32,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(170, 187, 204),
            size: crate::constants::DEFAULT_BRUSH_SIZE,
        }
    }
}

impl BrushSettings {
    pub fn rgba(self) -> [f32; 4] {
        color32_to_rgba(self.color)
    }

    pub fn stroke_point(self, document_point: [f32; 2], pressure: f32) -> StrokePoint {
        StrokePoint {
            x: document_point[0],
            y: document_point[1],
            radius: pressure_radius(self.size, pressure),
            opacity: pressure_opacity(pressure),
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

pub fn pressure_radius(brush_size: f32, pressure: f32) -> f32 {
    let pressure = pressure.clamp(0.0, 1.0);
    let pressure_scale = MIN_PRESSURE_SIZE + (1.0 - MIN_PRESSURE_SIZE) * pressure;
    brush_size * pressure_scale * 0.5
}

pub fn pressure_opacity(pressure: f32) -> f32 {
    let pressure = pressure.clamp(0.0, 1.0);
    MIN_PRESSURE_OPACITY + (1.0 - MIN_PRESSURE_OPACITY) * pressure.powf(PRESSURE_OPACITY_GAMMA)
}

pub fn color32_to_rgba(color: Color32) -> [f32; 4] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_changes_radius_with_minimum_floor() {
        assert_eq!(pressure_radius(100.0, 0.0), 22.5);
        assert_eq!(pressure_radius(100.0, 1.0), 50.0);
    }

    #[test]
    fn mouse_pressure_is_fully_opaque() {
        assert_eq!(pressure_opacity(1.0), 1.0);
    }
}
