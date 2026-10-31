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
