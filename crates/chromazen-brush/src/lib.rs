mod abr;
mod bundled;

use std::{error::Error, fmt, path::Path};

use chromazen_canvas::{BrushSpacing, StrokePoint};
use serde::{Deserialize, Serialize};

pub use abr::{AbrBrush, AbrError, ParsedAbr, parse_abr};
pub use bundled::{
    BUNDLED_BRUSH_IDS, DEFAULT_BRUSH_ID, LoadedBrushPreset, bundled_preset, bundled_stamp,
};

const BRUSH_SCHEMA_VERSION: u32 = 1;
pub const MAX_STAMP_DIMENSION: u32 = 4096;
const MIN_BRUSH_SPACING: f32 = 0.25;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrushError {
    message: String,
}

impl BrushError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BrushError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BrushError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrushPreset {
    pub schema_version: u32,
    pub name: String,
    pub stamp: String,
    pub size: SizeConfig,
    pub spacing: SpacingConfig,
    pub pressure: PressureConfig,
}

impl Default for BrushPreset {
    fn default() -> Self {
        Self {
            schema_version: BRUSH_SCHEMA_VERSION,
            name: "Charcoal".to_owned(),
            stamp: "stamp.png".to_owned(),
            size: SizeConfig::default(),
            spacing: SpacingConfig::default(),
            pressure: PressureConfig::default(),
        }
    }
}

impl BrushPreset {
    pub fn validate(&self) -> Result<(), BrushError> {
        if self.schema_version != BRUSH_SCHEMA_VERSION {
            return Err(BrushError::new(format!(
                "unsupported brush schema_version {}; expected {BRUSH_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.name.trim().is_empty() {
            return Err(BrushError::new("brush name must not be empty"));
        }
        validate_finite_positive("size.min", self.size.min)?;
        validate_finite_positive("size.max", self.size.max)?;
        validate_finite_positive("size.default", self.size.default)?;
        if self.size.max < self.size.min {
            return Err(BrushError::new(
                "size.max must be greater than or equal to size.min",
            ));
        }
        if !(self.size.min..=self.size.max).contains(&self.size.default) {
            return Err(BrushError::new(
                "size.default must be between size.min and size.max",
            ));
        }
        validate_finite_non_negative("spacing.ratio", self.spacing.ratio)?;
        validate_finite_at_least("spacing.minimum", self.spacing.minimum, MIN_BRUSH_SPACING)?;
        validate_unit("pressure.min_size", self.pressure.min_size)?;
        validate_unit("pressure.min_opacity", self.pressure.min_opacity)?;
        validate_finite_positive(
            "pressure.full_opacity_pressure",
            self.pressure.full_opacity_pressure,
        )?;
        validate_unit(
            "pressure.full_opacity_pressure",
            self.pressure.full_opacity_pressure,
        )?;
        validate_finite_positive("pressure.opacity_gamma", self.pressure.opacity_gamma)?;
        validate_stamp_path(&self.stamp)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SizeConfig {
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

impl Default for SizeConfig {
    fn default() -> Self {
        Self {
            default: 300.0,
            min: 1.0,
            max: 2000.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpacingConfig {
    pub ratio: f32,
    pub minimum: f32,
}

impl Default for SpacingConfig {
    fn default() -> Self {
        Self {
            ratio: 0.03,
            minimum: 1.0,
        }
    }
}

impl From<SpacingConfig> for BrushSpacing {
    fn from(spacing: SpacingConfig) -> Self {
        Self {
            ratio: spacing.ratio,
            minimum: spacing.minimum,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PressureConfig {
    pub min_size: f32,
    pub min_opacity: f32,
    pub full_opacity_pressure: f32,
    pub opacity_gamma: f32,
}

impl Default for PressureConfig {
    fn default() -> Self {
        Self {
            min_size: 0.3,
            min_opacity: 0.01,
            full_opacity_pressure: 0.9,
            opacity_gamma: 2.0,
        }
    }
}

impl PressureConfig {
    pub fn radius(self, brush_size: f32, pressure: f32) -> f32 {
        let pressure = pressure.clamp(0.0, 1.0);
        let pressure_scale = self.min_size + (1.0 - self.min_size) * pressure;
        brush_size * pressure_scale * 0.5
    }

    pub fn opacity(self, pressure: f32) -> f32 {
        let pressure = (pressure.clamp(0.0, 1.0) / self.full_opacity_pressure).min(1.0);
        self.min_opacity + (1.0 - self.min_opacity) * pressure.powf(self.opacity_gamma)
    }

    pub fn stroke_point(
        self,
        document_point: [f32; 2],
        brush_size: f32,
        pressure: f32,
    ) -> StrokePoint {
        StrokePoint {
            x: document_point[0],
            y: document_point[1],
            radius: self.radius(brush_size, pressure),
            opacity: self.opacity(pressure),
        }
    }
}

fn validate_stamp_path(stamp: &str) -> Result<(), BrushError> {
    let path = Path::new(stamp);
    if stamp.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(BrushError::new(
            "stamp must be a relative path inside the brush directory",
        ));
    }
    Ok(())
}

fn validate_finite_positive(field: &str, value: f32) -> Result<(), BrushError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(BrushError::new(format!(
            "{field} must be finite and greater than zero"
        )));
    }
    Ok(())
}

fn validate_finite_non_negative(field: &str, value: f32) -> Result<(), BrushError> {
    if !value.is_finite() || value < 0.0 {
        return Err(BrushError::new(format!(
            "{field} must be finite and non-negative"
        )));
    }
    Ok(())
}

fn validate_finite_at_least(field: &str, value: f32, minimum: f32) -> Result<(), BrushError> {
    if !value.is_finite() || value < minimum {
        return Err(BrushError::new(format!(
            "{field} must be finite and at least {minimum}"
        )));
    }
    Ok(())
}

fn validate_unit(field: &str, value: f32) -> Result<(), BrushError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(BrushError::new(format!("{field} must be between 0 and 1")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subpixel_minimum_brush_spacing_has_a_safe_floor() {
        let mut preset = BrushPreset::default();
        preset.spacing.minimum = 0.5;
        preset.validate().expect("half-pixel spacing");

        preset.spacing.minimum = 0.24;
        let error = preset.validate().expect_err("spacing below safe floor");
        assert!(error.to_string().contains("spacing.minimum"));
    }

    #[test]
    fn full_opacity_pressure_must_be_positive_and_at_most_one() {
        for pressure in [0.0, 1.1] {
            let mut preset = BrushPreset::default();
            preset.pressure.full_opacity_pressure = pressure;

            let error = preset.validate().expect_err("invalid pressure threshold");

            assert!(error.to_string().contains("pressure.full_opacity_pressure"));
        }
    }

    #[test]
    fn stamp_path_must_stay_inside_brush_directory() {
        for stamp in ["", "../tip.png", "/tip.png"] {
            let preset = BrushPreset {
                stamp: stamp.to_owned(),
                ..BrushPreset::default()
            };
            assert!(preset.validate().is_err(), "{stamp:?} should be rejected");
        }
    }

    #[test]
    fn pressure_changes_radius_with_minimum_floor() {
        let pressure = PressureConfig {
            min_size: 0.45,
            ..PressureConfig::default()
        };

        assert_eq!(pressure.radius(100.0, 0.0), 22.5);
        assert_eq!(pressure.radius(100.0, 1.0), 50.0);
    }

    #[test]
    fn pressure_uses_runtime_configuration() {
        let pressure = PressureConfig {
            min_size: 0.2,
            min_opacity: 0.4,
            full_opacity_pressure: 0.8,
            opacity_gamma: 2.0,
        };

        assert_eq!(pressure.radius(100.0, 0.0), 10.0);
        assert_eq!(pressure.opacity(0.0), 0.4);
        assert_eq!(pressure.opacity(0.8), 1.0);
    }

    #[test]
    fn stroke_point_contains_pressure_opacity() {
        let pressure = PressureConfig::default();

        assert_eq!(pressure.stroke_point([0.0, 0.0], 10.0, 1.0).opacity, 1.0);
        assert_eq!(
            pressure.stroke_point([0.0, 0.0], 10.0, 0.0).opacity,
            pressure.min_opacity
        );
    }
}
