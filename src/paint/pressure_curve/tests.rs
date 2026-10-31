use super::*;

#[test]
fn default_curve_leaves_pressure_unchanged() {
    let curve = PressureCurve::default();
    for step in 0..=20 {
        let pressure = step as f32 / 20.0;
        assert!(
            (curve.apply(pressure) - pressure).abs() < 1e-4,
            "{pressure}"
        );
    }
}

#[test]
fn pressure_outside_zero_until_and_full_from_is_clamped() {
    let curve = PressureCurve {
        zero_until: 0.2,
        full_from: 0.8,
        ..Default::default()
    };
    assert_eq!(curve.apply(0.1), 0.0);
    assert_eq!(curve.apply(0.9), 1.0);
    assert!((curve.apply(0.5) - 0.5).abs() < 1e-4);
}

#[test]
fn extreme_controls_never_decrease() {
    let curve = PressureCurve {
        controls: [[1.0, 1.0], [0.0, 0.0]],
        ..Default::default()
    };
    let mut previous = 0.0;
    for step in 0..=200 {
        let output = curve.apply(step as f32 / 200.0);
        assert!(output >= previous - 1e-5, "step {step}");
        previous = output;
    }
}

#[test]
fn soft_curve_raises_light_pressure() {
    let curve = PressureCurve {
        controls: [[0.0, 0.6], [0.4, 1.0]],
        ..Default::default()
    };
    assert!(curve.apply(0.25) > 0.4);
}

#[test]
fn invalid_curves_are_rejected() {
    let valid = PressureCurve::default();
    assert!(valid.validate().is_ok());
    for invalid in [
        PressureCurve {
            zero_until: 0.6,
            full_from: 0.6,
            ..valid
        },
        PressureCurve {
            full_from: 1.5,
            ..valid
        },
        PressureCurve {
            controls: [[0.5, -0.1], [0.5, 0.5]],
            ..valid
        },
        PressureCurve {
            zero_until: f32::NAN,
            ..valid
        },
    ] {
        assert!(invalid.validate().is_err(), "{invalid:?}");
    }
}
