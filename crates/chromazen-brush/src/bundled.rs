use image::RgbaImage;

use crate::{BrushError, BrushPreset, PressureConfig, SizeConfig, SpacingConfig};

pub const DEFAULT_BRUSH_ID: &str = "charcoal";
const SKETCH_ID: &str = "sketch";
const ROUNDED_ID: &str = "rounded";
const RECTANGLE_ID: &str = "rectangle";
const PAINT_ID: &str = "paint";

pub const BUNDLED_BRUSH_IDS: [&str; 5] = [
    DEFAULT_BRUSH_ID,
    SKETCH_ID,
    ROUNDED_ID,
    RECTANGLE_ID,
    PAINT_ID,
];

#[derive(Debug)]
pub struct LoadedBrushPreset {
    pub id: String,
    pub preset: BrushPreset,
    pub stamp_image: RgbaImage,
}

impl LoadedBrushPreset {
    pub fn bundled(id: &str) -> Option<Self> {
        let preset = bundled_preset(id)?;
        Some(Self {
            id: id.to_owned(),
            preset,
            stamp_image: bundled_stamp(id).expect("bundled brush stamp is valid"),
        })
    }

    pub fn bundled_charcoal() -> Self {
        Self::bundled(DEFAULT_BRUSH_ID).expect("charcoal is a bundled brush")
    }
}

pub fn bundled_preset(id: &str) -> Option<BrushPreset> {
    Some(match id {
        DEFAULT_BRUSH_ID => BrushPreset::default(),
        SKETCH_ID => BrushPreset {
            name: "Sketch".to_owned(),
            size: SizeConfig {
                default: 18.0,
                min: 1.0,
                max: 200.0,
            },
            spacing: SpacingConfig {
                ratio: 0.08,
                minimum: 1.0,
            },
            pressure: PressureConfig {
                min_size: 0.25,
                min_opacity: 0.01,
                full_opacity_pressure: 1.0,
                opacity_gamma: 10.0,
            },
            ..BrushPreset::default()
        },
        ROUNDED_ID => stamp_preset(ROUNDED_ID, "Rounded", 60.0, 0.001),
        RECTANGLE_ID => stamp_preset(RECTANGLE_ID, "Rectangle", 80.0, 0.001),
        PAINT_ID => BrushPreset {
            pressure: PressureConfig {
                min_size: 0.5,
                min_opacity: 0.3,
                full_opacity_pressure: 0.8,
                opacity_gamma: 1.2,
            },
            ..stamp_preset(PAINT_ID, "Paint", 80.0, 0.05)
        },
        _ => return None,
    })
}

fn stamp_preset(id: &str, name: &str, default_size: f32, spacing: f32) -> BrushPreset {
    BrushPreset {
        name: name.to_owned(),
        stamp: format!("{id}.png"),
        size: SizeConfig {
            default: default_size,
            ..SizeConfig::default()
        },
        spacing: SpacingConfig {
            ratio: spacing,
            minimum: 0.5,
        },
        ..BrushPreset::default()
    }
}

pub fn bundled_stamp(id: &str) -> Result<RgbaImage, BrushError> {
    let bytes: &[u8] = match id {
        ROUNDED_ID => include_bytes!("../../../assets/stamps/rounded.png"),
        RECTANGLE_ID => include_bytes!("../../../assets/stamps/rectangle.png"),
        PAINT_ID => include_bytes!("../../../assets/stamps/paint.png"),
        _ => include_bytes!("../../../assets/stamps/charcoal.png"),
    };
    image::load_from_memory(bytes)
        .map(image::DynamicImage::into_rgba8)
        .map_err(|error| BrushError::new(format!("failed to decode bundled brush: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_brushes_are_valid_and_decode() {
        for id in BUNDLED_BRUSH_IDS {
            let brush = LoadedBrushPreset::bundled(id).expect("bundled brush");
            brush.preset.validate().expect("valid bundled preset");
            assert!(brush.stamp_image.width() > 0);
        }
    }

    #[test]
    fn unknown_id_is_not_bundled() {
        assert!(bundled_preset("missing").is_none());
    }
}
