use egui::Color32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PaintTool {
    #[default]
    Brush,
    Eraser,
    Smudge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub color: Color32,
    pub size: f32,
    pub opacity: f32,
    pub pressure: PressureSettings,
    pub spacing: BrushSpacing,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(170, 187, 204),
            size: 300.0,
            opacity: 1.0,
            pressure: PressureSettings::default(),
            spacing: BrushSpacing::default(),
        }
    }
}

impl BrushSettings {
    pub fn rgba(self) -> [f32; 4] {
        color32_to_rgba(self.color)
    }

    pub fn radius(self, pressure: f32) -> f32 {
        pressure_radius(self.size, pressure, self.pressure)
    }

    pub fn stroke_point(self, document_point: [f32; 2], pressure: f32) -> StrokePoint {
        StrokePoint {
            x: document_point[0],
            y: document_point[1],
            radius: self.radius(pressure),
            opacity: self.opacity * pressure_opacity(pressure, self.pressure),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PressureSettings {
    pub(crate) min_size: f32,
    pub(crate) min_opacity: f32,
    pub(crate) full_opacity_pressure: f32,
    pub(crate) opacity_gamma: f32,
}

impl Default for PressureSettings {
    fn default() -> Self {
        Self {
            min_size: 0.45,
            min_opacity: 0.08,
            full_opacity_pressure: 0.8,
            opacity_gamma: 1.35,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BrushSpacing {
    pub(crate) ratio: f32,
    pub(crate) minimum: f32,
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

fn pressure_radius(brush_size: f32, pressure: f32, settings: PressureSettings) -> f32 {
    let pressure = pressure.clamp(0.0, 1.0);
    let pressure_scale = settings.min_size + (1.0 - settings.min_size) * pressure;
    brush_size * pressure_scale * 0.5
}

fn pressure_opacity(pressure: f32, settings: PressureSettings) -> f32 {
    let pressure = (pressure.clamp(0.0, 1.0) / settings.full_opacity_pressure).min(1.0);
    settings.min_opacity + (1.0 - settings.min_opacity) * pressure.powf(settings.opacity_gamma)
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
        let brush = BrushSettings {
            size: 100.0,
            ..BrushSettings::default()
        };
        assert_eq!(brush.radius(0.0), 22.5);
        assert_eq!(brush.radius(1.0), 50.0);
    }

    #[test]
    fn tool_opacity_scales_pressure_opacity() {
        let brush = BrushSettings {
            opacity: 0.5,
            ..BrushSettings::default()
        };

        assert_eq!(brush.stroke_point([0.0, 0.0], 1.0).opacity, 0.5);
        assert_eq!(
            brush.stroke_point([0.0, 0.0], 0.0).opacity,
            brush.pressure.min_opacity * 0.5
        );
    }

    #[test]
    fn pressure_uses_runtime_configuration() {
        let settings = PressureSettings {
            min_size: 0.2,
            min_opacity: 0.4,
            full_opacity_pressure: 0.8,
            opacity_gamma: 2.0,
        };

        assert_eq!(pressure_radius(100.0, 0.0, settings), 10.0);
        assert_eq!(pressure_opacity(0.0, settings), 0.4);
        assert_eq!(pressure_opacity(0.8, settings), 1.0);
    }
}
